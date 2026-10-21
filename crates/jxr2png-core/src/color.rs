//! Color math for linear scRGB input.
//!
//! scRGB is linear light with BT.709 primaries, where 1.0 is 80 nits.
//! Values above 1.0 are HDR highlights and negative values describe colors
//! outside the BT.709 gamut.

/// Luminance of scRGB 1.0, in nits.
pub const SCRGB_WHITE_NITS: f32 = 80.0;

/// Peak luminance of the PQ curve, in nits.
pub const PQ_MAX_NITS: f32 = 10_000.0;

/// Linear BT.709 to linear BT.2020 (ITU-R BT.2087).
const BT709_TO_BT2020: [[f32; 3]; 3] = [
    [0.627_404, 0.329_283, 0.043_313],
    [0.069_097, 0.919_540, 0.011_362],
    [0.016_391, 0.088_013, 0.895_595],
];

pub fn bt709_to_bt2020(rgb: [f32; 3]) -> [f32; 3] {
    let m = &BT709_TO_BT2020;
    [
        m[0][0] * rgb[0] + m[0][1] * rgb[1] + m[0][2] * rgb[2],
        m[1][0] * rgb[0] + m[1][1] * rgb[1] + m[1][2] * rgb[2],
        m[2][0] * rgb[0] + m[2][1] * rgb[1] + m[2][2] * rgb[2],
    ]
}

/// Chromaticities of an RGB color space: the CIE xy coordinates of its red,
/// green and blue primaries and of its white point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Primaries {
    pub red: [f64; 2],
    pub green: [f64; 2],
    pub blue: [f64; 2],
    pub white: [f64; 2],
}

const D65: [f64; 2] = [0.3127, 0.3290];

impl Primaries {
    /// BT.709 and sRGB, the primaries of scRGB.
    pub const BT709: Self = Self {
        red: [0.640, 0.330],
        green: [0.300, 0.600],
        blue: [0.150, 0.060],
        white: D65,
    };
    pub const BT2020: Self = Self {
        red: [0.708, 0.292],
        green: [0.170, 0.797],
        blue: [0.131, 0.046],
        white: D65,
    };
    /// Display P3 (DCI-P3 primaries with a D65 white).
    pub const DISPLAY_P3: Self = Self {
        red: [0.680, 0.320],
        green: [0.265, 0.690],
        blue: [0.150, 0.060],
        white: D65,
    };

    /// Linear RGB to CIE XYZ, with white at Y = 1.
    fn xyz_matrix(&self) -> [[f64; 3]; 3] {
        let xyz = |[x, y]: [f64; 2]| [x / y, 1.0, (1.0 - x - y) / y];
        let (r, g, b) = (xyz(self.red), xyz(self.green), xyz(self.blue));
        let m = [[r[0], g[0], b[0]], [r[1], g[1], b[1]], [r[2], g[2], b[2]]];
        let s = mul_vec(&invert(&m), xyz(self.white));
        m.map(|row| [row[0] * s[0], row[1] * s[1], row[2] * s[2]])
    }
}

/// The matrix that converts linear RGB in `from` to linear RGB in `to`,
/// adapting the white point with the Bradford transform if they differ.
pub fn conversion_matrix(from: &Primaries, to: &Primaries) -> [[f32; 3]; 3] {
    let mut m = mul(&invert(&to.xyz_matrix()), &from.xyz_matrix());
    if from.white != to.white {
        const BRADFORD: [[f64; 3]; 3] = [
            [0.8951, 0.2664, -0.1614],
            [-0.7502, 1.7135, 0.0367],
            [0.0389, -0.0685, 1.0296],
        ];
        let xyz = |[x, y]: [f64; 2]| [x / y, 1.0, (1.0 - x - y) / y];
        let (src, dst) = (
            mul_vec(&BRADFORD, xyz(from.white)),
            mul_vec(&BRADFORD, xyz(to.white)),
        );
        let scale = [
            [dst[0] / src[0], 0.0, 0.0],
            [0.0, dst[1] / src[1], 0.0],
            [0.0, 0.0, dst[2] / src[2]],
        ];
        let adapt = mul(&invert(&BRADFORD), &mul(&scale, &BRADFORD));
        m = mul(&invert(&to.xyz_matrix()), &mul(&adapt, &from.xyz_matrix()));
    }
    m.map(|row| row.map(|v| v as f32))
}

/// Applies a 3×3 color matrix to one pixel.
pub fn apply_matrix(m: &[[f32; 3]; 3], rgb: [f32; 3]) -> [f32; 3] {
    [
        m[0][0] * rgb[0] + m[0][1] * rgb[1] + m[0][2] * rgb[2],
        m[1][0] * rgb[0] + m[1][1] * rgb[1] + m[1][2] * rgb[2],
        m[2][0] * rgb[0] + m[2][1] * rgb[1] + m[2][2] * rgb[2],
    ]
}

