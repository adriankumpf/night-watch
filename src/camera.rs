use anyhow::Result;
use image::Pixel;
use log::debug;
use serde::Deserialize;

use crate::home_assistant::{Entity, HomeAssistant};
use crate::Source;

#[derive(Debug, Deserialize)]
struct Attributes {
    options: Vec<String>,
}

pub struct Camera<'a> {
    home_assistant: &'a HomeAssistant,
    source: Source,
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
            Source::Select(select) => self.selected_camera(&select).await?,
            Source::Camera(camera) => camera.clone(), // TODO
        };

        let image = self.home_assistant.get_camera_image(&camera).await?;

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
        let night_vision = f < 0.005;

        debug!("{}.night_vision={} ({:.8})", camera, night_vision, f);

        Ok(night_vision)
    }

    async fn selected_camera(&self, select: &String) -> Result<String> {
        let select: Entity<Attributes, String> = self
            .home_assistant
            .get_entity(&format!("input_select.{}", select))
            .await?;

        Ok(select.state.to_lowercase())
    }
}
