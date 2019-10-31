mod camera;
mod home_assistant;
mod sun;

use std::time::Duration;

use anyhow::Result;
use chrono::{offset::Utc, DateTime};
use log::{info, warn, LevelFilter};
use reqwest::Url;
use structopt::StructOpt;
use tokio::timer;

use camera::Camera;
use home_assistant::HomeAssistant;
use sun::{Event, Sun};

#[derive(StructOpt, Clone, Debug)]
#[structopt(name = "night-watch")]
struct Args {
    /// Input select entity for the camera
    #[structopt(short, long, default_value = "night_watch")]
    select: String,

    /// Base URL of HA
    #[structopt(short, long, default_value = "http://localhost:8123")]
    url: Url,

    /// Access token for HA
    #[structopt(short, long, env = "TOKEN")]
    token: String,

    /// The event sent to HA when the camera turns on night vision
    #[structopt(short = "N", long, default_value = "close_rollershutters")]
    night_event: String,

    /// The event sent to HA when the camera turns off night vision
    #[structopt(short = "D", long, default_value = "open_rollershutters")]
    day_event: String,

    /// Polling interval (in seconds)
    #[structopt(short, long, default_value = "30")]
    interval: u16,

    /// Activates debug mode
    #[structopt(short, long)]
    debug: bool,
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

async fn wait_for_homeassistant(camera: &Camera<'_>) {
    let mut i = 0;
    loop {
        match camera.selected_camera().await {
            Err(_error) => {
                let secs = 2u64.pow(i);
                warn!("Home Assistant is not available. Retrying in {}s", secs);
                timer::delay_for(std::time::Duration::from_secs(secs)).await;
                i += 1;
            }
            Ok(camera) => break info!("Camera: {}", camera),
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::from_args();

    init_logger(args.debug);

    let ha = HomeAssistant::new(args.url, &args.token)?;
    let cam = Camera::new(&ha, &args.select);
    let sun = Sun::new(&ha);

    wait_for_homeassistant(&cam).await;

    'main: loop {
        for next_event in &sun.next_events().await? {
            let event_in = until(&next_event);
            let event_in_hours = event_in.num_minutes() as f32 / 60.0;
            info!("Next {} in {:.1} hours", next_event, event_in_hours);

            if let Ok(sleep_for) = (event_in - chrono::Duration::minutes(45)).to_std() {
                timer::delay_for(sleep_for).await;
            }

            info!("{} in {} min", next_event, until(&next_event).num_minutes());

            let ha_event = 'wait_for_event: loop {
                let night_vision = cam.night_vision().await?;

                let (do_send, ha_event) = match next_event {
                    Event::Sunrise(_) => (!night_vision, &args.day_event),
                    Event::Sunset(_) => (night_vision, &args.night_event),
                };

                if do_send {
                    break 'wait_for_event ha_event;
                }

                timer::delay_for(Duration::from_secs(args.interval.into())).await;
            };

            let result = ha.send_event(&ha_event).await?;
            let diff = -1 * until(&next_event).num_minutes();

            info!("{} [{:+}]", result.message, diff);
        }
    }
}
