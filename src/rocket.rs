//! The hangar: the rockets you can fly, and their 3D models.
//!
//! Models are built from a few lathe sections, fins and cones every frame
//! (a few hundred triangles) and drawn by the `solid` pipeline.

use glam::{Quat, Vec3};

use crate::gpu_types::LineVertex;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Style {
    Classic,
    Lander,
    Heavy,
    Shuttle,
    Ion,
    Arrow,
}

pub struct RocketSpec {
    pub name: &'static str,
    pub tagline: &'static str,
    /// Engine acceleration at full throttle (simulation units / s²). Earth's gravity is 10.
    pub accel: f64,
    /// Simulated seconds of full-throttle burn in the tank (infinite for the ion drive).
    pub burn_time: f64,
    /// Steering rate in radians per (real) second.
    pub turn_rate: f64,
    /// Fastest touchdown the landing gear survives.
    pub max_landing_speed: f64,
    /// Length in world units.
    pub length: f64,
    pub style: Style,
    pub body: Vec3,
    pub accent: Vec3,
    pub flame: Vec3,
}

pub const ROCKETS: [RocketSpec; 6] = [
    RocketSpec {
        name: "Sparrow",
        tagline: "Small, quick and easy to steer. A great first rocket.",
        accel: 30.0,
        burn_time: 14.0,
        turn_rate: 2.4,
        max_landing_speed: 5.0,
        length: 0.05,
        style: Style::Classic,
        body: Vec3::new(0.92, 0.92, 0.95),
        accent: Vec3::new(0.9, 0.16, 0.12),
        flame: Vec3::new(1.0, 0.6, 0.2),
    },
    RocketSpec {
        name: "Moonhopper",
        tagline: "A sturdy lander on four legs. Forgives bumpy landings.",
        accel: 22.0,
        burn_time: 24.0,
        turn_rate: 1.8,
        max_landing_speed: 9.0,
        length: 0.045,
        style: Style::Lander,
        body: Vec3::new(0.85, 0.7, 0.3),
        accent: Vec3::new(0.75, 0.76, 0.8),
        flame: Vec3::new(0.7, 0.8, 1.0),
    },
    RocketSpec {
        name: "Goliath",
        tagline: "A giant with booster rockets. Lots of power, slow to turn.",
        accel: 48.0,
        burn_time: 30.0,
        turn_rate: 1.0,
        max_landing_speed: 4.0,
        length: 0.085,
        style: Style::Heavy,
        body: Vec3::new(0.95, 0.95, 0.95),
        accent: Vec3::new(0.95, 0.5, 0.1),
        flame: Vec3::new(1.0, 0.7, 0.3),
    },
    RocketSpec {
        name: "Star Shuttle",
        tagline: "A winged spaceplane that glides in for gentle landings.",
        accel: 34.0,
        burn_time: 20.0,
        turn_rate: 2.0,
        max_landing_speed: 6.0,
        length: 0.065,
        style: Style::Shuttle,
        body: Vec3::new(0.95, 0.95, 0.97),
        accent: Vec3::new(0.08, 0.09, 0.12),
        flame: Vec3::new(0.8, 0.85, 1.0),
    },
    RocketSpec {
        name: "Ion Glider",
        tagline: "Never runs out of fuel, but too gentle to lift off big planets.",
        accel: 7.0,
        burn_time: f64::INFINITY,
        turn_rate: 1.6,
        max_landing_speed: 5.0,
        length: 0.06,
        style: Style::Ion,
        body: Vec3::new(0.75, 0.8, 0.9),
        accent: Vec3::new(0.15, 0.3, 0.75),
        flame: Vec3::new(0.3, 0.75, 1.0),
    },
    RocketSpec {
        name: "Warp Arrow",
        tagline: "Outrageously fast. Cross the whole Solar System in a blink.",
        accel: 420.0,
        burn_time: 12.0,
        turn_rate: 2.8,
        max_landing_speed: 6.0,
        length: 0.07,
        style: Style::Arrow,
        body: Vec3::new(0.16, 0.17, 0.22),
        accent: Vec3::new(0.6, 0.25, 0.95),
        flame: Vec3::new(0.75, 0.4, 1.0),
    },
];

/// Builds a model in rocket space (nose +Y, top +Z, length 1 centred on the origin)
/// and writes camera-relative triangles.
struct Builder<'a> {
    out: &'a mut Vec<LineVertex>,
    origin: Vec3,
    rot: Quat,
    scale: f32,
}

