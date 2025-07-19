use futures::StreamExt;
use governor::{
    Jitter, Quota, RateLimiter,
    clock::{QuantaClock, QuantaInstant},
    middleware::NoOpMiddleware,
    state::{InMemoryState, NotKeyed},
};
use http::Method;
use reqwest::{Client, IntoUrl, Request, Response};
use reqwest_middleware::{ClientWithMiddleware, Middleware, Next, RequestBuilder};
use reqwest_retry::{RetryTransientMiddleware, policies::ExponentialBackoff};
use serde::de::DeserializeOwned;
use std::{
    fs::File,
    io::{BufWriter, Write},
    num::NonZeroU32,
    ops::{Deref, DerefMut},
    sync::Arc,
    time::Duration,
};
use tokio::sync::Semaphore;

use crate::{Error, Result};

const RETRY_LIMIT: u32 = 3;

#[derive(Debug, Clone)]
pub struct ArchiveClient(ClientWithMiddleware);

impl ArchiveClient {
    pub fn new(client: Client, limit: usize) -> Self {
        let retry_policy = ExponentialBackoff::builder().build_with_max_retries(RETRY_LIMIT);
        let client = reqwest_middleware::ClientBuilder::new(client)
            .with(SemaphoreMiddleware::new(limit))
            .with(RetryTransientMiddleware::new_with_policy(retry_policy))
            .build();

        Self(client)
    }

    pub async fn fetch_with_method<T: DeserializeOwned>(
        &self,
        method: Method,
        url: impl IntoUrl,
    ) -> Result<T> {
        let request = self.0.request(method, url);
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
        for _ in 0..=RETRY_LIMIT {
            let request = self.0.request(method.clone(), url.clone());
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
        &self.0
    }
}

impl DerefMut for ArchiveClient {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

type ArchiveRateLimiter =
    RateLimiter<NotKeyed, InMemoryState, QuantaClock, NoOpMiddleware<QuantaInstant>>;
#[derive(Debug, Clone)]
pub struct SemaphoreMiddleware(Arc<(Semaphore, ArchiveRateLimiter)>);

impl SemaphoreMiddleware {
    pub fn new(limit: usize) -> Self {
        let semaphore = Semaphore::new(5);
        let rate_limiter =
            RateLimiter::direct(Quota::per_minute(NonZeroU32::new(limit as u32).unwrap()));
        Self(Arc::new((semaphore, rate_limiter)))
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
        let (semaphore, rate_limiter) = self.0.as_ref();
        let _ = semaphore.acquire().await.unwrap();
        rate_limiter
            .until_ready_with_jitter(Jitter::up_to(Duration::from_millis(800)))
            .await;
        next.run(req, extensions).await
    }
}
