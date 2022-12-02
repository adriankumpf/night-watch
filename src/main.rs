mod camera;
mod home_assistant;
mod sun;

use std::fmt;
use std::time::Duration;

use anyhow::Result;
use chrono::{offset::Utc, DateTime};
use clap::{crate_version, Parser};
use log::{debug, info, warn, LevelFilter};
use reqwest::Url;
use tokio::time;

use camera::Camera;
use home_assistant::HomeAssistant;
use sun::{Event, Sun};

#[derive(Parser, Debug)]
#[command(version = crate_version!())]
struct Args {
    /// Activates debug mode
    #[arg(short, long)]
    debug: bool,

    /// Tests the connection to HA and blocks until it is available
    #[arg(short, long)]
    test_connection: bool,

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

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
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
    env_logger::builder()
        .format_timestamp(None)
        .format_module_path(false)
        .filter(
            Some("night_watch"),
            if debug {
                LevelFilter::Debug
            } else {
                LevelFilter::Info
            },
        )
        .init();
}

async fn wait_for_homeassistant(camera: &Camera<'_>) -> Result<()> {
    let mut i = 0;

    loop {
        match camera.night_vision().await {
            Ok(_night_vision) => return Ok(()),
            Err(error) => {
                if let Some(reqwest_error) = error.downcast_ref::<reqwest::Error>() {
                    if reqwest_error.is_connect() {
                        let secs = 2u64.pow(i);
                        warn!("Home Assistant is not available. Retrying in {secs}s");
                        time::sleep(std::time::Duration::from_secs(secs)).await;
                        i += 1;
                        continue;
                    } else if let Some(reqwest::StatusCode::NOT_FOUND) = reqwest_error.status() {
                        let secs = 2u64.pow(i);
                        warn!("Camera '{camera}' is not available. Retrying in {secs}s");
                        time::sleep(std::time::Duration::from_secs(secs)).await;
                        i += 1;
                        continue;
                    }
                }

                return Err(error);
            }
        }
    }
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

    let ha = HomeAssistant::new(args.url, &args.token)?;
    let cam = Camera::new(&ha, source);
    let sun = Sun::new(&ha);

    if args.test_connection {
        wait_for_homeassistant(&cam).await?;
    }

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

            if let Ok(sleep_for) = (event_in - chrono::Duration::minutes(45)).to_std() {
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
