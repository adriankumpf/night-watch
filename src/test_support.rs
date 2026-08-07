//! Helpers shared by the unit tests.

use std::io::Cursor;

use image::{ImageFormat, Rgb, RgbImage};
use wiremock::{MockServer, ResponseTemplate};

use crate::home_assistant::HomeAssistant;

pub const TOKEN: &str = "s3cr3t";

/// A client for `base`, retrying transient failures if `retry`.
pub fn client(base: &str, retry: bool) -> HomeAssistant {
    HomeAssistant::new(base.parse().expect("a valid base URL"), TOKEN, retry).unwrap()
}

/// A client for `server`, without retries.
pub fn home_assistant(server: &MockServer) -> HomeAssistant {
    client(&server.uri(), false)
}

/// An image in which every pixel has the same colour.
pub fn solid(width: u32, height: u32, colour: [u8; 3]) -> RgbImage {
    RgbImage::from_pixel(width, height, Rgb(colour))
}

/// A `200 application/json` response carrying `body`.
pub fn json_response(body: impl Into<String>) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_raw(body.into(), "application/json")
}

/// `image` served as JPEG, the way HA's camera proxy serves it.
pub fn jpeg_response(image: &RgbImage) -> ResponseTemplate {
    let mut buffer = Cursor::new(Vec::new());

    image
        .write_to(&mut buffer, ImageFormat::Jpeg)
        .expect("encoding a JPEG");

    ResponseTemplate::new(200).set_body_raw(buffer.into_inner(), "image/jpeg")
}
