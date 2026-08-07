use std::fmt;
use std::mem;

use anyhow::Result;
use chrono::{DateTime, offset::Utc};
use serde::Deserialize;
use tracing::debug;

use crate::home_assistant::{Entity, HomeAssistant};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum State {
    BelowHorizon,
    AboveHorizon,
}

#[derive(Debug, Deserialize)]
struct Attributes {
    next_rising: DateTime<Utc>,
    next_setting: DateTime<Utc>,
}

#[derive(Clone, Copy, Debug)]
pub enum Event {
    Sunset(DateTime<Utc>),
    Sunrise(DateTime<Utc>),
}

impl Event {
    /// The time this event occurs at.
    pub const fn at(self) -> DateTime<Utc> {
        let (Self::Sunset(at) | Self::Sunrise(at)) = self;
        at
    }

    /// Whether both events are sunrises or both are sunsets, ignoring their times.
    pub fn same_kind(self, other: Self) -> bool {
        mem::discriminant(&self) == mem::discriminant(&other)
    }
}

impl fmt::Display for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Sunset(_) => "Sunset",
            Self::Sunrise(_) => "Sunrise",
        })
    }
}

/// The next sunset and sunrise, in the order they will occur.
pub async fn next_events(home_assistant: &HomeAssistant) -> Result<[Event; 2]> {
    let sun: Entity<Attributes, State> = home_assistant.get_entity("sun.sun").await?;

    let sunset = Event::Sunset(sun.attributes.next_setting);
    let sunrise = Event::Sunrise(sun.attributes.next_rising);

    let events = match sun.state {
        State::AboveHorizon => [sunset, sunrise],
        State::BelowHorizon => [sunrise, sunset],
    };

    debug!("Next events: {events:#?}");

    Ok(events)
}

#[cfg(test)]
mod tests {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer};

    use super::*;
    use crate::test_support::{home_assistant, json_response};

    /// The attributes HA reports on `sun.sun`; only the next rising and setting
    /// are read, the rest are here to keep the payload realistic.
    const ATTRIBUTES: &str = r#"{
        "next_dawn": "2024-04-20T04:42:11.101010+00:00",
        "next_dusk": "2024-04-20T19:41:33.404040+00:00",
        "next_midnight": "2024-04-21T00:11:52.505050+00:00",
        "next_noon": "2024-04-20T12:11:41.606060+00:00",
        "next_rising": "2024-04-20T05:19:28.202020+00:00",
        "next_setting": "2024-04-20T19:04:16.303030+00:00",
        "elevation": 34.19,
        "friendly_name": "Sun"
    }"#;

    fn at(rfc3339: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(rfc3339).unwrap().to_utc()
    }

    /// A `sun.sun` state as HA serves it, with the sun currently `state`.
    async fn serve_sun(state: &str) -> MockServer {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/api/states/sun.sun"))
            .respond_with(json_response(format!(
                r#"{{"state": "{state}", "attributes": {ATTRIBUTES}}}"#
            )))
            .mount(&server)
            .await;

        server
    }

    #[test]
    fn events_of_the_same_kind_are_equivalent_regardless_of_time() {
        let sunset = Event::Sunset(at("2024-04-20T19:04:16Z"));
        let later = Event::Sunset(at("2024-04-21T19:05:44Z"));
        let sunrise = Event::Sunrise(at("2024-04-20T05:19:28Z"));

        assert!(sunset.same_kind(later));
        assert!(!sunset.same_kind(sunrise));
    }

    #[test]
    fn events_are_displayed_by_kind() {
        let now = Utc::now();

        assert_eq!(Event::Sunset(now).to_string(), "Sunset");
        assert_eq!(Event::Sunrise(now).to_string(), "Sunrise");
    }

    #[tokio::test]
    async fn the_sunset_comes_first_while_the_sun_is_up() {
        let server = serve_sun("above_horizon").await;

        let [first, second] = next_events(&home_assistant(&server)).await.unwrap();

        assert!(matches!(first, Event::Sunset(_)));
        assert_eq!(first.at(), at("2024-04-20T19:04:16.303030+00:00"));
        assert!(matches!(second, Event::Sunrise(_)));
        assert_eq!(second.at(), at("2024-04-20T05:19:28.202020+00:00"));
    }

    #[tokio::test]
    async fn the_sunrise_comes_first_while_the_sun_is_down() {
        let server = serve_sun("below_horizon").await;

        let [first, second] = next_events(&home_assistant(&server)).await.unwrap();

        assert!(matches!(first, Event::Sunrise(_)));
        assert!(matches!(second, Event::Sunset(_)));
    }
}
