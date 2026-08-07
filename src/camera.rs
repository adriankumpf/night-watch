use anyhow::Result;
use image::{Rgb, RgbImage};
use serde::Deserialize;
use tracing::debug;

use crate::home_assistant::{Entity, HomeAssistant};

/// Below this [`colour_spread`] the frame is effectively greyscale, i.e. the
/// camera has switched to infrared.
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

        let spread = colour_spread(&image);
        let night_vision = spread < NIGHT_VISION_THRESHOLD;

        debug!("{camera}.night_vision={night_vision} ({spread:.8})");

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

/// The mean spread between a pixel's colour channels, normalised by the largest
/// spread a single pixel can have. `0.0` for a perfectly greyscale image.
fn colour_spread(image: &RgbImage) -> f64 {
    let diff: u64 = image
        .pixels()
        .map(|&Rgb([r, g, b])| {
            u64::from(r.abs_diff(g)) + u64::from(r.abs_diff(b)) + u64::from(g.abs_diff(b))
        })
        .sum();

    let pixels = f64::from(image.width()) * f64::from(image.height());

    diff as f64 / pixels / (255.0 * 3.0)
}

#[cfg(test)]
mod tests {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;
    use crate::test_support::{home_assistant, jpeg, solid};

    /// The largest spread a single pixel can have: `|255-0| + |255-0| + |0-0|`.
    const SATURATED: f64 = 510.0 / (255.0 * 3.0);

    fn jpeg_response(image: RgbImage) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_raw(jpeg(image), "image/jpeg")
    }

    #[test]
    fn a_grey_frame_has_no_colour_spread() {
        assert_eq!(colour_spread(&solid(8, 8, [17, 17, 17])), 0.0);
    }

    #[test]
    fn colour_spread_does_not_depend_on_the_frame_size() {
        // Large enough that the sum of the per-pixel spreads (~6.1e9) no longer
        // fits into a u32.
        let large = colour_spread(&solid(4000, 3000, [255, 0, 0]));

        assert_eq!(large, colour_spread(&solid(2, 2, [255, 0, 0])));
        assert_eq!(large, SATURATED);
    }

    #[test]
    fn a_few_coloured_pixels_do_not_hide_night_vision() {
        /// A grey 100x100 frame with `n` fully saturated pixels in it.
        fn speckled(n: u32) -> RgbImage {
            let mut image = solid(100, 100, [17, 17, 17]);

            for x in 0..n {
                image.put_pixel(x, 0, Rgb([255, 0, 0]));
            }

            image
        }

        // Of the 10_000 pixels, 75 saturated ones are worth exactly the
        // threshold: 75 * (510 / 765) / 10_000 == 0.005.
        assert!(colour_spread(&speckled(74)) < NIGHT_VISION_THRESHOLD);
        assert!(colour_spread(&speckled(76)) >= NIGHT_VISION_THRESHOLD);
    }

    #[tokio::test]
    async fn detects_night_vision_from_the_camera_entity() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/api/camera_proxy/camera.front_door"))
            .respond_with(jpeg_response(solid(64, 64, [40, 40, 40])))
            .expect(1)
            .mount(&server)
            .await;

        let ha = home_assistant(&server);
        let camera = Camera::new(&ha, Source::new("front_door".into(), false));

        assert!(camera.night_vision().await.unwrap());
    }

    #[tokio::test]
    async fn resolves_the_camera_through_an_input_select() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/api/states/input_select.cameras"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                r#"{
                     "entity_id": "input_select.cameras",
                     "state": "Front_Door",
                     "attributes": {
                       "options": ["Front_Door", "Garden"],
                       "editable": true,
                       "friendly_name": "Cameras"
                     },
                     "last_changed": "2024-04-20T18:09:00.000000+00:00"
                   }"#,
                "application/json",
            ))
            .expect(1)
            .mount(&server)
            .await;

        // The selected option names the camera, lowercased and prefixed.
        Mock::given(method("GET"))
            .and(path("/api/camera_proxy/camera.front_door"))
            .respond_with(jpeg_response(solid(64, 64, [200, 30, 30])))
            .expect(1)
            .mount(&server)
            .await;

        let ha = home_assistant(&server);
        let camera = Camera::new(&ha, Source::new("cameras".into(), true));

        assert!(!camera.night_vision().await.unwrap());
    }
}
