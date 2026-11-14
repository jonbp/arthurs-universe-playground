//! Converts the simulation state into GPU-ready frame data.

use glam::{DVec3, Vec3, Vec4};

use crate::body::BodyKind;
use crate::camera::CameraFrame;
use crate::explore::SceneExtras;
use crate::gpu_types::*;
use crate::settings::PlanetDetail;
use crate::universe::{Universe, P_GLOW, P_HOT, P_IMMORTAL};

pub struct SceneOptions {
    pub trails: bool,
    pub exposure: f32,
    pub bloom: f32,
    pub lensing: bool,
    pub stars: f32,
    pub planet_detail: PlanetDetail,
    pub selected: Option<u64>,
    pub hovered: Option<u64>,
    pub time: f32,
}

/// A world-space polyline overlay (predictions, aim arrows, ...)
pub struct Polyline {
    pub points: Vec<DVec3>,
    pub color: Vec4,
    pub dashed: bool,
    pub fade: bool,
}

/// Fill `frame` (reusing its allocations) with everything needed to draw the universe.
pub fn build_frame(frame: &mut FrameData, u: &Universe, cam: &CameraFrame, opts: &SceneOptions, overlays: &[Polyline], extras: &SceneExtras) {
    let FrameData { bodies: bodies_ultra, bodies_hi, bodies_lo, rings, particles, lines, solids, .. } = frame;
    bodies_ultra.clear();
    bodies_hi.clear();
    bodies_lo.clear();
    solids.clear();
    solids.extend_from_slice(&extras.solids);
    rings.clear();
    particles.clear();
    particles.reserve(u.particles.len() + 64);
    lines.clear();

    // ---- lights ----
    let mut lights: Vec<(Vec3, f32, Vec3)> = Vec::new(); // rel pos, radius, color*intensity
    for b in &u.bodies {
        if b.kind.is_luminous() {
            lights.push((cam.rel(b.pos), b.radius as f32, b.light_color() * b.luminosity()));
        }
    }
    for f in &u.flashes {
        lights.push((cam.rel(f.pos), 1.0, f.color * f.intensity));
    }
    lights.sort_by(|a, b| b.2.length_squared().total_cmp(&a.2.length_squared()));
    lights.truncate(MAX_LIGHTS);

    // ---- bodies ----
    let mut black_holes: Vec<(f32, [f32; 4])> = Vec::new();
    for b in &u.bodies {
        let rel = cam.rel(b.pos);
        let px = cam.screen_radius(b.pos, b.radius);
        let q = b.rotation();
        let highlight = if Some(b.id) == opts.selected {
            1.0
        } else if Some(b.id) == opts.hovered {
            0.5
        } else {
            0.0
        };
        let s = &b.surface;
        let g = GpuBody {
            pos_radius: [rel.x, rel.y, rel.z, b.radius as f32],
            rot: [q.x, q.y, q.z, q.w],
            col_a: [s.colors[0].x, s.colors[0].y, s.colors[0].z, b.kind as u32 as f32],
            col_b: [s.colors[1].x, s.colors[1].y, s.colors[1].z, s.seed],
            col_c: [s.colors[2].x, s.colors[2].y, s.colors[2].z, b.paint_layer.unwrap_or(0) as f32],
            params: [s.terrain, s.sea_level, s.clouds, b.heat],
            atmo: [s.atmosphere.x, s.atmosphere.y, s.atmosphere.z, s.atmo_strength],
            extra: [b.emissive(), b.flash.min(3.0) * if b.kind.is_luminous() { 0.0 } else { 1.0 }, highlight, 0.0],
        };
        // A world that more than fills the view (flying low, walking) gets the finest mesh.
        // Thresholds are in pixels of screen radius.
        let (ultra_px, hi_px, max_ultra) = match opts.planet_detail {
            PlanetDetail::Low => (cam.size.y * 3.0, 160.0, 1),
            PlanetDetail::Standard => (cam.size.y * 1.5, 50.0, 2),
            PlanetDetail::High => (cam.size.y * 0.6, 16.0, 4),
        };
        if px > ultra_px && bodies_ultra.len() < max_ultra {
            bodies_ultra.push(g);
        } else if px > hi_px {
            bodies_hi.push(g);
        } else {
            bodies_lo.push(g);
        }

        if let Some(r) = &b.ring {
            let axis = b.axis();
            rings.push(GpuRing {
                center: [rel.x, rel.y, rel.z, b.radius as f32],
                normal: [axis.x, axis.y, axis.z, r.inner],
                color: [r.color.x, r.color.y, r.color.z, r.opacity],
                params: [r.outer, 0.0, s.seed, 1.0],
            });
        }

        match b.kind {
            BodyKind::BlackHole => {
                let axis = b.axis();
                let r = b.radius as f32;
                rings.push(GpuRing {
                    center: [rel.x, rel.y, rel.z, r],
                    normal: [axis.x, axis.y, axis.z, r * 2.6],
                    color: [1.0, 0.6, 0.3, 1.0],
                    params: [r * (7.0 + b.feeding * 1.5), 1.0, s.seed, 0.5 + b.feeding * 0.7],
                });
                if let Some(p) = cam.project(b.pos) {
                    let screen_r = px / cam.size.y;
                    if opts.lensing && screen_r > 0.0005 {
                        black_holes.push((
                            screen_r,
                            [p.x / cam.size.x, p.y / cam.size.y, screen_r, (b.feeding * 0.4).min(1.0)],
                        ));
                    }
                }
            }
            BodyKind::Star => {
                let c = b.light_color() * (1.2 + b.flash * 2.0);
                let size = b.radius as f32 * (5.0 + b.flash * 6.0);
                particles.push(GpuParticle { pos_size: [rel.x, rel.y, rel.z, size], color: [c.x, c.y, c.z, 1.0] });
            }
            BodyKind::NeutronStar => {
                let c = b.light_color() * (2.0 + b.flash * 3.0);
                particles.push(GpuParticle { pos_size: [rel.x, rel.y, rel.z, b.radius as f32 * 30.0], color: [c.x, c.y, c.z, 1.0] });
            }
            _ => {
                if b.heat > 0.05 || b.flash > 0.05 {
                    let c = Vec3::new(1.0, 0.45, 0.15) * (b.heat * 0.2 + b.flash * 0.5);
                    particles.push(GpuParticle { pos_size: [rel.x, rel.y, rel.z, b.radius as f32 * 3.0], color: [c.x, c.y, c.z, 1.0] });
                }
            }
        }

        // ---- trails ----
        if opts.trails && b.trail.len() > 1 && (!b.name.is_empty() || u.bodies.len() < 60) {
            let c = b.trail_color();
            let n = b.trail.len();
            let alpha_scale = if b.kind == BodyKind::BlackHole { 0.5 } else { 0.35 };
            let mut prev = cam.rel(b.trail[0]);
            for (k, p) in b.trail.iter().enumerate().skip(1) {
                let cur = cam.rel(*p);
                let a0 = (k - 1) as f32 / n as f32 * alpha_scale;
                let a1 = k as f32 / n as f32 * alpha_scale;
                lines.push(LineVertex { pos: [prev.x, prev.y, prev.z, 1.0], color: [c.x, c.y, c.z, a0] });
                lines.push(LineVertex { pos: [cur.x, cur.y, cur.z, 1.0], color: [c.x, c.y, c.z, a1] });
                prev = cur;
            }
            lines.push(LineVertex { pos: [prev.x, prev.y, prev.z, 1.0], color: [c.x, c.y, c.z, alpha_scale] });
            lines.push(LineVertex { pos: [rel.x, rel.y, rel.z, 1.0], color: [c.x, c.y, c.z, alpha_scale] });
        }
    }
    let ultra_count = bodies_ultra.len() as u32;
    let hi_count = bodies_hi.len() as u32;
    bodies_ultra.append(bodies_hi);
    bodies_ultra.append(bodies_lo);

    // ---- overlays ----
    for pl in overlays {
        let n = pl.points.len();
        for k in 1..n {
            if pl.dashed && k % 2 == 0 {
                continue;
            }
            let a = cam.rel(pl.points[k - 1]);
            let b = cam.rel(pl.points[k]);
            let fade = if pl.fade { 1.0 - k as f32 / n as f32 } else { 1.0 };
            let c = pl.color;
            lines.push(LineVertex { pos: [a.x, a.y, a.z, 1.0], color: [c.x, c.y, c.z, c.w * fade] });
            lines.push(LineVertex { pos: [b.x, b.y, b.z, 1.0], color: [c.x, c.y, c.z, c.w * fade] });
        }
    }

    // ---- particles ----
    let eye = cam.eye.as_vec3();
    for p in &u.particles {
        let lf = (p.life / p.max_life).clamp(0.0, 1.0);
        let fade = if p.flags & P_IMMORTAL != 0 { 1.0 } else { (lf * 3.0).min(1.0) };
        let mut c = p.color;
        if p.flags & P_HOT != 0 {
            c = c.lerp(Vec3::new(1.0, 0.85, 0.6) * 3.0, lf * lf * 0.7);
        }
        c *= fade;
        let rel = p.pos - eye;
        let shape = if p.flags & P_GLOW != 0 { 1.0 } else { 0.0 };
        particles.push(GpuParticle { pos_size: [rel.x, rel.y, rel.z, p.size], color: [c.x, c.y, c.z, shape] });
    }
    particles.extend_from_slice(&extras.glows);
    for f in &u.flashes {
        let rel = cam.rel(f.pos);
        // Glow brightness grows sub-linearly so huge flashes don't white out the screen.
        let c = f.color * f.intensity.sqrt() * 0.45;
        let size = f.size * (f.intensity / (f.intensity + 1.0));
        particles.push(GpuParticle { pos_size: [rel.x, rel.y, rel.z, size], color: [c.x, c.y, c.z, 1.0] });
    }

    // ---- globals ----
    let mut globals = Globals {
        view_proj: cam.view_proj.to_cols_array_2d(),
        inv_view_proj: cam.inv_view_proj.to_cols_array_2d(),
        cam_right: [cam.right.x, cam.right.y, cam.right.z, opts.stars],
        cam_up: [cam.up.x, cam.up.y, cam.up.z, 0.0],
        cam_fwd: [cam.fwd.x, cam.fwd.y, cam.fwd.z, cam.tan_half],
        screen: [cam.size.x, cam.size.y, opts.time, opts.exposure],
        params: [lights.len() as f32, opts.bloom, 0.0, 0.012],
        light_pos: [[0.0; 4]; MAX_LIGHTS],
        light_col: [[0.0; 4]; MAX_LIGHTS],
        black_holes: [[0.0; 4]; MAX_BLACK_HOLES],
        sky: extras.sky,
        sky_up: extras.sky_up,
        ui_blur: [[0.0; 4]; crate::gpu_types::MAX_UI_BLUR],
        ui_blur_params: [0.0; 4],
    };
    for (i, (p, r, c)) in lights.iter().enumerate() {
        globals.light_pos[i] = [p.x, p.y, p.z, *r];
        globals.light_col[i] = [c.x, c.y, c.z, 0.0];
    }
    black_holes.sort_by(|a, b| b.0.total_cmp(&a.0));
    black_holes.truncate(MAX_BLACK_HOLES);
    globals.params[2] = black_holes.len() as f32;
    for (i, (_, bh)) in black_holes.iter().enumerate() {
        globals.black_holes[i] = *bh;
    }

    frame.globals = globals;
    frame.ultra_count = ultra_count;
    frame.hi_count = hi_count;
}
