//! Ready-made universes and the procedural "create your own universe" generator.

use glam::{DQuat, DVec3, Vec3};

use crate::body::{black_hole_radius, BodyKind, Ring};
use crate::units::{self, au, km, AU, DAY, SUN_MASS, SUN_RADIUS};
use crate::universe::{Universe, P_GRAVITY, P_IMMORTAL};
use crate::util::Rng;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Preset {
    SolarSystem,
    RogueBlackHole,
    TheiaImpact,
    HeadOn,
    BinaryStars,
    BlackHoleFeast,
    Supernova,
    GalaxyCollision,
    Protoplanetary,
    Empty,
}

impl Preset {
    pub const ALL: [Preset; 10] = [
        Preset::SolarSystem,
        Preset::RogueBlackHole,
        Preset::TheiaImpact,
        Preset::HeadOn,
        Preset::BinaryStars,
        Preset::BlackHoleFeast,
        Preset::Supernova,
        Preset::GalaxyCollision,
        Preset::Protoplanetary,
        Preset::Empty,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Preset::SolarSystem => "The Solar System",
            Preset::RogueBlackHole => "Solar System vs Rogue Black Hole",
            Preset::TheiaImpact => "Theia Impact",
            Preset::HeadOn => "Head-on Planet Smash",
            Preset::BinaryStars => "Binary Star System",
            Preset::BlackHoleFeast => "Black Hole Feast",
            Preset::Supernova => "Supernova Countdown",
            Preset::GalaxyCollision => "Galaxy Collision",
            Preset::Protoplanetary => "Protoplanetary Disk",
            Preset::Empty => "Empty Universe",
        }
    }
    /// Built to scale, in real units (see `units.rs`)? The galaxies and the
    /// protoplanetary disk are models: at their true size and pace there would be
    /// nothing to watch in a lifetime.
    pub fn real_scale(self) -> bool {
        !matches!(self, Preset::GalaxyCollision | Preset::Protoplanetary)
    }
    pub fn camera_distance(self) -> f64 {
        match self {
            Preset::SolarSystem => au(7.0),
            Preset::RogueBlackHole => au(20.0),
            Preset::TheiaImpact => 60.0,
            Preset::HeadOn => 70.0,
            Preset::BinaryStars => au(9.0),
            Preset::BlackHoleFeast => 30.0 * GARGANTUA_RADIUS,
            Preset::Supernova => au(3.2),
            Preset::GalaxyCollision => 1300.0,
            Preset::Protoplanetary => 380.0,
            Preset::Empty => 3000.0,
        }
    }
    /// How fast time runs when the preset starts.
    pub fn time_scale(self) -> f64 {
        match self {
            // A week every second: the Earth goes round the Sun in under a minute.
            Preset::SolarSystem | Preset::RogueBlackHole | Preset::BinaryStars => 7.0 * DAY,
            // Things near a giant black hole move at a good part of the speed of light.
            Preset::BlackHoleFeast => 0.2,
            Preset::Supernova => 4.0,
            Preset::GalaxyCollision => 6.0,
            Preset::Protoplanetary => 5.0,
            Preset::TheiaImpact | Preset::HeadOn | Preset::Empty => 1.0,
        }
    }
}

/// Place a body in a circular orbit around `parent` at `dist`, angle `ang` and inclination `incl`.
fn orbit_around(u: &Universe, parent: u64, dist: f64, ang: f64, incl: f64, retrograde: bool) -> (DVec3, DVec3) {
    let p = u.get(parent).expect("parent");
    let rot = DQuat::from_rotation_x(incl);
    let r = rot * DVec3::new(ang.cos(), 0.0, ang.sin()) * dist;
    let speed = (u.g * p.mass / dist).sqrt();
    let mut dir = (rot * DVec3::Y).cross(r).normalize();
    if retrograde {
        dir = -dir;
    }
    (p.pos + r, p.vel + dir * speed)
}

fn add_orbiting(
    u: &mut Universe,
    parent: u64,
    name: &str,
    kind: BodyKind,
    mass: f64,
    radius: f64,
    dist: f64,
    ang: f64,
    incl: f64,
) -> u64 {
    let (pos, vel) = orbit_around(u, parent, dist, ang, incl, false);
    let b = u.new_body(name, kind, pos, vel).with_mass_radius(mass, radius);
    u.add(b)
}

/// The shape and tilt of a real orbit, as astronomers list it. Angles in degrees.
#[derive(Clone, Copy)]
struct Orbit {
    /// Half the long axis of the ellipse (game units).
    a: f64,
    /// Eccentricity: 0 is a circle.
    e: f64,
    /// Tilt of the orbit against the reference plane.
    incl: f64,
    /// Longitude of the ascending node: where the orbit crosses the plane going up.
    node: f64,
    /// Longitude of the closest point (node + argument of periapsis).
    peri: f64,
    /// Mean longitude: where along the orbit the body is.
    longitude: f64,
}

impl Orbit {
    /// Position and velocity on the orbit, relative to what it goes round, where `mu`
    /// is G times the two masses together. The reference plane is the world's XZ
    /// plane with +Y as north, and things go round anticlockwise seen from the north,
    /// like everything (nearly) in the Solar System.
    fn state(&self, mu: f64) -> (DVec3, DVec3) {
        let (node, incl) = (self.node.to_radians(), self.incl.to_radians());
        let arg = (self.peri - self.node).to_radians();
        let mean = (self.longitude - self.peri).to_radians();
        // Kepler's equation, by Newton's method.
        let mut ecc = mean;
        for _ in 0..30 {
            ecc -= (ecc - self.e * ecc.sin() - mean) / (1.0 - self.e * ecc.cos());
        }
        let (se, ce) = ecc.sin_cos();
        let b = (1.0 - self.e * self.e).sqrt();
        let rate = (mu / self.a.powi(3)).sqrt();
        // In the plane of the orbit, x towards the closest point.
        let (x, y) = (self.a * (ce - self.e), self.a * b * se);
        let k = self.a * rate / (1.0 - self.e * ce);
        let (vx, vy) = (-k * se, k * b * ce);
        let (sn, cn) = node.sin_cos();
        let (sa, ca) = arg.sin_cos();
        let (si, ci) = incl.sin_cos();
        // Columns of the usual rotation (node, then tilt, then argument), in
        // ecliptic-style axes: x, y in the plane and z north.
        let px = DVec3::new(cn * ca - sn * sa * ci, sn * ca + cn * sa * ci, sa * si);
        let py = DVec3::new(-cn * sa - sn * ca * ci, -sn * sa + cn * ca * ci, ca * si);
        let world = |v: DVec3| DVec3::new(v.x, v.z, -v.y);
        (world(px * x + py * y), world(px * vx + py * vy))
    }
}

