use std::fmt;

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
}

impl fmt::Display for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Sunset(_) => "Sunset",
            Self::Sunrise(_) => "Sunrise",
        })
    }
}

pub struct Sun<'a> {
    home_assistant: &'a HomeAssistant,
}

impl<'a> Sun<'a> {
    pub fn new(home_assistant: &'a HomeAssistant) -> Self {
        Self { home_assistant }
    }

    pub async fn next_events(&self) -> Result<[Event; 2]> {
        let sun: Entity<Attributes, State> = self.home_assistant.get_entity("sun.sun").await?;

        let sunset = Event::Sunset(sun.attributes.next_setting);
        let sunrise = Event::Sunrise(sun.attributes.next_rising);

        let events = match sun.state {
            State::AboveHorizon => [sunset, sunrise],
            State::BelowHorizon => [sunrise, sunset],
        };

        debug!("Next events: {events:#?}");

        Ok(events)
    }
}
