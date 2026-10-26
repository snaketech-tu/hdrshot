//! AVIF input: HDR AVIFs in PQ or HLG, as hdrshot and cameras write them,
//! and AVIFs with an ISO 21496-1 gain map, as Android and Adobe apps
//! write them. The AV1 images are decoded with `rav1d` ([`super::av1`]).
//!
//! Not available in the browser version: `rav1d` doesn't build for
//! WebAssembly.

use rayon::prelude::*;

use super::av1::{self, Planes};
use super::gainmap::{GainMapImage, GainMapKind};
use super::isobmff::{Meta, brands};
use super::png::Signal;
use super::{InputOptions, Orientation, icc_primaries, to_scrgb};
use crate::color::{Primaries, SCRGB_WHITE_NITS, srgb_decode};
use crate::decode::ScRgbImage;
use crate::{Error, Result};

/// The color description of an item: its `colr` boxes.
#[derive(Debug, Clone, Default)]
struct Colr {
    /// Primaries, transfer, matrix and full range.
    nclx: Option<(u8, u8, u8, bool)>,
    icc: Option<Vec<u8>>,
}

/// What a decoded item is for, which decides its channels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Want {
    /// RGB; a monochrome picture is gray.
    Rgb,
    /// A gain map: one channel if monochrome, RGB otherwise.
    Map,
    /// An alpha plane: the luma alone.
    Alpha,
}

/// A decoded item: RGB from 0 to 1 (or one channel), upright.
struct Decoded {
    width: u32,
    height: u32,
    channels: usize,
    samples: Vec<f32>,
    colr: Colr,
}

fn malformed() -> Error {
    Error::Decode("unreadable AVIF boxes".into())
}

pub(super) fn decode(bytes: &[u8], options: &InputOptions) -> Result<ScRgbImage> {
    let meta = Meta::parse(bytes).ok_or_else(malformed)?;
    let base = decode_item(&meta, bytes, meta.primary, Want::Rgb)?;
    let alpha = match meta.alpha(bytes) {
        Some(id) => {
            let alpha = decode_item(&meta, bytes, id, Want::Alpha)?;
            if (alpha.width, alpha.height) != (base.width, base.height) {
                return Err(Error::Decode(
                    "the AVIF's alpha plane doesn't match its size".into(),
                ));
            }
            Some(alpha.samples)
        }
        None => None,
    };
    let mut pixels: Vec<[f32; 3]> = base.samples.as_chunks::<3>().0.to_vec();
    // Premultiplied colors, marked by a `prem` reference to the alpha.
    if let Some(alpha) = &alpha
        && meta.references(b"prem", meta.primary).is_some()
    {
        pixels.par_iter_mut().zip(alpha).for_each(|(p, &a)| {
            if a > 0.0 {
                *p = p.map(|c| (c / a).min(1.0));
            }
        });
    }
    let (width, height) = (base.width, base.height);
    let nclx = base.colr.nclx;
    let primaries = || {
        base.colr
            .icc
            .as_deref()
            .and_then(icc_primaries)
            .or_else(|| cicp_primaries(nclx?.0))
            .unwrap_or(Primaries::BT709)
    };
    let hdr_signal = nclx.filter(|c| matches!(c.1, 16 | 18));

    // A gain map counts only with the `tmap` brand, as libavif decides.
    let gain_map = if brands(bytes).iter().any(|b| b == b"tmap") {
        meta.iso_gain_map(bytes)
    } else {
        None
    };
    match (gain_map, hdr_signal) {
        // The base is the SDR picture: apply the map.
        (Some((map_id, iso)), _) if iso.base_is_sdr() => {
            let map = decode_item(&meta, bytes, map_id, Want::Map)?;
            let map = GainMapImage {
                width: map.width,
                height: map.height,
                channels: map.channels,
                samples: map.samples,
            };
            let mut linear: Vec<[f32; 3]> =
                pixels.into_par_iter().map(|p| p.map(srgb_decode)).collect();
            super::gainmap::apply(&mut linear, width, height, &map, &GainMapKind::Iso(iso));
            let white = options.gain_map_white(false);
            let mut image = to_scrgb(
                width,
                height,
                linear,
                alpha,
                &primaries(),
                white / SCRGB_WHITE_NITS,
            );
            image.sdr_white = Some(white);
            Ok(image)
        }
        // A PQ or HLG base, with or without a map to SDR, which isn't
        // needed: the base is the HDR picture.
        (_, Some((p, t, _, _))) => {
            // Unspecified primaries are BT.709, as libavif reads them.
            let signal = Signal::from_cicp(if p == 2 { 1 } else { p }, t, 0, true)?;
            let pixels = pixels
                .into_par_iter()
                .map(|rgb| signal.to_nits(rgb))
                .collect();
            Ok(to_scrgb(
                width,
                height,
                pixels,
                alpha,
                &signal.primaries,
                1.0 / SCRGB_WHITE_NITS,
            ))
        }
        _ => Err(Error::NotHdr(
            "the AVIF is neither PQ nor HLG and has no gain map".into(),
        )),
    }
}