/// Add a body on `orbit` around `parent`. `frame` turns the reference plane: the
/// identity for an orbit listed against the plane of the Solar System, or the
/// parent's own tilt for a moon that goes round its equator. A heavy moon (ours,
/// Charon) and its planet are both set going round the point between them.
#[allow(clippy::too_many_arguments)]
fn add_on_orbit(u: &mut Universe, parent: u64, frame: DQuat, name: &str, kind: BodyKind, mass: f64, radius: f64, orbit: Orbit) -> u64 {
    let p = u.get(parent).expect("parent");
    let (ppos, pvel, pmass) = (p.pos, p.vel, p.mass);
    let (r, v) = orbit.state(u.g * (pmass + mass));
    let (r, v) = (frame * r, frame * v);
    let share = mass / (pmass + mass);
    let share = if share > 1e-3 { share } else { 0.0 };
    if let Some(p) = u.get_mut(parent) {
        p.pos -= r * share;
        p.vel -= v * share;
    }
    let b = u.new_body(name, kind, ppos + r * (1.0 - share), pvel + v * (1.0 - share)).with_mass_radius(mass, radius);
    u.add(b)
}

/// The turn that takes "straight up" to a body's spin axis: the frame of its equator.
fn equator(u: &Universe, id: u64) -> DQuat {
    let axis = u.get(id).expect("body").axis().as_dvec3();
    DQuat::from_rotation_arc(DVec3::Y, axis)
}

fn zero_momentum(u: &mut Universe) {
    let (mut p, mut m) = (DVec3::ZERO, 0.0);
    for b in &u.bodies {
        p += b.vel * b.mass;
        m += b.mass;
    }
    if m <= 0.0 {
        return;
    }
    let v = p / m;
    for b in &mut u.bodies {
        b.vel -= v;
    }
    // Particles (belts, dust) were launched relative to the same frame: shift them too.
    let vf = v.as_vec3();
    for p in &mut u.particles {
        p.vel -= vf;
    }
}

/// A ring of `count` specks in circular orbits between `r0` and `r1`, each `size` across (give or take).
#[allow(clippy::too_many_arguments)]
fn asteroid_belt(u: &mut Universe, center: DVec3, center_vel: DVec3, mass: f64, r0: f64, r1: f64, count: usize, color: Vec3, size: f32) {
    for _ in 0..count {
        let r = u.rng.range(r0, r1);
        let a = u.rng.range(0.0, std::f64::consts::TAU);
        let y = u.rng.normal() * (r1 - r0) * 0.04;
        let pos = center + DVec3::new(r * a.cos(), y, r * a.sin());
        let speed = (u.g * mass / r).sqrt() * u.rng.range(0.98, 1.02);
        let dir = DVec3::Y.cross(pos - center).normalize();
        let tint = color * u.rng.rangef(0.5, 1.2);
        u.emit(pos, center_vel + dir * speed, tint, size * u.rng.rangef(0.5, 1.5), 1.0, P_GRAVITY | P_IMMORTAL);
    }
}

pub fn build(u: &mut Universe, preset: Preset) {
    build_bodies(u, preset);
    // People start out wherever it is just right for them. (Not on the young Earth
    // that Theia is about to hit: nobody lives there yet.)
    if preset != Preset::TheiaImpact {
        crate::population::settle(u);
    }
}

fn build_bodies(u: &mut Universe, preset: Preset) {
    u.reset(preset.label());
    u.real_scale = preset.real_scale();
    u.g = 1.0;
    match preset {
        Preset::SolarSystem => solar_system(u),
        Preset::RogueBlackHole => {
            solar_system(u);
            // Ten times the mass of the Sun, coming in at 60 km/s to pass between the
            // Sun and the Earth. Its pull is felt across the whole inner Solar System.
            let start = DVec3::new(au(10.5), au(0.26), au(6.8));
            let aim = DVec3::new(au(0.58), 0.0, au(-0.81));
            let mut bh = u.new_body("Rogue Black Hole", BodyKind::BlackHole, start, (aim - start).normalize() * (60.0 / units::KM_PER_S));
            bh.mass = 10.0 * SUN_MASS;
            bh.radius = black_hole_radius(bh.mass, true);
            u.add(bh);
            u.event("A rogue black hole is heading for the Solar System...");
        }
        Preset::TheiaImpact => theia(u),
        Preset::HeadOn => head_on(u),
        Preset::BinaryStars => binary(u),
        Preset::BlackHoleFeast => black_hole_feast(u),
        Preset::Supernova => supernova(u),
        Preset::GalaxyCollision => galaxy_collision(u),
        Preset::Protoplanetary => protoplanetary(u),
        Preset::Empty => {
            u.event("An empty universe. Use the Spawn tool to create stars and planets!");
        }
    }
    for b in &mut u.bodies {
        b.trail.clear();
    }
}

/// One line of the table of planets. Sizes and masses are NASA's (the planetary
/// fact sheets and JPL's physical parameters); the orbits are JPL's "approximate
/// positions of the planets" for the year 2000, so every planet starts where it
/// really was on 1 January 2000, on its real, slightly oval and slightly tilted path.
struct Planet {
    name: &'static str,
    kind: BodyKind,
    /// G times the mass, km³/s².
    gm: f64,
    /// Mean radius, km.
    radius: f64,
    /// Semi-major axis in AU, eccentricity, inclination, mean longitude, longitude
    /// of perihelion and longitude of the ascending node (degrees).
    orbit: [f64; 6],
    /// Hours for one turn on its axis, and the tilt of that axis in degrees
    /// (more than 90 means it spins backwards, like Venus).
    day: f64,
    tilt: f64,
}

