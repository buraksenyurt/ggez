//!

use super::{context::GraphicsContext, gpu::arc::ArcBuffer, Color, DrawMode, LinearColor, Rect};
use crate::{GameError, GameResult};
use lyon::{
    math::Point as LPoint,
    path::{traits::PathBuilder, Polygon},
    tessellation as tess,
};
use std::sync::atomic::AtomicUsize;
use wgpu::util::DeviceExt;

/// Vertex format uploaded to vertex buffers.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vertex {
    /// `vec2` position.
    pub position: [f32; 2],
    /// `vec2` UV/texture coordinates.
    pub uv: [f32; 2],
    /// `vec4` color.
    pub color: [f32; 4],
}

impl Vertex {
    pub(crate) const fn layout() -> wgpu::VertexBufferLayout<'static> {
        const ATTRIBUTES: [wgpu::VertexAttribute; 3] = [
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x2,
                offset: 0,
                shader_location: 0,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x2,
                offset: 8,
                shader_location: 1,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x4,
                offset: 16,
                shader_location: 2,
            },
        ];

        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &ATTRIBUTES,
        }
    }
}

static NEXT_MESH_ID: AtomicUsize = AtomicUsize::new(0);

/// Mesh data stored on the GPU as a vertex and index buffer.
#[derive(Debug)]
pub struct Mesh {
    pub(crate) verts: ArcBuffer,
    pub(crate) inds: ArcBuffer,
    pub(crate) verts_capacity: usize,
    pub(crate) inds_capacity: usize,
    pub(crate) vertex_count: usize,
    pub(crate) index_count: usize,
    pub(crate) id: usize,
}

impl Mesh {
    /// Create a new mesh from a list of vertices and indices.
    pub fn new(gfx: &GraphicsContext, vertices: &[Vertex], indices: &[u32]) -> Self {
        Mesh {
            verts: Self::create_verts(gfx, vertices),
            inds: Self::create_inds(gfx, indices),
            verts_capacity: vertices.len(),
            inds_capacity: indices.len(),
            vertex_count: vertices.len(),
            index_count: indices.len(),
            id: NEXT_MESH_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst),
        }
    }

    /// Create a new mesh from [MeshData].
    pub fn from_data(gfx: &GraphicsContext, data: MeshData) -> Self {
        Self::new(gfx, &data.vertices, &data.indices)
    }

    /// Update the vertices of the mesh.
    #[allow(unsafe_code)]
    pub fn set_vertices(&mut self, gfx: &GraphicsContext, vertices: &[Vertex]) {
        self.vertex_count = vertices.len();
        if vertices.len() > self.verts_capacity {
            self.verts_capacity = vertices.len();
            self.verts = Self::create_verts(gfx, vertices);
            self.update_id();
        } else {
            gfx.queue.write_buffer(&self.verts, 0, unsafe {
                std::slice::from_raw_parts(
                    vertices as *const _ as *const u8,
                    vertices.len() * std::mem::size_of::<Vertex>(),
                )
            });
        }
    }

    /// Update the indices of the mesh.
    #[allow(unsafe_code)]
    pub fn set_indices(&mut self, gfx: &GraphicsContext, indices: &[u32]) {
        self.index_count = indices.len();
        if indices.len() > self.inds_capacity {
            self.inds_capacity = indices.len();
            self.inds = Self::create_inds(gfx, indices);
            self.update_id();
        } else {
            gfx.queue.write_buffer(&self.inds, 0, unsafe {
                std::slice::from_raw_parts(
                    indices as *const _ as *const u8,
                    indices.len() * std::mem::size_of::<u32>(),
                )
            });
        }
    }

    fn update_id(&mut self) {
        self.id = NEXT_MESH_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }

    #[allow(unsafe_code)]
    fn create_verts(gfx: &GraphicsContext, vertices: &[Vertex]) -> ArcBuffer {
        ArcBuffer::new(
            gfx.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: None,
                    contents: unsafe {
                        std::slice::from_raw_parts(
                            vertices.as_ptr() as *const u8,
                            std::mem::size_of::<Vertex>() * vertices.len(),
                        )
                    },
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                }),
        )
    }

    #[allow(unsafe_code)]
    fn create_inds(gfx: &GraphicsContext, indices: &[u32]) -> ArcBuffer {
        ArcBuffer::new(
            gfx.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: None,
                    contents: unsafe {
                        std::slice::from_raw_parts(indices.as_ptr() as *const u8, 4 * indices.len())
                    },
                    usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                }),
        )
    }
}

