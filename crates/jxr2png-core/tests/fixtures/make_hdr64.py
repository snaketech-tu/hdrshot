"""Writes hdr64.tif, the source of hdr64.jxr.

A 64x64 linear scRGB image (1.0 = 80 nits) in the pixel format Windows Game
Bar captures use, 128bppRGBAFloat. Encode it with jxrlib's encoder
(Debian/Ubuntu package libjxr-tools):

    python3 make_hdr64.py
    JxrEncApp -i hdr64.tif -o hdr64.jxr -c 28 -a 3

Game Bar stores alpha as a separate plane, but the `jxr` crate can't read
the planar alpha this jxrlib build writes, so the fixture uses interleaved
alpha (-a 3). The color data is coded the same way.

Layout, top to bottom:
- rows 0-31: gray ramp from 0 to 2.0 (160 nits) left to right
- rows 32-47: eight 8-pixel patches: red, green, blue, white 1.0, white 2.5,
  white 12.5 (1000 nits), a color outside Rec.709 (negative green and blue),
  black
- rows 48-63: highlight ramp from 1.0 to 25.0 (2000 nits)
"""

import struct

W = H = 64
PATCHES = [
    (1.0, 0.0, 0.0),
    (0.0, 1.0, 0.0),
    (0.0, 0.0, 1.0),
    (1.0, 1.0, 1.0),
    (2.5, 2.5, 2.5),
    (12.5, 12.5, 12.5),
    (1.2, -0.1, -0.05),
    (0.0, 0.0, 0.0),
]


def pixel(x, y):
    if y < 32:
        v = 2.0 * x / (W - 1)
        return (v, v, v)
    if y < 48:
        return PATCHES[x // 8]
    v = 1.0 + 24.0 * x / (W - 1)
    return (v, v, v)


def main():
    data = b"".join(
        struct.pack("<4f", *pixel(x, y), 1.0) for y in range(H) for x in range(W)
    )
    entries = []  # (tag, type, count, value or bytes)
    extra = bytearray()
    header = 8
    count = 12
    ifd_size = 2 + 12 * count + 4
    extra_at = header + ifd_size

    def array(values):
        at = extra_at + len(extra)
        extra.extend(struct.pack(f"<{len(values)}H", *values))
        return at

    bits = array([32] * 4)
    fmt = array([3] * 4)
    data_at = extra_at + len(extra)
    entries = [
        (256, 4, 1, W),  # ImageWidth
        (257, 4, 1, H),  # ImageLength
        (258, 3, 4, bits),  # BitsPerSample
        (259, 3, 1, 1),  # Compression: none
        (262, 3, 1, 2),  # Photometric: RGB
        (273, 4, 1, data_at),  # StripOffsets
        (277, 3, 1, 4),  # SamplesPerPixel
        (278, 4, 1, H),  # RowsPerStrip
        (279, 4, 1, len(data)),  # StripByteCounts
        (284, 3, 1, 1),  # PlanarConfiguration: chunky
        (338, 3, 1, 2),  # ExtraSamples: unassociated alpha
        (339, 3, 4, fmt),  # SampleFormat: IEEE float
    ]
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
    with open("hdr64.tif", "wb") as f:
        f.write(out)


if __name__ == "__main__":
    main()
