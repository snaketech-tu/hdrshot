"""Writes alpha32.tif and sdr32.tif, the sources of alpha32.jxr and sdr32.jxr.

- alpha32: 32x32 linear scRGB, 128bppRGBAFloat like hdr64 (see
  make_hdr64.py), white 2.0 (160 nits) everywhere, with alpha falling from
  1 at the left edge to 0 at the right edge.
- sdr32: 32x32 8-bit sRGB (24bppRGB), a red-to-blue ramp left to right with
  green rising top to bottom: an SDR picture in a JPEG XR file.

Encode them with jxrlib's encoder (Debian/Ubuntu package libjxr-tools):

    python3 make_inputs.py
    JxrEncApp -i alpha32.tif -o alpha32.jxr -c 28 -a 3 -q 1
    JxrEncApp -i sdr32.tif -o sdr32.jxr -q 1

-q 1 is lossless, so tests can compare exact values.
"""

import struct

N = 32


def tiff(path, data, bits, sample_format, samples, extra_samples):
    """A single-strip, uncompressed, chunky RGB(A) TIFF."""
    count = 11 + (1 if extra_samples else 0)
    header = 8
    extra_at = header + 2 + 12 * count + 4
    extra = bytearray()

    def array(values):
        at = extra_at + len(extra)
        extra.extend(struct.pack(f"<{len(values)}H", *values))
        return at

    bits_at = array([bits] * samples)
    fmt_at = array([sample_format] * samples)
    data_at = extra_at + len(extra)
    entries = [
        (256, 4, 1, N),  # ImageWidth
        (257, 4, 1, N),  # ImageLength
        (258, 3, samples, bits_at),  # BitsPerSample
        (259, 3, 1, 1),  # Compression: none
        (262, 3, 1, 2),  # Photometric: RGB
        (273, 4, 1, data_at),  # StripOffsets
        (277, 3, 1, samples),  # SamplesPerPixel
        (278, 4, 1, N),  # RowsPerStrip
        (279, 4, 1, len(data)),  # StripByteCounts
        (284, 3, 1, 1),  # PlanarConfiguration: chunky
    ]
    if extra_samples:
        entries.append((338, 3, 1, 2))  # ExtraSamples: unassociated alpha
    entries.append((339, 3, samples, fmt_at))  # SampleFormat
    assert len(entries) == count
    out = bytearray(b"II*\x00" + struct.pack("<I", header))
    out += struct.pack("<H", count)
    for tag, kind, n, value in entries:
        if kind == 3 and n == 1:
            out += struct.pack("<HHIHH", tag, kind, n, value, 0)
        else:
            out += struct.pack("<HHII", tag, kind, n, value)
    out += struct.pack("<I", 0)
    out += extra + data
    with open(path, "wb") as f:
        f.write(out)


def main():
    alpha = b"".join(
        struct.pack("<4f", 2.0, 2.0, 2.0, 1.0 - x / (N - 1))
        for y in range(N)
        for x in range(N)
    )
    tiff("alpha32.tif", alpha, 32, 3, 4, True)
    sdr = bytes(
        v
        for y in range(N)
        for x in range(N)
        for v in (255 - x * 8, y * 8, x * 8)
    )
    tiff("sdr32.tif", sdr, 8, 1, 3, False)


if __name__ == "__main__":
    main()