/// Borrowed mesh data.
#[derive(Debug, Clone)]
pub struct MeshData<'a> {
    /// List of vertices.
    pub vertices: &'a [Vertex],
    /// List of indices (indices into `vertices`).
    pub indices: &'a [u32],
}

/// Builder pattern for constructing meshes.
#[derive(Debug, Clone)]
pub struct MeshBuilder {
    buffer: tess::geometry_builder::VertexBuffers<Vertex, u32>,
}

impl Default for MeshBuilder {
    fn default() -> Self {
        Self {
            buffer: tess::VertexBuffers::new(),
        }
    }
}

impl MeshBuilder {
    /// Create a new [MeshBuilder].
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a new mesh for a line of one or more connected segments.
    pub fn line<P>(&mut self, points: &[P], width: f32, color: Color) -> GameResult<&mut Self>
    where
        P: Into<mint::Point2<f32>> + Clone,
    {
        self.polyline(DrawMode::stroke(width), points, color)
    }

    /// Create a new mesh for a circle.
    ///
    /// For the meaning of the `tolerance` parameter, [see here](https://docs.rs/lyon_geom/0.11.0/lyon_geom/#flattening).
    pub fn circle<P>(
        &mut self,
        mode: DrawMode,
        point: P,
        radius: f32,
        tolerance: f32,
        color: Color,
    ) -> GameResult<&mut Self>
    where
        P: Into<mint::Point2<f32>>,
    {
        assert!(
            tolerance > 0.0,
            "Tolerances <= 0 are invalid, see https://github.com/ggez/ggez/issues/892"
        );
        {
            let point = point.into();
            let buffers = &mut self.buffer;
            let vb = VertexBuilder {
                color: LinearColor::from(color),
            };
            match mode {
                DrawMode::Fill(fill_options) => {
                    let mut tessellator = tess::FillTessellator::new();
                    let _ = tessellator.tessellate_circle(
                        tess::math::point(point.x, point.y),
                        radius,
                        &fill_options.with_tolerance(tolerance),
                        &mut tess::BuffersBuilder::new(buffers, vb),
                    );
                }
                DrawMode::Stroke(options) => {
                    let mut tessellator = tess::StrokeTessellator::new();
                    let _ = tessellator.tessellate_circle(
                        tess::math::point(point.x, point.y),
                        radius,
                        &options.with_tolerance(tolerance),
                        &mut tess::BuffersBuilder::new(buffers, vb),
                    );
                }
            };
        }
        Ok(self)
    }

    /// Create a new mesh for an ellipse.
    ///
    /// For the meaning of the `tolerance` parameter, [see here](https://docs.rs/lyon_geom/0.11.0/lyon_geom/#flattening).
    pub fn ellipse<P>(
        &mut self,
        mode: DrawMode,
        point: P,
        radius1: f32,
        radius2: f32,
        tolerance: f32,
        color: Color,
    ) -> GameResult<&mut Self>
    where
        P: Into<mint::Point2<f32>>,
    {
        assert!(
            tolerance > 0.0,
            "Tolerances <= 0 are invalid, see https://github.com/ggez/ggez/issues/892"
        );
        {
            let buffers = &mut self.buffer;
            let point = point.into();
            let vb = VertexBuilder {
                color: LinearColor::from(color),
            };
            match mode {
                DrawMode::Fill(fill_options) => {
                    let builder = &mut tess::BuffersBuilder::new(buffers, vb);
                    let mut tessellator = tess::FillTessellator::new();
                    let _ = tessellator.tessellate_ellipse(
                        tess::math::point(point.x, point.y),
                        tess::math::vector(radius1, radius2),
                        tess::math::Angle { radians: 0.0 },
                        tess::path::Winding::Positive,
                        &fill_options.with_tolerance(tolerance),
                        builder,
                    );
                }
                DrawMode::Stroke(options) => {
                    let builder = &mut tess::BuffersBuilder::new(buffers, vb);
                    let mut tessellator = tess::StrokeTessellator::new();
                    let _ = tessellator.tessellate_ellipse(
                        tess::math::point(point.x, point.y),
                        tess::math::vector(radius1, radius2),
                        tess::math::Angle { radians: 0.0 },
                        tess::path::Winding::Positive,
                        &options.with_tolerance(tolerance),
                        builder,
                    );
                }
            };
        }
        Ok(self)
    }