fn cicp_primaries(code: u8) -> Option<Primaries> {
    Some(match code {
        1 => Primaries::BT709,
        9 => Primaries::BT2020,
        12 => Primaries::DISPLAY_P3,
        _ => return None,
    })
}

/// Decodes item `id`, an `av01` image or a `grid` of them, to values from
/// 0 to 1 in the channels `want` asks for.
fn decode_item(meta: &Meta, file: &[u8], id: u32, want: Want) -> Result<Decoded> {
    let mut colr = Colr::default();
    let mut transforms = Vec::new();
    for (kind, body) in meta.properties_of(file, id) {
        match &kind {
            b"colr" if body.starts_with(b"nclx") && body.len() >= 11 => {
                let code = |at: usize| u8::try_from(u16::from_be_bytes([body[at], body[at + 1]]));
                if let (Ok(p), Ok(t), Ok(m)) = (code(4), code(6), code(8)) {
                    colr.nclx = Some((p, t, m, body[10] & 0x80 != 0));
                }
            }
            b"colr" if body.starts_with(b"prof") || body.starts_with(b"rICC") => {
                colr.icc = Some(body[4..].to_vec());
            }
            b"irot" if !body.is_empty() => {
                // Quarter turns anticlockwise.
                let turns = 4 - (body[0] & 3);
                transforms.push(Orientation {
                    turns: turns % 4,
                    mirror: false,
                });
            }
            b"imir" if !body.is_empty() => {
                // Axis 1 swaps left and right. Axis 0 swaps top and bottom
                // (ISO/IEC 23008-12:2022), which is that mirror turned twice.
                transforms.push(Orientation {
                    turns: if body[0] & 1 == 0 { 2 } else { 0 },
                    mirror: true,
                });
            }
            _ => {}
        }
    }

    let (width, height, channels, samples) = match &meta.item_type(id).ok_or_else(malformed)? {
        b"av01" => {
            let planes = decode_av01(meta, file, id)?;
            let nclx = colr.nclx.unwrap_or(planes.color);
            colr.nclx.get_or_insert(planes.color);
            let (channels, samples) = convert(&planes, nclx.2, nclx.3, want);
            (planes.width as u32, planes.height as u32, channels, samples)
        }
        b"grid" => {
            let tiles = meta.references(b"dimg", id).ok_or_else(malformed)?;
            let grid = meta.item_data(file, id).ok_or_else(malformed)?;
            // Version, flags, rows - 1, columns - 1, then the output size
            // in 16 or 32 bits.
            let (rows, columns) = (
                usize::from(*grid.get(2).ok_or_else(malformed)?) + 1,
                usize::from(*grid.get(3).ok_or_else(malformed)?) + 1,
            );
            let wide = grid[1] & 1 != 0;
            let field = |i: usize| -> Option<u32> {
                if wide {
                    Some(u32::from_be_bytes(
                        grid.get(4 + 4 * i..8 + 4 * i)?.try_into().ok()?,
                    ))
                } else {
                    Some(u32::from(u16::from_be_bytes(
                        grid.get(4 + 2 * i..6 + 2 * i)?.try_into().ok()?,
                    )))
                }
            };
            let (w, h) = (
                field(0).ok_or_else(malformed)? as usize,
                field(1).ok_or_else(malformed)? as usize,
            );
            if tiles.len() != rows * columns || w == 0 || h == 0 || w * h > 1 << 28 {
                return Err(malformed());
            }
            let decoded: Vec<(Planes, (u8, u8, u8, bool))> = tiles
                .par_iter()
                .map(|&t| {
                    let planes = decode_av01(meta, file, t)?;
                    let color = planes.color;
                    Ok((planes, color))
                })
                .collect::<Result<_>>()?;
            let (tile_w, tile_h) = (decoded[0].0.width, decoded[0].0.height);
            let nclx = colr.nclx.unwrap_or(decoded[0].1);
            colr.nclx.get_or_insert(decoded[0].1);
            let converted: Vec<(usize, Vec<f32>)> = decoded
                .par_iter()
                .map(|(planes, _)| convert(planes, nclx.2, nclx.3, want))
                .collect();
            let channels = converted[0].0;
            let mut out = vec![0f32; w * h * channels];
            for (i, ((planes, _), (tile_channels, samples))) in
                decoded.iter().zip(&converted).enumerate()
            {
                if (planes.width, planes.height, *tile_channels) != (tile_w, tile_h, channels) {
                    return Err(malformed());
                }
                let (x0, y0) = ((i % columns) * tile_w, (i / columns) * tile_h);
                for y in 0..planes.height.min(h.saturating_sub(y0)) {
                    let n = planes.width.min(w.saturating_sub(x0)) * channels;
                    let from = y * planes.width * channels;
                    let to = ((y0 + y) * w + x0) * channels;
                    out[to..to + n].copy_from_slice(&samples[from..from + n]);
                }
            }
            (w as u32, h as u32, channels, out)
        }
        other => {
            return Err(Error::UnsupportedFormat(format!(
                "AVIF item type {}",
                String::from_utf8_lossy(other)
            )));
        }
    };

    let (mut width, mut height, mut samples) = (width, height, samples);
    for t in transforms {
        (samples, width, height) = if channels == 1 {
            t.apply(samples, width, height)
        } else {
            let pixels: Vec<[f32; 3]> = samples.as_chunks::<3>().0.to_vec();
            let (pixels, w, h) = t.apply(pixels, width, height);
            (pixels.into_flattened(), w, h)
        };
    }
    Ok(Decoded {
        width,
        height,
        channels,
        samples,
        colr,
    })
}

