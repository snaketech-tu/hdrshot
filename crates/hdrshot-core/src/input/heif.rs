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

use super::gainmap::{
    GainMapImage, GainMapKind, IsoParams, apple_exif_headroom, apple_xmp_headroom,
};
use super::png::Signal;
use super::{InputOptions, icc_primaries, to_scrgb};
use crate::color::{Primaries, SCRGB_WHITE_NITS, srgb_decode};
use crate::decode::ScRgbImage;
use crate::{Error, Result};

const APPLE_GAIN_MAP: &[u8] = b"urn:com:apple:photo:2020:aux:hdrgainmap";

/// Whether a file starting with `head` is HEIF with HEVC images.
pub(super) fn sniff(head: &[u8]) -> bool {
    let Some(size) = head
        .get(..4)
        .map(|b| u32::from_be_bytes(b.try_into().unwrap()))
    else {
        return false;
    };
    if head.get(4..8) != Some(b"ftyp") {
        return false;
    }
    let ftyp = head.get(8..(size as usize).min(head.len())).unwrap_or(&[]);
    ftyp.as_chunks::<4>()
        .0
        .iter()
        .enumerate()
        .any(|(i, brand)| {
            // The minor version, second, isn't a brand.
            i != 1
                && matches!(
                    brand,
                    b"heic" | b"heix" | b"heim" | b"heis" | b"hevc" | b"hevx" | b"hevm" | b"hevs"
                )
        })
}

/// The boxes of a HEIF `meta` box that say which item is what.
#[derive(Default)]
struct Meta {
    primary: u32,
    /// Where the `pitm` box's item ID is, and whether it has 4 bytes.
    pitm_at: usize,
    pitm_wide: bool,
    /// Item IDs and types.
    items: Vec<(u32, [u8; 4])>,
    /// References: type, from, to.
    refs: Vec<([u8; 4], u32, Vec<u32>)>,
    /// Properties: type and body.
    properties: Vec<([u8; 4], std::ops::Range<usize>)>,
    /// Item ID and the (1-based) indexes of its properties.
    associations: Vec<(u32, Vec<u16>)>,
    locations: Vec<Location>,
    idat: Option<std::ops::Range<usize>>,
}

/// Where an item's data is: its ID, construction method (0 for the file,
/// 1 for the `idat` box) and extents (offset and length).
type Location = (u32, u8, Vec<(u64, u64)>);

/// A box: its type and the range of its body in the file.
fn boxes(file: &[u8], range: std::ops::Range<usize>) -> Vec<([u8; 4], std::ops::Range<usize>)> {
    let mut out = Vec::new();
    let mut at = range.start;
    while at + 8 <= range.end {
        let size = u32::from_be_bytes(file[at..at + 4].try_into().unwrap()) as u64;
        let kind: [u8; 4] = file[at + 4..at + 8].try_into().unwrap();
        let (header, size) = match size {
            0 => (8, (range.end - at) as u64),
            1 => match file.get(at + 8..at + 16) {
                Some(b) => (16, u64::from_be_bytes(b.try_into().unwrap())),
                None => break,
            },
            s => (8, s),
        };
        let Some(end) = usize::try_from(size).ok().and_then(|s| at.checked_add(s)) else {
            break;
        };
        if size < header as u64 || end > range.end {
            break;
        }
        out.push((kind, at + header..end));
        at = end;
    }
    out
}

struct Cursor<'a> {
    data: &'a [u8],
    at: usize,
}

impl Cursor<'_> {
    fn uint(&mut self, bytes: usize) -> Option<u64> {
        let b = self.data.get(self.at..self.at + bytes)?;
        self.at += bytes;
        Some(b.iter().fold(0, |v, &x| v << 8 | u64::from(x)))
    }
    fn u8(&mut self) -> Option<u8> {
        self.uint(1).map(|v| v as u8)
    }
    fn u16(&mut self) -> Option<u16> {
        self.uint(2).map(|v| v as u16)
    }
    fn u32(&mut self) -> Option<u32> {
        self.uint(4).map(|v| v as u32)
    }
    fn kind(&mut self) -> Option<[u8; 4]> {
        let b = self.data.get(self.at..self.at + 4)?.try_into().ok()?;
        self.at += 4;
        Some(b)
    }
}

