//! PNG textures: decode a PNG file's bytes into 8-bit RGBA texels.

use std::io::Cursor;

use png::{BitDepth, ColorType, Decoder, Transformations};

/// The largest texture the loader accepts, in texels, on either axis.
///
/// Nothing in the 1.8 jar comes near it; a larger image is a hostile input,
/// and the atlas ceiling is the same number.
const MAX_DIMENSION: u32 = 4096;

/// An 8-bit RGBA image: four bytes per texel, row-major, the first row at the
/// top.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Texture {
    /// Width in texels.
    pub width: u32,
    /// Height in texels.
    pub height: u32,
    /// `width * height * 4` bytes: red, green, blue and alpha per texel, in
    /// row-major order from the top-left corner.
    pub rgba: Vec<u8>,
}

/// Errors from decoding a PNG.
#[derive(Debug, thiserror::Error)]
pub enum TextureError {
    /// The bytes are not a decodable PNG. Carries the decoder's own message.
    #[error("could not decode the PNG: {0}")]
    Decode(#[from] png::DecodingError),
    /// The image is wider or taller than the loader accepts.
    #[error("the image is {width}x{height} texels; the loader takes at most 4096 on either axis")]
    TooLarge {
        /// The image's declared width.
        width: u32,
        /// The image's declared height.
        height: u32,
    },
    /// The decoder returned a format this loader never asks for.
    #[error("the PNG decoded to an unsupported format: {color:?} at {depth:?}")]
    UnsupportedFormat {
        /// The colour type the decoder reported.
        color: ColorType,
        /// The bit depth the decoder reported.
        depth: BitDepth,
    },
}

impl Texture {
    /// Decodes `bytes` as a PNG image of 8-bit RGBA texels.
    ///
    /// Any colour type and bit depth the format allows decodes: greyscale (1
    /// to 16 bits per sample), greyscale with alpha, palette (with or without
    /// a transparency chunk), RGB and RGBA. A palette is expanded and a
    /// 16-bit sample is stripped to its high byte by the decoder's own
    /// transformations; greyscale is widened here. The result is always eight
    /// bits per channel.
    ///
    /// An image more than 4096 texels wide or high is refused before it is
    /// decoded: it is a hostile input, and the atlas ceiling is the same
    /// number.
    pub fn from_png(bytes: &[u8]) -> Result<Texture, TextureError> {
        let mut decoder = Decoder::new(Cursor::new(bytes));
        decoder.set_transformations(Transformations::EXPAND | Transformations::STRIP_16);
        let mut reader = decoder.read_info()?;

        let (width, height) = reader.info().size();
        if width > MAX_DIMENSION || height > MAX_DIMENSION {
            return Err(TextureError::TooLarge { width, height });
        }

        let (color, depth) = reader.output_color_type();
        let Some(size) = reader.output_buffer_size() else {
            // Unreachable within the ceiling: only an image too large for the
            // address space reports no size, and 4096 by 4096 texels is 64 MiB.
            // Kept total rather than unwrapped.
            return Err(TextureError::TooLarge { width, height });
        };

        let mut buffer = vec![0u8; size];
        reader.next_frame(&mut buffer)?;

        let texels = width as usize * height as usize;
        let rgba = match (color, depth) {
            (ColorType::Rgba, BitDepth::Eight) => buffer,
            (ColorType::Rgb, BitDepth::Eight) => {
                let mut rgba = Vec::with_capacity(texels * 4);
                for pixel in buffer.chunks_exact(3) {
                    rgba.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]);
                }
                rgba
            }
            (ColorType::GrayscaleAlpha, BitDepth::Eight) => {
                let mut rgba = Vec::with_capacity(texels * 4);
                for pixel in buffer.chunks_exact(2) {
                    rgba.extend_from_slice(&[pixel[0], pixel[0], pixel[0], pixel[1]]);
                }
                rgba
            }
            (ColorType::Grayscale, BitDepth::Eight) => {
                let mut rgba = Vec::with_capacity(texels * 4);
                for &grey in &buffer {
                    rgba.extend_from_slice(&[grey, grey, grey, 255]);
                }
                rgba
            }
            (color, depth) => return Err(TextureError::UnsupportedFormat { color, depth }),
        };

        Ok(Texture {
            width,
            height,
            rgba,
        })
    }
}
