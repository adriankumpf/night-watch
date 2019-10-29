use chrono::prelude::*;
use anyhow::Result;
use reqwest::{header, Url};
use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct Entity<T> {
    attributes: T,
    last_changed: DateTime<Utc>,
    last_updated: DateTime<Utc>,
    state: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Sun {
    azimuth: f32,
    elevation: f32,
    next_rising: DateTime<Utc>,
    next_setting: DateTime<Utc>,
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

    pub async fn get_sun<T>(&self, entitiy_id: &str) -> Result<Entity<Sun>> {
        let url = self.base.join(&format!("/api/states/{}", entitiy_id))?;
        let state = self.client.get(url).send().await?.json().await?;
        Ok(state)
    }
}
