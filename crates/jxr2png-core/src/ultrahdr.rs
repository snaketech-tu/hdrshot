//! Ultra HDR output: an SDR JPEG with a gain map that restores the HDR
//! picture.
//!
//! Every JPEG viewer shows the base image, which is the same tone-mapped
//! picture as the SDR PNG. Viewers that understand gain maps (Chrome, Edge,
//! Android, Instagram and Threads) brighten it by the gain map, up to what
//! the display can show, to recover the HDR picture.
//!
//! The file follows Google's Ultra HDR 1.1 layout: the base JPEG carries an
//! XMP container directory and an MPF index of the two images, and the gain
//! map JPEG carries its parameters both as Adobe `hdrgm` XMP and as
//! ISO 21496-1 binary metadata.
//!
//! The SDR tone map scales all three channels of a pixel by the same factor
//! (see [`crate::color::tone_map_sdr`]), so a single-channel gain map
//! restores HDR pixels exactly, apart from 8-bit rounding and colors outside
//! the sRGB gamut, which the base image can't hold.

use std::io::Write;

use jpeg_encoder::{ColorType, Encoder, SamplingFactor};
use rayon::prelude::*;

use crate::color::{SCRGB_WHITE_NITS, srgb_decode};
use crate::decode::ScRgbImage;
use crate::encode::{HdrTransform, LightLevels, sdr_rgb8};
use crate::{Error, HdrOptions, Result, SdrOptions};

/// JPEG quality of the base image. Screenshots have text and UI edges, so
/// it is high and keeps full color resolution.
const BASE_QUALITY: u8 = 95;

/// JPEG quality of the gain map, which is smooth and forgiving.
const GAIN_MAP_QUALITY: u8 = 90;

/// Added to SDR and HDR values before taking their ratio, so black pixels
/// don't need infinite gain. 1/64, as libultrahdr uses.
const OFFSET: f32 = 1.0 / 64.0;

/// Gain map parameters, in log2 units like the `hdrgm` XMP.
#[derive(Debug, Clone, Copy, PartialEq)]
struct GainMapParams {
    min_log2: f32,
    max_log2: f32,
    /// Display headroom, in stops, from which the full gain applies.
    capacity_max: f32,
}

/// Writes `image` as an Ultra HDR JPEG.
pub fn write_ultra_hdr<W: Write>(
    image: &ScRgbImage,
    levels: &LightLevels,
    hdr: &HdrOptions,
    sdr: &SdrOptions,
    mut out: W,
) -> Result<()> {
    let (width, height) = (
        u16::try_from(image.width).map_err(|_| too_large())?,
        u16::try_from(image.height).map_err(|_| too_large())?,
    );
    let base = sdr_rgb8(image, levels, sdr);
    let (gain_map, params) = gain_map(image, &base, hdr, sdr);

    let base_jpeg = encode_jpeg(&base, width, height, ColorType::Rgb, BASE_QUALITY)?;
    let map_jpeg = encode_jpeg(&gain_map, width, height, ColorType::Luma, GAIN_MAP_QUALITY)?;
    let file = assemble(&base_jpeg, &map_jpeg, &params);
    out.write_all(&file)?;
    Ok(())
}

fn too_large() -> Error {
    Error::EncodeUltraHdr("JPEG images can be at most 65,535 pixels wide or tall".into())
}

/// The 8-bit gain map for the HDR picture over the SDR base `base`, and its
/// parameters.
fn gain_map(
    image: &ScRgbImage,
    base: &[u8],
    hdr: &HdrOptions,
    sdr: &SdrOptions,
) -> (Vec<u8>, GainMapParams) {
    let transform = HdrTransform::new(image, hdr);
    // The gain map works in units of SDR white, which viewers put at the
    // display's SDR white level, as Windows does with scRGB 1.0.
    let to_relative = SCRGB_WHITE_NITS / sdr.white_nits;
    let log_gains: Vec<f32> = image
        .pixels
        .par_iter()
        .zip(base.par_chunks_exact(3))
        .map(|(&p, rgb)| {
            let hdr = p.into_iter().fold(0.0, f32::max).max(0.0) * transform.scale(p) * to_relative;
            let sdr = rgb
                .iter()
                .map(|&v| srgb_decode(f32::from(v) / 255.0))
                .fold(0.0, f32::max);
            ((hdr + OFFSET) / (sdr + OFFSET)).log2()
        })
        .collect();
    let min_log2 = log_gains
        .par_iter()
        .copied()
        .reduce(|| f32::INFINITY, f32::min)
        .min(0.0);
    let max_log2 = log_gains
        .par_iter()
        .copied()
        .reduce(|| f32::NEG_INFINITY, f32::max)
        // A picture with no highlights still gets a valid, tiny range.
        .max(min_log2 + 1.0 / 64.0);
    let range = max_log2 - min_log2;
    let map = log_gains
        .par_iter()
        .map(|&g| (((g - min_log2) / range).clamp(0.0, 1.0) * 255.0).round() as u8)
        .collect();
    (
        map,
        GainMapParams {
            min_log2,
            max_log2,
            capacity_max: max_log2.max(1.0 / 64.0),
        },
    )
}

