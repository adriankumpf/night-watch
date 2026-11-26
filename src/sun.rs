use std::fmt;
use std::mem;

use anyhow::Result;
use chrono::{DateTime, TimeDelta, offset::Utc};
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

/// HA's `sun.sun` entity at the time it was fetched.
pub struct Sun(Entity<Attributes, State>);

impl Sun {
    pub async fn fetch(home_assistant: &HomeAssistant) -> Result<Self> {
        Ok(Self(home_assistant.get_entity("sun.sun").await?))
    }

    /// The next sunset and sunrise, in the order they will occur.
    pub fn next_events(&self) -> [Event; 2] {
        let sunset = Event::Sunset(self.0.attributes.next_setting);
        let sunrise = Event::Sunrise(self.0.attributes.next_rising);

        let events = match self.0.state {
            State::AboveHorizon => [sunset, sunrise],
            State::BelowHorizon => [sunrise, sunset],
        };

        debug!("Next events: {events:#?}");

        events
    }

    /// The sun event that last occurred, if that was no more than `window`
    /// before `now`.
    ///
    /// HA only reports upcoming events, so the time is estimated as a day
    /// before the next one of the same kind. That is a few minutes off as the
    /// days lengthen or shorten, which is good enough to tell a recent event
    /// from an old one. Near the poles, where the next one can be days away,
    /// the estimate lands in the future and nothing is returned.
    pub fn previous_event(&self, now: DateTime<Utc>, window: TimeDelta) -> Option<Event> {
        let event = match self.0.state {
            State::AboveHorizon => {
                Event::Sunrise(self.0.attributes.next_rising - TimeDelta::days(1))
            }
            State::BelowHorizon => {
                Event::Sunset(self.0.attributes.next_setting - TimeDelta::days(1))
            }
        };

        let ago = now - event.at();

        (ago >= TimeDelta::zero() && ago <= window).then_some(event)
    }
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

    async fn fetch(state: &str) -> Sun {
        let server = serve_sun(state).await;

        Sun::fetch(&home_assistant(&server)).await.unwrap()
    }

    #[tokio::test]
    async fn the_sunset_comes_first_while_the_sun_is_up() {
        let [first, second] = fetch("above_horizon").await.next_events();

        assert!(matches!(first, Event::Sunset(_)));
        assert_eq!(first.at(), at("2024-04-20T19:04:16.303030+00:00"));
        assert!(matches!(second, Event::Sunrise(_)));
        assert_eq!(second.at(), at("2024-04-20T05:19:28.202020+00:00"));
    }

    #[tokio::test]
    async fn the_sunrise_comes_first_while_the_sun_is_down() {
        let [first, second] = fetch("below_horizon").await.next_events();

        assert!(matches!(first, Event::Sunrise(_)));
        assert!(matches!(second, Event::Sunset(_)));
    }

    #[tokio::test]
    async fn the_previous_sunset_is_a_day_before_the_next_one() {
        let sun = fetch("below_horizon").await;
        let window = TimeDelta::hours(2);

        // The next setting is 2024-04-20T19:04:16, so the last one was at
        // about 2024-04-19T19:04:16.
        let previous = sun.previous_event(at("2024-04-19T20:00:00Z"), window);

        assert!(matches!(previous, Some(Event::Sunset(_))));
        assert_eq!(
            previous.unwrap().at(),
            at("2024-04-19T19:04:16.303030+00:00")
        );
    }

    #[tokio::test]
    async fn the_previous_sunrise_is_reported_while_the_sun_is_up() {
        let sun = fetch("above_horizon").await;
        let previous = sun.previous_event(at("2024-04-19T05:30:00Z"), TimeDelta::hours(2));

        assert!(matches!(previous, Some(Event::Sunrise(_))));
    }

    #[tokio::test]
    async fn an_event_outside_the_window_is_not_reported() {
        let sun = fetch("below_horizon").await;
        let window = TimeDelta::hours(2);

        assert!(
            sun.previous_event(at("2024-04-19T21:05:00Z"), window)
                .is_none()
        );
        // Before the estimate, e.g. a polar day where the next setting is
        // further out than a day.
        assert!(
            sun.previous_event(at("2024-04-19T19:00:00Z"), window)
                .is_none()
        );
    }
}