fn mul(a: &[[f64; 3]; 3], b: &[[f64; 3]; 3]) -> [[f64; 3]; 3] {
    std::array::from_fn(|i| std::array::from_fn(|j| (0..3).map(|k| a[i][k] * b[k][j]).sum()))
}

fn mul_vec(m: &[[f64; 3]; 3], v: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| (0..3).map(|k| m[i][k] * v[k]).sum())
}

fn invert(m: &[[f64; 3]; 3]) -> [[f64; 3]; 3] {
    let cofactor = |r: usize, c: usize| {
        let (r1, r2) = ((r + 1) % 3, (r + 2) % 3);
        let (c1, c2) = ((c + 1) % 3, (c + 2) % 3);
        m[r1][c1] * m[r2][c2] - m[r1][c2] * m[r2][c1]
    };
    let det: f64 = (0..3).map(|c| m[0][c] * cofactor(0, c)).sum();
    std::array::from_fn(|i| std::array::from_fn(|j| cofactor(j, i) / det))
}

/// HLG (ARIB STD-B67) inverse OETF: a 0..1 signal to relative scene light
/// in 0..1.
pub fn hlg_decode(signal: f32) -> f32 {
    const A: f32 = 0.178_832_77;
    const B: f32 = 0.284_668_92;
    const C: f32 = 0.559_910_7;
    let e = signal.clamp(0.0, 1.0);
    if e <= 0.5 {
        e * e / 3.0
    } else {
        (((e - C) / A).exp() + B) / 12.0
    }
}

/// SMPTE ST 2084 (PQ) inverse EOTF: absolute luminance in nits to a 0..1 signal.
pub fn pq_encode(nits: f32) -> f32 {
    const M1: f32 = 2610.0 / 16384.0;
    const M2: f32 = 2523.0 / 4096.0 * 128.0;
    const C1: f32 = 3424.0 / 4096.0;
    const C2: f32 = 2413.0 / 4096.0 * 32.0;
    const C3: f32 = 2392.0 / 4096.0 * 32.0;
    let y = (nits / PQ_MAX_NITS).clamp(0.0, 1.0).powf(M1);
    ((C1 + C2 * y) / (1.0 + C3 * y)).powf(M2)
}

/// SMPTE ST 2084 (PQ) EOTF: a 0..1 signal to absolute luminance in nits.
pub fn pq_decode(signal: f32) -> f32 {
    const M1: f32 = 2610.0 / 16384.0;
    const M2: f32 = 2523.0 / 4096.0 * 128.0;
    const C1: f32 = 3424.0 / 4096.0;
    const C2: f32 = 2413.0 / 4096.0 * 32.0;
    const C3: f32 = 2392.0 / 4096.0 * 32.0;
    let e = signal.clamp(0.0, 1.0).powf(1.0 / M2);
    let y = ((e - C1).max(0.0) / (C2 - C3 * e)).powf(1.0 / M1);
    y * PQ_MAX_NITS
}

