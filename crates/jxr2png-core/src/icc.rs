//! ICC profile for the HDR PNG, used by viewers that ignore `cICP`.
//!
//! PNG decoders that understand `cICP` (Chrome, Edge, Safari, Photoshop)
//! use it and ignore this profile. Viewers that only do ICC color
//! management, such as the Windows Photo Viewer, can't show HDR, so the
//! profile's tone curves bake in the same highlight roll-off as the SDR
//! output. Without it they read PQ values as sRGB and the picture looks
//! grey and washed out.
//!
//! The profile is ICC v2 with matrix/TRC tags, the form every color
//! management engine supports, including Windows ICM.

use crate::color::{ToneCurve, pq_decode};

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
const D65: [f64; 3] = [0.950_455, 1.0, 1.089_058];

const TRC_ENTRIES: usize = 4096;

pub const PROFILE_NAME: &str = "BT.2100 PQ with SDR fallback";

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
    let tags: Vec<([u8; 4], Vec<u8>)> = vec![
        (*b"desc", text_description(PROFILE_NAME)),
        (*b"cprt", text("No copyright, use freely")),
        (*b"wtpt", xyz(D65)),
        (*b"rXYZ", xyz(colorants[0])),
        (*b"gXYZ", xyz(colorants[1])),
        (*b"bXYZ", xyz(colorants[2])),
        (*b"rTRC", curv(&trc)),
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
    out.extend_from_slice(&[2, 0x40, 0, 0]); // version 2.4
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

fn text(s: &str) -> Vec<u8> {
    let mut out = b"text\0\0\0\0".to_vec();
    out.extend_from_slice(s.as_bytes());
    out.push(0);
    out
}

/// ICC v2 `textDescriptionType`: ASCII text with empty Unicode and
/// ScriptCode parts.
fn text_description(s: &str) -> Vec<u8> {
    let mut out = b"desc\0\0\0\0".to_vec();
    out.extend_from_slice(&(s.len() as u32 + 1).to_be_bytes());
    out.extend_from_slice(s.as_bytes());
    out.push(0);
    out.extend_from_slice(&[0; 4]); // Unicode language code
    out.extend_from_slice(&[0; 4]); // Unicode count
    out.extend_from_slice(&[0; 2]); // ScriptCode code
    out.push(0); // ScriptCode count
    out.extend_from_slice(&[0; 67]);
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
        assert_eq!(&p[36..40], b"acsp");
        let count = u32::from_be_bytes(p[128..132].try_into().unwrap()) as usize;
        assert_eq!(count, 9);
        for i in 0..count {
            let entry = &p[132 + i * 12..144 + i * 12];
            let offset = u32::from_be_bytes(entry[4..8].try_into().unwrap()) as usize;
            let len = u32::from_be_bytes(entry[8..12].try_into().unwrap()) as usize;
            assert_eq!(offset % 4, 0);
            assert!(offset + len <= p.len());
        }
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
