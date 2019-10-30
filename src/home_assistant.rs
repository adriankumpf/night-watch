use anyhow::Result;
use image::RgbImage;
use log::warn;
use reqwest::{header, Url};
use serde::{de::DeserializeOwned, Deserialize};
use tokio::timer;

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
    client: reqwest::Client,
    base: Url,
}

impl HomeAssistant {
    pub fn new(base: Url, token: &str) -> Result<Self> {
        let mut headers = header::HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            header::HeaderValue::from_str(&format!("Bearer {}", token))?,
        );

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .build()?;

        Ok(Self { client, base })
    }

    pub async fn fetch_entity<T, S>(&self, entity: &str) -> Entity<T, S>
    where
        S: DeserializeOwned,
        T: DeserializeOwned,
    {
        let mut i = 0;

        loop {
            match self.get_entity(entity).await {
                Err(_error) => {
                    let secs = 2u64.pow(i);
                    warn!("Home Assistant is not available. Retrying in {}s", secs);
                    timer::delay_for(std::time::Duration::from_secs(secs)).await;
                    i += 1;
                }
                Ok(sun) => return sun,
            }
        }
    }

    pub async fn get_entity<T, S>(&self, entity: &str) -> Result<Entity<T, S>>
    where
        S: DeserializeOwned,
        T: DeserializeOwned,
    {
        let url = self.base.join(&format!("/api/states/{}", entity))?;

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
            .join(&format!("/api/camera_proxy/camera.{}", camera))?;

        let bytes = self
            .client
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?;

        let image = image::load_from_memory(&bytes)?.to_rgb();

        Ok(image)
    }

    pub async fn send_event(&self, event: &str) -> Result<EventResult> {
        let url = self.base.join(&format!("/api/events/{}", event))?;

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