#[rustfmt::skip]
const PLANETS: [Planet; 11] = [
    Planet { name: "Mercury", kind: BodyKind::Rocky,     gm: 22_031.87,       radius: 2439.4,   orbit: [0.38709927, 0.20563593, 7.00497902, 252.25032350, 77.45779628, 48.33076593],     day: 1407.6,  tilt: 0.034 },
    Planet { name: "Venus",   kind: BodyKind::Cloudy,    gm: 324_858.59,      radius: 6051.8,   orbit: [0.72333566, 0.00677672, 3.39467605, 181.97909950, 131.60246718, 76.67984255],    day: 5832.5,  tilt: 177.36 },
    Planet { name: "Earth",   kind: BodyKind::Earthlike, gm: 398_600.44,    radius: 6371.0,   orbit: [1.00000261, 0.01671123, -0.00001531, 100.46457166, 102.93768193, 0.0],           day: 23.9345, tilt: 23.44 },
    Planet { name: "Mars",    kind: BodyKind::Desert,    gm: 42_828.37,       radius: 3389.5,   orbit: [1.52371034, 0.09339410, 1.84969142, -4.55343205, -23.94362959, 49.55953891],     day: 24.6229, tilt: 25.19 },
    Planet { name: "Vesta",   kind: BodyKind::Asteroid,  gm: 17.29,           radius: 262.7,    orbit: [2.3618, 0.0887, 7.14, 275.9, 255.05, 103.85],                                    day: 5.342,   tilt: 29.0 },
    Planet { name: "Ceres",   kind: BodyKind::Rocky,     gm: 62.63,           radius: 469.7,    orbit: [2.7675, 0.0758, 10.59, 231.3, 153.9, 80.33],                                     day: 9.074,   tilt: 4.0 },
    Planet { name: "Jupiter", kind: BodyKind::GasGiant,  gm: 126_686_531.9,   radius: 69_911.0, orbit: [5.20288700, 0.04838624, 1.30439695, 34.39644051, 14.72847983, 100.47390909],     day: 9.925,   tilt: 3.13 },
    Planet { name: "Saturn",  kind: BodyKind::GasGiant,  gm: 37_931_206.2,    radius: 58_232.0, orbit: [9.53667594, 0.05386179, 2.48599187, 49.95424423, 92.59887831, 113.66242448],     day: 10.656,  tilt: 26.73 },
    Planet { name: "Uranus",  kind: BodyKind::IceGiant,  gm: 5_793_951.3,     radius: 25_362.0, orbit: [19.18916464, 0.04725744, 0.77263783, 313.23810451, 170.95427630, 74.01692503],   day: 17.24,   tilt: 97.77 },
    Planet { name: "Neptune", kind: BodyKind::IceGiant,  gm: 6_835_099.5,     radius: 24_622.0, orbit: [30.06992276, 0.00859048, 1.77004347, -55.12002969, 44.96476227, 131.78422574],   day: 16.11,   tilt: 28.32 },
    Planet { name: "Pluto",   kind: BodyKind::Ice,       gm: 869.6,           radius: 1188.3,   orbit: [39.48211675, 0.24882730, 17.14001206, 238.92903833, 224.06891629, 110.30393684], day: 153.29,  tilt: 122.53 },
];

/// One line of the table of moons (JPL's satellite parameters and mean elements).
struct Moon {
    planet: &'static str,
    name: &'static str,
    kind: BodyKind,
    /// G times the mass, km³/s², and mean radius, km.
    gm: f64,
    radius: f64,
    /// Semi-major axis (km), eccentricity and inclination (degrees) to the planet's
    /// equator; or to the plane of the Solar System for our own Moon.
    a: f64,
    e: f64,
    incl: f64,
}

#[rustfmt::skip]
const MOONS: [Moon; 21] = [
    Moon { planet: "Earth",   name: "Moon",      kind: BodyKind::Rocky,    gm: 4902.800,    radius: 1737.4,  a: 384_400.0,   e: 0.0554, incl: 5.16 },
    Moon { planet: "Mars",    name: "Phobos",    kind: BodyKind::Asteroid, gm: 0.0007087,   radius: 11.08,   a: 9375.0,      e: 0.015,  incl: 1.1 },
    Moon { planet: "Mars",    name: "Deimos",    kind: BodyKind::Asteroid, gm: 0.0000962,   radius: 6.2,     a: 23_457.0,    e: 0.0,    incl: 1.8 },
    Moon { planet: "Jupiter", name: "Io",        kind: BodyKind::Lava,     gm: 5959.91547,  radius: 1821.49, a: 421_800.0,   e: 0.004,  incl: 0.0 },
    Moon { planet: "Jupiter", name: "Europa",    kind: BodyKind::Ice,      gm: 3202.71210,  radius: 1560.80, a: 671_100.0,   e: 0.009,  incl: 0.5 },
    Moon { planet: "Jupiter", name: "Ganymede",  kind: BodyKind::Rocky,    gm: 9887.83275,  radius: 2631.20, a: 1_070_400.0, e: 0.001,  incl: 0.2 },
    Moon { planet: "Jupiter", name: "Callisto",  kind: BodyKind::Rocky,    gm: 7179.28340,  radius: 2410.30, a: 1_882_700.0, e: 0.007,  incl: 0.3 },
    Moon { planet: "Saturn",  name: "Mimas",     kind: BodyKind::Ice,      gm: 2.50349,     radius: 198.20,  a: 186_000.0,   e: 0.020,  incl: 1.6 },
    Moon { planet: "Saturn",  name: "Enceladus", kind: BodyKind::Ice,      gm: 7.21037,     radius: 252.10,  a: 238_400.0,   e: 0.005,  incl: 0.0 },
    Moon { planet: "Saturn",  name: "Tethys",    kind: BodyKind::Ice,      gm: 41.21353,    radius: 531.10,  a: 295_000.0,   e: 0.001,  incl: 1.1 },
    Moon { planet: "Saturn",  name: "Dione",     kind: BodyKind::Ice,      gm: 73.11607,    radius: 561.40,  a: 377_700.0,   e: 0.002,  incl: 0.0 },
    Moon { planet: "Saturn",  name: "Rhea",      kind: BodyKind::Ice,      gm: 153.94175,   radius: 763.50,  a: 527_200.0,   e: 0.001,  incl: 0.3 },
    Moon { planet: "Saturn",  name: "Titan",     kind: BodyKind::Cloudy,   gm: 8978.13710,  radius: 2574.76, a: 1_221_900.0, e: 0.029,  incl: 0.3 },
    Moon { planet: "Saturn",  name: "Iapetus",   kind: BodyKind::Rocky,    gm: 120.51511,   radius: 734.30,  a: 3_561_700.0, e: 0.028,  incl: 15.5 },
    Moon { planet: "Uranus",  name: "Miranda",   kind: BodyKind::Ice,      gm: 4.3,         radius: 235.8,   a: 129_846.0,   e: 0.001,  incl: 4.4 },
    Moon { planet: "Uranus",  name: "Ariel",     kind: BodyKind::Ice,      gm: 83.5,        radius: 578.9,   a: 190_929.0,   e: 0.001,  incl: 0.0 },
    Moon { planet: "Uranus",  name: "Umbriel",   kind: BodyKind::Rocky,    gm: 85.1,        radius: 584.7,   a: 265_986.0,   e: 0.004,  incl: 0.1 },
    Moon { planet: "Uranus",  name: "Titania",   kind: BodyKind::Ice,      gm: 226.9,       radius: 788.9,   a: 436_298.0,   e: 0.002,  incl: 0.1 },
    Moon { planet: "Uranus",  name: "Oberon",    kind: BodyKind::Rocky,    gm: 205.3,       radius: 761.4,   a: 583_511.0,   e: 0.002,  incl: 0.1 },
    // Triton goes round Neptune backwards.
    Moon { planet: "Neptune", name: "Triton",    kind: BodyKind::Ice,      gm: 1428.49546,  radius: 1352.60, a: 354_800.0,   e: 0.0,    incl: 157.3 },
    Moon { planet: "Pluto",   name: "Charon",    kind: BodyKind::Rocky,    gm: 106.1,       radius: 606.0,   a: 19_591.0,    e: 0.0,    incl: 0.0 },
];

