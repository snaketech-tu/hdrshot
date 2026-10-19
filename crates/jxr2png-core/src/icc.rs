//! ICC profile for the HDR PNG.
//!
//! The profile is ICC v4.4 and carries a `cicp` tag (BT.2020, PQ, full
//! range), matching the PNG's `cICP` chunk. Chrome and Edge detect HDR by
//! the tag alone, which keeps Discord copies without the chunk in HDR (see
//! [`crate::for_discord`]); Windows Photos also needs the chunk.
//!
//! Otherwise it is a plain matrix/TRC profile describing the pixels as they
//! are: BT.2020 primaries and the PQ curve, with 10,000 nits as 1.0. There
//! is no SDR fallback, so apps that only do SDR color management show the
//! picture dark.

use crate::color::pq_decode;

/// Linear BT.2020 to CIE XYZ (D65).
const BT2020_TO_XYZ: [[f64; 3]; 3] = [
    [0.636_958, 0.144_617, 0.168_881],
    [0.262_700, 0.677_998, 0.059_302],
    [0.000_000, 0.028_073, 1.060_985],
];

/// Bradford chromatic adaptation from D65 to D50 (the ICC PCS white).
const BRADFORD_D65_TO_D50: [[f64; 3]; 3] = [
    [1.047_811_2, 0.022_886_6, -0.050_127_0],
    [0.029_542_4, 0.990_484_4, -0.017_049_1],
    [-0.009_234_5, 0.015_043_6, 0.752_131_6],
];

const D50: [f64; 3] = [0.9642, 1.0, 0.8249];

/// Entries in the PQ `curv` table, shared by the three channels.
const TRC_ENTRIES: usize = 1024;

/// PQ's absolute peak, which the TRC maps to 1.0.
const PQ_PEAK_NITS: f32 = 10_000.0;

/// Largest profile some services keep (Discord drops bigger ones).
pub const MAX_PROFILE_SIZE: usize = 32 * 1024;

pub const PROFILE_NAME: &str = "BT.2100 PQ";

/// Coding-independent code points for the `cicp` tag: BT.2020 primaries,
/// PQ transfer, RGB (identity matrix), full range.
const CICP: [u8; 4] = [9, 16, 0, 1];

/// Build the BT.2100 PQ profile.
pub fn pq_profile() -> Vec<u8> {
    let colorants = adapted_colorants();
    let trc = pq_trc();
    let tags: Vec<([u8; 4], Vec<u8>)> = vec![
        (*b"desc", mluc(PROFILE_NAME)),
        (*b"cprt", mluc("No copyright, use freely")),
        (*b"wtpt", xyz(D50)),
        (*b"chad", sf32(&BRADFORD_D65_TO_D50)),
        (*b"cicp", cicp()),
        (*b"rXYZ", xyz(colorants[0])),
        (*b"gXYZ", xyz(colorants[1])),
        (*b"bXYZ", xyz(colorants[2])),
        (*b"rTRC", trc.clone()),
        (*b"gTRC", trc.clone()),
        (*b"bTRC", trc),
    ];
    let tag_count = tags.len();

    // Tags with identical contents share one copy, as the spec allows.
    let mut data: Vec<u8> = Vec::new();
    let mut table = Vec::new();
    let mut stored: Vec<(&[u8], usize)> = Vec::new();
    let base = 128 + 4 + 12 * tag_count;
    for (sig, body) in &tags {
        let offset = match stored.iter().find(|(b, _)| *b == body.as_slice()) {
            Some(&(_, offset)) => offset,
            None => {
                let offset = base + data.len();
                data.extend_from_slice(body);
                while !data.len().is_multiple_of(4) {
                    data.push(0);
                }
                stored.push((body, offset));
                offset
            }
        };
        table.push((*sig, offset, body.len()));
    }

    let size = base + data.len();
    let mut out = Vec::with_capacity(size);
    // Header.
    out.extend_from_slice(&(size as u32).to_be_bytes());
    out.extend_from_slice(&[0; 4]); // preferred CMM
    out.extend_from_slice(&[4, 0x40, 0, 0]); // version 4.4
    out.extend_from_slice(b"mntr");
    out.extend_from_slice(b"RGB ");
    out.extend_from_slice(b"XYZ ");
    for v in [2026u16, 10, 19, 0, 0, 0] {
        out.extend_from_slice(&v.to_be_bytes());
    }
    out.extend_from_slice(b"acsp");
    out.extend_from_slice(&[0; 4]); // platform
    out.extend_from_slice(&[0; 4]); // flags
    out.extend_from_slice(&[0; 4]); // manufacturer
    out.extend_from_slice(&[0; 4]); // model
    out.extend_from_slice(&[0; 8]); // attributes
    // Relative colorimetric, as in libjxl's PQ profile.
    out.extend_from_slice(&1u32.to_be_bytes());
    for v in D50 {
        out.extend_from_slice(&s15f16(v));
    }
    out.extend_from_slice(&[0; 4]); // creator
    out.extend_from_slice(&[0; 16]); // profile ID
    out.extend_from_slice(&[0; 28]); // reserved
    debug_assert_eq!(out.len(), 128);

    out.extend_from_slice(&(tag_count as u32).to_be_bytes());
    for (sig, offset, len) in table {
        out.extend_from_slice(&sig);
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&(len as u32).to_be_bytes());
    }
    out.extend_from_slice(&data);
    debug_assert_eq!(out.len(), size);
    out
}

