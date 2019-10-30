use std::cmp;
use std::fmt;

use anyhow::Result;
use chrono::{offset::Utc, DateTime};

use crate::home_assistant::HomeAssistant;

pub struct Sun<'a> {
    home_assistant: &'a HomeAssistant,
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

impl<'a> Sun<'a> {
    pub fn new(home_assistant: &'a HomeAssistant) -> Self {
        Self { home_assistant }
    }

    pub async fn next(&self) -> Result<Event> {
        let sun = self.home_assistant.fetch_sun().await;

        let dawn = Event::Dawn {
            start: sun.attributes.next_dawn,
            end: sun.attributes.next_rising,
        };
        let dusk = Event::Dusk {
            start: sun.attributes.next_dusk,
            end: sun.attributes.next_setting,
        };

        Ok(cmp::min(dusk, dawn))
    }
}
