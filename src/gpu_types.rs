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
    /// Bodies drawn with the high-detail sphere come first.
    pub bodies: Vec<GpuBody>,
    pub hi_count: u32,
    pub rings: Vec<GpuRing>,
    pub particles: Vec<GpuParticle>,
    pub lines: Vec<LineVertex>,
}
