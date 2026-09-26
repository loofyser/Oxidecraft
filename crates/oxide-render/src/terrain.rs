//! The vertex layout the terrain pipeline uploads, and the mesher's output type.

/// One terrain vertex: a position and a colour with the face brightness baked in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vertex {
    /// World-space position.
    pub position: [f32; 3],
    /// Linear colour, already multiplied by the face brightness.
    pub color: [f32; 3],
}

/// The size of one vertex in the byte stream the GPU receives.
pub const VERTEX_BYTES: usize = 24;

/// Serialises vertices into the byte layout the vertex buffer uses.
///
/// Each vertex becomes [`VERTEX_BYTES`] bytes: three little-endian `f32`
/// position components, then three little-endian `f32` colour components, in
/// vertex order. The layout is pinned by
/// `crates/oxide-render/tests/terrain_data.rs` and consumed by the terrain
/// pipeline's vertex attributes (`Float32x3` at offsets 0 and 12 with an
/// `array_stride` of [`VERTEX_BYTES`]); the test and the pipeline descriptor
/// change together with any change here.
pub fn vertex_bytes(vertices: &[Vertex]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(vertices.len() * VERTEX_BYTES);
    for vertex in vertices {
        for component in vertex.position {
            bytes.extend_from_slice(&component.to_le_bytes());
        }
        for component in vertex.color {
            bytes.extend_from_slice(&component.to_le_bytes());
        }
    }
    bytes
}

/// A section's geometry, ready to be uploaded.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChunkMesh {
    /// Vertices, in triangle order.
    pub vertices: Vec<Vertex>,
    /// Indices into `vertices`.
    pub indices: Vec<u32>,
}

impl ChunkMesh {
    /// Whether the mesh draws nothing.
    ///
    /// The draw is indexed, so this reads `indices`; a mesh with vertices but
    /// no indices is empty.
    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }

    /// The number of vertices.
    pub fn vertex_count(&self) -> usize {
        self.vertices.len()
    }
}

/// Identifies one section's mesh: chunk x, chunk z, section index.
pub type SectionKey = (i32, i32, u8);
