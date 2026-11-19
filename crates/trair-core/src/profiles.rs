// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

//! Profiles between contacts: name, glow colour, picture, "about me" and
//! badges travel end to end encrypted inside the chat (see
//! [`crate::chats::Messenger::share_profile`]).
//!
//! What arrives is checked here before it is stored: sizes, text lengths
//! and the picture. The picture is decoded with a memory limit, must be at
//! most [`MAX_PICTURE_SIDE`] pixels each way and is written anew as a plain
//! RGBA PNG, so no crafted file reaches the app's image decoder and no
//! hidden chunks (text, location, colour profiles) are kept.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::badges::MAX_BADGES;
use crate::error::{CoreError, Result};

/// Version of the profile inside a message.
pub const PROFILE_VERSION: u8 = 1;
/// Longest chosen name, in characters.
pub const MAX_NAME_CHARS: usize = 64;
/// Longest "about me", in characters.
pub const MAX_ABOUT_CHARS: usize = 500;
/// Largest picture file, in bytes (a 128 × 128 PNG is far smaller).
pub const MAX_PICTURE_BYTES: usize = 96 * 1024;
/// Largest picture side, in pixels.
pub const MAX_PICTURE_SIDE: u32 = 128;
/// Largest badge, as JSON bytes.
pub const MAX_BADGE_BYTES: usize = 2048;
/// Memory the PNG decoder may use.
const DECODE_LIMIT: usize = 1024 * 1024;

/// A profile as it travels.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Wire {
    /// Always [`PROFILE_VERSION`].
    pub v: u8,
    /// Newer profiles have a higher number; older ones are ignored.
    pub rev: u64,
    /// The name the person chose.
    pub name: String,
    /// Glow colour (ARGB), if any.
    #[serde(default)]
    pub glow: Option<u32>,
    /// "About me".
    #[serde(default)]
    pub about: String,
    /// Picture, PNG, base64.
    #[serde(default)]
    pub picture: Option<String>,
    /// Signed badges, as issued (checked when shown, they expire).
    #[serde(default)]
    pub badges: Vec<Value>,
}

/// A checked profile, ready to store or to send.
#[derive(Debug, Clone, PartialEq)]
pub struct Checked {
    /// See [`Wire::rev`].
    pub rev: u64,
    /// See [`Wire::name`] (trimmed).
    pub name: String,
    /// See [`Wire::glow`].
    pub glow: Option<u32>,
    /// See [`Wire::about`] (trimmed).
    pub about: String,
    /// The picture, written anew.
    pub picture: Option<Vec<u8>>,
    /// See [`Wire::badges`].
    pub badges: Vec<Value>,
}

fn refused(why: &str) -> CoreError {
    CoreError::Rejected(format!("profile: {why}"))
}

/// Check a profile from its JSON form.
pub fn check(value: &Value) -> Result<Checked> {
    let w: Wire = serde_json::from_value(value.clone()).map_err(|_| refused("unreadable"))?;
    if w.v != PROFILE_VERSION {
        return Err(refused("unknown version"));
    }
    if w.rev == 0 {
        return Err(refused("no version number"));
    }
    let name = w.name.trim().to_string();
    if name.is_empty()
        || name.chars().count() > MAX_NAME_CHARS
        || name.chars().any(char::is_control)
    {
        return Err(refused("name"));
    }
    let about = w.about.trim().to_string();
    if about.chars().count() > MAX_ABOUT_CHARS || about.chars().any(|c| c.is_control() && c != '\n')
    {
        return Err(refused("about"));
    }
    if w.badges.len() > MAX_BADGES
        || w.badges.iter().any(|b| {
            !b.is_object() || serde_json::to_vec(b).map_or(true, |j| j.len() > MAX_BADGE_BYTES)
        })
    {
        return Err(refused("badges"));
    }
    let picture = match &w.picture {
        None => None,
        Some(b64) => {
            if b64.len() > MAX_PICTURE_BYTES.div_ceil(3) * 4 {
                return Err(refused("picture too large"));
            }
            let raw = STANDARD.decode(b64).map_err(|_| refused("picture"))?;
            Some(clean_picture(&raw)?)
        }
    };
    Ok(Checked {
        rev: w.rev,
        name,
        glow: w.glow,
        about,
        picture,
        badges: w.badges,
    })
}

impl Checked {
    /// The JSON form, as it travels.
    pub fn to_wire(&self) -> Value {
        serde_json::to_value(Wire {
            v: PROFILE_VERSION,
            rev: self.rev,
            name: self.name.clone(),
            glow: self.glow,
            about: self.about.clone(),
            picture: self.picture.as_ref().map(|p| STANDARD.encode(p)),
            badges: self.badges.clone(),
        })
        .unwrap_or(Value::Null)
    }
}

/// Decode a PNG with limits and write it anew: 8-bit RGBA, no other
/// chunks. Anything else (other formats, animations, too large) is refused.
pub fn clean_picture(bytes: &[u8]) -> Result<Vec<u8>> {
    if bytes.len() > MAX_PICTURE_BYTES {
        return Err(refused("picture too large"));
    }
    let mut decoder = png::Decoder::new_with_limits(
        std::io::Cursor::new(bytes),
        png::Limits {
            bytes: DECODE_LIMIT,
        },
    );
    decoder.set_transformations(
        png::Transformations::normalize_to_color8() | png::Transformations::ALPHA,
    );
    let mut reader = decoder
        .read_info()
        .map_err(|_| refused("picture is no PNG"))?;
    let (width, height) = (reader.info().width, reader.info().height);
    if width == 0 || height == 0 || width > MAX_PICTURE_SIDE || height > MAX_PICTURE_SIDE {
        return Err(refused("picture size"));
    }
    if reader.info().is_animated() {
        return Err(refused("animated picture"));
    }
    let size = reader
        .output_buffer_size()
        .ok_or_else(|| refused("picture size"))?;
    let mut buf = vec![0u8; size];
    let frame = reader
        .next_frame(&mut buf)
        .map_err(|_| refused("picture is damaged"))?;
    let pixels = &buf[..frame.buffer_size()];
    let rgba: Vec<u8> = match (frame.color_type, frame.bit_depth) {
        (png::ColorType::Rgba, png::BitDepth::Eight) => pixels.to_vec(),
        (png::ColorType::GrayscaleAlpha, png::BitDepth::Eight) => pixels
            .chunks_exact(2)
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        _ => return Err(refused("picture colours")),
    };
    if rgba.len() != (width as usize) * (height as usize) * 4 {
        return Err(refused("picture is damaged"));
    }
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|_| refused("picture"))?;
        writer
            .write_image_data(&rgba)
            .map_err(|_| refused("picture"))?;
        writer.finish().map_err(|_| refused("picture"))?;
    }
    Ok(out)
}
