mod camera;
mod home_assistant;

use anyhow::Result;
use chrono::prelude::*;
use reqwest::Url;
use structopt::StructOpt;
use tokio::timer;

use camera::Camera;
use home_assistant::{EventResult, HomeAssistant, State};

#[derive(StructOpt, Debug)]
#[structopt(name = "night-watch")]
struct Args {
    /// The camera (HA entitiy)
    #[structopt(short, long)]
    camera: String,

    /// The HA url
    #[structopt(short, long, default_value = "http://localhost:8123")]
    url: Url,

    /// The access token for HA
    #[structopt(short, long, env = "TOKEN")]
    token: String,

    /// The close event
    #[structopt(long, default_value = "close_rollershutters")]
    night_event: String,

    /// The open event
    #[structopt(long, default_value = "open_rollershutters")]
    day_event: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::from_args();

    let ha = HomeAssistant::new(args.url, &args.token)?;
    let cam = Camera::new(&ha, args.camera);

    loop {
        let sun = ha.fetch_sun().await;

        let next_event = std::cmp::min(sun.attributes.next_dawn, sun.attributes.next_dusk);

        let (kind, time, event) = match sun.state {
            State::BelowHorizon => ("Sunrise", sun.attributes.next_rising, &args.day_event),
            State::AboveHorizon => ("Sunset", sun.attributes.next_setting, &args.night_event),
        };

        let sleep_for_hours = (time - Utc::now()).num_minutes() as f32 / 60.0;
        println!("Next {} in {:.1} hours", kind, sleep_for_hours);

        timer::delay_for((next_event - Utc::now()).to_std()?).await;

        println!("{} in {} minutes", kind, (time - Utc::now()).num_minutes());

        loop {
            let night_vision = cam.night_vision().await?;

            if match sun.state {
                State::BelowHorizon => !night_vision,
                State::AboveHorizon => night_vision,
            } {
                break;
            }

            timer::delay_for(std::time::Duration::from_secs(10)).await;
        }

        let EventResult { message } = ha.send_event(&event).await?;
        println!("{} [{:+}]", message, (Utc::now() - time).num_minutes());
    }
}
