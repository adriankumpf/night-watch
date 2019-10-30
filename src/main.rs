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
    /// Default camera
    #[structopt(short, long, default_value = "default")]
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

fn spawn_mqtt_client(camera: Arc<RwLock<String>>, args: Args) -> MqttClient {
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

    mqtt_client
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
        .filter(
            Some("night_watch"),
            if args.debug {
                LevelFilter::Debug
            } else {
                LevelFilter::Info
            },
        )
        .init();

    let camera = Arc::new(RwLock::new(args.camera.clone()));
    let _mqtt_client = spawn_mqtt_client(camera.clone(), args.clone());

    let ha = HomeAssistant::new(args.url, &args.token)?;
    let cam = Camera::new(&ha);
    let sun = Sun::new(&ha);

    'events: loop {
        let next_event = sun.next().await;

        let event_in = until(&next_event);
        let event_in_hours = event_in.num_minutes() as f32 / 60.0;
        info!("Next {} in {:.1} hours", next_event, event_in_hours);

        if let Ok(sleep_for) = (event_in - chrono::Duration::minutes(45)).to_std() {
            timer::delay_for(sleep_for).await;
        }

        info!("{} in {} min", next_event, until(&next_event).num_minutes());

        loop {
            let night_vision = cam.night_vision(&camera.read().unwrap()).await?;

            if match next_event {
                Event::Sunrise(_) => !night_vision,
                Event::Sunset(_) => night_vision,
            } {
                let event = match next_event {
                    Event::Sunset(_) => &args.night_event,
                    Event::Sunrise(_) => &args.day_event,
                };

                let result = ha.send_event(&event).await?;
                info!(
                    "{} [{:+}]",
                    result.message,
                    until(&next_event).num_minutes() * -1
                );
                continue 'events;
            }

            timer::delay_for(Duration::from_secs(30)).await;
        }
    }
}