    /// Create a new mesh for a series of connected lines.
    pub fn polyline<P>(
        &mut self,
        mode: DrawMode,
        points: &[P],
        color: Color,
    ) -> GameResult<&mut Self>
    where
        P: Into<mint::Point2<f32>> + Clone,
    {
        if points.len() < 2 {
            return Err(GameError::LyonError(
                "MeshBuilder::polyline() got a list of < 2 points".to_string(),
            ));
        }

        self.polyline_inner(mode, points, false, color)
    }

    /// Create a new mesh for a closed polygon.
    /// The points given must be in clockwise order,
    /// otherwise at best the polygon will not draw.
    pub fn polygon<P>(
        &mut self,
        mode: DrawMode,
        points: &[P],
        color: Color,
    ) -> GameResult<&mut Self>
    where
        P: Into<mint::Point2<f32>> + Clone,
    {
        if points.len() < 3 {
            return Err(GameError::LyonError(
                "MeshBuilder::polygon() got a list of < 3 points".to_string(),
            ));
        }

        self.polyline_inner(mode, points, true, color)
    }

    fn polyline_inner<P>(
        &mut self,
        mode: DrawMode,
        points: &[P],
        is_closed: bool,
        color: Color,
    ) -> GameResult<&mut Self>
    where
        P: Into<mint::Point2<f32>> + Clone,
    {
        let vb = VertexBuilder {
            color: LinearColor::from(color),
        };
        self.polyline_with_vertex_builder(mode, points, is_closed, vb)
    }

    /// Create a new mesh for a given polyline using a custom vertex builder.
    /// The points given must be in clockwise order.
    pub fn polyline_with_vertex_builder<P, V>(
        &mut self,
        mode: DrawMode,
        points: &[P],
        is_closed: bool,
        vb: V,
    ) -> GameResult<&mut Self>
    where
        P: Into<mint::Point2<f32>> + Clone,
        V: tess::StrokeVertexConstructor<Vertex> + tess::FillVertexConstructor<Vertex>,
    {
        {
            assert!(points.len() > 1);
            let buffers = &mut self.buffer;
            let points: Vec<LPoint> = points
                .iter()
                .cloned()
                .map(|p| {
                    let mint_point: mint::Point2<f32> = p.into();
                    tess::math::point(mint_point.x, mint_point.y)
                })
                .collect();
            let polygon = Polygon {
                points: &points,
                closed: is_closed,
            };
            match mode {
                DrawMode::Fill(options) => {
                    let builder = &mut tess::BuffersBuilder::new(buffers, vb);
                    let tessellator = &mut tess::FillTessellator::new();
                    let _ = tessellator.tessellate_polygon(polygon, &options, builder)?;
                }
                DrawMode::Stroke(options) => {
                    let builder = &mut tess::BuffersBuilder::new(buffers, vb);
                    let tessellator = &mut tess::StrokeTessellator::new();
                    let _ = tessellator.tessellate_polygon(polygon, &options, builder)?;
                }
            };
        }
        Ok(self)
    }

    /// Create a new mesh for a rectangle.
    pub fn rectangle(
        &mut self,
        mode: DrawMode,
        bounds: Rect,
        color: Color,
    ) -> GameResult<&mut Self> {
        {
            let buffers = &mut self.buffer;
            let rect = tess::math::rect(bounds.x, bounds.y, bounds.w, bounds.h);
            let vb = VertexBuilder {
                color: LinearColor::from(color),
            };
            match mode {
                DrawMode::Fill(fill_options) => {
                    let builder = &mut tess::BuffersBuilder::new(buffers, vb);
                    let mut tessellator = tess::FillTessellator::new();
                    let _ = tessellator.tessellate_rectangle(&rect, &fill_options, builder);
                }
                DrawMode::Stroke(options) => {
                    let builder = &mut tess::BuffersBuilder::new(buffers, vb);
                    let mut tessellator = tess::StrokeTessellator::new();
                    let _ = tessellator.tessellate_rectangle(&rect, &options, builder);
                }
            };
        }
        Ok(self)
    }

