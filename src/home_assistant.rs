use std::time::Duration;

use anyhow::Result;
use image::RgbImage;
use reqwest::{Method, Response, Url, header};
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
use reqwest_retry::{Jitter, RetryTransientMiddleware, policies::ExponentialBackoff};
use serde::{Deserialize, de::DeserializeOwned};

#[derive(Debug, Deserialize)]
pub struct Entity<T, S> {
    pub attributes: T,
    pub state: S,
}

#[derive(Debug, Deserialize)]
pub struct EventResult {
    pub message: String,
}

pub struct HomeAssistant {
    client: ClientWithMiddleware,
    base: Url,
}

impl HomeAssistant {
    pub fn new(base: Url, token: &str, retry: bool) -> Result<Self> {
        let mut headers = header::HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            header::HeaderValue::from_str(&format!("Bearer {token}"))?,
        );

        let mut client = ClientBuilder::new(
            reqwest::Client::builder()
                .default_headers(headers)
                .build()?,
        );

        if retry {
            let retry_policy = ExponentialBackoff::builder()
                .base(2)
                .jitter(Jitter::None)
                .retry_bounds(Duration::from_secs(1), Duration::from_secs(10))
                .build_with_total_retry_duration_and_limit_retries(Duration::from_secs(2 * 60));

            client = client.with(RetryTransientMiddleware::new_with_policy(retry_policy));
        }

        Ok(Self {
            client: client.build(),
            base,
        })
    }

    async fn request(&self, method: Method, path: &str) -> Result<Response> {
        let url = self.base.join(path)?;

        let response = self
            .client
            .request(method, url)
            .send()
            .await?
            .error_for_status()?;

        Ok(response)
    }

    pub async fn get_entity<T, S>(&self, entity: &str) -> Result<Entity<T, S>>
    where
        S: DeserializeOwned,
        T: DeserializeOwned,
    {
        let path = format!("/api/states/{entity}");

        Ok(self.request(Method::GET, &path).await?.json().await?)
    }

    pub async fn get_camera_image(&self, entity: &str) -> Result<RgbImage> {
        let path = format!("/api/camera_proxy/{entity}");
        let bytes = self.request(Method::GET, &path).await?.bytes().await?;

        Ok(image::load_from_memory(&bytes)?.into_rgb8())
    }

    pub async fn send_event(&self, event: &str) -> Result<EventResult> {
        let path = format!("/api/events/{event}");

        Ok(self.request(Method::POST, &path).await?.json().await?)
    }
}