/// The Solar System, to scale: every size, mass, distance, orbit, day and tilt is
/// the real one, so every year and month comes out right too. (What is not real:
/// which way each axis leans, and where each moon is along its orbit.)
pub fn solar_system(u: &mut Universe) {
    let mut sun = u.new_body("Sun", BodyKind::Star, DVec3::ZERO, DVec3::ZERO).with_mass_radius(SUN_MASS, SUN_RADIUS).with_temperature(units::SUN_TEMPERATURE);
    sun.spin_rate = units::spin_rate(609.12);
    sun.tilt = 7.25f32.to_radians();
    let sun = u.add(sun);

    for p in &PLANETS {
        let [a, e, incl, longitude, peri, node] = p.orbit;
        let orbit = Orbit { a: au(a), e, incl, node, peri, longitude };
        let id = add_on_orbit(u, sun, DQuat::IDENTITY, p.name, p.kind, units::mass(p.gm), km(p.radius), orbit);
        let b = u.get_mut(id).expect("planet");
        b.spin_rate = units::spin_rate(p.day);
        b.tilt = (p.tilt as f32).to_radians();
        match p.name {
            "Mercury" => b.surface.colors = [Vec3::new(0.5, 0.47, 0.44), Vec3::new(0.32, 0.3, 0.28), Vec3::ONE],
            "Earth" => b.surface.seed = 3.3,
            "Jupiter" => {
                b.surface.colors = [Vec3::new(0.87, 0.75, 0.6), Vec3::new(0.62, 0.38, 0.22), Vec3::new(0.95, 0.92, 0.86)];
                b.surface.seed = 1.0;
            }
            "Saturn" => {
                b.surface.colors = [Vec3::new(0.9, 0.82, 0.6), Vec3::new(0.75, 0.62, 0.4), Vec3::new(0.95, 0.9, 0.75)];
                // From the inner edge of the C ring to the outer edge of the A ring.
                b.ring = Some(Ring { inner: km(74_500.0) as f32, outer: km(136_780.0) as f32, color: Vec3::new(0.85, 0.78, 0.62), opacity: 0.9 });
            }
            "Uranus" => {
                b.surface.colors = [Vec3::new(0.6, 0.85, 0.9), Vec3::new(0.5, 0.78, 0.85), Vec3::ONE];
                b.ring = Some(Ring { inner: km(41_837.0) as f32, outer: km(51_149.0) as f32, color: Vec3::new(0.6, 0.65, 0.7), opacity: 0.4 });
            }
            "Neptune" => b.surface.colors = [Vec3::new(0.25, 0.42, 0.9), Vec3::new(0.15, 0.3, 0.75), Vec3::ONE],
            "Pluto" => b.surface.colors = [Vec3::new(0.85, 0.75, 0.65), Vec3::new(0.55, 0.35, 0.25), Vec3::ONE],
            _ => {}
        }
    }

    for (k, m) in MOONS.iter().enumerate() {
        let planet = u.bodies.iter().find(|b| b.name == m.planet).expect("planet").id;
        // Our Moon's orbit is listed against the plane of the Solar System; the others go round their planet's equator.
        let frame = if m.name == "Moon" { DQuat::IDENTITY } else { equator(u, planet) };
        // Spread around their orbits by the golden angle.
        let longitude = (k as f64 * 137.508) % 360.0;
        let orbit = Orbit { a: km(m.a), e: m.e, incl: m.incl, node: 0.0, peri: longitude * 0.37, longitude };
        let mass = units::mass(m.gm);
        let id = add_on_orbit(u, planet, frame, m.name, m.kind, mass, km(m.radius), orbit);
        let (pmass, ptilt) = u.get(planet).map(|p| (p.mass, p.tilt)).expect("planet");
        // Every one of these moons keeps the same face to its planet: one turn per orbit.
        let month = std::f64::consts::TAU * (km(m.a).powi(3) / (u.g * (pmass + mass))).sqrt();
        let b = u.get_mut(id).expect("moon");
        b.spin_rate = std::f64::consts::TAU / month;
        b.tilt = if m.name == "Moon" { 1.54f32.to_radians() } else { ptilt };
        match m.name {
            "Io" => b.surface.colors = [Vec3::new(0.75, 0.65, 0.2), Vec3::new(0.4, 0.2, 0.05), Vec3::ONE],
            "Titan" => b.surface.colors = [Vec3::new(0.85, 0.55, 0.2), Vec3::new(0.7, 0.45, 0.15), Vec3::ONE],
            // Pluto always shows Charon the same face, too.
            "Charon" => u.get_mut(planet).expect("Pluto").spin_rate = std::f64::consts::TAU / month,
            _ => {}
        }
    }

    // The main asteroid belt, between Mars and Jupiter.
    asteroid_belt(u, DVec3::ZERO, DVec3::ZERO, SUN_MASS, au(2.1), au(3.3), 3000, Vec3::new(0.55, 0.5, 0.45) * 1.8, 0.3);
    zero_momentum(u);
    u.event("Welcome to the Solar System, at its real size! Zoom in on a planet to see it.");
}

/// A far-off Sun to light a close-up scene: the real size at the real distance, but
/// almost weightless, so that it does not drag the worlds in the scene towards it.
fn lamp_sun(u: &mut Universe, name: &str, direction: DVec3, temperature: f32) -> u64 {
    let mut sun = u.new_body(name, BodyKind::Star, direction.normalize() * AU, DVec3::ZERO).with_mass_radius(5.0, SUN_RADIUS).with_temperature(temperature);
    sun.spin_rate = units::spin_rate(609.12);
    u.add(sun)
}

