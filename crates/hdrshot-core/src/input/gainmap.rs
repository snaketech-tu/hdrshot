//! Gain map photos: an SDR picture plus a map of how much brighter each
//! pixel is in the HDR picture, as phones save them.
//!
//! Three kinds of metadata describe the map:
//! - ISO 21496-1, binary, written by Android 15 and later, iOS 18 and later
//!   and Adobe apps, in JPEG, HEIC and AVIF;
//! - Adobe's `hdrgm` XMP, the original Ultra HDR JPEG format (Android 14
//!   and Lightroom);
//! - Apple's own: a headroom in XMP or in the EXIF maker note, and a map
//!   that is linear after the BT.709 transfer function is undone (iPhones
//!   before iOS 18, and iPhone files that keep it next to ISO metadata).
//!
//! The decoders here render the full HDR picture, the one a phone shows on
//! a display with enough headroom.

use rayon::prelude::*;

/// How a gain map value turns into a brightness factor.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum GainMapKind {
    /// ISO 21496-1 or `hdrgm` parameters.
    Iso(IsoParams),
    /// Apple's: `1 + (headroom - 1) * linear(value)`.
    Apple { headroom: f32 },
}

/// ISO 21496-1 parameters for each channel, in log2 units where the
/// standard uses them. `hdrgm` XMP has the same meaning.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct IsoParams {
    pub min: [f32; 3],
    pub max: [f32; 3],
    pub gamma: [f32; 3],
    pub offset_base: [f32; 3],
    pub offset_alternate: [f32; 3],
    /// Headroom of the base and of the alternate picture, in stops.
    pub base_headroom: f32,
    pub alternate_headroom: f32,
}

impl IsoParams {
    /// Whether the base picture is the SDR one and the map makes it HDR.
    /// Files the other way around are already HDR without the map.
    pub fn base_is_sdr(&self) -> bool {
        self.alternate_headroom > self.base_headroom
    }

    /// Parses ISO 21496-1 metadata, as it follows the
    /// `urn:iso:std:iso:ts:21496:-1` name in a JPEG APP2 segment, or the
    /// version byte of a HEIF `tmap` item. `None` for a version this
    /// doesn't know, or for data that doesn't add up, such as the version
    /// only, which JPEGs put in their base image.
    pub fn parse(data: &[u8]) -> Option<Self> {
        let mut r = Reader { data, at: 0 };
        let minimum_version = r.u16()?;
        let _writer_version = r.u16()?;
        if minimum_version != 0 {
            return None;
        }
        let flags = r.u8()?;
        let channels = if flags & 0x80 != 0 { 3 } else { 1 };
        let common = (flags & 0x08 != 0).then(|| r.u32()).flatten();
        if flags & 0x08 != 0 && common.is_none_or(|d| d == 0) {
            return None;
        }
        let unsigned = |r: &mut Reader| -> Option<f32> {
            let n = r.u32()?;
            let d = common.map_or_else(|| r.u32(), Some)?;
            (d != 0).then(|| (f64::from(n) / f64::from(d)) as f32)
        };
        let base_headroom = unsigned(&mut r)?;
        let alternate_headroom = unsigned(&mut r)?;
        let signed = |r: &mut Reader| -> Option<f32> {
            let n = r.u32()? as i32;
            let d = common.map_or_else(|| r.u32(), Some)?;
            (d != 0).then(|| (f64::from(n) / f64::from(d)) as f32)
        };
        let mut p = Self {
            min: [0.0; 3],
            max: [0.0; 3],
            gamma: [1.0; 3],
            offset_base: [0.0; 3],
            offset_alternate: [0.0; 3],
            base_headroom,
            alternate_headroom,
        };
        for c in 0..channels {
            p.min[c] = signed(&mut r)?;
            p.max[c] = signed(&mut r)?;
            let gamma_n = r.u32()?;
            let gamma_d = common.map_or_else(|| r.u32(), Some)?;
            p.gamma[c] = (gamma_d != 0 && gamma_n != 0)
                .then(|| (f64::from(gamma_n) / f64::from(gamma_d)) as f32)?;
            p.offset_base[c] = signed(&mut r)?;
            p.offset_alternate[c] = signed(&mut r)?;
        }
        p.fill_channels(channels);
        p.valid().then_some(p)
    }

