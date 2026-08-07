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

#[cfg(test)]
mod tests {
    use serde::Deserialize;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;
    use crate::test_support::{TOKEN, home_assistant, jpeg, solid};

    #[derive(Debug, Deserialize)]
    struct Attributes {
        friendly_name: String,
    }

    #[tokio::test]
    async fn authenticates_and_deserializes_entities() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/api/states/binary_sensor.door"))
            .and(header("authorization", format!("Bearer {TOKEN}").as_str()))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                r#"{"state": "on", "attributes": {"friendly_name": "Door"}}"#,
                "application/json",
            ))
            .expect(1)
            .mount(&server)
            .await;

        let entity: Entity<Attributes, String> = home_assistant(&server)
            .get_entity("binary_sensor.door")
            .await
            .unwrap();

        assert_eq!(entity.state, "on");
        assert_eq!(entity.attributes.friendly_name, "Door");
    }

    #[tokio::test]
    async fn posts_events() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/api/events/close_rollershutters"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                r#"{"message": "Event close_rollershutters fired."}"#,
                "application/json",
            ))
            .expect(1)
            .mount(&server)
            .await;

        let result = home_assistant(&server)
            .send_event("close_rollershutters")
            .await
            .unwrap();

        assert_eq!(result.message, "Event close_rollershutters fired.");
    }

    #[tokio::test]
    async fn decodes_camera_images() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/api/camera_proxy/camera.front_door"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_raw(jpeg(solid(32, 16, [10, 20, 30])), "image/jpeg"),
            )
            .mount(&server)
            .await;

        let image = home_assistant(&server)
            .get_camera_image("camera.front_door")
            .await
            .unwrap();

        assert_eq!(image.dimensions(), (32, 16));
    }

    #[tokio::test]
    async fn undecodable_camera_images_are_an_error() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/api/camera_proxy/camera.front_door"))
            .respond_with(ResponseTemplate::new(200).set_body_raw("not a jpeg", "image/jpeg"))
            .mount(&server)
            .await;

        assert!(
            home_assistant(&server)
                .get_camera_image("camera.front_door")
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn error_responses_are_not_retried_by_default() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(401))
            .expect(1)
            .mount(&server)
            .await;

        let error = home_assistant(&server)
            .get_entity::<(), String>("sun.sun")
            .await
            .unwrap_err();

        assert!(error.to_string().contains("401"), "{error}");
    }

    #[tokio::test]
    async fn transient_errors_are_retried_when_enabled() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(500))
            .up_to_n_times(1)
            .with_priority(1)
            .expect(1)
            .mount(&server)
            .await;

        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_raw(r#"{"message": "Event fired."}"#, "application/json"),
            )
            .expect(1)
            .mount(&server)
            .await;

        let ha = HomeAssistant::new(server.uri().parse().unwrap(), TOKEN, true).unwrap();
        let result = ha.send_event("open_rollershutters").await.unwrap();

        assert_eq!(result.message, "Event fired.");
    }
}