/// sRGB OETF: linear 0..1 to an encoded 0..1 signal.
pub fn srgb_encode(v: f32) -> f32 {
    let v = v.clamp(0.0, 1.0);
    if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

/// sRGB EOTF: encoded 0..1 signal to linear 0..1.
pub fn srgb_decode(v: f32) -> f32 {
    if v <= 0.040_45 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// Highlight compression for SDR output.
///
/// Works on values relative to SDR white (1.0). Below `knee` the curve is the
/// identity; above it, an extended Reinhard curve rolls the range
/// `knee..peak` smoothly into `knee..1`, with matching slope at the knee.
#[derive(Debug, Clone, Copy)]
pub struct ToneCurve {
    knee: f32,
    /// Peak input, expressed in units of the compressed range above the knee.
    white: f32,
}

impl ToneCurve {
    pub const DEFAULT_KNEE: f32 = 0.75;

    /// `peak` is the input value (relative to SDR white) that should map to 1.0.
    pub fn new(peak: f32) -> Self {
        let knee = Self::DEFAULT_KNEE;
        let range = 1.0 - knee;
        Self {
            knee,
            white: ((peak - knee) / range).max(1.0),
        }
    }

    pub fn apply(&self, v: f32) -> f32 {
        if v <= self.knee {
            return v;
        }
        let range = 1.0 - self.knee;
        let x = (v - self.knee) / range;
        let w2 = self.white * self.white;
        let y = x * (1.0 + x / w2) / (1.0 + x);
        (self.knee + range * y).min(1.0)
    }
}

/// Map one scRGB pixel to linear BT.709 SDR in 0..1.
///
/// `white` is the scRGB value treated as SDR white. The curve is driven by
/// the largest channel, and all channels are scaled by the same factor so
/// hue and saturation are kept and no channel clips.
pub fn tone_map_sdr(rgb: [f32; 3], white: f32, curve: &ToneCurve) -> [f32; 3] {
    let rgb = rgb.map(|c| c.max(0.0) / white);
    let peak = rgb[0].max(rgb[1]).max(rgb[2]);
    if peak <= 0.0 {
        return [0.0; 3];
    }
    let scale = curve.apply(peak) / peak;
    rgb.map(|c| c * scale)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32, eps: f32) -> bool {
        (a - b).abs() <= eps
    }

    #[test]
    fn pq_reference_points() {
        assert!(close(pq_encode(0.0), 0.0, 1e-6));
        assert!(close(pq_encode(10_000.0), 1.0, 1e-5));
        // BT.2100 / BT.2408 reference values.
        assert!(close(pq_encode(100.0), 0.508, 1e-3));
        assert!(close(pq_encode(203.0), 0.58, 1e-3));
        assert!(close(pq_encode(1000.0), 0.752, 1e-3));
    }

    #[test]
    fn pq_roundtrip() {
        for nits in [0.0, 0.5, 80.0, 203.0, 1000.0, 4000.0, 10_000.0] {
            let back = pq_decode(pq_encode(nits));
            assert!(close(back, nits, nits * 1e-3 + 1e-3), "{nits} -> {back}");
        }
    }

    #[test]
    fn bt2020_matrix_keeps_white() {
        let w = bt709_to_bt2020([1.0, 1.0, 1.0]);
        for c in w {
            assert!(close(c, 1.0, 1e-4));
        }
    }

    #[test]
    fn conversion_matrix_matches_bt2087() {
        let m = conversion_matrix(&Primaries::BT709, &Primaries::BT2020);
        for (row, expected) in m.iter().zip(&BT709_TO_BT2020) {
            for (a, b) in row.iter().zip(expected) {
                assert!(close(*a, *b, 1e-4), "{m:?}");
            }
        }
        // Going there and back is the identity.
        let back = conversion_matrix(&Primaries::BT2020, &Primaries::BT709);
        let rgb = apply_matrix(&back, apply_matrix(&m, [0.2, 0.5, 0.9]));
        for (a, b) in rgb.iter().zip([0.2, 0.5, 0.9]) {
            assert!(close(*a, b, 1e-5));
        }
    }

    #[test]
    fn white_adaptation_keeps_white_neutral() {
        // ACES AP0 has a D60-ish white; its white must stay white in BT.709.
        let aces = Primaries {
            red: [0.7347, 0.2653],
            green: [0.0, 1.0],
            blue: [0.0001, -0.077],
            white: [0.32168, 0.33767],
        };
        let white = apply_matrix(&conversion_matrix(&aces, &Primaries::BT709), [1.0; 3]);
        for c in white {
            assert!(close(c, 1.0, 1e-3), "{white:?}");
        }
    }

    #[test]
    fn hlg_reference_points() {
        assert!(close(hlg_decode(0.0), 0.0, 1e-6));
        assert!(close(hlg_decode(0.5), 1.0 / 12.0, 1e-6));
        assert!(close(hlg_decode(1.0), 1.0, 1e-4));
    }

    #[test]
    fn srgb_roundtrip() {
        for i in 0..=100 {
            let v = i as f32 / 100.0;
            assert!(close(srgb_decode(srgb_encode(v)), v, 1e-5));
        }
    }

    #[test]
    fn tone_curve_is_identity_below_knee_and_hits_one_at_peak() {
        let curve = ToneCurve::new(5.0);
        assert_eq!(curve.apply(0.5), 0.5);
        assert!(close(curve.apply(5.0), 1.0, 1e-5));
        let mut prev = 0.0;
        for i in 0..1000 {
            let y = curve.apply(i as f32 / 100.0);
            assert!(y >= prev && y <= 1.0);
            prev = y;
        }
    }

    #[test]
    fn tone_map_keeps_ratios() {
        let curve = ToneCurve::new(10.0);
        let out = tone_map_sdr([8.0, 4.0, 2.0], 1.0, &curve);
        assert!(close(out[1] / out[0], 0.5, 1e-5));
        assert!(close(out[2] / out[0], 0.25, 1e-5));
        assert!(out[0] <= 1.0);
    }
}