fn theia(u: &mut Universe) {
    let sun_id = lamp_sun(u, "Sun", DVec3::new(-400.0, 30.0, -300.0), units::SUN_TEMPERATURE);
    let mut earth = u.new_body("Proto-Earth", BodyKind::Earthlike, DVec3::ZERO, DVec3::ZERO).with_mass_radius(60.0, 2.6);
    earth.surface.clouds = 0.5;
    earth.surface.sea_level = -0.2;
    let earth_id = u.add(earth);
    // Like the real one: Theia drifts in slowly and strikes at about 45 degrees, at not
    // much more than escape speed. Faster, and what is left of it would fly off; more
    // head-on, and it would all sink in. This way part of it is caught in orbit.
    let mut theia = u.new_body("Theia", BodyKind::Desert, DVec3::new(32.0, 0.5, 5.85), DVec3::new(-4.25, 0.0, 0.0)).with_mass_radius(14.0, 1.7);
    theia.surface.colors = [Vec3::new(0.5, 0.35, 0.3), Vec3::new(0.3, 0.2, 0.15), Vec3::ONE];
    u.add(theia);
    // Put the pair's shared centre of mass at rest in the middle of the view, so the
    // wreckage (and the moon that forms in it) stays on screen instead of drifting off.
    let pair: Vec<usize> = u.bodies.iter().enumerate().filter(|(_, b)| b.id != sun_id).map(|(i, _)| i).collect();
    let mass: f64 = pair.iter().map(|&i| u.bodies[i].mass).sum();
    let centre = pair.iter().map(|&i| u.bodies[i].pos * u.bodies[i].mass).sum::<DVec3>() / mass;
    let drift = pair.iter().map(|&i| u.bodies[i].vel * u.bodies[i].mass).sum::<DVec3>() / mass;
    for i in pair {
        u.bodies[i].pos -= centre;
        u.bodies[i].vel -= drift;
    }
    // How the pieces fly when Theia is torn is partly chance, and only some throws
    // leave a moon. This is the story of the one that did, so the dice are fixed.
    u.rng = Rng::new(17);
    let _ = earth_id;
    u.event("Theia is drifting towards the Proto-Earth. Watch what happens to the part of it that misses!");
}

fn head_on(u: &mut Universe) {
    lamp_sun(u, "Distant Sun", DVec3::new(-300.0, 150.0, 200.0), 6200.0);
    let a = u.new_body("Aqua", BodyKind::Earthlike, DVec3::new(-28.0, 0.0, 0.0), DVec3::new(7.5, 0.0, 0.0)).with_mass_radius(45.0, 2.4);
    u.add(a);
    let mut b = u.new_body("Ember", BodyKind::Lava, DVec3::new(28.0, 0.0, 1.0), DVec3::new(-7.5, 0.0, 0.0)).with_mass_radius(40.0, 2.2);
    b.surface.terrain = 0.03;
    u.add(b);
    u.event("Two worlds on a head-on collision course!");
}

fn binary(u: &mut Universe) {
    // A star a little bigger than the Sun and an orange dwarf, a fifth of the
    // Earth-Sun distance apart, going round each other every 24 days. The planets
    // go round the pair, all beyond 4x the stars' separation (closer than about
    // 3.5x and an orbit round two stars does not last).
    let m1 = 1.1 * SUN_MASS;
    let m2 = 0.7 * SUN_MASS;
    let sep = au(0.2);
    let r1 = sep * m2 / (m1 + m2);
    let r2 = sep * m1 / (m1 + m2);
    let v_rel = (u.g * (m1 + m2) / sep).sqrt();
    let v1 = v_rel * m2 / (m1 + m2);
    let v2 = v_rel * m1 / (m1 + m2);
    let a = u.new_body("Alpha", BodyKind::Star, DVec3::new(r1, 0.0, 0.0), DVec3::new(0.0, 0.0, -v1)).with_mass_radius(m1, 1.15 * SUN_RADIUS).with_temperature(6100.0);
    let b = u.new_body("Beta", BodyKind::Star, DVec3::new(-r2, 0.0, 0.0), DVec3::new(0.0, 0.0, v2)).with_mass_radius(m2, 0.68 * SUN_RADIUS).with_temperature(4300.0);
    u.add(a);
    u.add(b);
    // Circumbinary planets orbit the pair's barycentre.
    let total = m1 + m2;
    let mk = |u: &mut Universe, name: &str, kind: BodyKind, dist: f64, ang: f64| {
        let pos = DVec3::new(ang.cos(), 0.0, ang.sin()) * dist;
        let speed = (u.g * total / dist).sqrt();
        let vel = DVec3::Y.cross(pos).normalize() * speed;
        let b = u.new_body(name, kind, pos, vel);
        u.add(b)
    };
    mk(u, "Tatooine", BodyKind::Desert, au(0.85), 0.3);
    mk(u, "Verdant", BodyKind::Earthlike, au(1.3), 5.5);
    let gg = mk(u, "Goliath", BodyKind::GasGiant, au(3.6), 2.5);
    if let Some(b) = u.get_mut(gg) {
        let r = b.radius as f32;
        b.ring = Some(Ring { inner: r * 1.3, outer: r * 2.0, color: Vec3::new(0.7, 0.6, 0.55), opacity: 0.6 });
        b.tilt = 0.3;
    }
    mk(u, "Frost", BodyKind::Ice, au(7.5), 4.4);
    zero_momentum(u);
    u.event("Two suns in the sky. Planets orbit the pair together.");
}

/// Gargantua: four million times the mass of the Sun, like the black hole in the
/// middle of our own galaxy. Its real size follows from that: 12 million km.
const GARGANTUA_MASS: f64 = 4.0e6 * SUN_MASS;
const GARGANTUA_RADIUS: f64 = 2.0 * GARGANTUA_MASS / (units::LIGHT_SPEED * units::LIGHT_SPEED);