    /// Reads `hdrgm` XMP, as Ultra HDR JPEGs carry in their gain map image.
    pub fn from_hdrgm(xmp: &str) -> Option<Self> {
        let values = |name: &str, default: Option<f32>| -> Option<Vec<f32>> {
            match xmp_values(xmp, name) {
                Some(v) if !v.is_empty() => Some(v),
                Some(_) => None,
                None => default.map(|d| vec![d]),
            }
        };
        let max = values("hdrgm:GainMapMax", None)?;
        let capacity_max = *values("hdrgm:HDRCapacityMax", None)?.first()?;
        let min = values("hdrgm:GainMapMin", Some(0.0))?;
        let gamma = values("hdrgm:Gamma", Some(1.0))?;
        let offset_sdr = values("hdrgm:OffsetSDR", Some(1.0 / 64.0))?;
        let offset_hdr = values("hdrgm:OffsetHDR", Some(1.0 / 64.0))?;
        let capacity_min = *values("hdrgm:HDRCapacityMin", Some(0.0))?.first()?;
        let base_is_hdr = xmp_text(xmp, "hdrgm:BaseRenditionIsHDR")
            .is_some_and(|v| v.trim().eq_ignore_ascii_case("true"));
        let channel = |v: &[f32], c: usize| v.get(c).or(v.first()).copied().unwrap_or(0.0);
        let mut p = Self {
            min: std::array::from_fn(|c| channel(&min, c)),
            max: std::array::from_fn(|c| channel(&max, c)),
            gamma: std::array::from_fn(|c| channel(&gamma, c)),
            offset_base: std::array::from_fn(|c| channel(&offset_sdr, c)),
            offset_alternate: std::array::from_fn(|c| channel(&offset_hdr, c)),
            base_headroom: capacity_min,
            alternate_headroom: capacity_max,
        };
        if base_is_hdr {
            std::mem::swap(&mut p.base_headroom, &mut p.alternate_headroom);
            std::mem::swap(&mut p.offset_base, &mut p.offset_alternate);
        }
        p.valid().then_some(p)
    }

    fn fill_channels(&mut self, channels: usize) {
        for c in channels..3 {
            self.min[c] = self.min[0];
            self.max[c] = self.max[0];
            self.gamma[c] = self.gamma[0];
            self.offset_base[c] = self.offset_base[0];
            self.offset_alternate[c] = self.offset_alternate[0];
        }
    }

    fn valid(&self) -> bool {
        let all = [
            &self.min,
            &self.max,
            &self.gamma,
            &self.offset_base,
            &self.offset_alternate,
        ];
        all.iter().all(|v| v.iter().all(|x| x.is_finite()))
            && self.gamma.iter().all(|&g| g > 0.0)
            && (0..3).all(|c| self.max[c] >= self.min[c])
            && self.base_headroom.is_finite()
            && self.alternate_headroom.is_finite()
    }
}