impl Meta {
    fn parse(file: &[u8]) -> Option<Self> {
        let (_, meta) = boxes(file, 0..file.len())
            .into_iter()
            .find(|(k, _)| k == b"meta")?;
        let mut m = Self::default();
        // A full box: version and flags first.
        for (kind, body) in boxes(file, meta.start + 4..meta.end) {
            let data = &file[body.clone()];
            let mut c = Cursor { data, at: 0 };
            match &kind {
                b"pitm" => {
                    let version = c.u8()?;
                    c.at = 4;
                    m.pitm_at = body.start + 4;
                    m.pitm_wide = version != 0;
                    m.primary = if m.pitm_wide {
                        c.u32()?
                    } else {
                        u32::from(c.u16()?)
                    };
                }
                b"iinf" => {
                    let version = c.u8()?;
                    let skip = if version == 0 { 6 } else { 8 };
                    for (k, infe) in boxes(file, body.start + skip..body.end) {
                        if &k != b"infe" {
                            continue;
                        }
                        let mut c = Cursor {
                            data: &file[infe],
                            at: 0,
                        };
                        let version = c.u8()?;
                        if version < 2 {
                            continue;
                        }
                        c.at = 4;
                        let id = if version == 2 {
                            u32::from(c.u16()?)
                        } else {
                            c.u32()?
                        };
                        c.u16()?; // protection index
                        m.items.push((id, c.kind()?));
                    }
                }
                b"iref" => {
                    let wide = c.u8()? != 0;
                    for (k, r) in boxes(file, body.start + 4..body.end) {
                        let mut c = Cursor {
                            data: &file[r],
                            at: 0,
                        };
                        let id = |c: &mut Cursor| {
                            if wide {
                                c.u32()
                            } else {
                                c.u16().map(u32::from)
                            }
                        };
                        let from = id(&mut c)?;
                        let count = c.u16()?;
                        let to = (0..count).map(|_| id(&mut c)).collect::<Option<_>>()?;
                        m.refs.push((k, from, to));
                    }
                }
                b"iprp" => {
                    for (k, b) in boxes(file, body.clone()) {
                        match &k {
                            b"ipco" => m.properties = boxes(file, b),
                            b"ipma" => m.parse_ipma(&file[b])?,
                            _ => {}
                        }
                    }
                }
                b"iloc" => m.parse_iloc(data)?,
                b"idat" => m.idat = Some(body),
                _ => {}
            }
        }
        Some(m)
    }

    fn parse_ipma(&mut self, data: &[u8]) -> Option<()> {
        let mut c = Cursor { data, at: 0 };
        let version = c.u8()?;
        let flags = c.uint(3)?;
        for _ in 0..c.u32()? {
            let id = if version == 0 {
                u32::from(c.u16()?)
            } else {
                c.u32()?
            };
            let count = c.u8()?;
            let indexes = (0..count)
                .map(|_| {
                    if flags & 1 != 0 {
                        c.u16().map(|v| v & 0x7FFF)
                    } else {
                        c.u8().map(|v| u16::from(v & 0x7F))
                    }
                })
                .collect::<Option<_>>()?;
            self.associations.push((id, indexes));
        }
        Some(())
    }