fn black_hole_feast(u: &mut Universe) {
    let rs = GARGANTUA_RADIUS;
    let mut bh = u.new_body("Gargantua", BodyKind::BlackHole, DVec3::ZERO, DVec3::ZERO);
    bh.mass = GARGANTUA_MASS;
    bh.radius = black_hole_radius(bh.mass, true);
    bh.feeding = 2.0;
    let bh_id = u.add(bh);
    let circular = |r: f64| (GARGANTUA_MASS / r).sqrt();
    // A star like the Sun on a plunging orbit: it will be pulled apart.
    let mut star = u.new_body("Doomed Star", BodyKind::Star, DVec3::new(18.0 * rs, 0.5 * rs, 0.0), DVec3::new(0.0, 0.0, -0.44 * circular(18.0 * rs))).with_temperature(4800.0);
    star.spin_rate = units::spin_rate(400.0);
    u.add(star);
    // Planets far enough out (for now) not to be pulled apart themselves.
    add_orbiting(u, bh_id, "Miller", BodyKind::Earthlike, units::EARTH_MASS, 2.0, 12.0 * rs, 2.0, 0.05);
    add_orbiting(u, bh_id, "Mann", BodyKind::Ice, 8.0, 1.5, 20.0 * rs, 4.0, -0.04);
    add_orbiting(u, bh_id, "Edmunds", BodyKind::Desert, 10.0, 1.6, 27.0 * rs, 1.0, 0.02);
    let v = circular(30.0 * rs);
    let s2 = u.new_body("Wanderer", BodyKind::Rocky, DVec3::new(-10.0 * rs, 0.0, 29.0 * rs), DVec3::new(0.15 * v, 0.0, -0.52 * v)).with_mass_radius(3.0, 1.1);
    u.add(s2);
    // Glowing gas spiralling around it.
    asteroid_belt(u, DVec3::ZERO, DVec3::ZERO, GARGANTUA_MASS, 4.4 * rs, 10.0 * rs, 4000, Vec3::new(1.0, 0.55, 0.25) * 1.5, (rs * 0.012) as f32);
    u.event("A giant black hole, four million times as heavy as the Sun. Fly things close and watch them get spaghettified.");
}

fn supernova(u: &mut Universe) {
    // A blue giant: twelve times the mass of the Sun and six times as wide.
    let star = u.new_body("Rigel B", BodyKind::Star, DVec3::ZERO, DVec3::ZERO).with_mass_radius(12.0 * SUN_MASS, 6.0 * SUN_RADIUS).with_temperature(20000.0);
    let id = u.add(star);
    let planet = |u: &mut Universe, name: &str, kind: BodyKind, dist: f64, ang: f64, incl: f64| {
        let (mass, radius) = kind.defaults(true);
        add_orbiting(u, id, name, kind, mass, radius, dist, ang, incl)
    };
    planet(u, "Cinder", BodyKind::Lava, au(0.3), 0.4, 0.0);
    planet(u, "Oasis", BodyKind::Earthlike, au(0.6), 2.1, 0.02);
    let gg = planet(u, "Titanus", BodyKind::GasGiant, au(1.0), 4.0, -0.03);
    if let Some(b) = u.get_mut(gg) {
        let r = b.radius as f32;
        b.ring = Some(Ring { inner: r * 1.3, outer: r * 2.1, color: Vec3::new(0.7, 0.75, 0.8), opacity: 0.7 });
        b.tilt = 0.4;
    }
    planet(u, "Glacier", BodyKind::Ice, au(1.6), 5.5, 0.05);
    planet(u, "Rust", BodyKind::Desert, au(2.3), 1.0, 0.0);
    zero_momentum(u);
    // Twenty seconds of play at the speed the preset starts at.
    u.trigger_supernova(id, 20.0 * Preset::Supernova.time_scale());
}

fn galaxy(u: &mut Universe, name: &str, center: DVec3, vel: DVec3, tilt: DQuat, mass: f64, count: usize, radius: f64, warm: Vec3, cool: Vec3) {
    let mut bh = u.new_body(name, BodyKind::BlackHole, center, vel);
    bh.mass = mass;
    bh.radius = black_hole_radius(mass, false);
    bh.feeding = 2.0;
    u.add(bh);
    let arms = 2;
    let tilt_axis = tilt * DVec3::Y;
    for k in 0..count {
        let bulge = k % 7 == 0;
        let (pos, v) = if bulge {
            let r = u.rng.range(10.0, radius * 0.18);
            let dir = u.rng.unit();
            let speed = (u.g * mass / r).sqrt();
            let mut t = dir.cross(tilt_axis);
            if t.length_squared() < 1e-6 {
                t = dir.cross(DVec3::X);
            }
            let t = (t.normalize() + u.rng.unit() * 0.3).normalize();
            (dir * r, t * speed)
        } else {
            let x = u.rng.f64();
            let r = (radius * 0.08) + (-(1.0 - x * 0.97).ln()) * radius * 0.28;
            let arm = (k % arms) as f64 * std::f64::consts::PI;
            let theta = arm + (r / 20.0).ln() * 2.3 + u.rng.normal() * 0.28;
            let local = DVec3::new(r * theta.cos(), u.rng.normal() * 1.5, r * theta.sin());
            let speed = (u.g * mass / r).sqrt();
            let t = DVec3::Y.cross(local).normalize() * -speed;
            (tilt * local, tilt * t)
        };
        let rr = pos.length() / radius;
        let c = if bulge || rr < 0.2 {
            warm * u.rng.rangef(0.7, 1.4)
        } else if u.rng.f32() < 0.15 {
            Vec3::new(1.0, 0.45, 0.5) * 1.3
        } else {
            cool * u.rng.rangef(0.6, 1.4)
        };
        u.emit(center + pos, vel + v, c, u.rng.rangef(0.6, 1.6), 1.0, P_GRAVITY | P_IMMORTAL);
    }
}

fn galaxy_collision(u: &mut Universe) {
    u.max_particles = 150_000;
    let m = 20000.0;
    galaxy(
        u,
        "Milky Way core",
        DVec3::new(-420.0, 0.0, -170.0),
        DVec3::new(3.6, 0.0, 0.0),
        DQuat::IDENTITY,
        m,
        34000,
        230.0,
        Vec3::new(1.0, 0.85, 0.6) * 1.6,
        Vec3::new(0.65, 0.8, 1.0) * 1.6,
    );
    galaxy(
        u,
        "Andromeda core",
        DVec3::new(420.0, 40.0, 170.0),
        DVec3::new(-3.6, -0.2, 0.0),
        DQuat::from_rotation_x(0.9) * DQuat::from_rotation_z(0.4),
        m,
        34000,
        200.0,
        Vec3::new(1.0, 0.8, 0.55) * 1.6,
        Vec3::new(0.85, 0.75, 1.0) * 1.6,
    );
    u.trail_interval = 3.0;
    u.event("Two galaxies are about to collide. Speed up time to watch tidal tails form!");
}