    /// Create a new mesh for a rounded rectangle.
    pub fn rounded_rectangle(
        &mut self,
        mode: DrawMode,
        bounds: Rect,
        radius: f32,
        color: Color,
    ) -> GameResult<&mut Self> {
        {
            let buffers = &mut self.buffer;
            let rect = tess::math::rect(bounds.x, bounds.y, bounds.w, bounds.h);
            let radii = tess::path::builder::BorderRadii::new(radius);
            let vb = VertexBuilder {
                color: LinearColor::from(color),
            };
            let mut path_builder = tess::path::Path::builder();
            path_builder.add_rounded_rectangle(&rect, &radii, tess::path::Winding::Positive);
            let path = path_builder.build();

            match mode {
                DrawMode::Fill(fill_options) => {
                    let builder = &mut tess::BuffersBuilder::new(buffers, vb);
                    let mut tessellator = tess::FillTessellator::new();
                    let _ = tessellator.tessellate_path(&path, &fill_options, builder);
                }
                DrawMode::Stroke(options) => {
                    let builder = &mut tess::BuffersBuilder::new(buffers, vb);
                    let mut tessellator = tess::StrokeTessellator::new();
                    let _ = tessellator.tessellate_path(&path, &options, builder);
                }
            };
        }
        Ok(self)
    }

    /// Create a new [`Mesh`](struct.Mesh.html) from a raw list of triangles.
    /// The length of the list must be a multiple of 3.
    ///
    /// Currently does not support UV's or indices.
    pub fn triangles<P>(&mut self, triangles: &[P], color: Color) -> GameResult<&mut Self>
    where
        P: Into<mint::Point2<f32>> + Clone,
    {
        {
            if (triangles.len() % 3) != 0 {
                return Err(GameError::LyonError(String::from(
                    "Called Mesh::triangles() with points that have a length not a multiple of 3.",
                )));
            }
            let tris = triangles
                .iter()
                .cloned()
                .map(|p| {
                    // Gotta turn ggez Point2's into lyon points
                    let mint_point = p.into();
                    lyon::math::point(mint_point.x, mint_point.y)
                })
                // Removing this collect might be nice, but is not easy.
                // We can chunk a slice, but can't chunk an arbitrary
                // iterator.
                // Using the itertools crate doesn't really make anything
                // nicer, so we'll just live with it.
                .collect::<Vec<_>>();
            let tris = tris.chunks(3);
            let vb = VertexBuilder {
                color: LinearColor::from(color),
            };
            for tri in tris {
                // Ideally this assert makes bounds-checks only happen once.
                assert!(tri.len() == 3);
                let first_index: u32 = self.buffer.vertices.len().try_into().unwrap();
                self.buffer.vertices.push(vb.new_vertex(tri[0]));
                self.buffer.vertices.push(vb.new_vertex(tri[1]));
                self.buffer.vertices.push(vb.new_vertex(tri[2]));
                self.buffer.indices.push(first_index);
                self.buffer.indices.push(first_index + 1);
                self.buffer.indices.push(first_index + 2);
            }
        }
        Ok(self)
    }

    /// Takes the accumulated geometry and return it as [MeshData].
    pub fn build(&self) -> MeshData {
        MeshData {
            vertices: &self.buffer.vertices,
            indices: &self.buffer.indices,
        }
    }
}

#[derive(Copy, Clone, PartialEq, Debug)]
struct VertexBuilder {
    color: LinearColor,
}

impl VertexBuilder {
    fn new_vertex(self, position: LPoint) -> Vertex {
        Vertex {
            position: [position.x, position.y],
            uv: [position.x, position.y],
            color: self.color.into(),
        }
    }
}

impl tess::StrokeVertexConstructor<Vertex> for VertexBuilder {
    fn new_vertex(&mut self, vertex: tess::StrokeVertex) -> Vertex {
        let position = vertex.position();
        Vertex {
            position: [position.x, position.y],
            uv: [0.0, 0.0],
            color: self.color.into(),
        }
    }
}

impl tess::FillVertexConstructor<Vertex> for VertexBuilder {
    fn new_vertex(&mut self, vertex: tess::FillVertex) -> Vertex {
        let position = vertex.position();
        Vertex {
            position: [position.x, position.y],
            uv: [0.0, 0.0],
            color: self.color.into(),
        }
    }
}