    fn parse_iloc(&mut self, data: &[u8]) -> Option<()> {
        let mut c = Cursor { data, at: 0 };
        let version = c.u8()?;
        c.at = 4;
        let sizes = c.u16()?;
        let (offset_size, length_size) = (usize::from(sizes >> 12), usize::from(sizes >> 8 & 15));
        let base_offset_size = usize::from(sizes >> 4 & 15);
        let index_size = if version >= 1 {
            usize::from(sizes & 15)
        } else {
            0
        };
        let count = if version < 2 {
            u32::from(c.u16()?)
        } else {
            c.u32()?
        };
        for _ in 0..count {
            let id = if version < 2 {
                u32::from(c.u16()?)
            } else {
                c.u32()?
            };
            let method = if version >= 1 {
                (c.u16()? & 15) as u8
            } else {
                0
            };
            c.u16()?; // data reference index
            let base = c.uint(base_offset_size)?;
            let extents = (0..c.u16()?)
                .map(|_| {
                    c.uint(index_size)?;
                    Some((base + c.uint(offset_size)?, c.uint(length_size)?))
                })
                .collect::<Option<_>>()?;
            self.locations.push((id, method, extents));
        }
        Some(())
    }

    fn item_type(&self, id: u32) -> Option<[u8; 4]> {
        self.items.iter().find(|i| i.0 == id).map(|i| i.1)
    }

    fn property(&self, id: u32, kind: &[u8; 4]) -> Option<std::ops::Range<usize>> {
        let (_, indexes) = self.associations.iter().find(|a| a.0 == id)?;
        indexes.iter().find_map(|&i| {
            let (k, body) = self.properties.get(usize::from(i).checked_sub(1)?)?;
            (k == kind).then(|| body.clone())
        })
    }

    /// The bytes of an item, such as a `tmap` item's metadata.
    fn item_data(&self, file: &[u8], id: u32) -> Option<Vec<u8>> {
        let (_, method, extents) = self.locations.iter().find(|l| l.0 == id)?;
        let source = match method {
            0 => file,
            1 => &file[self.idat.clone()?],
            _ => return None,
        };
        let mut out = Vec::new();
        for &(offset, length) in extents {
            let start = usize::try_from(offset).ok()?;
            let end = if length == 0 {
                source.len()
            } else {
                start.checked_add(usize::try_from(length).ok()?)?
            };
            out.extend_from_slice(source.get(start..end)?);
        }
        Some(out)
    }

    /// The ISO 21496-1 gain map: the `tmap` item derived from the primary
    /// image and a gain map image, and its parameters.
    fn iso_gain_map(&self, file: &[u8]) -> Option<(u32, IsoParams)> {
        self.refs.iter().find_map(|(kind, from, to)| {
            if kind != b"dimg" || self.item_type(*from) != Some(*b"tmap") {
                return None;
            }
            let [base, map] = to[..] else {
                return None;
            };
            if base != self.primary {
                return None;
            }
            let data = self.item_data(file, *from)?;
            // A version byte, 0, comes before the metadata.
            let params = IsoParams::parse(data.strip_prefix(&[0])?)?;
            Some((map, params))
        })
    }

    /// Apple's gain map: an auxiliary image of the primary one.
    fn apple_gain_map(&self, file: &[u8]) -> Option<u32> {
        self.refs.iter().find_map(|(kind, from, to)| {
            if kind != b"auxl" || !to.contains(&self.primary) {
                return None;
            }
            let aux = &file[self.property(*from, b"auxC")?];
            // A full box: version and flags, then the type as a C string.
            let urn = aux.get(4..)?.split(|&b| b == 0).next()?;
            (urn == APPLE_GAIN_MAP).then_some(*from)
        })
    }

    /// A copy of `file` whose primary image is item `id`.
    fn with_primary(&self, file: &[u8], id: u32) -> Option<Vec<u8>> {
        let mut copy = file.to_vec();
        if self.pitm_wide {
            copy.get_mut(self.pitm_at..self.pitm_at + 4)?
                .copy_from_slice(&id.to_be_bytes());
        } else {
            copy.get_mut(self.pitm_at..self.pitm_at + 2)?
                .copy_from_slice(&u16::try_from(id).ok()?.to_be_bytes());
        }
        Some(copy)
    }
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
