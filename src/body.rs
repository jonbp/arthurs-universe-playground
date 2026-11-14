use std::collections::VecDeque;

use glam::{DVec3, Quat, Vec3};

use crate::util::{blackbody, Rng};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BodyKind {
    Star = 0,
    Rocky = 1,
    Earthlike = 2,
    Desert = 3,
    Cloudy = 4,
    GasGiant = 5,
    IceGiant = 6,
    Lava = 7,
    Ice = 8,
    Asteroid = 9,
    NeutronStar = 10,
    BlackHole = 11,
}

impl BodyKind {
    pub const ALL: [BodyKind; 12] = [
        BodyKind::Star,
        BodyKind::Rocky,
        BodyKind::Earthlike,
        BodyKind::Desert,
        BodyKind::Cloudy,
        BodyKind::GasGiant,
        BodyKind::IceGiant,
        BodyKind::Lava,
        BodyKind::Ice,
        BodyKind::Asteroid,
        BodyKind::NeutronStar,
        BodyKind::BlackHole,
    ];

    pub fn label(self) -> &'static str {
        match self {
            BodyKind::Star => "Star",
            BodyKind::Rocky => "Rocky / Moon",
            BodyKind::Earthlike => "Earth-like",
            BodyKind::Desert => "Desert (Mars)",
            BodyKind::Cloudy => "Cloudy (Venus)",
            BodyKind::GasGiant => "Gas giant",
            BodyKind::IceGiant => "Ice giant",
            BodyKind::Lava => "Lava world",
            BodyKind::Ice => "Ice world",
            BodyKind::Asteroid => "Asteroid",
            BodyKind::NeutronStar => "Neutron star",
            BodyKind::BlackHole => "Black hole",
        }
    }

    pub fn is_luminous(self) -> bool {
        matches!(self, BodyKind::Star | BodyKind::NeutronStar)
    }

    pub fn is_stellar(self) -> bool {
        matches!(self, BodyKind::Star | BodyKind::NeutronStar | BodyKind::BlackHole)
    }

    /// Can the surface be painted / terraformed?
    pub fn paintable(self) -> bool {
        !self.is_stellar()
    }

    /// Default visual + physical parameters for a freshly spawned body of this kind.
    pub fn defaults(self) -> (f64, f64) {
        // (mass, radius)
        match self {
            BodyKind::Star => (10000.0, 10.0),
            BodyKind::Rocky => (3.0, 1.0),
            BodyKind::Earthlike => (40.0, 2.0),
            BodyKind::Desert => (5.0, 1.4),
            BodyKind::Cloudy => (30.0, 1.9),
            BodyKind::GasGiant => (250.0, 6.0),
            BodyKind::IceGiant => (40.0, 3.5),
            BodyKind::Lava => (20.0, 1.6),
            BodyKind::Ice => (6.0, 1.2),
            BodyKind::Asteroid => (0.5, 0.5),
            BodyKind::NeutronStar => (4000.0, 0.7),
            BodyKind::BlackHole => (20000.0, 6.0),
        }
    }
}

/// Event-horizon radius as a function of mass (game units).
pub fn black_hole_radius(mass: f64) -> f64 {
    // Linear (like a Schwarzschild radius) up to stellar-ish masses, then
    // sub-linear so very heavy holes don't swallow whole planetary systems visually.
    if mass <= 30_000.0 {
        (mass * 3.0e-4).max(0.6)
    } else {
        9.0 * (mass / 30_000.0).sqrt()
    }
}

#[derive(Clone, Debug)]
pub struct Surface {
    pub colors: [Vec3; 3],
    pub atmosphere: Vec3,
    pub atmo_strength: f32,
    /// Relative terrain displacement amplitude.
    pub terrain: f32,
    /// Sea level for earthlike planets (in elevation units, about -1..1).
    pub sea_level: f32,
    pub clouds: f32,
    pub seed: f32,
}

