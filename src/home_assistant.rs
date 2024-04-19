use anyhow::Result;
use image::RgbImage;
use reqwest::{header, Url};
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
use reqwest_retry::{policies::ExponentialBackoff, RetryTransientMiddleware};
use retry_policies::Jitter;
use serde::{de::DeserializeOwned, Deserialize};
use std::time::Duration;

#[derive(Debug, Deserialize)]
pub struct Entity<T, S> {
    pub attributes: T,
    pub state: S,
}

#[derive(Debug, Deserialize)]
pub struct EventResult {
    pub message: String,
}

#[derive(Clone)]
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
                .build_with_total_retry_duration_and_max_retries(Duration::from_secs(2 * 60));

            client = client.with(RetryTransientMiddleware::new_with_policy(retry_policy));
        };

        let client = client.build();

        Ok(Self { client, base })
    }

    pub async fn get_entity<T, S>(&self, entity: &str) -> Result<Entity<T, S>>
    where
        S: DeserializeOwned,
        T: DeserializeOwned,
    {
        let url = self.base.join(&format!("/api/states/{entity}"))?;

        let state = self
            .client
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        Ok(state)
    }

    pub async fn get_camera_image(&self, camera: &str) -> Result<RgbImage> {
        let url = self
            .base
            .join(&format!("/api/camera_proxy/camera.{camera}"))?;

        let bytes = self
            .client
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?;

        let image = image::load_from_memory(&bytes)?.to_rgb8();

        Ok(image)
    }

    pub async fn send_event(&self, event: &str) -> Result<EventResult> {
        let url = self.base.join(&format!("/api/events/{event}"))?;

        let result = self
            .client
            .post(url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        Ok(result)
    }
}
