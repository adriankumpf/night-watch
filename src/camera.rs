use anyhow::Result;
use image::Pixel;
use log::debug;

use crate::home_assistant::HomeAssistant;

pub struct Camera<'a> {
    home_assistant: &'a HomeAssistant,
    entitiy: String,
}

impl<'a> Camera<'a> {
    pub fn new(home_assistant: &'a HomeAssistant, entitiy: String) -> Self {
        Self {
            home_assistant,
            entitiy,
        }
    }

    pub async fn night_vision(&self) -> Result<bool> {
        let image = self.home_assistant.get_image(&self.entitiy).await?;

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
        let night_vision = f < 0.001;

        debug!("{} – night_vision: {}", f, night_vision);

        Ok(night_vision)
    }
}
