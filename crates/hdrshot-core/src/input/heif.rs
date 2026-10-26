//! HEIC input: iPhone photos and the HEIF files of cameras, through the
//! pure-Rust `heic-decoder`.
//!
//! iPhone photos are SDR pictures with a gain map: Apple's own, an
//! auxiliary image of type `urn:com:apple:photo:2020:aux:hdrgainmap`, and
//! since iOS 18 also an ISO 21496-1 `tmap` item. Cameras such as Canon,
//! Nikon and Sony save HDR HEIFs in PQ or HLG instead, marked in the `colr`
//! box like an HDR PNG's `cICP` chunk.
//!
//! The decoder reads only a file's primary image, so a gain map is decoded
//! from a copy of the file whose `pitm` box names the gain map instead.

use heic_decoder::DecodedImage;

use super::gainmap::{GainMapImage, GainMapKind, apple_exif_headroom, apple_xmp_headroom};
use super::isobmff::{Meta, brands};
use super::png::Signal;
use super::{InputOptions, icc_primaries, to_scrgb};
use crate::color::{Primaries, SCRGB_WHITE_NITS, srgb_decode};
use crate::decode::ScRgbImage;
use crate::{Error, Result};

/// Whether a file starting with `head` is HEIF with HEVC images.
pub(super) fn sniff(head: &[u8]) -> bool {
    brands(head).iter().any(|brand| {
        matches!(
            brand,
            b"heic" | b"heix" | b"heim" | b"heis" | b"hevc" | b"hevx" | b"hevm" | b"hevs"
        )
    })
}

pub(super) fn decode(bytes: &[u8], options: &InputOptions) -> Result<ScRgbImage> {
    let image = heic_decoder::decode(bytes).map_err(decode_error)?;
    let (width, height) = (image.width as u32, image.height as u32);
    let color = image.color_info;
    let primaries = image
        .metadata
        .icc_profile
        .as_deref()
        .and_then(icc_primaries)
        .or_else(|| {
            Some(match color?.color_primaries {
                1 => Primaries::BT709,
                9 => Primaries::BT2020,
                12 => Primaries::DISPLAY_P3,
                _ => return None,
            })
        })
        .unwrap_or(Primaries::BT709);
    let rgba = image.to_rgba16().map_err(decode_error)?;
    let samples = rgba.data;
    let alpha: Vec<f32> = samples
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| f32::from(p[3]) / 65535.0)
        .collect();
    let alpha = image.alpha.is_some().then_some(alpha);
    let normalized = samples
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| [p[0], p[1], p[2]].map(|v| f32::from(v) / 65535.0));

    // A PQ or HLG file, as cameras save.
    if let Some(c) = color
        && matches!(c.transfer_characteristics, 16 | 18)
    {
        let code = |v: u16| u8::try_from(v).unwrap_or(u8::MAX);
        // Matrix 0: the samples are RGB already.
        let signal = Signal::from_cicp(
            code(c.color_primaries),
            code(c.transfer_characteristics),
            0,
            true,
        )?;
        let pixels = normalized.map(|rgb| signal.to_nits(rgb)).collect();
        return Ok(to_scrgb(
            width,
            height,
            pixels,
            alpha,
            &signal.primaries,
            1.0 / SCRGB_WHITE_NITS,
        ));
    }

    let mut pixels: Vec<[f32; 3]> = normalized.map(|rgb| rgb.map(srgb_decode)).collect();
    let meta = Meta::parse(bytes).ok_or_else(|| Error::Decode("unreadable HEIF boxes".into()))?;
    let (map_id, kind) = if let Some((id, iso)) = meta.iso_gain_map(bytes) {
        (id, Some(GainMapKind::Iso(iso)))
    } else if let Some(id) = meta.apple_gain_map(bytes) {
        (id, None)
    } else {
        return Err(Error::NotHdr("the HEIC has no gain map".into()));
    };
    let copy = meta
        .with_primary(bytes, map_id)
        .ok_or_else(|| Error::Decode("unreadable HEIF boxes".into()))?;
    let map = heic_decoder::decode(&copy).map_err(decode_error)?;
    let kind = match kind {
        Some(kind) => kind,
        None => {
            let headroom = map
                .metadata
                .xmp
                .as_deref()
                .and_then(|x| std::str::from_utf8(x).ok())
                .and_then(apple_xmp_headroom)
                .or_else(|| image.metadata.exif.as_deref().and_then(apple_exif_headroom))
                .ok_or_else(|| Error::Decode("the iPhone gain map's headroom is missing".into()))?;
            GainMapKind::Apple { headroom }
        }
    };
    let map = gain_map_image(&map)?;
    if !matches!(&kind, GainMapKind::Iso(iso) if !iso.base_is_sdr()) {
        super::gainmap::apply(&mut pixels, width, height, &map, &kind);
    }
    let white = options.gain_map_white(false);
    let mut image = to_scrgb(
        width,
        height,
        pixels,
        alpha,
        &primaries,
        white / SCRGB_WHITE_NITS,
    );
    image.sdr_white = Some(white);
    Ok(image)
}

/// A decoded gain map's values from 0 to 1: the luma plane of a
/// monochrome map, RGB otherwise.
fn gain_map_image(map: &DecodedImage) -> Result<GainMapImage> {
    let (width, height) = (map.width as u32, map.height as u32);
    if map.chroma_format_idc == 0 {
        let max = ((1u32 << map.bit_depth_luma) - 1) as f32;
        // Full range unless marked otherwise.
        let narrow = map.color_info.is_some_and(|c| !c.full_range);
        let step = (max + 1.0) / 256.0;
        let (black, white) = if narrow {
            (16.0 * step, 235.0 * step)
        } else {
            (0.0, max)
        };
        return Ok(GainMapImage {
            width,
            height,
            channels: 1,
            samples: map
                .y
                .iter()
                .map(|&v| ((v as f32 - black) / (white - black)).clamp(0.0, 1.0))
                .collect(),
        });
    }
    let rgb = map.to_rgb16().map_err(decode_error)?;
    Ok(GainMapImage {
        width,
        height,
        channels: 3,
        samples: rgb.data.iter().map(|&v| f32::from(v) / 65535.0).collect(),
    })
}

/// Width and height after rotation, without decoding the pixels.
pub(super) fn dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    let info = heic_decoder::probe(bytes).ok()?;
    Some((
        u32::try_from(info.width).ok()?,
        u32::try_from(info.height).ok()?,
    ))
}

fn decode_error(e: impl std::fmt::Display) -> Error {
    Error::Decode(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniffs_heic_brands() {
        let ftyp = |major: &[u8; 4], compatible: &[u8]| {
            let mut b = Vec::new();
            b.extend_from_slice(&(16 + compatible.len() as u32).to_be_bytes());
            b.extend_from_slice(b"ftyp");
            b.extend_from_slice(major);
            b.extend_from_slice(&[0; 4]);
            b.extend_from_slice(compatible);
            b
        };
        assert!(sniff(&ftyp(b"heic", b"mif1heic")));
        assert!(sniff(&ftyp(b"mif1", b"mif1heix")));
        assert!(!sniff(&ftyp(b"avif", b"mif1avifmiaf")));
        assert!(!sniff(b"\x89PNG"));
    }
}
