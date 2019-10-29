use anyhow::Result;
use chrono::prelude::*;
use image::RgbImage;
use reqwest::{header, Url};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Entity<T, S> {
    pub attributes: T,
    pub last_changed: DateTime<Utc>,
    pub last_updated: DateTime<Utc>,
    pub state: S,
}

#[derive(Debug, Deserialize)]
pub struct EventResult {
    pub message: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    BelowHorizon,
    AboveHorizon,
}

#[derive(Debug, Deserialize)]
pub struct Sun {
    pub next_rising: DateTime<Utc>,
    pub next_setting: DateTime<Utc>,
    pub next_dawn: DateTime<Utc>,
    pub next_dusk: DateTime<Utc>,
}

#[derive(Clone)]
pub struct Client {
    client: reqwest::Client,
    base: Url,
}

impl Client {
    pub fn new(base: Url, token: &str) -> Result<Client> {
        let mut headers = header::HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            header::HeaderValue::from_str(&format!("Bearer {}", token))?,
        );

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .build()?;

        Ok(Client { client, base })
    }

    pub async fn get_sun(&self) -> Result<Entity<Sun, State>> {
        let url = self.base.join("/api/states/sun.sun")?;

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

    pub async fn get_image(&self, camera: &str) -> Result<RgbImage> {
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

    pub async fn post_event(&self, event: &str) -> Result<EventResult> {
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