/// Decodes one `av01` item.
fn decode_av01(meta: &Meta, file: &[u8], id: u32) -> Result<Planes> {
    let mut obus = Vec::new();
    // The configuration box's OBUs (the sequence header) come first.
    if let Some((_, av1c)) = meta.properties_of(file, id).find(|(k, _)| k == b"av1C") {
        obus.extend_from_slice(av1c.get(4..).unwrap_or(&[]));
    }
    obus.extend(meta.item_data(file, id).ok_or_else(malformed)?);
    av1::decode(&obus)
}

/// Converts YUV planes to RGB from 0 to 1, by the matrix coefficients
/// `matrix` (ITU-T H.273), or to one channel as `want` asks. Chroma is
/// interpolated from centered samples.
fn convert(p: &Planes, matrix: u8, full_range: bool, want: Want) -> (usize, Vec<f32>) {
    let max = ((1u32 << p.bit_depth) - 1) as f32;
    let step = (max + 1.0) / 256.0;
    let (y_black, y_scale, c_scale) = if full_range {
        (0.0, max, max)
    } else {
        (16.0 * step, 219.0 * step, 224.0 * step)
    };
    let half = (max + 1.0) / 2.0;
    let luma = |v: u16| (f32::from(v) - y_black) / y_scale;
    let gray = || -> Vec<f32> { p.y.par_iter().map(|&v| luma(v).clamp(0.0, 1.0)).collect() };
    let Some((sx, sy)) = p.subsampling.filter(|_| want != Want::Alpha) else {
        return if want == Want::Rgb {
            (3, gray().into_iter().flat_map(|v| [v; 3]).collect())
        } else {
            (1, gray())
        };
    };
    // Red and blue weights of the luma (H.273); identity is GBR.
    let weights = match matrix {
        0 => None,
        1 => Some((0.2126, 0.0722)),
        9 | 10 => Some((0.2627, 0.0593)),
        4 => Some((0.30, 0.11)),
        7 => Some((0.212, 0.087)),
        _ => Some((0.299, 0.114)),
    };
    let (cw, ch) = (p.chroma_width(), p.chroma_height());
    let chroma = |plane: &[u16], x: usize, y: usize| -> f32 {
        let fx = ((x as f32 + 0.5) / (1 << sx) as f32 - 0.5).clamp(0.0, (cw - 1) as f32);
        let fy = ((y as f32 + 0.5) / (1 << sy) as f32 - 0.5).clamp(0.0, (ch - 1) as f32);
        let (x0, y0) = (fx as usize, fy as usize);
        let (x1, y1) = ((x0 + 1).min(cw - 1), (y0 + 1).min(ch - 1));
        let (ax, ay) = (fx - x0 as f32, fy - y0 as f32);
        let at = |x: usize, y: usize| f32::from(plane[y * cw + x]);
        let top = at(x0, y0) * (1.0 - ax) + at(x1, y0) * ax;
        let bottom = at(x0, y1) * (1.0 - ax) + at(x1, y1) * ax;
        top * (1.0 - ay) + bottom * ay
    };
    let samples = (0..p.width * p.height)
        .into_par_iter()
        .flat_map_iter(|i| {
            let (x, y) = (i % p.width, i / p.width);
            let yv = luma(p.y[i]);
            let cb = chroma(&p.u, x, y);
            let cr = chroma(&p.v, x, y);
            let rgb = match weights {
                None => [cr / max, f32::from(p.y[i]) / max, cb / max],
                Some((kr, kb)) => {
                    let (u, v) = ((cb - half) / c_scale, (cr - half) / c_scale);
                    let kg = 1.0 - kr - kb;
                    let r = yv + 2.0 * (1.0 - kr) * v;
                    let b = yv + 2.0 * (1.0 - kb) * u;
                    let g = (yv - kr * r - kb * b) / kg;
                    [r, g, b]
                }
            };
            rgb.map(|c| c.clamp(0.0, 1.0))
        })
        .collect();
    (3, samples)
}

/// Width and height of the primary image, from its `ispe` property, with
/// a quarter turn swapping them.
pub(super) fn dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    let meta = Meta::parse(bytes)?;
    let mut size = None;
    let mut swap = false;
    for (kind, body) in meta.properties_of(bytes, meta.primary) {
        match &kind {
            b"ispe" if body.len() >= 12 => {
                size = Some((
                    u32::from_be_bytes(body[4..8].try_into().ok()?),
                    u32::from_be_bytes(body[8..12].try_into().ok()?),
                ));
            }
            b"irot" if !body.is_empty() => swap = body[0] & 1 == 1,
            _ => {}
        }
    }
    let (w, h) = size?;
    Some(if swap { (h, w) } else { (w, h) })
}