/// Apple's headroom from the maker note of an iPhone photo's EXIF
/// (starting at its TIFF header), by Apple's published formula: tags 33 and
/// 48 give the number of stops.
pub(super) fn apple_exif_headroom(exif: &[u8]) -> Option<f32> {
    let tiff = Tiff::new(exif)?;
    let exif_ifd = tiff
        .entries(tiff.u32(4)? as usize)?
        .find(|e| e.tag == 0x8769)?
        .value;
    let maker_note = tiff
        .entries(exif_ifd as usize)?
        .find(|e| e.tag == 0x927C)?
        .value as usize;
    const HEADER: &[u8] = b"Apple iOS\0\0\x01MM";
    if exif.get(maker_note..maker_note + HEADER.len())? != HEADER {
        return None;
    }
    // Maker note offsets count from the start of the maker note, and it is
    // always big-endian.
    let note = Tiff {
        data: exif.get(maker_note..)?,
        big_endian: true,
    };
    let (mut maker33, mut maker48) = (None, None);
    for e in note.entries(HEADER.len())? {
        // SRATIONAL, as Apple writes them; other kinds aren't these tags.
        if (e.tag == 33 || e.tag == 48) && e.kind == 10 {
            let at = e.value as usize;
            let (n, d) = (note.u32(at)? as i32, note.u32(at + 4)? as i32);
            let v = (d != 0).then(|| f64::from(n) / f64::from(d));
            if e.tag == 33 {
                maker33 = v;
            } else {
                maker48 = v;
            }
        }
    }
    if maker33.is_none() && maker48.is_none() {
        return None;
    }
    let (m33, m48) = (maker33.unwrap_or(0.0), maker48.unwrap_or(0.0));
    let stops = match (m33 < 1.0, m48 <= 0.01) {
        (true, true) => -20.0 * m48 + 1.8,
        (true, false) => -0.101 * m48 + 1.601,
        (false, true) => -70.0 * m48 + 3.0,
        (false, false) => -0.303 * m48 + 2.303,
    };
    Some(2f64.powf(stops.max(0.0)) as f32)
}

/// Apple's headroom from the gain map's XMP, which newer iPhones write.
pub(super) fn apple_xmp_headroom(xmp: &str) -> Option<f32> {
    xmp_values(xmp, "HDRGainMap:HDRGainMapHeadroom")?
        .first()
        .copied()
        .filter(|h| h.is_finite() && *h >= 1.0)
}

/// Whether XMP marks an Apple gain map.
pub(super) fn is_apple_gain_map(xmp: &str) -> bool {
    xmp.contains("HDRGainMap:HDRGainMapVersion") || xmp.contains("hdrgainmap")
}

/// A decoded gain map, with values from 0 to 1.
pub(super) struct GainMapImage {
    pub width: u32,
    pub height: u32,
    /// 1 or 3.
    pub channels: usize,
    pub samples: Vec<f32>,
}

impl GainMapImage {
    /// The map's value for each channel at the center of base pixel
    /// (`x`, `y`) of a `width` by `height` picture, interpolated between
    /// the map's pixels, which are usually fewer.
    fn sample(&self, x: u32, y: u32, width: u32, height: u32) -> [f32; 3] {
        let coord = |p: u32, from: u32, to: u32| -> (usize, usize, f32) {
            let f = ((p as f32 + 0.5) * to as f32 / from as f32 - 0.5).clamp(0.0, to as f32 - 1.0);
            let i = f.floor() as usize;
            (i, (i + 1).min(to as usize - 1), f - i as f32)
        };
        let (x0, x1, fx) = coord(x, width, self.width);
        let (y0, y1, fy) = coord(y, height, self.height);
        let w = self.width as usize;
        let at = |xi: usize, yi: usize, c: usize| self.samples[(yi * w + xi) * self.channels + c];
        std::array::from_fn(|c| {
            let c = c.min(self.channels - 1);
            let top = at(x0, y0, c) * (1.0 - fx) + at(x1, y0, c) * fx;
            let bottom = at(x0, y1, c) * (1.0 - fx) + at(x1, y1, c) * fx;
            top * (1.0 - fy) + bottom * fy
        })
    }
}

/// Applies `map` in full to `base`, linear RGB in units of SDR white, and
/// returns the HDR picture in the same units.
pub(super) fn apply(
    base: &mut [[f32; 3]],
    width: u32,
    height: u32,
    map: &GainMapImage,
    kind: &GainMapKind,
) {
    if map.width == 0 || map.height == 0 || map.samples.len() < (map.width * map.height) as usize {
        return;
    }
    base.par_chunks_mut(width as usize)
        .enumerate()
        .for_each(|(y, row)| {
            for (x, p) in row.iter_mut().enumerate() {
                let g = map.sample(x as u32, y as u32, width, height);
                *p = match kind {
                    GainMapKind::Apple { headroom } => std::array::from_fn(|c| {
                        p[c] * (1.0 + (headroom - 1.0) * bt709_decode(g[c]))
                    }),
                    GainMapKind::Iso(iso) => std::array::from_fn(|c| {
                        let v = g[c].max(0.0).powf(1.0 / iso.gamma[c]);
                        let log = iso.min[c] + (iso.max[c] - iso.min[c]) * v;
                        (p[c] + iso.offset_base[c]) * log.exp2() - iso.offset_alternate[c]
                    }),
                };
            }
        });
}