fn protoplanetary(u: &mut Universe) {
    let star = u.new_body("Young Sun", BodyKind::Star, DVec3::ZERO, DVec3::ZERO).with_mass_radius(8000.0, 9.0).with_temperature(4800.0);
    u.add(star);
    for k in 0..140 {
        let r = u.rng.range(28.0, 220.0);
        let a = u.rng.range(0.0, std::f64::consts::TAU);
        let pos = DVec3::new(r * a.cos(), u.rng.normal() * 0.6, r * a.sin());
        let speed = (u.g * 8000.0 / r).sqrt() * (1.0 + u.rng.normal() * 0.025);
        let vel = DVec3::Y.cross(pos).normalize() * speed;
        let kind = if r < 80.0 && u.rng.f32() < 0.3 { BodyKind::Lava } else if r > 150.0 && u.rng.f32() < 0.4 { BodyKind::Ice } else { BodyKind::Rocky };
        let mass = u.rng.range(0.5, 3.0);
        let radius = 0.45 * mass.cbrt() * 1.2;
        let _ = k;
        let mut b = u.new_body("", kind, pos, vel).with_mass_radius(mass, radius);
        b.surface.terrain = 0.06;
        u.add(b);
    }
    asteroid_belt(u, DVec3::ZERO, DVec3::ZERO, 8000.0, 20.0, 260.0, 9000, Vec3::new(0.8, 0.5, 0.3) * 2.5, 0.11);
    zero_momentum(u);
    u.collisions = crate::universe::CollisionMode::AlwaysMerge;
    u.event("A young star surrounded by planetesimals. Speed up time and watch them merge into planets.");
}

// ----------------------------------------------------------------------
// Custom universe generator
// ----------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CenterKind {
    SunLike,
    RedDwarf,
    BlueGiant,
    Binary,
    BlackHole,
    Nothing,
}

impl CenterKind {
    pub const ALL: [CenterKind; 6] =
        [CenterKind::SunLike, CenterKind::RedDwarf, CenterKind::BlueGiant, CenterKind::Binary, CenterKind::BlackHole, CenterKind::Nothing];
    pub fn label(self) -> &'static str {
        match self {
            CenterKind::SunLike => "Sun-like star",
            CenterKind::RedDwarf => "Red dwarf",
            CenterKind::BlueGiant => "Blue giant",
            CenterKind::Binary => "Binary stars",
            CenterKind::BlackHole => "Black hole",
            CenterKind::Nothing => "Nothing (rogue planets)",
        }
    }
}

#[derive(Clone)]
pub struct CustomParams {
    pub name: String,
    pub center: CenterKind,
    pub planets: usize,
    pub moons: bool,
    pub belt: bool,
    pub rings: bool,
    pub gravity: f64,
    pub spacing: f64,
    pub seed: u64,
    pub chaos: f64,
}

impl Default for CustomParams {
    fn default() -> Self {
        CustomParams {
            name: "Arthur's Universe".into(),
            center: CenterKind::SunLike,
            planets: 7,
            moons: true,
            belt: true,
            rings: true,
            gravity: 1.0,
            spacing: 1.0,
            seed: 42,
            chaos: 0.0,
        }
    }
}

const SYLLABLES: [&str; 24] = [
    "ar", "thu", "ra", "zen", "ko", "vel", "mi", "do", "xa", "ri", "lu", "no", "ta", "pe", "qua", "sol", "ve", "ga", "ny", "or", "is", "el", "um", "bo",
];

fn random_name(rng: &mut Rng) -> String {
    let n = 2 + rng.below(2);
    let mut s = String::new();
    for _ in 0..n {
        s.push_str(SYLLABLES[rng.below(SYLLABLES.len())]);
    }
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => s,
    }
}

pub fn build_custom(u: &mut Universe, p: &CustomParams) {
    build_custom_bodies(u, p);
    crate::population::settle(u);
}