const SEGS: usize = 18;

impl Builder<'_> {
    fn tri(&mut self, a: Vec3, b: Vec3, c: Vec3, color: Vec3, glow: f32) {
        for p in [a, b, c] {
            let w = self.origin + self.rot * (p * self.scale);
            self.out.push(LineVertex { pos: [w.x, w.y, w.z, 1.0], color: [color.x, color.y, color.z, glow] });
        }
    }

    fn quad(&mut self, a: Vec3, b: Vec3, c: Vec3, d: Vec3, color: Vec3, glow: f32) {
        self.tri(a, b, c, color, glow);
        self.tri(a, c, d, color, glow);
    }

    /// Lathe section around the axis through `c` (parallel to Y), from (y0, r0) to (y1, r1).
    /// A zero radius at either end makes a cone (or, with y0 == y1, a disc).
    fn section(&mut self, c: Vec3, y0: f32, r0: f32, y1: f32, r1: f32, color: Vec3, glow: f32) {
        let ring = |y: f32, r: f32, k: usize| {
            let a = k as f32 / SEGS as f32 * std::f32::consts::TAU;
            c + Vec3::new(a.cos() * r, y, a.sin() * r)
        };
        for k in 0..SEGS {
            let (a, b, c2, d) = (ring(y0, r0, k), ring(y0, r0, k + 1), ring(y1, r1, k + 1), ring(y1, r1, k));
            if r1 <= 1e-6 {
                self.tri(a, b, d, color, glow);
            } else if r0 <= 1e-6 {
                self.tri(a, c2, d, color, glow);
            } else {
                self.quad(a, b, c2, d, color, glow);
            }
        }
    }

    fn disc(&mut self, c: Vec3, y: f32, r: f32, color: Vec3, glow: f32) {
        self.section(c, y, 0.0, y, r, color, glow);
    }

    /// A thin plate through four corners (drawn double-sided by the pipeline).
    fn plate(&mut self, pts: [Vec3; 4], color: Vec3) {
        self.quad(pts[0], pts[1], pts[2], pts[3], color, 0.0);
    }

    /// `n` plates fanned around the Y axis; `shape` is in the (radial, y) plane.
    fn fins(&mut self, n: usize, phase: f32, shape: [(f32, f32); 4], color: Vec3) {
        for k in 0..n {
            let a = phase + k as f32 / n as f32 * std::f32::consts::TAU;
            let dir = Vec3::new(a.cos(), 0.0, a.sin());
            self.plate(shape.map(|(r, y)| dir * r + Vec3::Y * y), color);
        }
    }

    /// A strut between two points (a thin three-sided prism).
    fn strut(&mut self, a: Vec3, b: Vec3, r: f32, color: Vec3) {
        let axis = (b - a).normalize_or_zero();
        let side = axis.cross(if axis.y.abs() < 0.9 { Vec3::Y } else { Vec3::X }).normalize_or_zero();
        let other = axis.cross(side);
        let ring = |p: Vec3, k: usize| {
            let t = k as f32 / 3.0 * std::f32::consts::TAU;
            p + (side * t.cos() + other * t.sin()) * r
        };
        for k in 0..3 {
            self.quad(ring(a, k), ring(a, k + 1), ring(b, k + 1), ring(b, k), color, 0.0);
        }
    }

    /// Exhaust plume below (y, r): a bright inner cone inside a dimmer outer one.
    fn flame(&mut self, c: Vec3, y: f32, r: f32, len: f32, color: Vec3) {
        if len <= 0.001 {
            return;
        }
        self.section(c, y - len, 0.0, y, r, color * 2.5, 1.0);
        self.section(c, y - len * 0.55, 0.0, y, r * 0.55, Vec3::new(1.0, 0.95, 0.85) * 5.0, 1.0);
    }
}

