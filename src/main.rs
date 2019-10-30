mod client;

use anyhow::Result;
use chrono::prelude::*;
use image::{Pixel, RgbImage};
use reqwest::Url;
use structopt::StructOpt;
use tokio::timer;

use client::{EventResult, HomeAssistant, State};

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
    close_event: String,

    /// The open event
    #[structopt(long, default_value = "open_rollershutters")]
    open_event: String,
}

fn is_grayscale(image: RgbImage) -> bool {
    let mut diff = 0;

    for p in image.pixels() {
        let channels = p.channels();
        let (r, g, b) = (channels[0], channels[1], channels[2]);

        let rg = ((r as i32) - (g as i32)).abs() as u32;
        let rb = ((r as i32) - (b as i32)).abs() as u32;
        let gb = ((g as i32) - (b as i32)).abs() as u32;

        diff += rg + rb + gb;
    }

    let f = (diff as f64) / (image.width() * image.height()) as f64 / (255.0 * 3.0);
    let is_grayscale = f < 0.001;

    println!("{} – is_grayscale: {}", f, is_grayscale);

    is_grayscale
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::from_args();

    let ha = HomeAssistant::new(args.url, &args.token)?;

    loop {
        let sun = ha.fetch_sun().await;

        let next_event = std::cmp::min(sun.attributes.next_dawn, sun.attributes.next_dusk);

        let (kind, time, event) = match sun.state {
            State::BelowHorizon => ("Sunrise", sun.attributes.next_rising, &args.open_event),
            State::AboveHorizon => ("Sunset", sun.attributes.next_setting, &args.close_event),
        };

        let sleep_for_hours = (time - Utc::now()).num_minutes() as f32 / 60.0;
        println!("Next {} in {:.1} hours", kind, sleep_for_hours);

        timer::delay_for((next_event - Utc::now()).to_std()?).await;

        println!("{} in {} minutes", kind, (time - Utc::now()).num_minutes());

        loop {
            let image = ha.get_image(&args.camera).await?;

            if match sun.state {
                State::BelowHorizon => !is_grayscale(image),
                State::AboveHorizon => is_grayscale(image),
            } {
                break;
            }

            timer::delay_for(std::time::Duration::from_secs(10)).await;
        }

        let EventResult { message } = ha.send_event(&event).await?;
        println!("{} [{:+}]", message, (Utc::now() - time).num_minutes());
    }
}
