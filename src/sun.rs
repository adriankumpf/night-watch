use std::{cmp, fmt};

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
    pub next_dawn: DateTime<Utc>,
    pub next_dusk: DateTime<Utc>,
}

#[derive(Ord, PartialOrd, Eq, PartialEq, Debug)]
pub enum Event {
    Dusk {
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    },
    Dawn {
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    },
}

impl fmt::Display for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Event::Dusk { start: _, end: _ } => write!(f, "Dusk"),
            Event::Dawn { start: _, end: _ } => write!(f, "Dawn"),
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

    pub async fn next(&self) -> Event {
        let sun: Entity<Attributes, State> = self.home_assistant.fetch_entity("sun.sun").await;

        let dawn = Event::Dawn {
            start: sun.attributes.next_dawn,
            end: sun.attributes.next_rising,
        };
        let dusk = Event::Dusk {
            start: sun.attributes.next_dusk,
            end: sun.attributes.next_setting,
        };

        cmp::min(dusk, dawn)
    }
}
