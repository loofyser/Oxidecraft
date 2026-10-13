//! The screenshot readback: the finished frame to CPU bytes, top-row-first.
//!
//! The source reads the already-rendered frame back, never re-rendering
//! (`ScreenShotHelper.java:60-83` over `glGetTexImage`/`glReadPixels`). The
//! port keeps the same shape: the renderer holds the finished frame in a
//! retained texture the present blits from — one render, not two — and
//! [`capture_texture`] copies that texture to a CPU-readable buffer,
//! top-row-first. The source flips the rows upright
//! (`TextureUtil.processPixelValues`, `TextureUtil.java:348-359`) because its
//! `glReadPixels` rows run bottom-up; the port's copy rows already run
//! top-first, so the flip has no counterpart — like the FBO crop branch
//! (`:86-98`), which has none either, the capture running at surface size.

use std::sync::mpsc;

/// One captured frame: `width * height * 4` bytes of 8-bit RGBA, row-major,
/// the first row at the top — the PNG writer's own layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedFrame {
    /// The frame's width in texels.
    pub width: u32,
    /// The frame's height in texels.
    pub height: u32,
    /// `width * height * 4` bytes: red, green, blue and alpha per texel, in
    /// row-major order from the top-left corner.
    pub rgba: Vec<u8>,
}

/// Copies `texture` (`width` by `height` texels in `format`) to the CPU and
/// packs it top-row-first, or `None` when the size is zero or the buffer
/// never maps.
///
/// The copy rows already run top-first, so no flip is applied — the packed
/// bytes match the presented frame row for row. A texture stored blue-first
/// ([`wgpu::TextureFormat::Bgra8Unorm`] and its sRGB twin) has each texel's
/// red and blue bytes swapped, because the PNG writer reads red-first.
pub fn capture_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
) -> Option<CapturedFrame> {
    if width == 0 || height == 0 {
        return None;
    }
    let row_bytes = u64::from(width) * 4;
    let stride = row_bytes.next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as u64);
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("oxide-render screenshot readback"),
        size: stride * u64::from(height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("oxide-render screenshot readback encoder"),
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(stride as u32),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(Some(encoder.finish()));

    let slice = buffer.slice(..);
    let (sender, receiver) = mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = sender.send(result);
    });
    let _ = device.poll(wgpu::PollType::Wait);
    receiver.recv().ok()?.ok()?;
    let raw = slice.get_mapped_range().to_vec();
    buffer.unmap();
    let swap_rb = matches!(
        format,
        wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
    );
    Some(CapturedFrame {
        width,
        height,
        rgba: pack_rows(
            &raw,
            stride as usize,
            width as usize,
            height as usize,
            swap_rb,
        ),
    })
}

/// Packs one readback — `height` rows of `stride` bytes, top-first — into
/// `width * height * 4` top-row-first bytes, swapping red and blue per texel
/// when `swap_rb` is set.
fn pack_rows(raw: &[u8], stride: usize, width: usize, height: usize, swap_rb: bool) -> Vec<u8> {
    let mut out = vec![0u8; width * height * 4];
    for row in 0..height {
        let src = &raw[row * stride..row * stride + width * 4];
        let dest = &mut out[row * width * 4..(row + 1) * width * 4];
        dest.copy_from_slice(src);
        if swap_rb {
            for texel in dest.chunks_exact_mut(4) {
                texel.swap(0, 2);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    //! The packer's row order and channel order: a two-row fixture whose
    //! rows name themselves, and a blue-first texel.

    use super::*;

    /// Two rows, two texels wide: the top row red, the bottom row blue, so a
    /// flipping packer would read blue where red belongs.
    fn two_rows() -> Vec<u8> {
        vec![
            255, 0, 0, 255, 255, 0, 0, 255, // the top row, first in memory
            0, 0, 255, 255, 0, 0, 255, 255, // the bottom row
        ]
    }

    #[test]
    fn the_packer_keeps_top_first_order() {
        let packed = pack_rows(&two_rows(), 8, 2, 2, false);
        assert_eq!(packed, two_rows(), "no row moves");
    }

    #[test]
    fn the_packer_swaps_red_and_blue_for_blue_first_textures() {
        let packed = pack_rows(&[10, 20, 30, 255], 4, 1, 1, true);
        assert_eq!(packed, vec![30, 20, 10, 255]);
    }
}