fn encode_jpeg(
    data: &[u8],
    width: u16,
    height: u16,
    color: ColorType,
    quality: u8,
) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut encoder = Encoder::new(&mut bytes, quality);
    encoder.set_sampling_factor(SamplingFactor::R_4_4_4);
    encoder.set_optimized_huffman_tables(true);
    encoder
        .encode(data, width, height, color)
        .map_err(|e| Error::EncodeUltraHdr(e.to_string()))?;
    Ok(bytes)
}

/// A JPEG marker segment: `FF`, the marker, the length and `payload`.
fn segment(marker: u8, payload: &[u8]) -> Vec<u8> {
    let mut s = vec![0xFF, marker];
    s.extend_from_slice(&u16::try_from(payload.len() + 2).unwrap().to_be_bytes());
    s.extend_from_slice(payload);
    s
}

/// Where to insert new segments in an encoded JPEG: after SOI and the JFIF
/// APP0 segment if there is one.
fn insert_at(jpeg: &[u8]) -> usize {
    if jpeg.get(2..4) == Some(&[0xFF, 0xE0]) {
        4 + usize::from(u16::from_be_bytes([jpeg[4], jpeg[5]]))
    } else {
        2
    }
}

fn splice(jpeg: &[u8], segments: &[u8]) -> Vec<u8> {
    let at = insert_at(jpeg);
    let mut out = Vec::with_capacity(jpeg.len() + segments.len());
    out.extend_from_slice(&jpeg[..at]);
    out.extend_from_slice(segments);
    out.extend_from_slice(&jpeg[at..]);
    out
}

const XMP_NAMESPACE: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";
const ISO_NAMESPACE: &[u8] = b"urn:iso:std:iso:ts:21496:-1\0";

fn xmp_segment(xmp: &str) -> Vec<u8> {
    segment(0xE1, &[XMP_NAMESPACE, xmp.as_bytes()].concat())
}

fn iso_segment(payload: &[u8]) -> Vec<u8> {
    segment(0xE2, &[ISO_NAMESPACE, payload].concat())
}

/// XMP of the base image: the container directory naming the gain map.
fn primary_xmp(gain_map_length: usize) -> String {
    format!(
        r#"<x:xmpmeta xmlns:x="adobe:ns:meta/" x:xmptk="jxr2png">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about=""
    xmlns:hdrgm="http://ns.adobe.com/hdr-gain-map/1.0/"
    xmlns:Container="http://ns.google.com/photos/1.0/container/"
    xmlns:Item="http://ns.google.com/photos/1.0/container/item/"
    hdrgm:Version="1.0">
   <Container:Directory>
    <rdf:Seq>
     <rdf:li rdf:parseType="Resource">
      <Container:Item Item:Semantic="Primary" Item:Mime="image/jpeg"/>
     </rdf:li>
     <rdf:li rdf:parseType="Resource">
      <Container:Item Item:Semantic="GainMap" Item:Mime="image/jpeg" Item:Length="{gain_map_length}"/>
     </rdf:li>
    </rdf:Seq>
   </Container:Directory>
  </rdf:Description>
 </rdf:RDF>
</x:xmpmeta>"#
    )
}

/// XMP of the gain map image: its parameters.
fn gain_map_xmp(params: &GainMapParams) -> String {
    format!(
        r#"<x:xmpmeta xmlns:x="adobe:ns:meta/" x:xmptk="jxr2png">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about=""
    xmlns:hdrgm="http://ns.adobe.com/hdr-gain-map/1.0/"
    hdrgm:Version="1.0"
    hdrgm:GainMapMin="{:.6}"
    hdrgm:GainMapMax="{:.6}"
    hdrgm:Gamma="1"
    hdrgm:OffsetSDR="{OFFSET:.6}"
    hdrgm:OffsetHDR="{OFFSET:.6}"
    hdrgm:HDRCapacityMin="0"
    hdrgm:HDRCapacityMax="{:.6}"
    hdrgm:BaseRenditionIsHDR="False"/>
 </rdf:RDF>
</x:xmpmeta>"#,
        params.min_log2, params.max_log2, params.capacity_max
    )
}

