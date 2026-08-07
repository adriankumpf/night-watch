use anyhow::Result;
use image::Rgb;
use serde::Deserialize;
use tracing::debug;

use crate::home_assistant::{Entity, HomeAssistant};

/// Mean spread between a pixel's colour channels, normalised to `0.0..=1.0`.
/// Below this the frame is effectively greyscale, i.e. the camera has switched
/// to infrared.
const NIGHT_VISION_THRESHOLD: f64 = 0.005;

/// Where the camera entity comes from.
pub enum Source {
    /// The camera entity itself.
    Camera(String),
    /// An `input_select` whose selected option names the camera entity.
    Select(String),
}

impl Source {
    pub fn new(entity: String, from_select: bool) -> Self {
        if from_select {
            Self::Select(entity)
        } else {
            Self::Camera(entity)
        }
    }
}

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
            Source::Select(select) => self.selected_camera(select).await?,
            Source::Camera(camera) => camera.clone(),
        };

        let image = self
            .home_assistant
            .get_camera_image(&format!("camera.{camera}"))
            .await?;

        let diff: u64 = image
            .pixels()
            .map(|&Rgb([r, g, b])| {
                u64::from(r.abs_diff(g)) + u64::from(r.abs_diff(b)) + u64::from(g.abs_diff(b))
            })
            .sum();

        let pixels = f64::from(image.width()) * f64::from(image.height());
        let f = diff as f64 / pixels / (255.0 * 3.0);
        let night_vision = f < NIGHT_VISION_THRESHOLD;

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
