//! CPU copy of the planet shader's terrain (see `shaders/common.wgsl` and
//! `shaders/planet.wgsl`), so rockets can land on, and astronauts can walk on,
//! exactly the ground the GPU draws. Keep the two in sync.

use glam::{DVec3, Vec3};

use crate::body::{Body, BodyKind};
use crate::paint::{PaintStore, MAX_LAYERS, PAINT_H, PAINT_W};

fn fract(x: f32) -> f32 {
    x - x.floor()
}

fn vfract(v: Vec3) -> Vec3 {
    Vec3::new(fract(v.x), fract(v.y), fract(v.z))
}

fn mix(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

// Integer hashes of the noise cell (`mix32`, `cell_hash`, `lattice`, `lattice3` in
// common.wgsl). Integer maths is exact, so the GPU and the CPU get identical values;
// the float maths around them is smooth, so rounding differences stay invisible.
fn mix32(x: u32) -> u32 {
    let mut h = x;
    h = (h ^ (h >> 16)).wrapping_mul(0x7feb352d);
    h = (h ^ (h >> 15)).wrapping_mul(0x846ca68b);
    h ^ (h >> 16)
}

fn cell_hash(cell: Vec3) -> u32 {
    let c = |v: f32| v as i32 as u32;
    mix32(c(cell.x).wrapping_mul(0x8da6b343) ^ c(cell.y).wrapping_mul(0xd8163841) ^ c(cell.z).wrapping_mul(0xcb1ab31f))
}

fn unit_float(h: u32) -> f32 {
    (h >> 8) as f32 * (1.0 / 16777216.0)
}

fn lattice(cell: Vec3) -> f32 {
    unit_float(cell_hash(cell))
}

fn lattice3(cell: Vec3) -> Vec3 {
    let a = cell_hash(cell);
    let b = mix32(a ^ 0x9e3779b9);
    let c = mix32(b ^ 0x85ebca6b);
    Vec3::new(unit_float(a), unit_float(b), unit_float(c))
}

fn noise3(x: Vec3) -> f32 {
    let i = x.floor();
    let f = vfract(x);
    let u = f * f * (Vec3::splat(3.0) - 2.0 * f);
    let h = |dx: f32, dy: f32, dz: f32| lattice(i + Vec3::new(dx, dy, dz));
    mix(
        mix(mix(h(0.0, 0.0, 0.0), h(1.0, 0.0, 0.0), u.x), mix(h(0.0, 1.0, 0.0), h(1.0, 1.0, 0.0), u.x), u.y),
        mix(mix(h(0.0, 0.0, 1.0), h(1.0, 0.0, 1.0), u.x), mix(h(0.0, 1.0, 1.0), h(1.0, 1.0, 1.0), u.x), u.y),
        u.z,
    )
}

fn fbm(p: Vec3, octaves: u32) -> f32 {
    let (mut sum, mut amp, mut q) = (0.0, 0.5, p);
    for _ in 0..octaves {
        sum += amp * noise3(q);
        q = q * 2.03 + Vec3::new(1.7, 9.2, 3.1);
        amp *= 0.5;
    }
    sum
}

fn ridged(p: Vec3, octaves: u32) -> f32 {
    let (mut sum, mut amp, mut q) = (0.0, 0.5, p);
    for _ in 0..octaves {
        let n = 1.0 - (noise3(q) * 2.0 - 1.0).abs();
        sum += amp * n * n;
        q = q * 2.1 + Vec3::new(5.3, 1.3, 7.7);
        amp *= 0.5;
    }
    sum
}

fn craters(p: Vec3, scale: f32, seed: f32) -> f32 {
    let q = p * scale + Vec3::splat(seed * 13.0);
    let i = q.floor();
    let f = vfract(q);
    let mut h = 0.0;
    for z in -1..=1 {
        for y in -1..=1 {
            for x in -1..=1 {
                let o = Vec3::new(x as f32, y as f32, z as f32);
                let rnd = lattice3(i + o);
                let r = 0.18 + 0.3 * rnd.z * rnd.z;
                if rnd.x > 0.55 {
                    continue;
                }
                let d = (o + rnd - f).length() / r;
                if d < 1.4 {
                    let bowl = (d * d - 1.0) * 0.6;
                    let rim = (-((d - 1.0) / 0.18).powi(2)).exp() * 0.35;
                    h += if d < 1.0 { bowl + rim } else { rim * smoothstep(1.4, 1.0, d) } * r;
                }
            }
        }
    }
    h
}

/// Procedural base elevation in roughly [-1, 1] (`base_height` in planet.wgsl).
fn base_height(kind: BodyKind, p: Vec3, seed: f32) -> f32 {
    let sp = p + Vec3::new(seed * 7.13, seed * 3.71, seed * 5.17);
    match kind {
        BodyKind::Rocky => (fbm(sp * 2.5, 4) - 0.5) * 0.6 + craters(p, 3.0, seed) + 0.5 * craters(p, 8.0, seed + 3.0),
        BodyKind::Earthlike => {
            let c = fbm(sp * 1.4, 5) * 2.0 - 1.0;
            let m = ridged(sp * 4.0, 4);
            c + c.max(0.0) * m * 0.8
        }
        BodyKind::Desert => (fbm(sp * 2.0, 5) - 0.5) * 1.2 + 0.6 * craters(p, 5.0, seed),
        BodyKind::Lava => fbm(sp * 2.5, 4) - 0.5,
        BodyKind::Ice => (fbm(sp * 3.0, 3) - 0.5) * 0.4 - ridged(sp * 5.0, 3) * 0.3,
        BodyKind::Asteroid => (fbm(sp * 1.3, 3) - 0.5) * 3.0 + craters(p, 3.5, seed),
        _ => 0.0,
    }
}

/// Bilinear sample of a paint layer: (height 0..1, material id, material strength 0..1).
/// Matches the GPU sampler: linear filtering, U repeats, V clamps.
fn paint_sample(paint: &PaintStore, layer: Option<u32>, p: Vec3) -> (f32, u8, f32) {
    let Some(layer) = layer.filter(|l| *l > 0 && (*l as usize) < MAX_LAYERS) else {
        return (0.5, 0, 0.0);
    };
    let data = &paint.layers[layer as usize];
    let u = p.z.atan2(p.x) / std::f32::consts::TAU + 0.5;
    let v = p.y.clamp(-1.0, 1.0).acos() / std::f32::consts::PI;
    let x = u * PAINT_W as f32 - 0.5;
    let y = (v * PAINT_H as f32 - 0.5).clamp(0.0, PAINT_H as f32 - 1.0);
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let w = PAINT_W as i64;
    let xi = |x: f32| (x as i64).rem_euclid(w) as usize;
    let yi = |y: f32| (y as usize).min(PAINT_H - 1);
    let t = |x: f32, y: f32| data[yi(y) * PAINT_W + xi(x)];
    let (a, b, c, d) = (t(x0, y0), t(x0 + 1.0, y0), t(x0, y0 + 1.0), t(x0 + 1.0, y0 + 1.0));
    let ch = |k: usize| mix(mix(a[k] as f32, b[k] as f32, fx), mix(c[k] as f32, d[k] as f32, fx), fy) / 255.0;
    // The material id is categorical: take it from the nearest texel.
    let nearest = t(if fx < 0.5 { x0 } else { x0 + 1.0 }, if fy < 0.5 { y0 } else { y0 + 1.0 });
    (ch(0), nearest[1] / 32, ch(2))
}

/// What the ground is like at one point of a body.
#[derive(Clone, Copy, Debug)]
pub struct Ground {
    /// Relative radial displacement: the surface is at `radius * (1 + disp)`.
    pub disp: f32,
    /// Elevation in the shader's units (about -1..2), before sea-level clamping.
    pub elev: f32,
    /// Under the sea of an Earth-like world.
    pub water: bool,
    /// Painted material id (0 = none; see `Brush::material`) and its strength.
    pub material: u8,
    pub material_strength: f32,
}

/// Does this kind of body have ground you can land and stand on?
pub fn has_solid_surface(kind: BodyKind) -> bool {
    !kind.is_stellar() && !matches!(kind, BodyKind::GasGiant | BodyKind::IceGiant)
}

/// Sample the ground at body-local unit direction `local`.
pub fn ground(b: &Body, paint: &PaintStore, local: Vec3) -> Ground {
    let p = local.normalize();
    let (h, material, material_strength) = paint_sample(paint, b.paint_layer, p);
    let paint_h = (h - 0.5) * 2.0;
    if b.kind.is_stellar() {
        return Ground { disp: 0.0, elev: 0.0, water: false, material: 0, material_strength: 0.0 };
    }
    let s = &b.surface;
    let base = base_height(b.kind, p, s.seed);
    let elev = base + paint_h;
    if b.kind == BodyKind::Earthlike {
        let water = elev < s.sea_level;
        Ground { disp: elev.max(s.sea_level) * s.terrain, elev, water, material, material_strength }
    } else {
        Ground { disp: base * s.terrain + paint_h * 0.07, elev, water: false, material, material_strength }
    }
}

/// Distance from the body's centre to its surface along body-local direction `local`.
pub fn surface_radius(b: &Body, paint: &PaintStore, local: Vec3) -> f64 {
    b.radius * (1.0 + ground(b, paint, local).disp as f64)
}

/// Like [`surface_radius`], for a world-space direction from the body's centre.
pub fn surface_radius_world(b: &Body, paint: &PaintStore, dir: DVec3) -> f64 {
    surface_radius(b, paint, b.rotation().inverse() * dir.as_vec3())
}

/// Upper bound on how far terrain can rise above the base radius (relative).
pub fn max_displacement(b: &Body) -> f64 {
    if b.kind.is_stellar() { 0.0 } else { 3.0 * b.surface.terrain.abs() as f64 + 0.08 }
}

/// A short, friendly name for the ground at this spot.
pub fn ground_name(b: &Body, g: &Ground) -> &'static str {
    if g.material > 0 && g.material_strength > 0.3 {
        return match g.material {
            1 => "Water",
            2 => "Lava",
            3 => "Ice",
            4 => "Forest",
            5 => "Bare rock",
            6 => "Sand",
            _ => "Scorched ground",
        };
    }
    match b.kind {
        BodyKind::Earthlike => {
            let h = g.elev - b.surface.sea_level;
            if g.water {
                "Ocean"
            } else if h > 0.65 {
                "Snowy peaks"
            } else if h > 0.35 {
                "Mountains"
            } else if h < 0.04 {
                "Beach"
            } else {
                "Land"
            }
        }
        BodyKind::Rocky => "Grey dust and craters",
        BodyKind::Desert => "Red desert",
        BodyKind::Cloudy => "Rocky plains under thick clouds",
        BodyKind::Lava => "Cracked volcanic crust",
        BodyKind::Ice => "Ice sheet",
        BodyKind::Asteroid => "Rubble and boulders",
        _ => "No solid ground",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paint::Brush;
    use crate::util::Rng;

    fn body(kind: BodyKind) -> Body {
        Body::new(1, "Test", kind, DVec3::ZERO, DVec3::ZERO, &mut Rng::new(7))
    }

    #[test]
    fn noise_stays_in_range() {
        let rng = Rng::new(3);
        for _ in 0..2000 {
            let p = rng.unit().as_vec3() * 5.0;
            let n = noise3(p);
            assert!((0.0..=1.0).contains(&n), "noise {n}");
            assert!((0.0..=1.0).contains(&fbm(p, 5)));
            assert!((0.0..=1.0).contains(&ridged(p, 4)));
        }
    }

    #[test]
    fn terrain_stays_within_bound() {
        let paint = PaintStore::new();
        let rng = Rng::new(11);
        for kind in BodyKind::ALL {
            let b = body(kind);
            let bound = max_displacement(&b);
            for _ in 0..500 {
                let d = ground(&b, &paint, rng.unit().as_vec3()).disp as f64;
                assert!(d.abs() <= bound + 1e-6, "{kind:?}: {d} > {bound}");
            }
        }
    }

    #[test]
    fn seas_are_flat_and_painting_raises_ground() {
        let mut paint = PaintStore::new();
        let mut b = body(BodyKind::Earthlike);
        let rng = Rng::new(5);
        let sea = b.surface.sea_level * b.surface.terrain;
        let wet = (0..500).map(|_| ground(&b, &paint, rng.unit().as_vec3())).filter(|g| g.water).collect::<Vec<_>>();
        assert!(!wet.is_empty(), "an Earth-like world should have oceans");
        assert!(wet.iter().all(|g| (g.disp - sea).abs() < 1e-6));

        let spot = Vec3::new(0.3, 0.5, -0.4).normalize();
        let before = surface_radius(&b, &paint, spot);
        let layer = paint.alloc().unwrap();
        b.paint_layer = Some(layer);
        for _ in 0..6 {
            paint.apply(layer, spot, 0.3, Brush::Raise, 1.0);
        }
        for _ in 0..3 {
            paint.apply(layer, spot, 0.3, Brush::Sand, 1.0);
        }
        let g = ground(&b, &paint, spot);
        assert!(surface_radius(&b, &paint, spot) > before, "raised land should be higher");
        assert_eq!(ground_name(&b, &g), "Sand");
    }

    #[test]
    fn stars_and_giants_have_no_ground() {
        assert!(!has_solid_surface(BodyKind::Star));
        assert!(!has_solid_surface(BodyKind::BlackHole));
        assert!(!has_solid_surface(BodyKind::GasGiant));
        assert!(has_solid_surface(BodyKind::Earthlike));
        assert!(has_solid_surface(BodyKind::Asteroid));
    }
}
