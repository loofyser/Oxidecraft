//! The dim pass: one full-frame tint quad, drawn over the finished scene.
//!
//! The interim death view's backdrop: while the player is dead the client
//! tints the whole frame the way the source's death screen dims it, and the
//! overlay text then draws over the tint. The pass is one triangle that
//! covers the viewport — the corners are generated in the vertex stage from
//! the vertex index, so there is no vertex buffer — and its fragment stage
//! writes the tint as a colour, blended with the client's own
//! `src_alpha / one_minus_src_alpha` pair so the scene shows through.
//!
//! The source's screen is a two-stop gradient — `GuiGameOver.drawScreen`
//! fills `drawGradientRect(0, 0, width, height, 1615855616, -1602211792)` —
//! and one flat quad is this milestone's stand-in: the client sets the tint
//! from the gradient's first stop, and the real gradient is the death
//! screen's own ticket.
//!
//! The pass draws in a pass with no depth attachment (the overlay's own), so
//! the tint cannot be hidden by terrain; the client draws it before the text
//! so the text stays legible on top.

/// The dim shader: one triangle over the whole clip space, the tint from a
/// uniform.
///
/// The three vertices are `(-1, -1)`, `(3, -1)` and `(-1, 3)`: the triangle
/// covers the clip-space square with room to spare, and culling is off, so
/// one draw covers the viewport without a vertex buffer.
const SHADER: &str = r#"
struct Dim {
    colour: vec4<f32>,
};

@group(0) @binding(0) var<uniform> dim: Dim;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let x = f32((index << 1u) & 2u) * 2.0 - 1.0;
    let y = f32(index & 2u) * 2.0 - 1.0;
    return vec4<f32>(x, y, 0.0, 1.0);
}

@fragment
fn fs_main() -> @location(0) vec4<f32> {
    return dim.colour;
}
"#;

/// The size of the tint uniform in bytes: one `vec4<f32>`.
const UNIFORM_BYTES: usize = 16;

/// The full-frame tint quad, its pipeline and the tint it last held.
pub struct DimPass {
    /// The pipeline: one blended triangle, no depth state, no culling.
    pipeline: wgpu::RenderPipeline,
    /// The uniform buffer holding the tint.
    uniform: wgpu::Buffer,
    /// The bind group the pipeline reads the tint through.
    bind_group: wgpu::BindGroup,
    /// The tint the next draws use; `None` draws nothing at all.
    colour: Option<[f32; 4]>,
}

impl DimPass {
    /// Builds the pass for colour attachments in `format`.
    ///
    /// The pipeline has no depth-stencil state and no culling: it is meant for
    /// a pass that attaches only the colour target the scene has just drawn
    /// into, like the overlay's. Its fragment stage blends the tint with
    /// `src_alpha / one_minus_src_alpha`, the client's own pair.
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("oxide dim shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("oxide dim layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(UNIFORM_BYTES as u64),
                },
                count: None,
            }],
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("oxide dim uniform"),
            size: UNIFORM_BYTES as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("oxide dim bind group"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("oxide dim pipeline layout"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("oxide dim pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                // The triangle is drawn for coverage, not for a face.
                cull_mode: None,
                ..wgpu::PrimitiveState::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[color_target(format)],
            }),
            multiview: None,
            cache: None,
        });
        Self {
            pipeline,
            uniform,
            bind_group,
            colour: None,
        }
    }

    /// Sets the tint the next draws use, or clears it with `None`.
    ///
    /// The cleared state issues nothing, which is the state every frame of a
    /// live player is in; the client sets a colour while the death view is up
    /// and clears it when the respawn arrives. The uniform is rewritten either
    /// way, so a cleared pass never reads a stale tint should it draw again.
    pub fn set_colour(&mut self, queue: &wgpu::Queue, colour: Option<[f32; 4]>) {
        self.colour = colour;
        queue.write_buffer(&self.uniform, 0, &colour_bytes(colour.unwrap_or([0.0; 4])));
    }

    /// Draws the tint quad when one is set.
    ///
    /// Meant for a pass with no depth attachment, like the overlay's: the tint
    /// covers the frame whatever the scene left in the depth buffer.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        if self.colour.is_none() {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}

/// The tint uniform as the little-endian bytes the GPU copies.
fn colour_bytes(colour: [f32; 4]) -> [u8; UNIFORM_BYTES] {
    let mut bytes = [0u8; UNIFORM_BYTES];
    for (slot, value) in bytes.chunks_exact_mut(4).zip(colour) {
        slot.copy_from_slice(&value.to_le_bytes());
    }
    bytes
}

/// The pass's colour target: the surface's format, blended with the client's
/// `src_alpha / one_minus_src_alpha` pair.
fn color_target(format: wgpu::TextureFormat) -> Option<wgpu::ColorTargetState> {
    Some(wgpu::ColorTargetState {
        format,
        blend: Some(wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::SrcAlpha,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::SrcAlpha,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
        }),
        write_mask: wgpu::ColorWrites::ALL,
    })
}

#[cfg(test)]
mod tests {
    //! The tint's byte encoding, against synthetic colours.

    use super::colour_bytes;

    #[test]
    fn the_colour_bytes_are_four_little_endian_floats() {
        let bytes = colour_bytes([1.0, 0.5, 0.0, 0.25]);
        assert_eq!(bytes.len(), 16, "one vec4<f32>");
        assert_eq!(&bytes[0..4], &1.0f32.to_le_bytes());
        assert_eq!(&bytes[4..8], &0.5f32.to_le_bytes());
        assert_eq!(&bytes[8..12], &0.0f32.to_le_bytes());
        assert_eq!(&bytes[12..16], &0.25f32.to_le_bytes());
    }
}
