//! The block atlas on the GPU: the mip chain, the view and the two samplers the terrain
//! pipelines read it through — the mipped pair every layer but the plain cutout draws with,
//! and the level-0 pair the plain cutout draws with.
//!
//! [`AtlasTexture::upload`] copies the stitched atlas's mip chain in order — level 0 is
//! `atlas.width` x `atlas.height` texels and each later level halves that, floored at one,
//! which is the chain `oxide_assets::atlas::Atlas` builds (Task 4). The texture is
//! `Rgba8Unorm` and the four bytes of a texel go out verbatim, with no sRGB anywhere: M2's
//! colour-space decision fixes vanilla's non-sRGB pipeline, so the atlas is sampled as it is
//! stored and every multiply on the way to the framebuffer happens in 8-bit colour space.
//!
//! The sampler is the client's own pair for block textures (`TextureUtil.java:269-270`, the
//! non-blur branch with mipmaps on, whose filters are `GL_NEAREST_MIPMAP_LINEAR` for
//! minification and `GL_NEAREST` for magnification — the same pair
//! `AbstractTexture.setBlurMipmapDirect` installs at `AbstractTexture.java:27-28`, and M2's
//! Decision 11 pins it). The wrap is `ClampToEdge` on every axis, a declared divergence: 1.8.9
//! leaves the block atlas at `GL_REPEAT` (`TextureMap.java:235` uploads each sprite with the
//! clamp flag off, which reaches `TextureUtil.setTextureClamped(false)` at
//! `TextureUtil.java:171` and sets `GL_REPEAT` at `TextureUtil.java:250-251`), where the
//! clamp reads the edge texel instead of wrapping to the opposite side. The two can be told
//! apart only by a coordinate outside `[0, 1]` or exactly `1.0`; the uvs this project's own
//! stitcher emits are content-rect offsets divided by the power-of-two level-0 size, so every
//! ordinary fragment centre lands strictly inside a sprite and the divergence stays
//! unobservable — the one theoretical exception is a sample at exactly 1.0 on a sprite flush
//! to the atlas's far edge, where the clamp is the safe side (it repeats the edge texel where
//! a wrapped sample would jump to the atlas's opposite edge).
//!
//! The second sampler is the plain cutout's. The client switches the block atlas to
//! `setBlurMipmap(false, false)` before its `CUTOUT` pass and restores the mipped pair after
//! (`EntityRenderer.java:1389`, `:1391`); with both flags false the switch directs the two
//! filters to `GL_NEAREST` (`AbstractTexture.java:27-28`, `:31-32`), so no mipmap minification
//! filter is live either and the pass samples mip level 0 alone. wgpu's samplers always carry
//! a mipmap filter, so [`plain_sampler_descriptor`] states the level-0-only switch with the
//! level-of-detail clamp instead ([`AtlasTexture::plain_sampler`] records the reasoning the
//! client's state is translated by).
//!
//! The draw-order rules that pair with the texture (opaque and cutout first, translucent
//! last, sorted back to front) live in [`crate::terrain_pass::TerrainPass::draw`].

use oxide_assets::atlas::Atlas;

/// The binding the atlas texture occupies in the pipelines' group 1.
///
/// The terrain shader binds `texture_2d<f32>` at this number and the layout
/// ([`AtlasTexture::bind_group_layout`]) declares it, so the two cannot drift apart; the
/// unit tests read both.
pub const ATLAS_BINDING: u32 = 0;

/// The binding the atlas sampler occupies in the pipelines' group 1.
pub const SAMPLER_BINDING: u32 = 1;

/// The atlas's texture and sampler on the GPU.
///
/// The view keeps its texture alive, so the texture handle itself is not stored; dropping the
/// `AtlasTexture` frees both.
pub struct AtlasTexture {
    /// A view of the whole mip chain, as the bind group's texture binding needs.
    view: wgpu::TextureView,
    /// The sampler every mipped terrain layer reads the atlas through.
    sampler: wgpu::Sampler,
    /// The level-0 sampler the plain cutout pass reads the atlas through: the state
    /// `setBlurMipmap(false, false)` installs (`GL_NEAREST`, no mips),
    /// stated with the level-of-detail clamp.
    plain_sampler: wgpu::Sampler,
}

