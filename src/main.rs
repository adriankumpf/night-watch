mod camera;
mod home_assistant;
mod sun;

use std::time::Duration;

use anyhow::Result;
use chrono::{TimeDelta, offset::Utc};
use clap::{Parser, crate_version};
use reqwest::Url;
use tokio::time;
use tracing::{debug, info, warn};
use tracing_subscriber::filter::{EnvFilter, LevelFilter};
use tracing_subscriber::fmt;

use camera::{Camera, Source};
use home_assistant::HomeAssistant;
use sun::Event;

/// How long before a predicted sun event to start polling the camera.
const LEAD_TIME: TimeDelta = TimeDelta::minutes(45);

/// How long to back off when HA reports an event that has already passed.
const STALE_EVENT_BACKOFF: Duration = Duration::from_secs(5);

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

    let ha = HomeAssistant::new(args.url, &args.token, args.retry)?;
    let cam = Camera::new(&ha, Source::new(args.entity, args.from_select));

    let mut last_event: Option<Event> = None;

    'main: loop {
        for event in sun::next_events(&ha).await? {
            if last_event.is_some_and(|last| last.same_kind(event)) {
                debug!("{event} was already handled!");
                continue;
            }

            let event_in = event.at() - Utc::now();

            info!(
                "Next {event} in {:.1} hours",
                event_in.num_minutes() as f32 / 60.0
            );

            if event_in <= TimeDelta::zero() {
                warn!("The {event} is in the past");
                time::sleep(STALE_EVENT_BACKOFF).await;
                continue 'main;
            }

            if let Ok(sleep_for) = (event_in - LEAD_TIME).to_std() {
                time::sleep(sleep_for).await;
            }

            info!("{event} in {} min", (event.at() - Utc::now()).num_minutes());

            let ha_event = loop {
                let night_vision = cam.night_vision().await?;

                match (event, night_vision) {
                    (Event::Sunrise(_), false) => break &args.day_event,
                    (Event::Sunset(_), true) => break &args.night_event,
                    _ => time::sleep(Duration::from_secs(args.interval.into())).await,
                }
            };

            let result = ha.send_event(ha_event).await?;
            let late = (Utc::now() - event.at()).num_minutes();

            info!("{} [{late:+}]", result.message);

            last_event = Some(event);
        }
    }
}
