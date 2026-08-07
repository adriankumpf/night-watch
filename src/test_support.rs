//! Helpers shared by the unit tests.

use std::io::Cursor;

use image::{DynamicImage, ImageFormat, Rgb, RgbImage};
use wiremock::MockServer;

use crate::home_assistant::HomeAssistant;

pub const TOKEN: &str = "s3cr3t";

/// A client for `server`, without retries.
pub fn home_assistant(server: &MockServer) -> HomeAssistant {
    HomeAssistant::new(server.uri().parse().unwrap(), TOKEN, false).unwrap()
}

/// An image in which every pixel has the same colour.
pub fn solid(width: u32, height: u32, colour: [u8; 3]) -> RgbImage {
    RgbImage::from_pixel(width, height, Rgb(colour))
}

/// `image` encoded as JPEG, the way HA's camera proxy serves it.
pub fn jpeg(image: RgbImage) -> Vec<u8> {
    let mut buffer = Cursor::new(Vec::new());

    DynamicImage::ImageRgb8(image)
        .write_to(&mut buffer, ImageFormat::Jpeg)
        .expect("encoding a JPEG");

    buffer.into_inner()
}
