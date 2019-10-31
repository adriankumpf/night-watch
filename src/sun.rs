use std::fmt;
use std::ops::Deref;

use anyhow::Result;
use chrono::{offset::Utc, DateTime};
use serde::Deserialize;

use crate::home_assistant::{Entity, HomeAssistant};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum State {
    BelowHorizon,
    AboveHorizon,
}

#[derive(Debug, Deserialize)]
struct Attributes {
    pub next_rising: DateTime<Utc>,
    pub next_setting: DateTime<Utc>,
}

#[derive(Ord, PartialOrd, Eq, PartialEq, Debug)]
pub enum Event {
    Sunset(DateTime<Utc>),
    Sunrise(DateTime<Utc>),
}

impl Deref for Event {
    type Target = DateTime<Utc>;

    fn deref(&self) -> &Self::Target {
        match *self {
            Event::Sunset(ref dt) => dt,
            Event::Sunrise(ref dt) => dt,
        }
    }
}

impl fmt::Display for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Event::Sunset(_) => write!(f, "Sunset"),
            Event::Sunrise(_) => write!(f, "Sunrise"),
        }
    }
}

pub struct Sun<'a> {
    home_assistant: &'a HomeAssistant,
}

impl<'a> Sun<'a> {
    pub fn new(home_assistant: &'a HomeAssistant) -> Self {
        Self { home_assistant }
    }

    pub async fn next(&self) -> Result<Event> {
        let sun: Entity<Attributes, State> = self.home_assistant.get_entity("sun.sun").await?;

        let event = match sun.state {
            State::AboveHorizon => Event::Sunset(sun.attributes.next_setting),
            State::BelowHorizon => Event::Sunrise(sun.attributes.next_rising),
        };

        Ok(event)
    }
}
