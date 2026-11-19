use futures::StreamExt;
use governor::{
    Jitter, Quota, RateLimiter,
    clock::QuantaClock,
    middleware::NoOpMiddleware,
    state::{InMemoryState, NotKeyed},
};
use http::{HeaderMap, Method, StatusCode};
use log::{error, trace, warn};
use reqwest::{Client, IntoUrl, RequestBuilder, Response};
use reqwest_retry::{RetryDecision, RetryPolicy, policies::ExponentialBackoff};
use serde::de::DeserializeOwned;
use std::{
    num::NonZeroU32,
    ops::Deref,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime},
};
use tokio::{
    fs::File,
    io::{AsyncWriteExt, BufWriter},
    sync::Semaphore,
    time::sleep,
};

#[derive(Debug, Clone)]
pub struct Limit {
    pub per_min: NonZeroU32,
    pub burst: NonZeroU32,
    pub concurrency: u32,
    pub retries: u8,
}

impl Default for Limit {
    fn default() -> Self {
        Self {
            per_min: NonZeroU32::new(60).unwrap(),
            burst: NonZeroU32::new(4).unwrap(),
            concurrency: 2,
            retries: 3,
        }
    }
}

impl Limit {
    fn semaphore(&self) -> Semaphore {
        Semaphore::new(self.concurrency as usize)
    }
    fn limiter(&self) -> RateLimiter<NotKeyed, InMemoryState, QuantaClock, NoOpMiddleware> {
        let quota = Quota::per_minute(self.per_min).allow_burst(self.burst);
        RateLimiter::direct(quota)
    }
}

#[derive(Debug, Clone)]
pub struct ArchiveClient {
    pub client: Client,
    limiters: Arc<(
        Semaphore,
        RateLimiter<NotKeyed, InMemoryState, QuantaClock, NoOpMiddleware>,
    )>,
    limits: Limit,
}

impl ArchiveClient {
    pub fn new(client: Client, limits: Limit) -> Self {
        let limiters = Arc::new((limits.semaphore(), limits.limiter()));
        Self {
            client,
            limits,
            limiters,
        }
    }

    pub async fn request<T, Fut>(
        &self,
        request: &RequestBuilder,
        callback: impl Fn(Response) -> Fut,
    ) -> Result<T>
    where
        Fut: Future<Output = Result<T>>,
    {
        let (semaphore, limiter) = self.limiters.deref();

        let policy =
            ExponentialBackoff::builder().build_with_max_retries(self.limits.retries as u32);
        let start = SystemTime::now();
        for n in 0.. {
            let permit = semaphore.acquire().await.unwrap();
            limiter
                .until_ready_with_jitter(Jitter::up_to(Duration::from_millis(200)))
                .await;

            let result = async {
                let request = request.try_clone().unwrap();
                let response = request.send().await?;

                match response.status() {
                    s if s.is_success() => {}
                    StatusCode::TOO_MANY_REQUESTS => {
                        return Err(RequestError::RateLimitExceeded);
                    }
                    StatusCode::UNAUTHORIZED => {
                        return Err(RequestError::InvalidSession);
                    }
                    s if s.is_server_error() || s == StatusCode::REQUEST_TIMEOUT => {
                        return Err(RequestError::TemporaryError);
                    }
                    s => {
                        const FALLBACK: &str = "Response body unavailable";
                        let body = response.text().await.unwrap_or(FALLBACK.to_string());
                        return Err(RequestError::UnexpectedStatus(s, body));
                    }
                }

                callback(response).await
            }
            .await;

            drop(permit);
            match result {
                Ok(result) => return Ok(result),
                Err(e) if e.is_transient() => {
                    warn!("{e}");
                    match policy.should_retry(start, n) {
                        RetryDecision::Retry { execute_after } => {
                            sleep(
                                execute_after
                                    .duration_since(SystemTime::now())
                                    .unwrap_or_default(),
                            )
                            .await
                        }
                        RetryDecision::DoNotRetry => {
                            warn!("Exceeded retry limit after {} attempts: {e}", n + 1);
                            return Err(e);
                        }
                    };
                }
                Err(e) => {
                    error!("{e}");
                    return Err(e);
                }
            }
        }
        unreachable!()
    }

    async fn fetch_with_method<T: DeserializeOwned>(
        &self,
        method: Method,
        url: impl IntoUrl,
    ) -> Result<T> {
        let url = url.into_url()?;
        let req = self.client.request(method, url.clone());
        let res = self
            .request(&req, |res| async move {
                let bytes = res.bytes().await?;
                serde_json::from_slice::<T>(&bytes).map_err(RequestError::UnexpectedResponse)
            })
            .await?;
        trace!("Fetched {url}");
        Ok(res)
    }

    pub async fn fetch<T: DeserializeOwned>(&self, url: impl IntoUrl) -> Result<T> {
        self.fetch_with_method(Method::GET, url).await
    }

    pub async fn download(&self, with_in: &Path, url: impl IntoUrl) -> Result<PathBuf> {
        let url = url.into_url()?;
        let req = self.client.request(Method::GET, url.clone());
        let path = self
            .request(&req, |res| async move {
                let mut stream = res.bytes_stream();

                let filename: String = (0..10).map(|_| fastrand::alphanumeric()).collect();
                let path = with_in.join(filename);
                let mut file = File::create(&path).await?;

                let mut buffer = BufWriter::new(&mut file);
                while let Some(bytes) = stream.next().await {
                    let bytes = bytes?;
                    buffer.write_all(&bytes).await?;
                }
                buffer.flush().await?;
                drop(buffer);

                file.sync_all().await?;
                Ok(path)
            })
            .await?;
        trace!("Downloaded {url}");
        Ok(path)
    }
}

