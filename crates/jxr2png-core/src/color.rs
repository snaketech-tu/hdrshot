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
    fn bt2020_matrix_keeps_white() {
        let w = bt709_to_bt2020([1.0, 1.0, 1.0]);
        for c in w {
            assert!(close(c, 1.0, 1e-4));
        }
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
