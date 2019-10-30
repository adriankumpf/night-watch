mod camera;
mod home_assistant;
mod sun;

use std::sync::{Arc, RwLock};
use std::thread;
use std::time::Duration;

use anyhow::Result;
use chrono::prelude::*;
use log::{info, LevelFilter};
use reqwest::Url;
use rumqtt::{mqttoptions, MqttClient, MqttOptions, Notification, QoS};
use structopt::StructOpt;
use tokio::timer;

use camera::Camera;
use home_assistant::HomeAssistant;
use sun::{Event, Sun};

#[derive(StructOpt, Clone, Debug)]
#[structopt(name = "night-watch")]
struct Args {
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

    /// Activate debug mode
    #[structopt(short, long)]
    debug: bool,

    /// MQTT topic
    #[structopt(long, default_value = "nightwatch/settings/camera")]
    topic: String,

    /// MQTT host
    #[structopt(long, default_value = "localhost")]
    mqtt_host: String,

    /// MQTT port
    #[structopt(long, default_value = "1883")]
    mqtt_port: u16,

    /// MQTT username
    #[structopt(long)]
    mqtt_username: Option<String>,

    /// MQTT password
    #[structopt(long)]
    mqtt_password: Option<String>,
}

impl From<Args> for MqttOptions {
    fn from(args: Args) -> MqttOptions {
        let mut mqtt_options = MqttOptions::new("night-watch", args.mqtt_host, 1883);

        if let (Some(user), Some(pass)) = (args.mqtt_username, args.mqtt_password) {
            mqtt_options = mqtt_options
                .set_security_opts(mqttoptions::SecurityOptions::UsernamePassword(user, pass))
        };

        mqtt_options
    }
}

fn spawn_mqtt_client(camera: Arc<RwLock<String>>, args: Args) {
    let topic = args.topic.clone();
    let (mut mqtt_client, notifications) = MqttClient::start(args.into()).unwrap();
    mqtt_client.subscribe(topic, QoS::AtLeastOnce).unwrap();

    thread::spawn(move || {
        for notification in notifications {
            if let Notification::Publish(packet) = notification {
                let mut camera = camera.write().unwrap();
                *camera = String::from_utf8_lossy(&packet.payload).to_string();
                info!("Chaging camera to '{}'", camera);
            }
        }
    });
}

#[inline]
fn until(time: &DateTime<Utc>) -> chrono::Duration {
    *time - Utc::now()
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::from_args();

    env_logger::builder()
        .format_timestamp(None)
        .format_module_path(false)
        .filter_level(if args.debug {
            LevelFilter::Debug
        } else {
            LevelFilter::Info
        })
        .init();

    let camera = Arc::new(RwLock::new("default".to_string()));
    spawn_mqtt_client(camera.clone(), args.clone());

    let ha = HomeAssistant::new(args.url, &args.token)?;
    let cam = Camera::new(&ha);
    let sun = Sun::new(&ha);

    'events: loop {
        let next_event = sun.next().await;

        let (start, end, event) = match next_event {
            Event::Dusk { start, end } => (start, end, &args.night_event),
            Event::Dawn { start, end } => (start, end, &args.day_event),
        };

        let sleep_for = until(&start);
        let sleep_for_hours = sleep_for.num_minutes() as f32 / 60.0;
        info!("Next {} in {:.1} hours", next_event, sleep_for_hours);
        timer::delay_for(sleep_for.to_std()?).await;
        info!("{} in {} minutes", next_event, until(&start).num_minutes());

        loop {
            let night_vision = cam.night_vision(&camera.read().unwrap()).await?;

            if match next_event {
                Event::Dawn { start: _, end: _ } => !night_vision,
                Event::Dusk { start: _, end: _ } => night_vision,
            } {
                let result = ha.send_event(&event).await?;
                info!("{} [{:+}]", result.message, until(&end).num_minutes() * -1);
                continue 'events;
            }

            timer::delay_for(Duration::from_secs(30)).await;
        }
    }
}
