//! ICC profile for the HDR PNG.
//!
//! The profile is ICC v4.4 and carries a `cicp` tag (BT.2020, PQ, full
//! range). Windows recognises HDR images by that tag, not by the PNG
//! `cICP` chunk, and then shows the pixels as HDR like the JXR itself.
//! Browsers and other apps that read the PNG `cICP` chunk ignore the
//! profile.
//!
//! Viewers that only do SDR color management, such as the Windows Photo
//! Viewer, use the profile's lookup tables instead, so they bake in the
//! same highlight roll-off as the SDR output. Without them they read PQ
//! values as sRGB and the picture looks grey and washed out. The `A2B0`
//! table applies the roll-off to each pixel's brightest channel, exactly
//! like the SDR PNG, so bright saturated lights keep their hue. The
//! matrix/TRC tags are a simpler per-channel approximation for engines that
//! ignore lookup tables.

use crate::color::{ToneCurve, pq_decode, tone_map_sdr};

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

const TRC_ENTRIES: usize = 4096;

/// Grid points per axis of the `A2B0` lookup table, spaced evenly in PQ.
const CLUT_POINTS: usize = 33;

/// Entries in the `A2B0` output curves.
const LUT_OUTPUT_ENTRIES: usize = 4096;

/// The CLUT stores encoded XYZ raised to this power and the output curves
/// undo it, which keeps precision in the shadows.
const LUT_GAMMA: f64 = 2.4;

/// Largest XYZ value the 16-bit ICC PCS encoding can hold (0x8000 is 1.0).
const PCS_XYZ_MAX: f64 = 1.0 + 32767.0 / 32768.0;

pub const PROFILE_NAME: &str = "BT.2100 PQ with SDR fallback";

/// Coding-independent code points for the `cicp` tag: BT.2020 primaries,
/// PQ transfer, RGB (identity matrix), full range.
const CICP: [u8; 4] = [9, 16, 0, 1];

/// Build a profile whose curves map PQ to SDR: `white_nits` becomes display
/// white, and highlights roll off up to `peak_nits`.
pub fn pq_sdr_fallback_profile(white_nits: f32, peak_nits: f32) -> Vec<u8> {
    let curve = ToneCurve::new(peak_nits.max(white_nits) / white_nits);
    let trc: Vec<u16> = (0..TRC_ENTRIES)
        .map(|i| {
            let nits = pq_decode(i as f32 / (TRC_ENTRIES - 1) as f32);
            let v = curve.apply(nits / white_nits).clamp(0.0, 1.0);
            (v * 65535.0).round() as u16
        })
        .collect();

    let colorants = adapted_colorants();
    let a2b0 = lut16(&colorants, &curve, white_nits);
    let tags: Vec<([u8; 4], Vec<u8>)> = vec![
        (*b"desc", mluc(PROFILE_NAME)),
        (*b"cprt", mluc("No copyright, use freely")),
        (*b"wtpt", xyz(D50)),
        (*b"chad", sf32(&BRADFORD_D65_TO_D50)),
        (*b"cicp", cicp()),
        (*b"rXYZ", xyz(colorants[0])),
        (*b"gXYZ", xyz(colorants[1])),
        (*b"bXYZ", xyz(colorants[2])),
        (*b"rTRC", curv(&trc)),
        (*b"A2B0", a2b0),
    ];
    // gTRC and bTRC share the rTRC data.
    let tag_count = tags.len() + 2;

    let mut data = Vec::new();
    let mut table = Vec::new();
    let mut offset = 128 + 4 + 12 * tag_count;
    let mut trc_entry = (0, 0);
    for (sig, body) in &tags {
        table.push((*sig, offset, body.len()));
        if sig == b"rTRC" {
            trc_entry = (offset, body.len());
        }
        data.extend_from_slice(body);
        offset += body.len();
        while data.len() % 4 != 0 {
            data.push(0);
            offset += 1;
        }
    }
    table.push((*b"gTRC", trc_entry.0, trc_entry.1));
    table.push((*b"bTRC", trc_entry.0, trc_entry.1));

    let size = 128 + 4 + 12 * tag_count + data.len();
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
    out.extend_from_slice(&0u32.to_be_bytes()); // perceptual intent
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

/// `lut16Type` mapping PQ BT.2020 RGB to PCS XYZ with the hue-preserving
/// SDR roll-off of [`crate::color::tone_map_sdr`].
fn lut16(colorants: &[[f64; 3]; 3], curve: &ToneCurve, white_nits: f32) -> Vec<u8> {
    let mut out = b"mft2\0\0\0\0".to_vec();
    out.extend_from_slice(&[3, 3, CLUT_POINTS as u8, 0]);
    // Identity matrix (only used with XYZ input).
    for (i, j) in (0..3).flat_map(|i| (0..3).map(move |j| (i, j))) {
        out.extend_from_slice(&s15f16(if i == j { 1.0 } else { 0.0 }));
    }
    out.extend_from_slice(&2u16.to_be_bytes());
    out.extend_from_slice(&(LUT_OUTPUT_ENTRIES as u16).to_be_bytes());
    // Identity input curves.
    for _ in 0..3 {
        out.extend_from_slice(&0u16.to_be_bytes());
        out.extend_from_slice(&u16::MAX.to_be_bytes());
    }
    let node = |i: usize| pq_decode(i as f32 / (CLUT_POINTS - 1) as f32) / white_nits;
    for r in 0..CLUT_POINTS {
        for g in 0..CLUT_POINTS {
            for b in 0..CLUT_POINTS {
                let rgb = tone_map_sdr([node(r), node(g), node(b)], 1.0, curve);
                for row in colorants_rows(colorants) {
                    let xyz: f64 = (0..3).map(|k| row[k] * f64::from(rgb[k])).sum();
                    let v = (xyz.max(0.0) / PCS_XYZ_MAX).min(1.0).powf(1.0 / LUT_GAMMA);
                    out.extend_from_slice(&((v * 65535.0).round() as u16).to_be_bytes());
                }
            }
        }
    }
    for _ in 0..3 {
        for i in 0..LUT_OUTPUT_ENTRIES {
            let v = (i as f64 / (LUT_OUTPUT_ENTRIES - 1) as f64).powf(LUT_GAMMA);
            out.extend_from_slice(&((v * 65535.0).round() as u16).to_be_bytes());
        }
    }
    out
}

/// Rows of the RGB to XYZ matrix whose columns are `colorants`.
fn colorants_rows(colorants: &[[f64; 3]; 3]) -> [[f64; 3]; 3] {
    [0, 1, 2].map(|axis| [colorants[0][axis], colorants[1][axis], colorants[2][axis]])
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

fn curv(table: &[u16]) -> Vec<u8> {
    let mut out = b"curv\0\0\0\0".to_vec();
    out.extend_from_slice(&(table.len() as u32).to_be_bytes());
    for v in table {
        out.extend_from_slice(&v.to_be_bytes());
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
        let p = pq_sdr_fallback_profile(203.0, 1000.0);
        assert_eq!(
            u32::from_be_bytes(p[0..4].try_into().unwrap()) as usize,
            p.len()
        );
        assert_eq!(&p[8..12], &[4, 0x40, 0, 0]);
        assert_eq!(&p[36..40], b"acsp");
        let count = u32::from_be_bytes(p[128..132].try_into().unwrap()) as usize;
        assert_eq!(count, 12);
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