pub fn browser_headers(user_agent: &str) -> reqwest::header::HeaderMap {
    let platform = match user_agent {
        ua if ua.contains("Windows") || ua.contains("Win64") => "\"Windows\"",
        ua if ua.contains("Macintosh") || ua.contains("Mac OS X") => "\"macOS\"",
        ua if ua.contains("Linux") || ua.contains("X11") => "\"Linux\"",
        _ => "\"Unknown\"",
    };

    let mobile = match user_agent.contains("Mobile") {
        true => "?1",
        false => "?0",
    };

    let ua = match user_agent {
        ua if ua.contains("Edg/") => "Edg",
        ua if ua.contains("Chrome/") => "Chrome",
        ua if ua.contains("Firefox/") => "Firefox",
        ua if ua.contains("Safari/") && !ua.contains("Chrome/") => "Safari",
        _ => "Unknown",
    };

    let version = match user_agent
        .split_whitespace()
        .find(|part| part.starts_with(ua))
    {
        Some(part) => match part.split('/').nth(1) {
            Some(v) => v.split('.').next().unwrap_or("12"),
            None => "12",
        },
        None => "12",
    };

    let ua = format!("\"Chromium\";v=\"{version}\",")
        + &match ua {
            "Edg" => format!("\"Microsoft Edge\";v=\"{version}\""),
            "Chrome" => format!("\"Google Chrome\";v=\"{version}\""),
            "Firefox" => format!("\"Firefox\";v=\"{version}\""),
            "Safari" => format!("\"Safari\";v=\"{version}\""),
            _ => format!("\"{ua}\";v=\"{version}\""),
        }
        + ", \"Not_A Brand\";v=\"99\"";

    use http::header;
    let mut headers = HeaderMap::new();
    headers.insert(header::USER_AGENT, user_agent.parse().unwrap());
    headers.insert(header::DNT, "1".parse().unwrap());
    headers.insert(header::ACCEPT_LANGUAGE, "ja-JP,ja;q=0.9,en-US;q=0.8,en;q=0.7".parse().unwrap());
    headers.insert(header::ACCEPT, "application/json, text/plain, */*".parse().unwrap());

    // sec-fetch headers
    headers.insert("sec-fetch-dest", "empty".parse().unwrap());
    headers.insert("sec-fetch-mode", "cors".parse().unwrap());
    headers.insert("sec-fetch-site", "same-site".parse().unwrap());

    if matches!(user_agent, ua if ua.contains("Edg/") || ua.contains("Chrome/")) {
        headers.insert("sec-ch-ua", ua.parse().unwrap());
        headers.insert("sec-ch-ua-platform", platform.parse().unwrap());
        headers.insert("sec-ch-ua-mobile", mobile.parse().unwrap());
    };

    headers
}

#[derive(thiserror::Error, Debug)]
pub enum RequestError {
    #[error("Rate limit exceeded")]
    RateLimitExceeded,
    #[error("Temporary error")]
    TemporaryError,
    #[error("Invalid session/token")]
    InvalidSession,
    #[error("Invalid response: {0}")]
    InvalidResponse(String),
    #[error("Unexpected response: {0}")]
    UnexpectedResponse(serde_json::Error),
    /// Non-success status not covered above, with the response body.
    #[error("Unexpected status: {0}")]
    UnexpectedStatus(StatusCode, String),
    #[error(transparent)]
    Reqwest(#[from] reqwest::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl RequestError {
    fn is_transient(&self) -> bool {
        match self {
            RequestError::RateLimitExceeded
            | RequestError::TemporaryError
            | RequestError::InvalidResponse(_)
            | RequestError::UnexpectedResponse(_) => true,
            RequestError::Reqwest(e) => {
                // A connection dropped while streaming a body is reported as a decode error
                // (`bytes_stream` maps body errors to `Kind::Decode`), so retry those too.
                // JSON is parsed with serde_json (`UnexpectedResponse`), not reqwest.
                e.is_timeout() || e.is_connect() || e.is_body() || e.is_request() || e.is_decode()
            }
            _ => false,
        }
    }
}

type Result<T> = std::result::Result<T, RequestError>;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    /// A body that breaks off mid-download surfaces as a reqwest *decode* error
    /// (`bytes_stream` maps body errors to `Kind::Decode`); it must be retried.
    #[tokio::test]
    async fn download_retries_when_connection_drops_mid_body() {
        const BODY: &[u8] = b"0123456789";
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let attempts = Arc::new(AtomicUsize::new(0));

        let served = attempts.clone();
        tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let attempt = served.fetch_add(1, Ordering::SeqCst);
                let mut buf = [0u8; 1024];
                let _ = socket.read(&mut buf).await;
                let header = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    BODY.len()
                );
                socket.write_all(header.as_bytes()).await.unwrap();
                if attempt == 0 {
                    // Send part of the body, then drop the connection.
                    socket.write_all(&BODY[..3]).await.unwrap();
                } else {
                    socket.write_all(BODY).await.unwrap();
                }
                socket.shutdown().await.ok();
            }
        });

        let client = ArchiveClient::new(Client::new(), Limit::default());
        let dir = tempfile::tempdir().unwrap();
        let path = client
            .download(dir.path(), format!("http://{addr}/file"))
            .await
            .expect("download should succeed after retrying");

        assert_eq!(std::fs::read(path).unwrap(), BODY);
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
    }
}