/// BT.709 inverse OETF: a 0..1 signal to linear 0..1.
fn bt709_decode(v: f32) -> f32 {
    let v = v.clamp(0.0, 1.0);
    if v < 0.081 {
        v / 4.5
    } else {
        ((v + 0.099) / 1.099).powf(1.0 / 0.45)
    }
}

/// The numbers of an XMP property, written as an attribute
/// (`name="1.5"`), an element (`<name>1.5</name>`) or a list of one value
/// per channel (`<name><rdf:Seq><rdf:li>1.5</rdf:li>...`). `None` if the
/// property isn't there; an empty list if it isn't a number.
pub(super) fn xmp_values(xmp: &str, name: &str) -> Option<Vec<f32>> {
    let text = xmp_text(xmp, name)?;
    let numbers: Vec<f32> = if text.contains("<rdf:li") {
        text.split("<rdf:li")
            .skip(1)
            .filter_map(|li| li.split_once('>')?.1.split('<').next())
            .map(|v| v.trim().parse().unwrap_or(f32::NAN))
            .collect()
    } else {
        vec![text.trim().parse().unwrap_or(f32::NAN)]
    };
    Some(numbers.into_iter().filter(|v| v.is_finite()).collect())
}

/// The raw text of an XMP property: the attribute's value or the element's
/// content.
pub(super) fn xmp_text<'a>(xmp: &'a str, name: &str) -> Option<&'a str> {
    let mut rest = xmp;
    while let Some(i) = rest.find(name) {
        let before = rest[..i].chars().next_back();
        let after = &rest[i + name.len()..];
        rest = after;
        match before {
            Some(c) if c.is_whitespace() => {
                if let Some(value) = after.trim_start().strip_prefix('=')
                    && let value = value.trim_start()
                    && let Some(quote) = value.chars().next().filter(|q| *q == '"' || *q == '\'')
                {
                    return value[1..].split(quote).next();
                }
            }
            Some('<') if after.starts_with(['>', ' ', '\n', '\r', '\t']) => {
                let content = after.split_once('>')?.1;
                return content.split(&format!("</{name}")).next();
            }
            _ => {}
        }
    }
    None
}

struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn take<const N: usize>(&mut self) -> Option<[u8; N]> {
        let bytes = self.data.get(self.at..self.at + N)?.try_into().ok()?;
        self.at += N;
        Some(bytes)
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.take::<1>()?[0])
    }
    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_be_bytes(self.take()?))
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_be_bytes(self.take()?))
    }
}

/// A TIFF structure, as in EXIF and MPF segments.
pub(super) struct Tiff<'a> {
    pub data: &'a [u8],
    pub big_endian: bool,
}

/// One entry of a TIFF directory. `value` holds the value itself for
/// values of up to four bytes, else the offset of it.
#[derive(Debug, Clone, Copy)]
pub(super) struct TiffEntry {
    pub tag: u16,
    pub kind: u16,
    pub count: u32,
    pub value: u32,
}

impl<'a> Tiff<'a> {
    /// A TIFF structure starting at its `II*\0` or `MM\0*` header.
    pub fn new(data: &'a [u8]) -> Option<Self> {
        let big_endian = match data.get(..4)? {
            b"II*\0" => false,
            b"MM\0*" => true,
            _ => return None,
        };
        Some(Self { data, big_endian })
    }