impl Surface {
    pub fn for_kind(kind: BodyKind, rng: &mut Rng) -> Surface {
        let seed = rng.f32() * 10.0;
        let v = |rng: &mut Rng, c: Vec3, amt: f32| {
            (c * (1.0 + Vec3::new(rng.rangef(-amt, amt), rng.rangef(-amt, amt), rng.rangef(-amt, amt))))
                .clamp(Vec3::ZERO, Vec3::ONE)
        };
        let mut s = Surface {
            colors: [Vec3::splat(0.5); 3],
            atmosphere: Vec3::ZERO,
            atmo_strength: 0.0,
            terrain: 0.0,
            sea_level: 0.0,
            clouds: 0.0,
            seed,
        };
        match kind {
            BodyKind::Star => {
                s.colors = [Vec3::new(1.0, 0.85, 0.6), Vec3::new(0.6, 0.25, 0.05), Vec3::ONE];
            }
            BodyKind::Rocky => {
                s.colors = [v(rng, Vec3::new(0.45, 0.43, 0.41), 0.15), v(rng, Vec3::new(0.25, 0.24, 0.23), 0.15), Vec3::ONE];
                s.terrain = 0.035;
            }
            BodyKind::Earthlike => {
                s.colors = [
                    v(rng, Vec3::new(0.12, 0.3, 0.08), 0.25),
                    v(rng, Vec3::new(0.55, 0.45, 0.28), 0.2),
                    v(rng, Vec3::new(0.02, 0.08, 0.25), 0.2),
                ];
                s.atmosphere = Vec3::new(0.3, 0.55, 1.0);
                s.atmo_strength = 0.9;
                s.terrain = 0.02;
                s.sea_level = 0.05;
                s.clouds = 0.85;
            }
            BodyKind::Desert => {
                s.colors = [
                    v(rng, Vec3::new(0.7, 0.32, 0.14), 0.15),
                    v(rng, Vec3::new(0.35, 0.16, 0.08), 0.15),
                    Vec3::new(0.95, 0.95, 0.98),
                ];
                s.atmosphere = Vec3::new(0.9, 0.55, 0.35);
                s.atmo_strength = 0.25;
                s.terrain = 0.025;
            }
            BodyKind::Cloudy => {
                s.colors = [v(rng, Vec3::new(0.9, 0.78, 0.5), 0.1), v(rng, Vec3::new(0.75, 0.6, 0.35), 0.1), Vec3::ONE];
                s.atmosphere = Vec3::new(1.0, 0.85, 0.55);
                s.atmo_strength = 0.6;
            }
            BodyKind::GasGiant => {
                s.colors = [
                    v(rng, Vec3::new(0.85, 0.72, 0.55), 0.15),
                    v(rng, Vec3::new(0.6, 0.38, 0.22), 0.2),
                    v(rng, Vec3::new(0.95, 0.92, 0.85), 0.05),
                ];
                s.atmosphere = Vec3::new(0.9, 0.8, 0.6);
                s.atmo_strength = 0.3;
            }
            BodyKind::IceGiant => {
                s.colors = [v(rng, Vec3::new(0.45, 0.75, 0.85), 0.15), v(rng, Vec3::new(0.3, 0.55, 0.8), 0.15), Vec3::ONE];
                s.atmosphere = Vec3::new(0.5, 0.8, 1.0);
                s.atmo_strength = 0.5;
            }
            BodyKind::Lava => {
                s.colors = [v(rng, Vec3::new(0.12, 0.08, 0.07), 0.2), Vec3::new(0.3, 0.1, 0.05), Vec3::ONE];
                s.atmosphere = Vec3::new(1.0, 0.4, 0.15);
                s.atmo_strength = 0.4;
                s.terrain = 0.02;
            }
            BodyKind::Ice => {
                s.colors = [v(rng, Vec3::new(0.85, 0.9, 0.95), 0.05), v(rng, Vec3::new(0.55, 0.35, 0.2), 0.2), Vec3::ONE];
                s.terrain = 0.015;
            }
            BodyKind::Asteroid => {
                s.colors = [v(rng, Vec3::new(0.38, 0.34, 0.3), 0.2), v(rng, Vec3::new(0.22, 0.2, 0.18), 0.2), Vec3::ONE];
                s.terrain = 0.12;
            }
            BodyKind::NeutronStar => {
                s.colors = [Vec3::new(0.7, 0.85, 1.0), Vec3::ONE, Vec3::ONE];
            }
            BodyKind::BlackHole => {
                s.colors = [Vec3::ZERO; 3];
            }
        }
        s
    }
}

#[derive(Clone, Debug)]
pub struct Ring {
    pub inner: f32,
    pub outer: f32,
    pub color: Vec3,
    pub opacity: f32,
}

#[derive(Clone)]
pub struct Body {
    pub id: u64,
    pub name: String,
    pub kind: BodyKind,
    pub pos: DVec3,
    pub vel: DVec3,
    pub acc: DVec3,
    pub mass: f64,
    pub radius: f64,
    pub surface: Surface,
    /// Stellar surface temperature (K); used for star color & brightness.
    pub temperature: f32,
    pub spin_rate: f64,
    pub spin: f64,
    pub tilt: f32,
    pub ring: Option<Ring>,
    pub paint_layer: Option<u32>,
    pub trail: VecDeque<DVec3>,
    pub trail_accum: f64,
    /// Seconds of collision immunity (fresh fragments).
    pub ghost: f64,
    pub flash: f32,
    pub heat: f32,
    /// Countdown until this star explodes (sim seconds).
    pub supernova_timer: Option<f64>,
    /// Accretion disk brightness for black holes.
    pub feeding: f32,
    pub alive: bool,
}

