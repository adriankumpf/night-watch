mod camera;
mod home_assistant;
mod sun;

use std::time::Duration;

use anyhow::Result;
use chrono::{DateTime, offset::Utc};
use clap::{Parser, crate_version};
use reqwest::Url;
use tokio::time;
use tracing::{debug, info, warn};
use tracing_subscriber::filter::{EnvFilter, LevelFilter};
use tracing_subscriber::fmt;

use camera::Camera;
use home_assistant::HomeAssistant;
use sun::{Event, Sun};

#[derive(Parser, Debug)]
#[command(version = crate_version!())]
struct Args {
    /// Print debug logs
    #[arg(short, long)]
    debug: bool,

    /// Retry failed requests with increasing intervals between attempts (up to 2 minutes)
    #[arg(short, long)]
    retry: bool,

    /// Fetches the camera entity from an input_select element instead
    #[arg(short = 's', long)]
    from_select: bool,

    /// Polling interval (in seconds)
    #[arg(short = 'I', long, default_value = "30", display_order = 1)]
    interval: u16,

    /// Event sent to HA when the camera turns on night vision
    #[arg(
        short = 'N',
        long,
        default_value = "close_rollershutters",
        display_order = 2
    )]
    night_event: String,

    /// Event sent to HA when the camera turns off night vision
    #[arg(
        short = 'D',
        long,
        default_value = "open_rollershutters",
        display_order = 2
    )]
    day_event: String,

    /// Base URL of HA
    #[arg(short = 'U', long, default_value = "http://localhost:8123")]
    url: Url,

    /// Access token for HA
    #[arg(short = 'T', long, env = "TOKEN", hide_env_values = true)]
    token: String,

    /// Entity
    #[arg()]
    entity: String,
}

pub enum Source {
    Camera(String),
    Select(String),
}

impl std::fmt::Display for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Camera(source) => write!(f, "{}", source),
            Self::Select(source) => write!(f, "{}", source),
        }
    }
}

#[inline]
fn until(time: &DateTime<Utc>) -> chrono::Duration {
    *time - Utc::now()
}

fn init_logger(debug: bool) {
    let format = fmt::format().without_time().with_target(false).compact();

    let level = if debug {
        LevelFilter::DEBUG
    } else {
        LevelFilter::INFO
    };

    let filter = EnvFilter::builder()
        .with_default_directive(level.into())
        .parse_lossy("");

    tracing_subscriber::fmt()
        .event_format(format)
        .with_env_filter(filter)
        .init();
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    init_logger(args.debug);

    let source = if args.from_select {
        Source::Select(args.entity)
    } else {
        Source::Camera(args.entity)
    };

    let ha = HomeAssistant::new(args.url, &args.token, args.retry)?;
    let cam = Camera::new(&ha, source);
    let sun = Sun::new(&ha);

    let mut last_event = None;

    'main: loop {
        for event in &sun.next_events().await? {
            if let (&Some(Event::Sunset(_)), Event::Sunset(_))
            | (&Some(Event::Sunrise(_)), Event::Sunrise(_)) = (&last_event, event)
            {
                debug!("{event} was already handled!");
                continue;
            }

            let event_in = until(event);
            let event_in_hours = event_in.num_minutes() as f32 / 60.0;
            info!("Next {event} in {event_in_hours:.1} hours");

            if event_in.num_milliseconds() <= 0 {
                warn!("The {event} is in the past");
                time::sleep(Duration::from_secs(5)).await;
                continue 'main;
            }

            let fourtyfive_minutes = chrono::Duration::try_minutes(45).unwrap();

            if let Ok(sleep_for) = (event_in - fourtyfive_minutes).to_std() {
                time::sleep(sleep_for).await;
            }

            info!("{event} in {} min", until(event).num_minutes());

            let ha_event = 'wait_for_event: loop {
                let night_vision = cam.night_vision().await?;

                let (do_send, ha_event) = match event {
                    Event::Sunrise(_) => (!night_vision, &args.day_event),
                    Event::Sunset(_) => (night_vision, &args.night_event),
                };

                if do_send {
                    break 'wait_for_event ha_event;
                }

                time::sleep(Duration::from_secs(args.interval.into())).await;
            };

            let result = ha.send_event(ha_event).await?;
            let diff = -until(event).num_minutes();

            info!("{} [{:+}]", result.message, diff);

            last_event = Some(event.clone());
        }
    }
}