impl AtlasTexture {
    /// Uploads `atlas` as an `Rgba8Unorm` texture with the atlas's own mip chain.
    ///
    /// The texture has `atlas.level_count` levels and level `l` is written from
    /// `atlas.levels[l]` at that level's own size, in order — the uploader's contract with the
    /// stitcher. A hand-built malformed atlas cannot panic the upload: the level count is
    /// clamped to the levels a texture of the atlas's level-0 size can hold, and a level whose
    /// declared size or byte count does not match its mip is skipped with a warning, leaving
    /// that level as wgpu initialised it (zero-filled) rather than half-written. Both guards
    /// are defensive only — a stitcher-built atlas's chain is exactly the shape the texture is
    /// created with, every level matches, and neither guard can fire for it — so an atlas that
    /// trips one is malformed input, and the texture's `mip_level_count` may then be below
    /// `atlas.level_count`.
    pub fn upload(device: &wgpu::Device, queue: &wgpu::Queue, atlas: &Atlas) -> AtlasTexture {
        let width = atlas.width.max(1);
        let height = atlas.height.max(1);
        let count = atlas.level_count.clamp(1, mip_limit(width, height));
        let texture = device.create_texture(&texture_descriptor(width, height, count));
        for (level, image) in atlas.levels.iter().enumerate().take(count as usize) {
            let (level_width, level_height) = mip_size(width, height, level as u32);
            let texels = level_width as usize * level_height as usize;
            if image.width != level_width
                || image.height != level_height
                || image.rgba.len() != texels * 4
            {
                tracing::warn!(
                    level,
                    width = image.width,
                    height = image.height,
                    bytes = image.rgba.len(),
                    "the atlas level does not match its mip level's size; the level is left as it is"
                );
                continue;
            }
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: level as u32,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &image.rgba,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(level_width * 4),
                    rows_per_image: Some(level_height),
                },
                wgpu::Extent3d {
                    width: level_width,
                    height: level_height,
                    depth_or_array_layers: 1,
                },
            );
        }
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&sampler_descriptor());
        let plain_sampler = device.create_sampler(&plain_sampler_descriptor());
        AtlasTexture {
            view,
            sampler,
            plain_sampler,
        }
    }

    /// The bind-group layout the atlas is bound through: group 1, binding [`ATLAS_BINDING`]
    /// the texture and binding [`SAMPLER_BINDING`] its sampler, both read by the fragment
    /// stage.
    ///
    /// The pass builds this layout once and uses it both for the pipelines' layout and for the
    /// atlas's bind group, so a bind group can never be created for a different shape than the
    /// pipelines expect.
    pub fn bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("oxide terrain atlas layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: ATLAS_BINDING,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: SAMPLER_BINDING,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        })
    }

    /// Builds the atlas's bind group under `layout`: the mipped pair the opaque, mipped
    /// cutout and translucent layers draw with.
    pub fn bind_group(
        &self,
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
    ) -> wgpu::BindGroup {
        self.bind_group_with(device, layout, &self.sampler)
    }

    /// Builds the atlas's bind group under `layout` with the level-0 sampler: the pair the
    /// plain cutout layer draws with.
    ///
    /// The same texture view, so the two bind groups differ in the sampler alone — the two
    /// texture states the client's pass list puts the atlas through
    /// (`EntityRenderer.java:1389`, `:1391`).
    pub fn plain_bind_group(
        &self,
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
    ) -> wgpu::BindGroup {
        self.bind_group_with(device, layout, &self.plain_sampler)
    }

    /// Builds one of the atlas's bind groups: its texture view with one of its two samplers.
    fn bind_group_with(
        &self,
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("oxide terrain atlas bind group"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: ATLAS_BINDING,
                    resource: wgpu::BindingResource::TextureView(&self.view),
                },
                wgpu::BindGroupEntry {
                    binding: SAMPLER_BINDING,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        })
    }

    /// The view of the atlas's whole mip chain.
    pub fn view(&self) -> &wgpu::TextureView {
        &self.view
    }

    /// The sampler every mipped terrain layer reads the atlas through.
    pub fn sampler(&self) -> &wgpu::Sampler {
        &self.sampler
    }

    /// The level-0 sampler the plain cutout layer reads the atlas through.
    pub fn plain_sampler(&self) -> &wgpu::Sampler {
        &self.plain_sampler
    }
}

/// The texture descriptor for an atlas of `width` x `height` level-0 texels and `levels` mip
/// levels.
///
/// `Rgba8Unorm` and no sRGB view format: the colour-space decision (M2 Decision 1) keeps the
/// sampled value the stored byte, so the shader's multiplies match the client's.
fn texture_descriptor(width: u32, height: u32, levels: u32) -> wgpu::TextureDescriptor<'static> {
    wgpu::TextureDescriptor {
        label: Some("oxide terrain atlas"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: levels,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    }
}