    pub fn u16(&self, at: usize) -> Option<u16> {
        let b = self.data.get(at..at + 2)?.try_into().ok()?;
        Some(if self.big_endian {
            u16::from_be_bytes(b)
        } else {
            u16::from_le_bytes(b)
        })
    }

    pub fn u32(&self, at: usize) -> Option<u32> {
        let b = self.data.get(at..at + 4)?.try_into().ok()?;
        Some(if self.big_endian {
            u32::from_be_bytes(b)
        } else {
            u32::from_le_bytes(b)
        })
    }

    /// The entries of the directory at `at`.
    pub fn entries(&self, at: usize) -> Option<impl Iterator<Item = TiffEntry> + '_> {
        let count = usize::from(self.u16(at)?).min(1000);
        Some((0..count).map_while(move |i| {
            let e = at + 2 + 12 * i;
            let kind = self.u16(e + 2)?;
            let value = if kind == 3 && self.u32(e + 4)? == 1 {
                u32::from(self.u16(e + 8)?)
            } else {
                self.u32(e + 8)?
            };
            Some(TiffEntry {
                tag: self.u16(e)?,
                kind,
                count: self.u32(e + 4)?,
                value,
            })
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_xmp_in_every_spelling() {
        let xmp = r#"<rdf:Description hdrgm:Version="1.0" hdrgm:GainMapMax="3.5"
            hdrgm:HDRCapacityMax='2'>
            <hdrgm:GainMapMin><rdf:Seq><rdf:li>0.1</rdf:li><rdf:li>0.2</rdf:li><rdf:li>0.3</rdf:li></rdf:Seq></hdrgm:GainMapMin>
            <HDRGainMap:HDRGainMapHeadroom>4.2</HDRGainMap:HDRGainMapHeadroom>"#;
        assert_eq!(xmp_values(xmp, "hdrgm:GainMapMax"), Some(vec![3.5]));
        assert_eq!(xmp_values(xmp, "hdrgm:HDRCapacityMax"), Some(vec![2.0]));
        assert_eq!(
            xmp_values(xmp, "hdrgm:GainMapMin"),
            Some(vec![0.1, 0.2, 0.3])
        );
        assert_eq!(apple_xmp_headroom(xmp), Some(4.2));
        assert_eq!(xmp_values(xmp, "hdrgm:Gamma"), None);
        let p = IsoParams::from_hdrgm(xmp).unwrap();
        assert_eq!(p.min, [0.1, 0.2, 0.3]);
        assert_eq!(p.max, [3.5; 3]);
        assert_eq!(p.offset_base, [1.0 / 64.0; 3]);
        assert!(p.base_is_sdr());
    }

    #[test]
    fn full_gain_restores_the_maximum() {
        let iso = IsoParams {
            min: [0.0; 3],
            max: [3.0; 3],
            gamma: [1.0; 3],
            offset_base: [0.0; 3],
            offset_alternate: [0.0; 3],
            base_headroom: 0.0,
            alternate_headroom: 3.0,
        };
        let map = GainMapImage {
            width: 1,
            height: 1,
            channels: 1,
            samples: vec![1.0],
        };
        let mut base = vec![[0.5; 3]; 4];
        apply(&mut base, 2, 2, &map, &GainMapKind::Iso(iso));
        assert_eq!(base[3], [4.0; 3]);
        let mut base = vec![[0.5; 3]];
        apply(&mut base, 1, 1, &map, &GainMapKind::Apple { headroom: 4.0 });
        assert!((base[0][0] - 2.0).abs() < 1e-5);
    }

    #[test]
    fn map_is_interpolated() {
        let map = GainMapImage {
            width: 2,
            height: 1,
            channels: 1,
            samples: vec![0.0, 1.0],
        };
        // Base pixels 0..4 over a 2-pixel map: the ends clamp to the map's
        // pixels, the middle blends them.
        let v: Vec<f32> = (0..4).map(|x| map.sample(x, 0, 4, 1)[0]).collect();
        assert_eq!(v, [0.0, 0.25, 0.75, 1.0]);
    }
}