/// `curveType` table of the PQ EOTF, normalized so 10,000 nits is 1.0.
fn pq_trc() -> Vec<u8> {
    let mut out = b"curv\0\0\0\0".to_vec();
    out.extend_from_slice(&(TRC_ENTRIES as u32).to_be_bytes());
    for i in 0..TRC_ENTRIES {
        let linear = pq_decode(i as f32 / (TRC_ENTRIES - 1) as f32) / PQ_PEAK_NITS;
        out.extend_from_slice(&((linear.clamp(0.0, 1.0) * 65535.0).round() as u16).to_be_bytes());
    }
    out
}

/// BT.2020 primaries in XYZ, adapted to D50, as (red, green, blue).
fn adapted_colorants() -> [[f64; 3]; 3] {
    let mut m = [[0.0; 3]; 3];
    for (i, row) in m.iter_mut().enumerate() {
        for (j, cell) in row.iter_mut().enumerate() {
            *cell = (0..3)
                .map(|k| BRADFORD_D65_TO_D50[i][k] * BT2020_TO_XYZ[k][j])
                .sum();
        }
    }
    [0, 1, 2].map(|c| [m[0][c], m[1][c], m[2][c]])
}

fn s15f16(v: f64) -> [u8; 4] {
    ((v * 65536.0).round() as i32).to_be_bytes()
}

fn xyz(v: [f64; 3]) -> Vec<u8> {
    let mut out = b"XYZ \0\0\0\0".to_vec();
    for c in v {
        out.extend_from_slice(&s15f16(c));
    }
    out
}

fn sf32(m: &[[f64; 3]; 3]) -> Vec<u8> {
    let mut out = b"sf32\0\0\0\0".to_vec();
    for v in m.iter().flatten() {
        out.extend_from_slice(&s15f16(*v));
    }
    out
}

fn cicp() -> Vec<u8> {
    let mut out = b"cicp\0\0\0\0".to_vec();
    out.extend_from_slice(&CICP);
    out
}

/// `multiLocalizedUnicodeType` with a single en-US UTF-16 record.
fn mluc(s: &str) -> Vec<u8> {
    let text: Vec<u8> = s.encode_utf16().flat_map(u16::to_be_bytes).collect();
    let mut out = b"mluc\0\0\0\0".to_vec();
    out.extend_from_slice(&1u32.to_be_bytes()); // record count
    out.extend_from_slice(&12u32.to_be_bytes()); // record size
    out.extend_from_slice(b"enUS");
    out.extend_from_slice(&(text.len() as u32).to_be_bytes());
    out.extend_from_slice(&28u32.to_be_bytes()); // offset of the text
    out.extend_from_slice(&text);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_is_well_formed() {
        let p = pq_profile();
        assert_eq!(
            u32::from_be_bytes(p[0..4].try_into().unwrap()) as usize,
            p.len()
        );
        assert!(p.len() <= MAX_PROFILE_SIZE, "{} bytes", p.len());
        assert_eq!(&p[8..12], &[4, 0x40, 0, 0]);
        assert_eq!(&p[36..40], b"acsp");
        let count = u32::from_be_bytes(p[128..132].try_into().unwrap()) as usize;
        assert_eq!(count, 11);
        let mut cicp = None;
        for i in 0..count {
            let entry = &p[132 + i * 12..144 + i * 12];
            let offset = u32::from_be_bytes(entry[4..8].try_into().unwrap()) as usize;
            let len = u32::from_be_bytes(entry[8..12].try_into().unwrap()) as usize;
            assert_eq!(offset % 4, 0);
            assert!(offset + len <= p.len());
            if &entry[0..4] == b"cicp" {
                cicp = Some(&p[offset..offset + len]);
            }
        }
        assert_eq!(cicp, Some(&b"cicp\0\0\0\0\x09\x10\x00\x01"[..]));
    }

    #[test]
    fn colorants_sum_to_d50() {
        let c = adapted_colorants();
        for axis in 0..3 {
            let sum: f64 = c.iter().map(|p| p[axis]).sum();
            assert!((sum - D50[axis]).abs() < 2e-3, "{axis}: {sum}");
        }
    }
}
