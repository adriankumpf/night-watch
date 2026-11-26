use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use image::RgbImage;
use reqwest::{Method, Response, StatusCode, Url, header};
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
use reqwest_retry::{
    DefaultRetryableStrategy, Jitter, RetryTransientMiddleware, Retryable, RetryableStrategy,
    policies::ExponentialBackoff,
};
use serde::{Deserialize, de::DeserializeOwned};
use tracing::warn;

/// Upper bound for a single request. The callers retry failures, which only
/// helps if a request that HA never answers fails rather than hangs.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

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

/// The bearer header, marked sensitive so that anything that formats the
/// headers redacts it rather than printing the token, hyper's own request
/// logging included.
fn auth_header(token: &str) -> Result<header::HeaderValue> {
    let mut value = header::HeaderValue::from_str(&format!("Bearer {token}"))?;
    value.set_sensitive(true);

    Ok(value)
}

/// Whether `error` is a response no amount of retrying will change: HA
/// rejecting the token. Everything else, a 404 included, can be HA still
/// starting up or an integration reloading, and is worth another try.
pub fn is_permanent(error: &anyhow::Error) -> bool {
    error
        .chain()
        .filter_map(|cause| cause.downcast_ref::<reqwest::Error>())
        .filter_map(reqwest::Error::status)
        .any(|status| matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN))
}

/// The default strategy, but saying what failed. The middleware's own
/// warning only gives the attempt number and the delay.
struct LoggedStrategy;

impl RetryableStrategy for LoggedStrategy {
    fn handle(&self, result: &reqwest_middleware::Result<Response>) -> Option<Retryable> {
        let retryable = DefaultRetryableStrategy.handle(result);

        if retryable == Some(Retryable::Transient) {
            match result {
                Ok(response) => warn!("{} answered {}", response.url(), response.status()),
                Err(error) => warn!("{error:#}"),
            }
        }

        retryable
    }
}

impl HomeAssistant {
    pub fn new(mut base: Url, token: &str, retry: bool) -> Result<Self> {
        if base.cannot_be_a_base() {
            return Err(anyhow!("{base} cannot be a base URL"));
        }

        // Drop a trailing empty segment once, so that `url` is a plain append.
        base.path_segments_mut()
            .expect("checked just above")
            .pop_if_empty();

        let mut headers = header::HeaderMap::new();
        headers.insert(header::AUTHORIZATION, auth_header(token)?);

        let mut client = ClientBuilder::new(
            reqwest::Client::builder()
                .default_headers(headers)
                .timeout(REQUEST_TIMEOUT)
                .build()?,
        );

        if retry {
            let retry_policy = ExponentialBackoff::builder()
                .base(2)
                .jitter(Jitter::None)
                .retry_bounds(Duration::from_secs(1), Duration::from_secs(10))
                .build_with_total_retry_duration_and_limit_retries(Duration::from_secs(2 * 60));

            client = client.with(RetryTransientMiddleware::new_with_policy_and_strategy(
                retry_policy,
                LoggedStrategy,
            ));
        }

        Ok(Self {
            client: client.build(),
            base,
        })
    }

    /// The base URL with `segments` appended to whatever path it already has,
    /// so a HA behind a reverse proxy at a sub-path keeps working. Each segment
    /// is escaped, so an entity id can never escape the API path.
    fn url(&self, segments: &[&str]) -> Url {
        let mut url = self.base.clone();

        url.path_segments_mut()
            .expect("`new` rejects a base URL that cannot be one")
            .extend(segments);

        url
    }

    async fn request(&self, method: Method, segments: &[&str]) -> Result<Response> {
        let response = self
            .client
            .request(method, self.url(segments))
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
        Ok(self
            .request(Method::GET, &["api", "states", entity])
            .await?
            .json()
            .await?)
    }