/// Append the rocket's triangles. `origin` is the camera-relative position of its
/// centre, `rot` takes rocket space to world space, `throttle` 0..1 sizes the flame.
pub fn build_mesh(spec: &RocketSpec, out: &mut Vec<LineVertex>, origin: Vec3, rot: Quat, throttle: f32, time: f32) {
    let mut b = Builder { out, origin, rot, scale: spec.length as f32 };
    let (body, accent) = (spec.body, spec.accent);
    let dark = Vec3::splat(0.07);
    let window = Vec3::new(0.25, 0.55, 0.9);
    let o = Vec3::ZERO;
    // Flickering plume, longer with more throttle.
    let flicker = 0.85 + 0.15 * (time * 47.0).sin() * (time * 31.0).cos();
    let plume = throttle.clamp(0.0, 1.0) * flicker;

    match spec.style {
        Style::Classic => {
            b.section(o, -0.38, 0.11, 0.18, 0.11, body, 0.0);
            b.section(o, 0.18, 0.11, 0.5, 0.0, accent, 0.0);
            b.section(o, 0.02, 0.112, 0.07, 0.112, accent, 0.0);
            b.section(o, -0.5, 0.075, -0.38, 0.05, dark, 0.0);
            b.disc(o, -0.38, 0.11, dark, 0.0);
            b.fins(3, 0.5, [(0.1, -0.1), (0.3, -0.5), (0.3, -0.38), (0.1, -0.38)], accent);
            b.plate([Vec3::new(-0.035, 0.17, 0.113), Vec3::new(0.035, 0.17, 0.113), Vec3::new(0.035, 0.24, 0.113), Vec3::new(-0.035, 0.24, 0.113)], window);
            b.flame(o, -0.5, 0.07, plume * 0.7, spec.flame);
        }
        Style::Lander => {
            // Squat octagonal cabin on a descent stage with four splayed legs.
            b.section(o, -0.05, 0.3, 0.2, 0.3, accent, 0.0);
            b.section(o, 0.2, 0.3, 0.42, 0.14, accent, 0.0);
            b.disc(o, 0.42, 0.14, dark, 0.0);
            b.section(o, -0.28, 0.34, -0.05, 0.34, body, 0.0);
            b.disc(o, -0.28, 0.34, dark, 0.0);
            b.disc(o, -0.05, 0.34, body, 0.0);
            b.section(o, -0.42, 0.14, -0.28, 0.07, dark, 0.0);
            for k in 0..4 {
                let a = (k as f32 + 0.5) * std::f32::consts::FRAC_PI_2;
                let dir = Vec3::new(a.cos(), 0.0, a.sin());
                let foot = dir * 0.52 + Vec3::Y * -0.47;
                b.strut(dir * 0.32 + Vec3::Y * -0.1, foot, 0.022, Vec3::splat(0.6));
                b.strut(dir * 0.3 + Vec3::Y * -0.27, foot, 0.016, Vec3::splat(0.6));
                b.disc(foot, -0.03, 0.07, Vec3::splat(0.5), 0.0);
            }
            b.strut(Vec3::new(0.0, 0.42, 0.0), Vec3::new(0.12, 0.485, 0.0), 0.012, Vec3::splat(0.8));
            b.flame(o, -0.42, 0.12, plume * 0.5, spec.flame);
        }
        Style::Heavy => {
            b.section(o, -0.42, 0.085, 0.26, 0.085, body, 0.0);
            b.section(o, 0.26, 0.085, 0.5, 0.0, body, 0.0);
            b.section(o, 0.1, 0.087, 0.16, 0.087, accent, 0.0);
            b.section(o, -0.2, 0.087, -0.15, 0.087, dark, 0.0);
            b.section(o, -0.5, 0.06, -0.42, 0.04, dark, 0.0);
            b.disc(o, -0.42, 0.085, dark, 0.0);
            for k in 0..2 {
                let c = Vec3::new(if k == 0 { 0.135 } else { -0.135 }, 0.0, 0.0);
                b.section(c, -0.42, 0.05, 0.08, 0.05, accent, 0.0);
                b.section(c, 0.08, 0.05, 0.22, 0.0, body, 0.0);
                b.section(c, -0.5, 0.04, -0.42, 0.028, dark, 0.0);
                b.disc(c, -0.42, 0.05, dark, 0.0);
                b.flame(c, -0.5, 0.036, plume * 0.55, spec.flame);
            }
            b.fins(2, std::f32::consts::FRAC_PI_2, [(0.08, -0.2), (0.2, -0.48), (0.2, -0.4), (0.08, -0.4)], accent);
            b.flame(o, -0.5, 0.055, plume * 0.75, spec.flame);
        }
        Style::Shuttle => {
            b.section(o, -0.4, 0.1, 0.2, 0.1, body, 0.0);
            b.section(o, 0.2, 0.1, 0.42, 0.05, body, 0.0);
            b.section(o, 0.42, 0.05, 0.5, 0.0, accent, 0.0);
            b.disc(o, -0.4, 0.1, dark, 0.0);
            b.section(o, -0.5, 0.065, -0.4, 0.045, dark, 0.0);
            // Delta wings in the X/Y plane and a tail fin along +Z.
            for s in [1.0f32, -1.0] {
                b.plate(
                    [Vec3::new(0.09 * s, 0.12, -0.03), Vec3::new(0.46 * s, -0.42, -0.03), Vec3::new(0.46 * s, -0.5, -0.03), Vec3::new(0.09 * s, -0.42, -0.03)],
                    accent,
                );
            }
            b.plate([Vec3::new(0.0, -0.1, 0.09), Vec3::new(0.0, -0.5, 0.34), Vec3::new(0.0, -0.42, 0.34), Vec3::new(0.0, -0.42, 0.09)], accent);
            b.plate([Vec3::new(-0.05, 0.3, 0.085), Vec3::new(0.05, 0.3, 0.085), Vec3::new(0.04, 0.38, 0.065), Vec3::new(-0.04, 0.38, 0.065)], window);
            b.flame(o, -0.5, 0.06, plume * 0.65, spec.flame);
        }
        Style::Ion => {
            b.section(o, -0.3, 0.07, 0.3, 0.07, body, 0.0);
            b.section(o, 0.3, 0.07, 0.5, 0.0, body, 0.0);
            b.disc(o, -0.3, 0.07, dark, 0.0);
            b.section(o, -0.47, 0.11, -0.3, 0.05, accent, 0.0);
            b.section(o, -0.5, 0.09, -0.47, 0.11, spec.flame * (0.5 + plume * 3.0), 1.0);
            // Two long solar panels.
            for s in [1.0f32, -1.0] {
                b.strut(Vec3::new(0.06 * s, 0.0, 0.0), Vec3::new(0.14 * s, 0.0, 0.0), 0.012, Vec3::splat(0.7));
                b.plate(
                    [Vec3::new(0.14 * s, 0.14, 0.0), Vec3::new(0.5 * s, 0.14, 0.0), Vec3::new(0.5 * s, -0.14, 0.0), Vec3::new(0.14 * s, -0.14, 0.0)],
                    accent,
                );
            }
            b.flame(o, -0.5, 0.08, plume * 0.9, spec.flame);
        }
        Style::Arrow => {
            // A long needle with swept blades and a glowing drive ring.
            b.section(o, -0.36, 0.07, 0.1, 0.06, body, 0.0);
            b.section(o, 0.1, 0.06, 0.5, 0.0, body, 0.0);
            b.disc(o, -0.36, 0.07, dark, 0.0);
            b.section(o, -0.3, 0.13, -0.22, 0.13, accent * (0.6 + plume * 2.0), 0.6);
            b.fins(4, 0.0, [(0.06, 0.0), (0.13, -0.3), (0.13, -0.22), (0.06, -0.22)], body);
            b.fins(4, std::f32::consts::FRAC_PI_4, [(0.05, 0.22), (0.26, -0.5), (0.2, -0.36), (0.06, -0.3)], accent);
            b.section(o, -0.44, 0.05, -0.36, 0.035, dark, 0.0);
            b.flame(o, -0.44, 0.045, plume * 1.3, spec.flame);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_rocket_builds_a_model_that_fits_its_length() {
        for spec in &ROCKETS {
            let mut out = Vec::new();
            build_mesh(spec, &mut out, Vec3::ZERO, Quat::IDENTITY, 0.0, 0.0);
            assert!(out.len() >= 100 && out.len() % 3 == 0, "{}: {} verts", spec.name, out.len());
            let half = spec.length as f32 * 0.5 + 1e-5;
            for v in &out {
                assert!(v.pos[1].abs() <= half, "{} pokes past its length: y={}", spec.name, v.pos[1]);
                assert!(v.pos.iter().all(|c| c.is_finite()));
            }
            // The model reaches (close to) both ends, so it sits on the ground properly.
            let lo = out.iter().map(|v| v.pos[1]).fold(f32::MAX, f32::min);
            assert!(lo < -half * 0.95, "{}: lowest point {lo}", spec.name);
            let before = out.len();
            build_mesh(spec, &mut out, Vec3::ZERO, Quat::IDENTITY, 1.0, 0.3);
            assert!(out.len() - before > before, "{}: full throttle should add a flame", spec.name);
        }
    }

    #[test]
    fn rockets_differ_in_what_they_can_lift_off_from() {
        // Earth's surface gravity is 10: the ion glider can't lift off, the rest can.
        for spec in &ROCKETS {
            assert_eq!(spec.accel > 10.0, spec.style != Style::Ion, "{}", spec.name);
        }
    }
}