/// ISO 21496-1 metadata of the gain map image (the JPEG form, without the
/// leading version byte): one channel, applied in the base image's color
/// space, from SDR to HDR. Values are fractions over a fixed denominator.
fn iso_metadata(params: &GainMapParams) -> Vec<u8> {
    const DENOMINATOR: u32 = 1 << 16;
    let signed = |v: f32| (v * DENOMINATOR as f32).round() as i32;
    let unsigned = |v: f32| (v.max(0.0) * DENOMINATOR as f32).round() as u32;
    let mut m = Vec::with_capacity(5 + 16 + 40);
    m.extend_from_slice(&0u16.to_be_bytes()); // minimum version
    m.extend_from_slice(&0u16.to_be_bytes()); // writer version
    m.push(0x40); // flags: use the base colour space
    for headroom in [0.0, params.capacity_max] {
        m.extend_from_slice(&unsigned(headroom).to_be_bytes());
        m.extend_from_slice(&DENOMINATOR.to_be_bytes());
    }
    for value in [params.min_log2, params.max_log2] {
        m.extend_from_slice(&signed(value).to_be_bytes());
        m.extend_from_slice(&DENOMINATOR.to_be_bytes());
    }
    m.extend_from_slice(&unsigned(1.0).to_be_bytes()); // gamma
    m.extend_from_slice(&DENOMINATOR.to_be_bytes());
    for offset in [OFFSET, OFFSET] {
        m.extend_from_slice(&signed(offset).to_be_bytes());
        m.extend_from_slice(&DENOMINATOR.to_be_bytes());
    }
    m
}

/// The MPF APP2 segment indexing the two images. `tiff_at` is where its
/// TIFF header will sit in the file; image offsets count from there.
fn mpf_segment(primary_length: usize, gain_map_length: usize, tiff_at: usize) -> Vec<u8> {
    let mut tiff = Vec::with_capacity(82);
    tiff.extend_from_slice(b"MM\0\x2a");
    tiff.extend_from_slice(&8u32.to_be_bytes());
    tiff.extend_from_slice(&3u16.to_be_bytes());
    let entry = |tiff: &mut Vec<u8>, tag: u16, kind: u16, count: u32, value: [u8; 4]| {
        tiff.extend_from_slice(&tag.to_be_bytes());
        tiff.extend_from_slice(&kind.to_be_bytes());
        tiff.extend_from_slice(&count.to_be_bytes());
        tiff.extend_from_slice(&value);
    };
    const UNDEFINED: u16 = 7;
    const LONG: u16 = 4;
    entry(&mut tiff, 0xB000, UNDEFINED, 4, *b"0100"); // MPFVersion
    entry(&mut tiff, 0xB001, LONG, 1, 2u32.to_be_bytes()); // NumberOfImages
    // MPEntry: two 16-byte entries, right after this IFD.
    let entries_at = 8 + 2 + 3 * 12 + 4;
    entry(
        &mut tiff,
        0xB002,
        UNDEFINED,
        32,
        (entries_at as u32).to_be_bytes(),
    );
    tiff.extend_from_slice(&0u32.to_be_bytes()); // no next IFD
    let mp_entry = |tiff: &mut Vec<u8>, attribute: u32, size: usize, offset: usize| {
        tiff.extend_from_slice(&attribute.to_be_bytes());
        tiff.extend_from_slice(&(size as u32).to_be_bytes());
        tiff.extend_from_slice(&(offset as u32).to_be_bytes());
        tiff.extend_from_slice(&[0; 4]); // no dependent images
    };
    // The primary image is a baseline MP primary image at offset 0.
    mp_entry(&mut tiff, 0x0003_0000, primary_length, 0);
    mp_entry(&mut tiff, 0, gain_map_length, primary_length - tiff_at);
    segment(0xE2, &[b"MPF\0".as_slice(), &tiff].concat())
}

/// Length of the MPF segment, which doesn't depend on its values.
const MPF_SEGMENT_LENGTH: usize = 4 + 4 + 8 + 2 + 3 * 12 + 4 + 32;

