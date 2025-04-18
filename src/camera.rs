use std::fmt;

use anyhow::Result;
use image::Pixel;
use serde::Deserialize;
use tracing::debug;

use crate::Source;
use crate::home_assistant::{Entity, HomeAssistant};

#[derive(Debug, Deserialize)]
struct Attributes {
    options: Vec<String>,
}

pub struct Camera<'a> {
    home_assistant: &'a HomeAssistant,
    source: Source,
}

impl fmt::Display for Camera<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.source)
    }
}

impl<'a> Camera<'a> {
    pub fn new(home_assistant: &'a HomeAssistant, source: Source) -> Self {
        Self {
            home_assistant,
            source,
        }
    }

    pub async fn night_vision(&self) -> Result<bool> {
        let camera = match &self.source {
            Source::Select(select) => self.selected_camera(select).await?,
            Source::Camera(camera) => camera.clone(),
        };

        let image = self.home_assistant.get_camera_image(&camera).await?;

        let mut diff = 0;

        for p in image.pixels() {
            let channels = p.channels();
            let (r, g, b) = (channels[0], channels[1], channels[2]);

            let rg = ((r as i32) - (g as i32)).unsigned_abs();
            let rb = ((r as i32) - (b as i32)).unsigned_abs();
            let gb = ((g as i32) - (b as i32)).unsigned_abs();

            diff += rg + rb + gb;
        }

        let f = (diff as f64) / (image.width() * image.height()) as f64 / (255.0 * 3.0);
        let night_vision = f < 0.005;

        debug!("{camera}.night_vision={night_vision} ({f:.8})");

        Ok(night_vision)
    }

    async fn selected_camera(&self, select: &str) -> Result<String> {
        let select: Entity<Attributes, String> = self
            .home_assistant
            .get_entity(&format!("input_select.{select}"))
            .await?;

        debug!("Select options: {:?}", select.attributes.options);

        Ok(select.state.to_lowercase())
    }
}
