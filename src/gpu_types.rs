//! Plain-old-data structs shared with the shaders (layouts must match the WGSL).

use bytemuck::{Pod, Zeroable};

pub const MAX_LIGHTS: usize = 8;
pub const MAX_BLACK_HOLES: usize = 8;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct Globals {
    pub view_proj: [[f32; 4]; 4],
    pub inv_view_proj: [[f32; 4]; 4],
    pub cam_right: [f32; 4],
    pub cam_up: [f32; 4],
    pub cam_fwd: [f32; 4],
    pub screen: [f32; 4],
    pub params: [f32; 4],
    pub light_pos: [[f32; 4]; MAX_LIGHTS],
    pub light_col: [[f32; 4]; MAX_LIGHTS],
    pub black_holes: [[f32; 4]; MAX_BLACK_HOLES],
    /// rgb = atmosphere colour seen from the camera, a = how much it hides the stars.
    pub sky: [f32; 4],
    /// xyz = "up" where the camera is, w = sunlight reaching solid models (0..1).
    pub sky_up: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Default)]
pub struct GpuBody {
    pub pos_radius: [f32; 4],
    pub rot: [f32; 4],
    pub col_a: [f32; 4],
    pub col_b: [f32; 4],
    pub col_c: [f32; 4],
    pub params: [f32; 4],
    pub atmo: [f32; 4],
    pub extra: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Default)]
pub struct GpuRing {
    pub center: [f32; 4],
    pub normal: [f32; 4],
    pub color: [f32; 4],
    pub params: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Default)]
pub struct GpuParticle {
    pub pos_size: [f32; 4],
    pub color: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Default)]
pub struct LineVertex {
    pub pos: [f32; 4],
    pub color: [f32; 4],
}

/// Everything the renderer needs for one frame.
pub struct FrameData {
    pub globals: Globals,
    /// Bodies in drawing order: close-up ("ultra") spheres, then high-detail, then low-detail.
    pub bodies: Vec<GpuBody>,
    /// How many bodies at the start of `bodies` fill the view and get the finest sphere.
    pub ultra_count: u32,
    /// How many after those get the high-detail sphere.
    pub hi_count: u32,
    pub rings: Vec<GpuRing>,
    pub particles: Vec<GpuParticle>,
    pub lines: Vec<LineVertex>,
    /// Opaque lit triangles (rockets). Same vertex layout as lines; colour alpha = glow.
    pub solids: Vec<LineVertex>,
    /// Scratch lists, appended to `bodies` once the frame is built.
    pub bodies_hi: Vec<GpuBody>,
    pub bodies_lo: Vec<GpuBody>,
}

impl FrameData {
    /// An empty frame. Keep it around and refill it every frame so the (large)
    /// particle and line buffers are allocated once, not 60 times a second.
    pub fn new() -> FrameData {
        FrameData {
            globals: Globals::zeroed(),
            bodies: Vec::new(),
            ultra_count: 0,
            hi_count: 0,
            rings: Vec::new(),
            particles: Vec::new(),
            lines: Vec::new(),
            solids: Vec::new(),
            bodies_hi: Vec::new(),
            bodies_lo: Vec::new(),
        }
    }
}
