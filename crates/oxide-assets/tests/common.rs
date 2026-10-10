//! The shared PNG writer the `oxide-assets` integration suites build their
//! fixtures with: a hand-built zlib stream of stored (uncompressed) blocks
//! for the pixel data, hand-computed chunk CRCs and Adler-32. Nothing here
//! reads an image from disk, and the writer shares no code with the decoder
//! it feeds.
//!
//! Every suite reaches this module with `mod common;` at the top of its own
//! file: each integration test is a crate of its own and cannot reach
//! another one's helpers without a module file of its own.
//!
//! No suite uses the whole set, so unused helpers are the norm, not a smell:
//! the allow keeps each suite's own build warning-free.
//!
#![allow(dead_code)]

/// The PNG signature: the eight bytes every PNG file starts with.
pub const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

/// The colour type of a greyscale image.
pub const GREYSCALE: u8 = 0;

/// The colour type of an RGBA image.
pub const RGBA: u8 = 6;

/// Builds an 8-bit greyscale PNG from one byte per texel.
pub fn grey_png(width: u32, height: u32, pixels: &[u8]) -> Vec<u8> {
    png(width, height, 8, GREYSCALE, pixels, None)
}

/// Builds an 8-bit RGBA PNG from four bytes per texel.
pub fn rgba_png(width: u32, height: u32, pixels: &[u8]) -> Vec<u8> {
    png(width, height, 8, RGBA, pixels, None)
}

/// Builds an 8-bit RGBA PNG whose every texel is `rgba`, with the writer
/// below.
pub fn solid_png(width: u32, height: u32, rgba: [u8; 4]) -> Vec<u8> {
    let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
    for _ in 0..width as usize * height as usize {
        pixels.extend_from_slice(&rgba);
    }
    rgba_png(width, height, &pixels)
}

/// Builds an 8-bit RGBA PNG whose rows are `colours`, top row first.
pub fn rows_png(width: u32, colours: &[[u8; 4]]) -> Vec<u8> {
    let height = colours.len() as u32;
    let mut pixels = Vec::with_capacity(width as usize * colours.len() * 4);
    for colour in colours {
        for _ in 0..width as usize {
            pixels.extend_from_slice(colour);
        }
    }
    rgba_png(width, height, &pixels)
}

/// Builds an 8-bit RGBA PNG of `side` x `side` texels whose top-left 2x2
/// block is `block` (row-major from `(0, 0)`) and whose other texels are
/// `rest`.
pub fn block_png(side: u32, block: [[u8; 4]; 4], rest: [u8; 4]) -> Vec<u8> {
    let mut pixels = Vec::with_capacity((side * side * 4) as usize);
    for y in 0..side {
        for x in 0..side {
            let colour = if x < 2 && y < 2 {
                block[(y * 2 + x) as usize]
            } else {
                rest
            };
            pixels.extend_from_slice(&colour);
        }
    }
    rgba_png(side, side, &pixels)
}

/// Builds a 16-bit greyscale PNG with a `tRNS` chunk from one `u16` sample
/// per texel: `samples` are stored big-endian, and `transparent` is the
/// sample the transparency chunk names.
pub fn grey16_trns_png(width: u32, height: u32, samples: &[u16], transparent: u16) -> Vec<u8> {
    assert_eq!(
        samples.len(),
        width as usize * height as usize,
        "the fixture must supply whole scanlines"
    );
    let mut pixels = Vec::with_capacity(samples.len() * 2);
    for sample in samples {
        pixels.extend_from_slice(&sample.to_be_bytes());
    }
    png(
        width,
        height,
        16,
        GREYSCALE,
        &pixels,
        Some(&transparent.to_be_bytes()),
    )
}

/// Builds a PNG of `width` x `height` texels with `bit_depth` and
/// `color_type`, from raw sample bytes in row-major order (big-endian pairs
/// for depths above 8), with an optional `tRNS` chunk between the IHDR and
/// the IDAT.
///
/// Every scanline is written with filter type `None`; the scanlines go into
/// a zlib stream of stored blocks; every chunk's CRC and the stream's
/// Adler-32 are computed here. The result is a complete PNG built without an
/// encoder.
pub fn png(
    width: u32,
    height: u32,
    bit_depth: u8,
    color_type: u8,
    pixels: &[u8],
    trns: Option<&[u8]>,
) -> Vec<u8> {
    let bytes_per_texel = match (color_type, bit_depth) {
        (GREYSCALE, 8) => 1,
        (GREYSCALE, 16) => 2,
        (RGBA, 8) => 4,
        _ => panic!(
            "the fixtures build 8-bit greyscale, 16-bit greyscale and 8-bit RGBA images, \
             not colour type {color_type} at depth {bit_depth}"
        ),
    };
    assert_eq!(
        pixels.len(),
        width as usize * height as usize * bytes_per_texel,
        "the fixture must supply whole scanlines"
    );

    let mut scanlines = Vec::with_capacity(pixels.len() + height as usize);
    for row in pixels.chunks_exact(width as usize * bytes_per_texel) {
        scanlines.push(0); // the filter type: None
        scanlines.extend_from_slice(row);
    }

    let mut out = SIGNATURE.to_vec();
    out.extend_from_slice(&ihdr(width, height, bit_depth, color_type));
    if let Some(transparent) = trns {
        out.extend_from_slice(&chunk(b"tRNS", transparent));
    }
    out.extend_from_slice(&chunk(b"IDAT", &zlib_stored(&scanlines)));
    out.extend_from_slice(&chunk(b"IEND", &[]));
    out
}

/// The IHDR chunk: dimensions, bit depth, colour type, and the three method
/// bytes, all zero (no compression, adaptive filtering, no interlace).
fn ihdr(width: u32, height: u32, bit_depth: u8, color_type: u8) -> Vec<u8> {
    let mut data = Vec::with_capacity(13);
    data.extend_from_slice(&width.to_be_bytes());
    data.extend_from_slice(&height.to_be_bytes());
    data.extend_from_slice(&[bit_depth, color_type, 0, 0, 0]);
    chunk(b"IHDR", &data)
}

/// One PNG chunk: length, type, data and the CRC of type and data.
fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut crc_input = Vec::with_capacity(4 + data.len());
    crc_input.extend_from_slice(kind);
    crc_input.extend_from_slice(data);

    let mut out = Vec::with_capacity(12 + data.len());
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(&crc_input);
    out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
    out
}

/// Wraps `raw` in a zlib stream: the header, stored deflate blocks of at
/// most 65535 bytes each (the last one marked as final), and the Adler-32.
fn zlib_stored(raw: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];

    let mut offset = 0;
    loop {
        let end = (offset + 65_535).min(raw.len());
        let block = &raw[offset..end];
        let last = end == raw.len();

        out.push(u8::from(last)); // BFINAL in bit 0, BTYPE 00: stored
        let length = block.len() as u16;
        out.extend_from_slice(&length.to_le_bytes());
        out.extend_from_slice(&(!length).to_le_bytes());
        out.extend_from_slice(block);

        offset = end;
        if last {
            break;
        }
    }

    out.extend_from_slice(&adler32(raw).to_be_bytes());
    out
}

/// The CRC-32 PNG chunks carry: the reflected polynomial `0xedb88320`.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

/// The Adler-32 of `bytes`, the checksum a zlib stream ends with.
fn adler32(bytes: &[u8]) -> u32 {
    const MODULUS: u32 = 65_521;
    let mut a = 1u32;
    let mut b = 0u32;
    for &byte in bytes {
        a = (a + u32::from(byte)) % MODULUS;
        b = (b + a) % MODULUS;
    }
    (b << 16) | a
}
