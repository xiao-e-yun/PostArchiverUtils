use futures::StreamExt;
use governor::{
    Jitter, Quota, RateLimiter,
    clock::{QuantaClock, QuantaInstant},
    middleware::NoOpMiddleware,
    state::{InMemoryState, NotKeyed},
};
use http::Method;
use log::trace;
use reqwest::{Client, IntoUrl, Request, Response};
use reqwest_middleware::{ClientWithMiddleware, Middleware, Next, RequestBuilder};
use reqwest_retry::{RetryTransientMiddleware, policies::ExponentialBackoff};
use serde::de::DeserializeOwned;
use std::{
    fs::File,
    io::{BufWriter, Write},
    num::NonZeroU32,
    ops::{Deref, DerefMut},
    time::Duration,
};
use tokio::sync::Semaphore;

use crate::{Error, Result};

pub struct ArchiveClientBuilder {
    client: Client,
    pre_min_limit: u32,
    pre_sec_limit: Option<u32>,
    max_conn_limit: Option<u32>,
    retry_limit: u32,
}

impl ArchiveClientBuilder {
    pub fn new(client: Client, pre_min_limit: u32) -> Self {
        Self {
            client,
            pre_min_limit,
            pre_sec_limit: None,
            max_conn_limit: None,
            retry_limit: 3,
        }
    }

    pub fn pre_min_limit(mut self, limit: u32) -> Self {
        self.pre_min_limit = limit;
        self
    }

    pub fn pre_sec_limit(mut self, limit: u32) -> Self {
        self.pre_sec_limit = Some(limit);
        self
    }

    pub fn max_conn_limit(mut self, limit: u32) -> Self {
        self.max_conn_limit = Some(limit);
        self
    }

    pub fn retry_limit(mut self, limit: u32) -> Self {
        self.retry_limit = limit;
        self
    }

    pub fn build(self) -> ArchiveClient {
        let retry_policy = ExponentialBackoff::builder().build_with_max_retries(self.retry_limit);
        let client = reqwest_middleware::ClientBuilder::new(self.client)
            .with(SemaphoreMiddleware::new(
                self.pre_min_limit,
                self.pre_sec_limit.or(self.max_conn_limit).unwrap_or(4),
                self.max_conn_limit.or(self.pre_sec_limit).unwrap_or(4),
            ))
            .with(RetryTransientMiddleware::new_with_policy(retry_policy))
            .build();

        ArchiveClient {
            inner: client,
            retry: self.retry_limit,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ArchiveClient{
    inner: ClientWithMiddleware,
    retry: u32
}

impl ArchiveClient {
    pub fn builder(client: Client, pre_min_limit:u32) -> ArchiveClientBuilder {
        ArchiveClientBuilder::new(client, pre_min_limit)
    }

    pub async fn fetch_with_method<T: DeserializeOwned>(
        &self,
        method: Method,
        url: impl IntoUrl,
    ) -> Result<T> {
        let request = self.inner.request(method, url);
        let response = request.send().await?;
        let response = response.bytes().await?;
        serde_json::from_slice(&response).map_err(|e| {
            Error::UnexpectedResponse(e, String::from_utf8(response.to_vec()).unwrap())
        })
    }

    pub async fn fetch<T: DeserializeOwned>(&self, url: impl IntoUrl) -> Result<T> {
        self.fetch_with_method(Method::GET, url).await
    }

    pub async fn download_with_method(
        &self,
        method: Method,
        url: impl IntoUrl + Clone,
        file: &mut File,
    ) -> Result<()> {
        async fn handle(request: RequestBuilder, file: &mut File) -> Result<()> {
            file.set_len(0)?;

            let response = request.send().await?;
            let mut stream = response.bytes_stream();

            let mut buffer = BufWriter::new(file);
            while let Some(bytes) = stream.next().await {
                let bytes = bytes?;
                buffer.write_all(&bytes)?;
            }
            buffer.flush()?;
            Ok(())
        }

        let mut err = Ok(());
        for _ in 0..=self.retry {
            let request = self.request(method.clone(), url.clone());
            match handle(request, file).await {
                Ok(_) => return Ok(()),
                Err(e) => err = Err(e),
            }
        }
        err
    }

    pub async fn download(&self, url: impl IntoUrl + Clone, file: &mut File) -> Result<()> {
        self.download_with_method(Method::GET, url, file).await
    }
}

impl Deref for ArchiveClient {
    type Target = ClientWithMiddleware;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for ArchiveClient {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

type ArchiveRateLimiter =
    RateLimiter<NotKeyed, InMemoryState, QuantaClock, NoOpMiddleware<QuantaInstant>>;
#[derive(Debug)]
pub struct SemaphoreMiddleware {
    max_conn_semaphore: Semaphore,
    pre_sec_limiter: ArchiveRateLimiter,
    pre_min_limiter: ArchiveRateLimiter,
}

impl SemaphoreMiddleware {
    pub fn new(pre_min_limit: u32, pre_sec_limit: u32, max_conn_limit: u32) -> Self {
        let semaphore = Semaphore::new(max_conn_limit as usize);
        let min_rate_limiter = RateLimiter::direct(Quota::per_minute(
            NonZeroU32::new(pre_min_limit).unwrap(),
        ));
        let sec_rate_limiter = RateLimiter::direct(Quota::per_second(
            NonZeroU32::new(pre_sec_limit).unwrap(),
        ));
        Self {
            max_conn_semaphore: semaphore,
            pre_sec_limiter: sec_rate_limiter,
            pre_min_limiter: min_rate_limiter,
        }
    }
}

#[async_trait::async_trait]
impl Middleware for SemaphoreMiddleware {
    async fn handle(
        &self,
        req: Request,
        extensions: &mut http::Extensions,
        next: Next<'_>,
    ) -> reqwest_middleware::Result<Response> {
        let _ = self.max_conn_semaphore.acquire().await.unwrap();
        self.pre_sec_limiter.until_ready().await;
        self.pre_min_limiter.until_ready_with_jitter(Jitter::up_to(Duration::from_millis(800))).await;
        trace!("Fetching: {}", req.url());
        next.run(req, extensions).await
    }
}
