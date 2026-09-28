//! The vertex layout the terrain pipeline uploads, and the mesher's output type.

/// One terrain vertex: a position, its atlas uv, the packed light pair and the
/// colour with the face brightness and the tint baked in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vertex {
    /// World-space position.
    pub position: [f32; 3],
    /// Atlas uv of the corner, in level-0 texture coordinates.
    pub uv: [f32; 2],
    /// The light sampled at the corner, as the pair the client's light sampler
    /// produces: each channel is the 0..15 level shifted left four bits with
    /// eight added, so a full-sky corner is 248. The first channel is the block
    /// field and the second the sky field — the order the client hands the two
    /// to `glMultiTexCoord2f` (`ItemRenderer.java:113-116`) — so the first
    /// addresses the lightmap's column, which is the block level, and the
    /// second its row, the sky level.
    pub light: [u16; 2],
    /// Linear colour, already multiplied by the face brightness and the tint.
    pub colour: [u8; 4],
}

/// The size of one vertex in the byte stream the GPU receives.
pub const VERTEX_BYTES: usize = 28;

/// Serialises vertices into the byte layout the vertex buffer uses.
///
/// Each vertex becomes [`VERTEX_BYTES`] bytes: three little-endian `f32`
/// position components, two little-endian `f32` uv components, two
/// little-endian `u16` light channels, then four `u8` colour channels, in
/// vertex order. The layout is pinned by
/// `crates/oxide-render/tests/terrain_data.rs` and consumed by the terrain
/// pipeline's vertex attributes (`Float32x3` at 0, `Float32x2` at 12,
/// `Uint16x2` at 20 and `Unorm8x4` at 24, with an `array_stride` of
/// [`VERTEX_BYTES`]); the test and the pipeline descriptor change together
/// with any change here.
pub fn vertex_bytes(vertices: &[Vertex]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(vertices.len() * VERTEX_BYTES);
    for vertex in vertices {
        for component in vertex.position {
            bytes.extend_from_slice(&component.to_le_bytes());
        }
        for component in vertex.uv {
            bytes.extend_from_slice(&component.to_le_bytes());
        }
        for channel in vertex.light {
            bytes.extend_from_slice(&channel.to_le_bytes());
        }
        bytes.extend_from_slice(&vertex.colour);
    }
    bytes
}

/// A section's geometry, ready to be uploaded: one buffer per render layer.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChunkMesh {
    /// The three layers, indexed by [`Layer::index`] — opaque, cutout,
    /// translucent, which is also the order the client draws them in.
    pub layers: [LayerMesh; 3],
}

impl ChunkMesh {
    /// Whether the mesh draws nothing.
    ///
    /// The draw is indexed, so this reads every layer's `indices`; a mesh with
    /// vertices but no indices is empty.
    pub fn is_empty(&self) -> bool {
        self.layers.iter().all(LayerMesh::is_empty)
    }

    /// One layer's geometry.
    pub fn layer(&self, layer: Layer) -> &LayerMesh {
        &self.layers[layer.index()]
    }

    /// The number of vertices across the layers.
    pub fn vertex_count(&self) -> usize {
        self.layers.iter().map(|layer| layer.vertices.len()).sum()
    }
}

/// One render layer's geometry.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LayerMesh {
    /// Vertices, in triangle order.
    pub vertices: Vec<Vertex>,
    /// Indices into `vertices`.
    pub indices: Vec<u32>,
}

impl LayerMesh {
    /// Whether the layer draws nothing.
    ///
    /// The draw is indexed, so this reads `indices`; a layer with vertices but
    /// no indices is empty.
    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }
}

/// A terrain render layer: the client's `RenderLayer` buckets, one draw pass
/// each.
///
/// The client's four layers collapse to three buckets here: `CUTOUT` and
/// `CUTOUT_MIPPED` differ only in whether the pass mipmaps its textures
/// (`RenderLayer.java:43-77`), and both draw in the same pass with an alpha
/// test. The variants are declared in the pass order the client draws them
/// (`EntityRenderer`'s pass list), which is also the order
/// [`ChunkMesh::layers`] is indexed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    /// The solid pass: `RenderLayer.SOLID`.
    Opaque,
    /// The cutout pass: `RenderLayer.CUTOUT` and `RenderLayer.CUTOUT_MIPPED`.
    Cutout,
    /// The translucent pass: `RenderLayer.TRANSLUCENT`.
    Translucent,
}

impl Layer {
    /// Every layer, in the order the client draws them.
    pub const ALL: [Layer; 3] = [Layer::Opaque, Layer::Cutout, Layer::Translucent];

    /// The layer's slot in [`ChunkMesh::layers`].
    pub fn index(self) -> usize {
        self as usize
    }
}

/// Identifies one section's mesh: chunk x, chunk z, section index.
pub type SectionKey = (i32, i32, u8);