fn assemble(base: &[u8], map: &[u8], params: &GainMapParams) -> Vec<u8> {
    let mut map_segments = xmp_segment(&gain_map_xmp(params));
    map_segments.extend(iso_segment(&iso_metadata(params)));
    let map = splice(map, &map_segments);

    let mut head = xmp_segment(&primary_xmp(map.len()));
    head.extend(iso_segment(&[0, 0, 0, 0])); // ISO 21496-1 version only
    let mpf_at = insert_at(base) + head.len();
    let primary_length = base.len() + head.len() + MPF_SEGMENT_LENGTH;
    // The TIFF header follows the marker, length and "MPF\0".
    let mpf = mpf_segment(primary_length, map.len(), mpf_at + 8);
    debug_assert_eq!(mpf.len(), MPF_SEGMENT_LENGTH);
    head.extend(mpf);

    let mut file = splice(base, &head);
    debug_assert_eq!(file.len(), primary_length);
    file.extend_from_slice(&map);
    file
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::light_levels;

    fn sample() -> ScRgbImage {
        // A gradient from black to 1000 nits, with SDR white in it.
        let pixels = (0..64 * 32)
            .map(|i| [(i % 64) as f32 / 63.0 * 12.5; 3])
            .collect();
        ScRgbImage::new(64, 32, pixels)
    }

    fn write(image: &ScRgbImage) -> Vec<u8> {
        let mut file = Vec::new();
        let sdr = SdrOptions::default();
        write_ultra_hdr(
            image,
            &light_levels(image),
            &HdrOptions::default(),
            &sdr,
            &mut file,
        )
        .unwrap();
        file
    }

    #[test]
    fn layout_has_both_images_indexed_by_mpf() {
        let file = write(&sample());
        // Two JPEGs back to back.
        let second = file
            .windows(4)
            .rposition(|w| w == [0xFF, 0xD9, 0xFF, 0xD8])
            .unwrap()
            + 2;
        assert_eq!(&file[..2], &[0xFF, 0xD8]);
        // MPF names the second image's size and offset from its TIFF header.
        let mpf = file.windows(4).position(|w| w == b"MPF\0").unwrap() + 4;
        let u32_at = |at: usize| u32::from_be_bytes(file[at..at + 4].try_into().unwrap()) as usize;
        let entries = mpf + u32_at(mpf + 8 + 2 + 2 * 12 + 8);
        assert_eq!(u32_at(entries + 4), second, "primary size");
        assert_eq!(
            u32_at(entries + 16 + 4),
            file.len() - second,
            "gain map size"
        );
        assert_eq!(mpf + u32_at(entries + 16 + 8), second, "gain map offset");
        // The primary XMP names the gain map's length too.
        let text = String::from_utf8_lossy(&file[..second]);
        assert!(text.contains(&format!("Item:Length=\"{}\"", file.len() - second)));
        let map = String::from_utf8_lossy(&file[second..]);
        assert!(map.contains("hdrgm:GainMapMax"));
        assert!(map.contains("urn:iso:std:iso:ts:21496:-1"));
    }

    #[test]
    fn gain_map_restores_the_hdr_picture() {
        let image = sample();
        let sdr = SdrOptions::default();
        let base = sdr_rgb8(&image, &light_levels(&image), &sdr);
        let (map, params) = gain_map(&image, &base, &HdrOptions::default(), &sdr);
        // 1000 nits over an SDR white of 80 nits is 3.6 stops of headroom.
        assert!(
            (params.max_log2 - (12.5f32 + OFFSET).log2()).abs() < 0.05,
            "{params:?}"
        );
        // Applying the map, as viewers do at full headroom, gives back the
        // original brightness.
        for (i, p) in image.pixels.iter().enumerate() {
            let sdr = srgb_decode(f32::from(base[i * 3]) / 255.0);
            let recovery = f32::from(map[i]) / 255.0;
            let log = params.min_log2 + recovery * (params.max_log2 - params.min_log2);
            let hdr = (sdr + OFFSET) * log.exp2() - OFFSET;
            assert!(
                (hdr - p[0]).abs() <= p[0] * 0.03 + 0.01,
                "{i}: {hdr} vs {}",
                p[0]
            );
        }
    }

    #[test]
    fn iso_metadata_layout() {
        let params = GainMapParams {
            min_log2: 0.0,
            max_log2: 2.5,
            capacity_max: 2.5,
        };
        let m = iso_metadata(&params);
        assert_eq!(m.len(), 5 + 2 * 8 + 5 * 8);
        assert_eq!(m[4], 0x40);
        let at = |i: usize| u32::from_be_bytes(m[i..i + 4].try_into().unwrap());
        assert_eq!(at(13) as f32 / at(17) as f32, 2.5, "alternate headroom");
        assert_eq!(at(29) as f32 / at(33) as f32, 2.5, "gain map max");
    }
}