fn build_custom_bodies(u: &mut Universe, p: &CustomParams) {
    u.reset(&p.name);
    u.real_scale = true;
    u.g = p.gravity;
    u.rng = Rng::new(p.seed);
    let mut rng = Rng::new(p.seed.wrapping_add(7));
    let mut central_mass = 0.0;
    let mut center_id = None;
    match p.center {
        CenterKind::SunLike => {
            let s = u.new_body(&random_name(&mut rng), BodyKind::Star, DVec3::ZERO, DVec3::ZERO).with_mass_radius(SUN_MASS, SUN_RADIUS).with_temperature(units::SUN_TEMPERATURE);
            central_mass = s.mass;
            center_id = Some(u.add(s));
        }
        CenterKind::RedDwarf => {
            let s = u.new_body(&random_name(&mut rng), BodyKind::Star, DVec3::ZERO, DVec3::ZERO).with_mass_radius(0.3 * SUN_MASS, 0.32 * SUN_RADIUS).with_temperature(3300.0);
            central_mass = s.mass;
            center_id = Some(u.add(s));
        }
        CenterKind::BlueGiant => {
            let s = u.new_body(&random_name(&mut rng), BodyKind::Star, DVec3::ZERO, DVec3::ZERO).with_mass_radius(4.0 * SUN_MASS, 3.0 * SUN_RADIUS).with_temperature(14000.0);
            central_mass = s.mass;
            center_id = Some(u.add(s));
        }
        CenterKind::Binary => {
            let (m1, m2, sep) = (1.0 * SUN_MASS, 0.7 * SUN_MASS, au(0.2));
            let v_rel = (u.g * (m1 + m2) / sep).sqrt();
            let a = u.new_body(&random_name(&mut rng), BodyKind::Star, DVec3::new(sep * m2 / (m1 + m2), 0.0, 0.0), DVec3::new(0.0, 0.0, -v_rel * m2 / (m1 + m2)))
                .with_mass_radius(m1, SUN_RADIUS)
                .with_temperature(6000.0);
            let b = u.new_body(&random_name(&mut rng), BodyKind::Star, DVec3::new(-sep * m1 / (m1 + m2), 0.0, 0.0), DVec3::new(0.0, 0.0, v_rel * m1 / (m1 + m2)))
                .with_mass_radius(m2, 0.68 * SUN_RADIUS)
                .with_temperature(4300.0);
            u.add(a);
            u.add(b);
            central_mass = m1 + m2;
        }
        CenterKind::BlackHole => {
            let mut bh = u.new_body(&random_name(&mut rng), BodyKind::BlackHole, DVec3::ZERO, DVec3::ZERO);
            bh.mass = 10.0 * SUN_MASS;
            bh.radius = black_hole_radius(bh.mass, true);
            central_mass = bh.mass;
            center_id = Some(u.add(bh));
            // A black hole gives no light: add a distant companion star as a lamp.
            // ...on a wide circular orbit so it doesn't fall in.
            let lamp_pos = DVec3::new(0.0, au(20.0), au(-80.0));
            let lamp_vel = DVec3::Y.cross(lamp_pos).normalize() * (u.g * central_mass / lamp_pos.length()).sqrt();
            // (Bright enough to light the planets from that far, and almost weightless.)
            let lamp = u.new_body("Companion", BodyKind::Star, lamp_pos, lamp_vel).with_mass_radius(1.0, 4.0 * SUN_RADIUS).with_temperature(20000.0);
            u.add(lamp);
        }
        CenterKind::Nothing => {
            let lamp_pos = DVec3::new(-1500.0, 800.0, -3000.0).normalize() * au(60.0);
            let lamp = u.new_body("Distant Star", BodyKind::Star, lamp_pos, DVec3::ZERO).with_mass_radius(1.0, 4.0 * SUN_RADIUS).with_temperature(16000.0);
            u.add(lamp);
        }
    }
    // Where the first planet goes.
    let base = match p.center {
        CenterKind::Binary => au(0.85), // circumbinary orbits need > ~3.5x the binary separation
        CenterKind::BlueGiant => au(2.0),
        CenterKind::RedDwarf => au(0.07),
        CenterKind::BlackHole => au(0.6),
        CenterKind::SunLike => au(0.4),
        // Rogue planets with no star: a loose huddle.
        CenterKind::Nothing => 300.0,
    };
    // Previous planet's (distance, mass); the first planet sits at `base`.
    let mut prev: Option<(f64, f64)> = None;
    let mut next_min = base * p.spacing;
    let belt_slot = if p.belt && p.planets > 2 { Some(p.planets / 2) } else { None };
    for k in 0..p.planets {
        if Some(k) == belt_slot && central_mass > 0.0 {
            let inner = prev.map(|(d, _)| d * 1.35).unwrap_or(next_min).max(next_min);
            let outer = inner * 1.3;
            asteroid_belt(u, DVec3::ZERO, DVec3::ZERO, central_mass, inner, outer, 2000, Vec3::new(0.55, 0.5, 0.45) * 1.8, 0.3);
            // keep the next planet clear of the belt
            prev = None;
            next_min = outer * 1.3;
        }
        let frac = k as f64 / p.planets.max(1) as f64;
        let kind = {
            let r = rng.f64();
            if frac < 0.3 {
                [BodyKind::Lava, BodyKind::Rocky, BodyKind::Desert, BodyKind::Cloudy][rng.below(4)]
            } else if frac < 0.55 {
                if r < 0.45 { BodyKind::Earthlike } else { [BodyKind::Desert, BodyKind::Cloudy, BodyKind::Rocky][rng.below(3)] }
            } else if r < 0.6 {
                BodyKind::GasGiant
            } else if r < 0.85 {
                BodyKind::IceGiant
            } else {
                BodyKind::Ice
            }
        };
        let (m0, r0) = kind.defaults(true);
        let scale = rng.range(0.6, 1.5);
        let mass = m0 * scale;
        let radius = r0 * scale.cbrt();
        // Spacing: at least ~10 mutual Hill radii from the previous planet
        // (Delta = K * ((m1+m2)/3M)^(1/3) * (a1+a2)/2), plus a pleasing random spread.
        let dist = match prev {
            Some((pd, pm)) if central_mass > 0.0 => {
                let x = ((mass + pm) / (3.0 * central_mass)).cbrt() * 10.0 / 2.0;
                let hill_ratio = if x < 0.75 { (1.0 + x) / (1.0 - x) } else { 7.0 };
                let pretty = rng.range(1.3, 1.55) * p.spacing.sqrt();
                (pd * hill_ratio.max(pretty)).max(next_min)
            }
            Some((pd, _)) => (pd * rng.range(1.3, 1.55) * p.spacing.sqrt()).max(next_min),
            None => next_min,
        };
        prev = Some((dist, mass));
        let ang = rng.range(0.0, std::f64::consts::TAU);
        let incl = rng.normal() * 0.03;
        let pos = DQuat::from_rotation_x(incl) * DVec3::new(ang.cos(), 0.0, ang.sin()) * dist;
        let mut vel = if central_mass > 0.0 {
            let speed = (u.g * central_mass / dist).sqrt();
            DVec3::Y.cross(pos).normalize() * speed
        } else {
            DVec3::new(rng.normal(), 0.0, rng.normal()) * 1.5
        };
        vel *= 1.0 + rng.normal() * p.chaos * 0.3;
        if p.chaos > 0.0 {
            let speed = vel.length().max(1.0);
            vel += DVec3::new(rng.normal(), rng.normal() * 0.3, rng.normal()) * p.chaos * 0.15 * speed;
        }
        let name = random_name(&mut rng);
        let mut b = u.new_body(&name, kind, pos, vel).with_mass_radius(mass, radius);
        b.tilt = (rng.normal() * 0.3) as f32;
        if p.rings && matches!(kind, BodyKind::GasGiant | BodyKind::IceGiant) && rng.f32() < 0.5 {
            let c = Vec3::new(rng.rangef(0.55, 0.9), rng.rangef(0.5, 0.8), rng.rangef(0.45, 0.8));
            b.ring = Some(crate::body::Ring { inner: radius as f32 * 1.3, outer: radius as f32 * rng.rangef(1.8, 2.4), color: c, opacity: 0.8 });
        }
        let pid = u.add(b);
        if p.moons {
            let nm = match kind {
                BodyKind::GasGiant => 1 + rng.below(4),
                BodyKind::IceGiant => rng.below(3),
                BodyKind::Earthlike | BodyKind::Desert => rng.below(2),
                _ => 0,
            };
            // stay well inside the Hill sphere
            let hill = if central_mass > 0.0 { dist * (mass / (3.0 * central_mass)).cbrt() } else { 60.0 };
            for m in 0..nm {
                let md = radius * 2.2 + (m as f64 + 1.0) * radius * 1.4;
                // prograde moons are only long-term stable inside ~0.3 r_H
                if md > hill * 0.3 {
                    break;
                }
                let mk = [BodyKind::Rocky, BodyKind::Ice, BodyKind::Lava, BodyKind::Asteroid][rng.below(4)];
                let mname = format!("{} {}", name, ["I", "II", "III", "IV"][m]);
                // Single moons may be hefty (like ours); moon families must be light
                // (Galilean moons are ~1e-5..1e-4 of Jupiter) or they scatter each other.
                let mm = if nm == 1 { mass * rng.range(0.002, 0.012) } else { mass * rng.range(2e-5, 1e-4) };
                let mr = (radius * 0.18 * rng.range(0.7, 1.3)).max(0.2);
                add_orbiting(u, pid, &mname, mk, mm, mr, md, rng.range(0.0, std::f64::consts::TAU), rng.normal() * 0.05);
            }
        }
    }
    let _ = center_id;
    zero_momentum(u);
    u.event(format!("Created universe '{}'", p.name));
}