    /// The current frame of `entity`. HA has been seen answering `200` with a
    /// body that is no image at all, so a decoding error names the entity and
    /// what came back instead.
    pub async fn get_camera_image(&self, entity: &str) -> Result<RgbImage> {
        let response = self
            .request(Method::GET, &["api", "camera_proxy", entity])
            .await?;

        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("no content type")
            .to_owned();

        let bytes = response.bytes().await?;

        let image = image::load_from_memory(&bytes).with_context(|| {
            format!(
                "decoding the frame of {entity} ({} bytes, {content_type})",
                bytes.len()
            )
        })?;

        Ok(image.into_rgb8())
    }

    pub async fn send_event(&self, event: &str) -> Result<EventResult> {
        Ok(self
            .request(Method::POST, &["api", "events", event])
            .await?
            .json()
            .await?)
    }
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;
    use crate::test_support::{TOKEN, client, home_assistant, jpeg_response, json_response, solid};

    #[derive(Debug, Deserialize)]
    struct Attributes {
        friendly_name: String,
    }

    #[test]
    fn paths_are_appended_to_the_base_url() {
        let states = ["api", "states", "sun.sun"];

        for base in ["http://ha.local:8123", "http://ha.local:8123/"] {
            let url = client(base, false).url(&states);
            assert_eq!(url.as_str(), "http://ha.local:8123/api/states/sun.sun");
        }

        // A HA served under a sub-path by a reverse proxy.
        for base in ["https://home.example/hass", "https://home.example/hass/"] {
            let url = client(base, false).url(&states);
            assert_eq!(url.as_str(), "https://home.example/hass/api/states/sun.sun");
        }
    }

    #[test]
    fn entity_ids_cannot_escape_the_api_path() {
        let url = client("http://ha.local", false).url(&["api", "states", "../../evil"]);

        assert_eq!(url.as_str(), "http://ha.local/api/states/..%2F..%2Fevil");
    }

    #[test]
    fn the_authorization_header_does_not_print_the_token() {
        let auth = auth_header(TOKEN).unwrap();

        assert!(auth.is_sensitive());
        assert!(!format!("{auth:?}").contains(TOKEN), "{auth:?}");
    }

    #[test]
    fn a_url_that_cannot_be_a_base_is_rejected() {
        let base = "mailto:ha@example.com".parse().unwrap();

        let Err(error) = HomeAssistant::new(base, TOKEN, false) else {
            panic!("a mailto: URL cannot be a base and must be rejected");
        };

        assert!(
            error.to_string().contains("cannot be a base URL"),
            "{error}"
        );
    }

    #[tokio::test]
    async fn authenticates_and_deserializes_entities() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/api/states/binary_sensor.door"))
            .and(header("authorization", format!("Bearer {TOKEN}").as_str()))
            .respond_with(json_response(
                r#"{"state": "on", "attributes": {"friendly_name": "Door"}}"#,
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
            .respond_with(json_response(
                r#"{"message": "Event close_rollershutters fired."}"#,
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
            .respond_with(jpeg_response(&solid(32, 16, [10, 20, 30])))
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

        let error = home_assistant(&server)
            .get_camera_image("camera.front_door")
            .await
            .unwrap_err();

        // The entity and what HA sent instead are what tell a broken camera
        // apart from an error page.
        let message = format!("{error:#}");
        assert!(message.contains("camera.front_door"), "{message}");
        assert!(message.contains("10 bytes, image/jpeg"), "{message}");
        assert!(!is_permanent(&error));
    }

    #[tokio::test]
    async fn only_a_rejected_token_is_permanent() {
        let server = MockServer::start().await;

        for (status, permanent) in [(401, true), (403, true), (404, false), (500, false)] {
            server.reset().await;

            Mock::given(method("GET"))
                .respond_with(ResponseTemplate::new(status))
                .mount(&server)
                .await;

            let error = home_assistant(&server)
                .get_entity::<(), String>("sun.sun")
                .await
                .unwrap_err();

            assert_eq!(is_permanent(&error), permanent, "{status}: {error}");
        }
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
            .respond_with(json_response(r#"{"message": "Event fired."}"#))
            .expect(1)
            .mount(&server)
            .await;

        let ha = client(&server.uri(), true);
        let result = ha.send_event("open_rollershutters").await.unwrap();

        assert_eq!(result.message, "Event fired.");
    }
}