impl Body {
    pub fn new(id: u64, name: impl Into<String>, kind: BodyKind, pos: DVec3, vel: DVec3, rng: &mut Rng) -> Body {
        let (mass, mut radius) = kind.defaults();
        if kind == BodyKind::BlackHole {
            radius = black_hole_radius(mass);
        }
        Body {
            id,
            name: name.into(),
            kind,
            pos,
            vel,
            acc: DVec3::ZERO,
            mass,
            radius,
            surface: Surface::for_kind(kind, rng),
            temperature: 5800.0,
            spin_rate: rng.range(0.15, 0.6),
            spin: rng.range(0.0, std::f64::consts::TAU),
            tilt: rng.rangef(-0.4, 0.4),
            ring: None,
            paint_layer: None,
            trail: VecDeque::new(),
            trail_accum: 0.0,
            ghost: 0.0,
            flash: 0.0,
            heat: 0.0,
            supernova_timer: None,
            feeding: if kind == BodyKind::BlackHole { 0.5 } else { 0.0 },
            alive: true,
        }
    }

    pub fn with_mass_radius(mut self, mass: f64, radius: f64) -> Body {
        self.mass = mass;
        self.radius = radius;
        self
    }


    pub fn with_temperature(mut self, t: f32) -> Body {
        self.temperature = t;
        self.update_star_colors();
        self
    }



    pub fn update_star_colors(&mut self) {
        if self.kind == BodyKind::Star {
            let c = blackbody(self.temperature);
            let c = c / c.max_element().max(1e-3);
            self.surface.colors[0] = c;
            self.surface.colors[1] = c * Vec3::new(0.55, 0.35, 0.25);
        }
    }

    /// Orientation of the body's surface (local -> world).
    pub fn rotation(&self) -> Quat {
        Quat::from_rotation_z(self.tilt) * Quat::from_rotation_y(self.spin as f32)
    }

    /// Spin axis in world space.
    pub fn axis(&self) -> Vec3 {
        Quat::from_rotation_z(self.tilt) * Vec3::Y
    }

    /// Light output (relative to the Sun ~ 1.0)
    pub fn luminosity(&self) -> f32 {
        match self.kind {
            BodyKind::Star => {
                let t = self.temperature / 5800.0;
                let r = (self.radius / 12.0) as f32;
                // Heavier stars light up bigger systems (the Solar System's Sun is heavy).
                let mass_boost = (self.mass / 10_000.0).sqrt().clamp(1.0, 8.0) as f32;
                (r * r * t * t * t * t).clamp(0.05, 60.0).powf(0.5) * mass_boost * (1.0 + self.flash * 6.0)
            }
            BodyKind::NeutronStar => 0.6 + self.flash * 6.0,
            _ => 0.0,
        }
    }

    /// Surface brightness multiplier used by the shader for emissive bodies.
    pub fn emissive(&self) -> f32 {
        match self.kind {
            BodyKind::Star => 3.0 + (self.temperature / 5800.0).powf(1.5) * 2.0 + self.flash * 20.0,
            BodyKind::NeutronStar => 25.0 + self.flash * 40.0,
            _ => 0.0,
        }
    }

    pub fn light_color(&self) -> Vec3 {
        match self.kind {
            BodyKind::Star => {
                let c = blackbody(self.temperature);
                c / c.max_element().max(1e-3)
            }
            BodyKind::NeutronStar => Vec3::new(0.75, 0.85, 1.0),
            _ => Vec3::ZERO,
        }
    }

    /// Average display color, used for debris particles & trails.
    pub fn debris_color(&self) -> Vec3 {
        match self.kind {
            BodyKind::Star | BodyKind::NeutronStar => self.surface.colors[0] * 2.0,
            BodyKind::BlackHole => Vec3::new(1.0, 0.5, 0.2),
            BodyKind::Earthlike => self.surface.colors[0] * 0.5 + self.surface.colors[2] * 0.5 + Vec3::splat(0.1),
            _ => (self.surface.colors[0] + self.surface.colors[1]) * 0.5,
        }
    }

    pub fn trail_color(&self) -> Vec3 {
        match self.kind {
            BodyKind::Star => self.surface.colors[0],
            BodyKind::BlackHole => Vec3::new(0.7, 0.4, 1.0),
            BodyKind::Earthlike => Vec3::new(0.35, 0.65, 1.0),
            _ => (self.surface.colors[0] * 0.6 + Vec3::splat(0.3)).clamp(Vec3::ZERO, Vec3::ONE),
        }
    }
}
