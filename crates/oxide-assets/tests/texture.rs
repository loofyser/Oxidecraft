//! The PNG loader's tests.
//!
//! Every image here is built in memory by the shared writer in `common.rs`:
//! a hand-built zlib stream of stored (uncompressed) blocks for the pixel
//! data, hand-computed chunk CRCs and Adler-32. Nothing here reads an image
//! from disk, and the writer shares no code with the decoder it feeds.

mod common;

use common::{grey_png, grey16_trns_png, rgba_png};
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

#[test]
fn a_sixteen_bit_greyscale_png_with_trns_strips_to_the_high_byte() {
    // Three 16-bit greyscale samples with a `tRNS` chunk naming one of them:
    // the decoder strips every sample to its high byte (`STRIP_16`) and
    // expands the `tRNS` match to a zero alpha (`EXPAND`); the loader widens
    // the grey to RGB. The transparent sample's high byte is nonzero, so a
    // loader that kept the low byte, or that ignored the transparency, fails
    // a different channel of this assertion.
    let png = grey16_trns_png(3, 1, &[0x1234, 0xABCD, 0x80FF], 0x80FF);
    let texture = Texture::from_png(&png).expect("a valid 16-bit greyscale PNG with tRNS");
    assert_eq!(texture.width, 3);
    assert_eq!(texture.height, 1);
    assert_eq!(
        texture.rgba,
        [
            0x12, 0x12, 0x12, 255, // 0x1234 strips to its high byte, opaque
            0xAB, 0xAB, 0xAB, 255, // 0xABCD strips to its high byte, opaque
            0x80, 0x80, 0x80, 0, // the tRNS sample keeps its high byte, transparent
        ]
    );
}
