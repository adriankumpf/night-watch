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
