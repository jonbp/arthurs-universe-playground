//! CPU-side surface paint layers. Each paintable body can own one layer of a
//! GPU texture array (equirectangular RGBA8):
//!   R = height offset (128 = neutral), G = material id * 32 + 16, B = material strength.

use glam::Vec3;

pub const PAINT_W: usize = 512;
pub const PAINT_H: usize = 256;
pub const MAX_LAYERS: usize = 64;
pub const NEUTRAL: [u8; 4] = [128, 0, 0, 255];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Brush {
    Raise,
    Lower,
    Crater,
    Water,
    Lava,
    Ice,
    Vegetation,
    Rock,
    Sand,
    Scorch,
    Erase,
}

impl Brush {
    pub const ALL: [Brush; 11] = [
        Brush::Raise,
        Brush::Lower,
        Brush::Crater,
        Brush::Water,
        Brush::Lava,
        Brush::Ice,
        Brush::Vegetation,
        Brush::Rock,
        Brush::Sand,
        Brush::Scorch,
        Brush::Erase,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Brush::Raise => "Raise land",
            Brush::Lower => "Dig / lower",
            Brush::Crater => "Crater",
            Brush::Water => "Water",
            Brush::Lava => "Lava",
            Brush::Ice => "Ice",
            Brush::Vegetation => "Forest",
            Brush::Rock => "Rock",
            Brush::Sand => "Sand",
            Brush::Scorch => "Scorch",
            Brush::Erase => "Restore",
        }
    }
    fn material(self) -> Option<u8> {
        match self {
            Brush::Water => Some(1),
            Brush::Lava => Some(2),
            Brush::Ice => Some(3),
            Brush::Vegetation => Some(4),
            Brush::Rock => Some(5),
            Brush::Sand => Some(6),
            Brush::Scorch => Some(7),
            _ => None,
        }
    }
}

pub struct PaintStore {
    pub layers: Vec<Vec<[u8; 4]>>,
    pub dirty: Vec<bool>,
    free: Vec<u32>,
}

impl PaintStore {
    pub fn new() -> Self {
        let layers = vec![vec![NEUTRAL; PAINT_W * PAINT_H]; MAX_LAYERS];
        // Layer 0 is reserved as the "unpainted" neutral layer.
        let free = (1..MAX_LAYERS as u32).rev().collect();
        PaintStore { layers, dirty: vec![false; MAX_LAYERS], free }
    }

    pub fn alloc(&mut self) -> Option<u32> {
        self.free.pop()
    }

    pub fn release(&mut self, layer: u32) {
        if layer == 0 || layer as usize >= MAX_LAYERS {
            return;
        }
        self.layers[layer as usize].fill(NEUTRAL);
        self.dirty[layer as usize] = true;
        if !self.free.contains(&layer) {
            self.free.push(layer);
        }
    }

    pub fn reset_all(&mut self) {
        for l in 1..MAX_LAYERS {
            self.layers[l].fill(NEUTRAL);
            self.dirty[l] = true;
        }
        self.free = (1..MAX_LAYERS as u32).rev().collect();
    }


    /// Apply a brush stroke centred on unit vector `center` (body-local space).
    /// `radius` is the angular radius in radians, `strength` ~0..1 per application.
    pub fn apply(&mut self, layer: u32, center: Vec3, radius: f32, brush: Brush, strength: f32) {
        if layer == 0 || layer as usize >= MAX_LAYERS {
            return;
        }
        let c = center.normalize();
        let radius = radius.clamp(0.01, std::f32::consts::PI);
        let cos_r = radius.cos();
        let phi_c = c.y.clamp(-1.0, 1.0).acos();
        let row0 = (((phi_c - radius) / std::f32::consts::PI) * PAINT_H as f32).floor().max(0.0) as usize;
        let row1 = (((phi_c + radius) / std::f32::consts::PI) * PAINT_H as f32).ceil().min(PAINT_H as f32) as usize;
        let data = &mut self.layers[layer as usize];
        let mat = brush.material();
        for j in row0..row1 {
            let v = (j as f32 + 0.5) / PAINT_H as f32;
            let phi = v * std::f32::consts::PI;
            let (sp, cp) = phi.sin_cos();
            for i in 0..PAINT_W {
                let u = (i as f32 + 0.5) / PAINT_W as f32;
                let theta = (u - 0.5) * std::f32::consts::TAU;
                let (st, ct) = theta.sin_cos();
                let p = Vec3::new(sp * ct, cp, sp * st);
                let d = p.dot(c);
                if d < cos_r {
                    continue;
                }
                // normalized distance 0 (center) .. 1 (edge)
                let x = d.clamp(-1.0, 1.0).acos() / radius;
                let falloff = 1.0 - x * x * (3.0 - 2.0 * x);
                let w = falloff * strength;
                let texel = &mut data[j * PAINT_W + i];
                match brush {
                    Brush::Raise => texel[0] = add_u8(texel[0], w * 18.0),
                    Brush::Lower => texel[0] = add_u8(texel[0], -w * 18.0),
                    Brush::Crater => {
                        let bowl = if x < 1.0 { -(1.0 - x * x) * 1.3 } else { 0.0 };
                        let rim = (-((x - 0.92) / 0.12).powi(2)).exp() * 0.7;
                        texel[0] = add_u8(texel[0], (bowl + rim) * strength * 60.0);
                        // fresh crater floor is bare rock
                        apply_material(texel, 5, w * 0.6);
                    }
                    Brush::Erase => {
                        let h = texel[0] as f32;
                        texel[0] = (h + (128.0 - h) * w.min(1.0) * 0.5).round() as u8;
                        texel[2] = add_u8(texel[2], -w * 60.0);
                        if texel[2] == 0 {
                            texel[1] = 0;
                        }
                    }
                    _ => {
                        if let Some(m) = mat {
                            apply_material(texel, m, w);
                        }
                    }
                }
            }
        }
        self.dirty[layer as usize] = true;
    }
}

fn add_u8(v: u8, d: f32) -> u8 {
    (v as f32 + d).round().clamp(0.0, 255.0) as u8
}

fn apply_material(texel: &mut [u8; 4], mat: u8, w: f32) {
    let cur = texel[1] / 32;
    let amt = w * 70.0;
    if cur == mat || texel[2] == 0 {
        texel[1] = mat * 32 + 16;
        texel[2] = add_u8(texel[2], amt);
    } else {
        let s = texel[2] as f32 - amt;
        if s < 0.0 {
            texel[1] = mat * 32 + 16;
            texel[2] = (-s).min(255.0) as u8;
        } else {
            texel[2] = s as u8;
        }
    }
}
