//! The PNG loader's tests.
//!
//! Every image here is built in memory by the writer at the bottom of this
//! file: a hand-built zlib stream of stored (uncompressed) blocks for the
//! pixel data, hand-computed chunk CRCs and Adler-32. Nothing here reads an
//! image from disk, and the writer shares no code with the decoder it feeds.

use oxide_assets::texture::{Texture, TextureError};

#[test]
fn a_one_by_one_opaque_red_png_decodes_to_its_own_texel() {
    let texture = Texture::from_png(&rgba_png(1, 1, &[255, 0, 0, 255])).expect("a valid PNG");
    assert_eq!(texture.width, 1);
    assert_eq!(texture.height, 1);
    assert_eq!(texture.rgba, [255, 0, 0, 255]);
}

#[test]
fn a_two_by_two_png_keeps_every_texel_and_its_alpha() {
    // Four distinct texels, one of them fully transparent and one half:
    // every channel, alpha included, must survive byte for byte.
    let pixels = [
        10, 20, 30, 255, // top left
        40, 50, 60, 0, // top right, transparent
        70, 80, 90, 128, // bottom left, half transparent
        100, 110, 120, 200, // bottom right
    ];
    let texture = Texture::from_png(&rgba_png(2, 2, &pixels)).expect("a valid PNG");
    assert_eq!(texture.width, 2);
    assert_eq!(texture.height, 2);
    assert_eq!(texture.rgba, pixels);
}

#[test]
fn a_vertical_strip_keeps_its_rows_in_order() {
    // Every row carries its own colour, so a transposed or reordered decode
    // is a byte mismatch, not merely a size mismatch.
    let mut pixels = Vec::with_capacity(16 * 32 * 4);
    for y in 0..32u8 {
        for _ in 0..16 {
            pixels.extend_from_slice(&[y, y * 2, y * 3, 255]);
        }
    }
    let texture = Texture::from_png(&rgba_png(16, 32, &pixels)).expect("a valid PNG");
    assert_eq!(texture.width, 16);
    assert_eq!(texture.height, 32);
    assert_eq!(texture.rgba, pixels);
}

#[test]
fn a_greyscale_png_widens_to_rgba() {
    // Colour type 0 carries one sample per texel; it must reach all three
    // colour channels with a fully opaque alpha.
    let texture = Texture::from_png(&grey_png(2, 1, &[0, 255])).expect("a valid PNG");
    assert_eq!(texture.rgba, [0, 0, 0, 255, 255, 255, 255, 255]);
}

#[test]
fn a_truncated_png_is_an_error_not_a_panic() {
    let png = rgba_png(4, 4, &[128; 4 * 4 * 4]);
    let error = Texture::from_png(&png[..png.len() / 2]).expect_err("half an image cannot decode");
    assert!(matches!(error, TextureError::Decode(_)));
}

#[test]
fn bytes_that_are_not_a_png_are_an_error_not_a_panic() {
    let error = Texture::from_png(b"this is not a PNG file, it only looks like one")
        .expect_err("bytes without the PNG signature cannot decode");
    assert!(matches!(error, TextureError::Decode(_)));
}

#[test]
fn an_image_over_the_ceiling_is_refused() {
    // The bytes are a well-formed 4097 x 1 PNG; the loader's own ceiling,
    // not a decode failure, must refuse it.
    let png = rgba_png(4097, 1, &[7; 4097 * 4]);
    match Texture::from_png(&png) {
        Err(TextureError::TooLarge { width, height }) => assert_eq!((width, height), (4097, 1)),
        Err(other) => panic!("the ceiling must refuse the image, not the decoder: {other}"),
        Ok(texture) => panic!("a 4097-texel-wide image must not decode: {texture:?}"),
    }
}

/// The PNG signature: the eight bytes every PNG file starts with.
const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

/// The colour type of an 8-bit greyscale image.
const GREYSCALE: u8 = 0;

/// The colour type of an 8-bit RGBA image.
const RGBA: u8 = 6;

/// Builds an 8-bit greyscale PNG from one byte per texel.
fn grey_png(width: u32, height: u32, pixels: &[u8]) -> Vec<u8> {
    png(width, height, GREYSCALE, pixels)
}

/// Builds an 8-bit RGBA PNG from four bytes per texel.
fn rgba_png(width: u32, height: u32, pixels: &[u8]) -> Vec<u8> {
    png(width, height, RGBA, pixels)
}

/// Builds an 8-bit PNG of `width` x `height` texels with `color_type`, from
/// raw pixel bytes in row-major order.
///
/// Every scanline is written with filter type `None`; the scanlines go into
/// a zlib stream of stored blocks; every chunk's CRC and the stream's
/// Adler-32 are computed here. The result is a complete PNG built without an
/// encoder.
fn png(width: u32, height: u32, color_type: u8, pixels: &[u8]) -> Vec<u8> {
    let samples_per_texel = match color_type {
        GREYSCALE => 1,
        RGBA => 4,
        other => panic!("the fixtures build greyscale and RGBA images, not colour type {other}"),
    };
    assert_eq!(
        pixels.len(),
        width as usize * height as usize * samples_per_texel,
        "the fixture must supply whole scanlines"
    );

    let mut scanlines = Vec::with_capacity(pixels.len() + height as usize);
    for row in pixels.chunks_exact(width as usize * samples_per_texel) {
        scanlines.push(0); // the filter type: None
        scanlines.extend_from_slice(row);
    }

    let mut out = SIGNATURE.to_vec();
    out.extend_from_slice(&ihdr(width, height, color_type));
    out.extend_from_slice(&chunk(b"IDAT", &zlib_stored(&scanlines)));
    out.extend_from_slice(&chunk(b"IEND", &[]));
    out
}

/// The IHDR chunk of an 8-bit image: dimensions, colour type, and the three
/// method bytes, all zero (no compression, adaptive filtering, no interlace).
fn ihdr(width: u32, height: u32, color_type: u8) -> Vec<u8> {
    let mut data = Vec::with_capacity(13);
    data.extend_from_slice(&width.to_be_bytes());
    data.extend_from_slice(&height.to_be_bytes());
    data.extend_from_slice(&[8, color_type, 0, 0, 0]);
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
