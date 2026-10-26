//! The ISO base media file format boxes that HEIF and AVIF files share:
//! which items a file holds, how they relate and where their data is.

use super::gainmap::IsoParams;

const APPLE_GAIN_MAP: &[u8] = b"urn:com:apple:photo:2020:aux:hdrgainmap";

/// The major and compatible brands of the `ftyp` box a file starts with.
pub(super) fn brands(head: &[u8]) -> Vec<[u8; 4]> {
    let Some(size) = head
        .get(..4)
        .map(|b| u32::from_be_bytes(b.try_into().unwrap()))
    else {
        return Vec::new();
    };
    if head.get(4..8) != Some(b"ftyp") {
        return Vec::new();
    }
    let ftyp = head.get(8..(size as usize).min(head.len())).unwrap_or(&[]);
    ftyp.as_chunks::<4>()
        .0
        .iter()
        .enumerate()
        // The minor version, second, isn't a brand.
        .filter(|(i, _)| *i != 1)
        .map(|(_, b)| *b)
        .collect()
}

/// The boxes of a HEIF `meta` box that say which item is what.
#[derive(Default)]
pub(super) struct Meta {
    pub primary: u32,
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
    pub(super) fn parse(file: &[u8]) -> Option<Self> {
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

    /// The items that item `from` refers to with references of `kind`,
    /// such as the tiles of a grid (`dimg`).
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn references(&self, kind: &[u8; 4], from: u32) -> Option<&[u32]> {
        self.refs
            .iter()
            .find(|r| &r.0 == kind && r.1 == from)
            .map(|r| r.2.as_slice())
    }

    /// The properties of item `id` in their order, with their bodies.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn properties_of<'a>(
        &'a self,
        file: &'a [u8],
        id: u32,
    ) -> impl Iterator<Item = ([u8; 4], &'a [u8])> + 'a {
        self.associations
            .iter()
            .filter(move |a| a.0 == id)
            .flat_map(|a| a.1.iter())
            .filter_map(move |&i| {
                let (k, body) = self.properties.get(usize::from(i).checked_sub(1)?)?;
                Some((*k, file.get(body.clone())?))
            })
    }

    pub(super) fn item_type(&self, id: u32) -> Option<[u8; 4]> {
        self.items.iter().find(|i| i.0 == id).map(|i| i.1)
    }

    pub(super) fn property(&self, id: u32, kind: &[u8; 4]) -> Option<std::ops::Range<usize>> {
        let (_, indexes) = self.associations.iter().find(|a| a.0 == id)?;
        indexes.iter().find_map(|&i| {
            let (k, body) = self.properties.get(usize::from(i).checked_sub(1)?)?;
            (k == kind).then(|| body.clone())
        })
    }

    /// The bytes of an item, such as a `tmap` item's metadata.
    pub(super) fn item_data(&self, file: &[u8], id: u32) -> Option<Vec<u8>> {
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
    pub(super) fn iso_gain_map(&self, file: &[u8]) -> Option<(u32, IsoParams)> {
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
    pub(super) fn apple_gain_map(&self, file: &[u8]) -> Option<u32> {
        self.auxiliary(file, |urn| urn == APPLE_GAIN_MAP)
    }

    /// The primary image's alpha plane, an auxiliary image.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn alpha(&self, file: &[u8]) -> Option<u32> {
        self.auxiliary(file, |urn| {
            urn == b"urn:mpeg:mpegB:cicp:systems:auxiliary:alpha"
                || urn == b"urn:mpeg:hevc:2015:auxid:1"
        })
    }

    /// The auxiliary image of the primary one whose type `is_type` accepts.
    fn auxiliary(&self, file: &[u8], is_type: impl Fn(&[u8]) -> bool) -> Option<u32> {
        self.refs.iter().find_map(|(kind, from, to)| {
            if kind != b"auxl" || !to.contains(&self.primary) {
                return None;
            }
            let aux = &file[self.property(*from, b"auxC")?];
            // A full box: version and flags, then the type as a C string.
            let urn = aux.get(4..)?.split(|&b| b == 0).next()?;
            is_type(urn).then_some(*from)
        })
    }

    /// A copy of `file` whose primary image is item `id`.
    pub(super) fn with_primary(&self, file: &[u8], id: u32) -> Option<Vec<u8>> {
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
