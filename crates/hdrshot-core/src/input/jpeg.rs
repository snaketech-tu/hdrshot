//! Gain map JPEG input: Ultra HDR photos from Android phones, iPhone JPEGs
//! (the "Most Compatible" camera setting) and Adobe exports.
//!
//! The file is an SDR JPEG followed by a second JPEG holding the gain map,
//! found through the base image's MPF index or its XMP container directory.
//! Both are decoded with the pure-Rust `zune-jpeg`.

use zune_core::bytestream::ZCursor;
use zune_core::colorspace::ColorSpace;
use zune_core::options::DecoderOptions;
use zune_jpeg::JpegDecoder;

use super::gainmap::{
    GainMapImage, GainMapKind, IsoParams, Tiff, apple_exif_headroom, apple_xmp_headroom,
    is_apple_gain_map,
};
use super::{InputOptions, Orientation, icc_primaries, made_by_hdrshot, to_scrgb};
use crate::color::srgb_decode;
use crate::decode::ScRgbImage;
use crate::{Error, Result};

const XMP_NAMESPACE: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";
const ISO_NAMESPACE: &[u8] = b"urn:iso:std:iso:ts:21496:-1\0";

/// What the marker segments before the image data hold.
#[derive(Default)]
struct Segments<'a> {
    xmp: Option<&'a [u8]>,
    exif: Option<&'a [u8]>,
    /// ICC profile chunks with their sequence numbers.
    icc: Vec<(u8, &'a [u8])>,
    /// The MPF TIFF structure and where it starts in the file.
    mpf: Option<(usize, &'a [u8])>,
    iso: Option<&'a [u8]>,
    size: Option<(u32, u32)>,
}

impl<'a> Segments<'a> {
    /// Reads the segments of the JPEG at the start of `jpeg`, which lies
    /// at `offset` in the file.
    fn read(jpeg: &'a [u8], offset: usize) -> Self {
        let mut s = Self::default();
        let mut at = 2;
        while let (Some(&0xFF), Some(&marker)) = (jpeg.get(at), jpeg.get(at + 1)) {
            if marker == 0xFF {
                at += 1;
                continue;
            }
            let Some(len) = jpeg
                .get(at + 2..at + 4)
                .map(|b| usize::from(u16::from_be_bytes([b[0], b[1]])))
            else {
                break;
            };
            let Some(body) = jpeg.get(at + 4..at + 2 + len.max(2)) else {
                break;
            };
            match marker {
                0xE1 if body.starts_with(XMP_NAMESPACE) => {
                    s.xmp.get_or_insert(&body[XMP_NAMESPACE.len()..]);
                }
                0xE1 if body.starts_with(b"Exif\0\0") => {
                    s.exif.get_or_insert(&body[6..]);
                }
                0xE2 if body.starts_with(b"ICC_PROFILE\0") && body.len() > 14 => {
                    s.icc.push((body[12], &body[14..]));
                }
                0xE2 if body.starts_with(b"MPF\0") => {
                    s.mpf.get_or_insert((offset + at + 8, &body[4..]));
                }
                0xE2 if body.starts_with(ISO_NAMESPACE) => {
                    s.iso.get_or_insert(&body[ISO_NAMESPACE.len()..]);
                }
                // Start of frame, other than DHT, JPG and DAC.
                0xC0..=0xCF if !matches!(marker, 0xC4 | 0xC8 | 0xCC) => {
                    if body.len() >= 5 {
                        let h = u16::from_be_bytes([body[1], body[2]]);
                        let w = u16::from_be_bytes([body[3], body[4]]);
                        s.size = Some((u32::from(w), u32::from(h)));
                    }
                }
                0xDA => break,
                _ => {}
            }
            at += 2 + len;
        }
        s
    }

    fn xmp_text(&self) -> Option<&'a str> {
        std::str::from_utf8(self.xmp?).ok()
    }

    fn icc_profile(&self) -> Option<Vec<u8>> {
        if self.icc.is_empty() {
            return None;
        }
        let mut chunks = self.icc.clone();
        chunks.sort_by_key(|c| c.0);
        Some(
            chunks
                .into_iter()
                .flat_map(|c| c.1.iter().copied())
                .collect(),
        )
    }

    /// The EXIF orientation, if there is one.
    fn orientation(&self) -> Orientation {
        let value = || {
            let tiff = Tiff::new(self.exif?)?;
            let v = tiff
                .entries(tiff.u32(4)? as usize)?
                .find(|e| e.tag == 0x0112)?
                .value;
            u8::try_from(v).ok()
        };
        Orientation::from_exif(value().unwrap_or(1))
    }

    /// Where the other images of the file start, from the MPF index.
    fn mpf_images(&self) -> Vec<(usize, usize)> {
        let Some((tiff_at, mpf)) = self.mpf else {
            return Vec::new();
        };
        let images = || -> Option<Vec<(usize, usize)>> {
            let tiff = Tiff::new(mpf)?;
            let entries = tiff
                .entries(tiff.u32(4)? as usize)?
                .find(|e| e.tag == 0xB002)?;
            let (count, at) = (entries.count as usize / 16, entries.value as usize);
            Some(
                (0..count.min(16))
                    .filter_map(|i| {
                        let size = tiff.u32(at + 16 * i + 4)? as usize;
                        let offset = tiff.u32(at + 16 * i + 8)? as usize;
                        // Offset 0 is the base image itself.
                        (offset != 0).then_some((tiff_at + offset, size))
                    })
                    .collect(),
            )
        };
        images().unwrap_or_default()
    }
}