/// The mipped sampler descriptor: the client's `NEAREST_MIPMAP_LINEAR` pair, clamped on every
/// axis.
///
/// [`AtlasTexture::upload`]'s module doc records the clamp as the declared divergence from the
/// source's `GL_REPEAT`; the filters themselves are the source's own.
fn sampler_descriptor() -> wgpu::SamplerDescriptor<'static> {
    wgpu::SamplerDescriptor {
        label: Some("oxide terrain atlas sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        mipmap_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    }
}

/// The plain cutout's sampler descriptor: `GL_NEAREST` on every filter, mip level 0 alone.
///
/// The source's claim on this sampler is negative: between its `CUTOUT_MIPPED` and `CUTOUT`
/// passes it switches the atlas to `setBlurMipmap(false, false)` and restores the mipped pair
/// after (`EntityRenderer.java:1389`, `:1391`). With both flags false,
/// `AbstractTexture.setBlurMipmapDirect` lands on `GL_NEAREST` for the min and the mag filter
/// (`AbstractTexture.java:27-28`, installed at `:31-32`), and no mipmap minification filter is
/// set either — the mip chain exists but nothing selects it, so the pass samples mip level 0
/// alone. wgpu has no mipmap-less sampler state, so the level-0-only switch is stated with the
/// level-of-detail clamp: `lod_max_clamp` at zero pins every sample to level 0, and the
/// nearest filters keep the sample an exact texel like the source's. The clamp on every axis
/// is the same declared divergence [`sampler_descriptor`] records.
fn plain_sampler_descriptor() -> wgpu::SamplerDescriptor<'static> {
    wgpu::SamplerDescriptor {
        label: Some("oxide terrain atlas plain sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        mipmap_filter: wgpu::FilterMode::Nearest,
        lod_min_clamp: 0.0,
        lod_max_clamp: 0.0,
        ..Default::default()
    }
}

/// The most mip levels a `width` x `height` texture can hold: one per halving of the longer
/// axis, floored at one texel.
fn mip_limit(width: u32, height: u32) -> u32 {
    32 - width.max(height).leading_zeros()
}

/// The size of mip `level` of a `width` x `height` texture: each axis halved `level` times,
/// floored at one texel.
fn mip_size(width: u32, height: u32, level: u32) -> (u32, u32) {
    ((width >> level).max(1), (height >> level).max(1))
}

#[cfg(test)]
mod tests {
    use super::{
        ATLAS_BINDING, SAMPLER_BINDING, mip_limit, mip_size, plain_sampler_descriptor,
        sampler_descriptor, texture_descriptor,
    };
    use wgpu::{AddressMode, FilterMode, TextureDimension, TextureFormat, TextureUsages};

    #[test]
    fn the_atlas_texture_is_unorm_rgba_with_the_atlas_s_mip_chain() {
        let descriptor = texture_descriptor(16, 16, 5);
        assert_eq!(descriptor.format, TextureFormat::Rgba8Unorm);
        assert!(!descriptor.format.is_srgb(), "the atlas is never sRGB");
        assert_eq!(descriptor.mip_level_count, 5);
        assert_eq!(descriptor.dimension, TextureDimension::D2);
        assert_eq!(descriptor.sample_count, 1);
        assert_eq!(
            descriptor.size,
            wgpu::Extent3d {
                width: 16,
                height: 16,
                depth_or_array_layers: 1
            }
        );
        assert!(descriptor.usage.contains(TextureUsages::TEXTURE_BINDING));
        assert!(descriptor.usage.contains(TextureUsages::COPY_DST));
        assert!(descriptor.view_formats.is_empty(), "no sRGB view format");
    }

    #[test]
    fn the_sampler_is_the_clients_nearest_mipmapped_linear_pair_clamped() {
        let sampler = sampler_descriptor();
        assert_eq!(sampler.mag_filter, FilterMode::Nearest);
        assert_eq!(sampler.min_filter, FilterMode::Nearest);
        assert_eq!(sampler.mipmap_filter, FilterMode::Linear);
        assert_eq!(sampler.address_mode_u, AddressMode::ClampToEdge);
        assert_eq!(sampler.address_mode_v, AddressMode::ClampToEdge);
        assert_eq!(sampler.address_mode_w, AddressMode::ClampToEdge);
    }

    #[test]
    fn the_plain_sampler_is_the_clients_nearest_level_zero_pair_clamped() {
        // `setBlurMipmap(false, false)`'s state (`EntityRenderer.java:1389`): both filters
        // `GL_NEAREST` (`AbstractTexture.java:27-28`) with no mipmap minification filter
        // live, so the sample stays on level 0. wgpu's samplers always carry a mipmap
        // filter, so the level-0-only switch is stated with the level-of-detail clamp.
        let sampler = plain_sampler_descriptor();
        assert_eq!(sampler.mag_filter, FilterMode::Nearest);
        assert_eq!(sampler.min_filter, FilterMode::Nearest);
        assert_eq!(sampler.mipmap_filter, FilterMode::Nearest);
        assert_eq!(sampler.lod_min_clamp, 0.0);
        assert_eq!(sampler.lod_max_clamp, 0.0, "mip level 0 alone");
        assert_eq!(sampler.address_mode_u, AddressMode::ClampToEdge);
        assert_eq!(sampler.address_mode_v, AddressMode::ClampToEdge);
        assert_eq!(sampler.address_mode_w, AddressMode::ClampToEdge);
    }

    #[test]
    fn the_mip_chain_halves_each_axis_and_stops_at_one_texel() {
        assert_eq!(mip_limit(16, 16), 5);
        assert_eq!(mip_limit(1, 1), 1);
        assert_eq!(mip_limit(4096, 16), 13);
        assert_eq!(mip_size(16, 16, 0), (16, 16));
        assert_eq!(mip_size(16, 16, 4), (1, 1));
        assert_eq!(mip_size(16, 16, 5), (1, 1));
        assert_eq!(mip_size(64, 32, 3), (8, 4));
    }

    #[test]
    fn the_atlas_is_bound_at_group_one_binding_zero_and_one() {
        // The numbers the terrain shader declares; `terrain_pass`'s tests read the shader
        // against these constants.
        assert_eq!((ATLAS_BINDING, SAMPLER_BINDING), (0, 1));
    }
}