/// Whether the start of a JPEG suggests a gain map, so that a folder scan
/// should include it: it has an MPF index or Ultra HDR XMP, and it wasn't
/// written by hdrshot, which folder scans leave out like PNGs.
pub(super) fn has_gain_map(head: &[u8]) -> bool {
    let s = Segments::read(head, 0);
    let xmp = s.xmp_text().unwrap_or("");
    (s.mpf.is_some() || xmp.contains("hdrgm:")) && !made_by_hdrshot(xmp)
}

/// Width and height, after the EXIF orientation, from the start of the
/// file.
pub(super) fn dimensions(head: &[u8]) -> Option<(u32, u32)> {
    let s = Segments::read(head, 0);
    let (w, h) = s.size?;
    Some(if s.orientation().swaps() {
        (h, w)
    } else {
        (w, h)
    })
}

/// The gain map JPEG and how to apply it.
struct Found<'a> {
    jpeg: &'a [u8],
    kind: GainMapKind,
}

fn find_gain_map<'a>(bytes: &'a [u8], base: &Segments) -> Result<Found<'a>> {
    let mut candidates = base.mpf_images();
    // Without MPF, Ultra HDR's container directory gives the gain map's
    // length, and it is the end of the file.
    if let Some(len) = base
        .xmp_text()
        .and_then(|x| x.split("Item:Semantic=\"GainMap\"").nth(1))
        .and_then(|x| super::gainmap::xmp_values(x, "Item:Length"))
        .and_then(|v| v.first().copied())
        .map(|l| l as usize)
        .filter(|&l| l > 0 && l < bytes.len())
    {
        candidates.push((bytes.len() - len, len));
    }
    for (offset, size) in candidates {
        let Some(jpeg) = bytes.get(offset..(offset + size).min(bytes.len())) else {
            continue;
        };
        if !jpeg.starts_with(&[0xFF, 0xD8]) {
            continue;
        }
        let map = Segments::read(jpeg, offset);
        let xmp = map.xmp_text().unwrap_or("");
        let kind = if let Some(iso) = map.iso.and_then(IsoParams::parse) {
            GainMapKind::Iso(iso)
        } else if let Some(iso) = IsoParams::from_hdrgm(xmp) {
            GainMapKind::Iso(iso)
        } else if is_apple_gain_map(xmp) {
            let headroom = apple_xmp_headroom(xmp)
                .or_else(|| base.xmp_text().and_then(apple_xmp_headroom))
                .or_else(|| base.exif.and_then(apple_exif_headroom))
                .ok_or_else(|| Error::Decode("the iPhone gain map's headroom is missing".into()))?;
            GainMapKind::Apple { headroom }
        } else {
            // A depth map or another auxiliary image.
            continue;
        };
        return Ok(Found { jpeg, kind });
    }
    Err(Error::NotHdr("the JPEG has no gain map".into()))
}

pub(super) fn decode(bytes: &[u8], options: &InputOptions) -> Result<ScRgbImage> {
    let base = Segments::read(bytes, 0);
    let found = find_gain_map(bytes, &base)?;
    let orientation = base.orientation();

    let (width, height, rgb) = decode_jpeg(bytes, ColorSpace::RGB)?;
    let mut pixels: Vec<[f32; 3]> = rgb
        .as_chunks::<3>()
        .0
        .iter()
        .map(|p| p.map(|v| srgb_decode(f32::from(v) / 255.0)))
        .collect();

    let map = {
        let mut d = JpegDecoder::new(ZCursor::new(found.jpeg));
        d.decode_headers().map_err(decode_error)?;
        let gray = d.info().is_some_and(|i| i.components == 1);
        let (w, h, samples) = decode_jpeg(
            found.jpeg,
            if gray {
                ColorSpace::Luma
            } else {
                ColorSpace::RGB
            },
        )?;
        GainMapImage {
            width: w,
            height: h,
            channels: if gray { 1 } else { 3 },
            samples: samples.iter().map(|&v| f32::from(v) / 255.0).collect(),
        }
    };

    if let GainMapKind::Iso(iso) = &found.kind
        && !iso.base_is_sdr()
    {
        // The base is the HDR picture already.
    } else {
        super::gainmap::apply(&mut pixels, width, height, &map, &found.kind);
    }
    let (pixels, width, height) = orientation.apply(pixels, width, height);
    let primaries = base
        .icc_profile()
        .as_deref()
        .and_then(icc_primaries)
        .unwrap_or(crate::color::Primaries::BT709);
    let white = options.gain_map_white(base.xmp_text().is_some_and(made_by_hdrshot));
    let mut image = to_scrgb(
        width,
        height,
        pixels,
        None,
        &primaries,
        white / crate::color::SCRGB_WHITE_NITS,
    );
    image.sdr_white = Some(white);
    Ok(image)
}

/// Decodes a JPEG to 8-bit samples in `color`.
fn decode_jpeg(jpeg: &[u8], color: ColorSpace) -> Result<(u32, u32, Vec<u8>)> {
    let options = DecoderOptions::default()
        .jpeg_set_out_colorspace(color)
        .set_max_width(usize::from(u16::MAX))
        .set_max_height(usize::from(u16::MAX));
    let mut d = JpegDecoder::new_with_options(ZCursor::new(jpeg), options);
    let samples = d.decode().map_err(decode_error)?;
    let (w, h) = d
        .dimensions()
        .ok_or_else(|| Error::Decode("JPEG without a size".into()))?;
    Ok((w as u32, h as u32, samples))
}

fn decode_error(e: impl std::fmt::Display) -> Error {
    Error::Decode(e.to_string())
}
