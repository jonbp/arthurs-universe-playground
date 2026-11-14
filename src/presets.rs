//! Ready-made universes and the procedural "create your own universe" generator.

use glam::{DQuat, DVec3, Vec3};

use crate::body::{black_hole_radius, BodyKind};
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
    pub fn camera_distance(self) -> f64 {
        match self {
            Preset::SolarSystem => 2600.0,
            Preset::RogueBlackHole => 7500.0,
            Preset::TheiaImpact => 60.0,
            Preset::HeadOn => 70.0,
            Preset::BinaryStars => 2200.0,
            Preset::BlackHoleFeast => 260.0,
            Preset::Supernova => 520.0,
            Preset::GalaxyCollision => 1300.0,
            Preset::Protoplanetary => 380.0,
            Preset::Empty => 200.0,
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

fn zero_momentum(u: &mut Universe) {
    let (mut p, mut m) = (DVec3::ZERO, 0.0);
    let (mut c, mut mc) = (DVec3::ZERO, 0.0);
    for b in &u.bodies {
        p += b.vel * b.mass;
        m += b.mass;
        c += b.pos * b.mass;
        mc += b.mass;
    }
    if m <= 0.0 {
        return;
    }
    let v = p / m;
    let _com = c / mc;
    for b in &mut u.bodies {
        b.vel -= v;
    }
    // Particles (belts, dust) were launched relative to the same frame: shift them too.
    let vf = v.as_vec3();
    for p in &mut u.particles {
        p.vel -= vf;
    }
}

fn asteroid_belt(u: &mut Universe, center: DVec3, center_vel: DVec3, mass: f64, r0: f64, r1: f64, count: usize, color: Vec3) {
    for _ in 0..count {
        let r = u.rng.range(r0, r1);
        let a = u.rng.range(0.0, std::f64::consts::TAU);
        let y = u.rng.normal() * (r1 - r0) * 0.04;
        let pos = center + DVec3::new(r * a.cos(), y, r * a.sin());
        let speed = (u.g * mass / r).sqrt() * u.rng.range(0.98, 1.02);
        let dir = DVec3::Y.cross(pos - center).normalize();
        let tint = color * u.rng.rangef(0.5, 1.2);
        // Bigger belts get bigger "rocks" so they stay visible from afar.
        let size = u.rng.rangef(0.15, 0.45) * (r0 / 150.0).sqrt() as f32;
        u.emit(pos, center_vel + dir * speed, tint, size, 1.0, P_GRAVITY | P_IMMORTAL);
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
    u.g = 1.0;
    match preset {
        Preset::SolarSystem => solar_system(u),
        Preset::RogueBlackHole => {
            solar_system(u);
            let start = DVec3::new(6000.0, 150.0, 3900.0);
            let aim = DVec3::new(250.0, 0.0, -350.0);
            let mut bh = u.new_body("Rogue Black Hole", BodyKind::BlackHole, start, (aim - start).normalize() * 22.0);
            bh.mass = 300_000.0;
            bh.radius = black_hole_radius(bh.mass);
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

/// Mass of the Sun in the Solar System preset.
///
/// The Solar System is built to be dynamically stable:
/// * planets are spaced at (roughly) real distance ratios so neighbours sit
///   ~7-13 mutual Hill radii apart (real systems are stable above ~8; below ~3.5
///   orbits go chaotic within a few dozen orbits),
/// * every moon orbits inside ~0.3 of its planet's Hill radius r_H = a (m / 3M)^(1/3)
///   (prograde moons get stripped by the Sun's tide beyond ~0.4-0.5 r_H),
/// * terrestrial planets are ~30x heavier relative to the Sun than in reality so
///   the Moon can sit close to Earth, giants keep their real mass ratios,
/// * moon masses are realistic fractions of their planets so moons don't
///   destabilise each other.
pub const SUN_MASS: f64 = 400_000.0;

pub fn solar_system(u: &mut Universe) {
    let sun = u.new_body("Sun", BodyKind::Star, DVec3::ZERO, DVec3::ZERO).with_mass_radius(SUN_MASS, 16.0).with_temperature(5800.0);
    let sun = u.add(sun);
    let ang = |i: u32| (i as f64 * 2.39996) % std::f64::consts::TAU; // golden angle spread

    // Distances: Earth = 430 units; inner planets at real ratios, outer giants gently compressed.
    let id = add_orbiting(u, sun, "Mercury", BodyKind::Rocky, 2.2, 0.9, 166.0, ang(1), 0.05);
    if let Some(b) = u.get_mut(id) {
        b.surface.colors = [Vec3::new(0.5, 0.47, 0.44), Vec3::new(0.32, 0.3, 0.28), Vec3::ONE];
        b.spin_rate = 0.05;
    }
    let id = add_orbiting(u, sun, "Venus", BodyKind::Cloudy, 32.0, 1.9, 311.0, ang(2), 0.03);
    if let Some(b) = u.get_mut(id) {
        b.spin_rate = -0.02;
    }
    let earth = add_orbiting(u, sun, "Earth", BodyKind::Earthlike, 40.0, 2.0, 430.0, ang(3), 0.0);
    if let Some(b) = u.get_mut(earth) {
        b.tilt = 0.41;
        b.spin_rate = 0.5;
        b.surface.seed = 3.3;
    }
    // r_H(Earth) ~ 13.8 -> Moon at 0.29 r_H
    add_orbiting(u, earth, "Moon", BodyKind::Rocky, 0.5, 0.55, 4.0, 0.0, 0.09);
    let mars = add_orbiting(u, sun, "Mars", BodyKind::Desert, 4.3, 1.4, 655.0, ang(4), 0.03);
    if let Some(b) = u.get_mut(mars) {
        b.tilt = 0.44;
    }
    // r_H(Mars) ~ 10
    add_orbiting(u, mars, "Phobos", BodyKind::Asteroid, 0.002, 0.22, 2.3, 0.0, 0.0);
    add_orbiting(u, mars, "Deimos", BodyKind::Asteroid, 0.001, 0.16, 3.0, 2.0, 0.0);

    // Main belt between Mars and Jupiter (2.2-3.1 AU equivalent), clear of Jupiter's chaotic zone.
    asteroid_belt(u, DVec3::ZERO, DVec3::ZERO, SUN_MASS, 950.0, 1330.0, 3000, Vec3::new(0.55, 0.5, 0.45) * 1.8);
    add_orbiting(u, sun, "Ceres", BodyKind::Rocky, 0.004, 0.5, 1190.0, ang(9), 0.1);
    add_orbiting(u, sun, "Vesta", BodyKind::Asteroid, 0.001, 0.4, 1015.0, ang(10), 0.12);

    // Jupiter: real mass ratio (1e-3), r_H ~ 118
    let jup = add_orbiting(u, sun, "Jupiter", BodyKind::GasGiant, 400.0, 6.0, 1700.0, ang(5), 0.02);
    if let Some(b) = u.get_mut(jup) {
        b.surface.colors = [Vec3::new(0.87, 0.75, 0.6), Vec3::new(0.62, 0.38, 0.22), Vec3::new(0.95, 0.92, 0.86)];
        b.tilt = 0.05;
        b.spin_rate = 1.0;
        b.surface.seed = 1.0;
    }
    let io = add_orbiting(u, jup, "Io", BodyKind::Lava, 0.02, 0.6, 9.0, 0.0, 0.0);
    if let Some(b) = u.get_mut(io) {
        b.surface.colors = [Vec3::new(0.75, 0.65, 0.2), Vec3::new(0.4, 0.2, 0.05), Vec3::ONE];
    }
    add_orbiting(u, jup, "Europa", BodyKind::Ice, 0.012, 0.5, 11.5, 1.7, 0.0);
    add_orbiting(u, jup, "Ganymede", BodyKind::Rocky, 0.03, 0.75, 14.5, 3.3, 0.0);
    add_orbiting(u, jup, "Callisto", BodyKind::Rocky, 0.02, 0.7, 18.5, 4.6, 0.0);

    let sat = add_orbiting(u, sun, "Saturn", BodyKind::GasGiant, 120.0, 5.0, 2975.0, ang(6), 0.04);
    if let Some(b) = u.get_mut(sat) {
        b.surface.colors = [Vec3::new(0.9, 0.82, 0.6), Vec3::new(0.75, 0.62, 0.4), Vec3::new(0.95, 0.9, 0.75)];
        b.tilt = 0.47;
        b.spin_rate = 0.9;
        b.ring = Some(crate::body::Ring { inner: 6.5, outer: 11.5, color: Vec3::new(0.85, 0.78, 0.62), opacity: 0.9 });
    }
    let titan = add_orbiting(u, sat, "Titan", BodyKind::Cloudy, 0.03, 0.75, 15.0, 0.5, 0.0);
    if let Some(b) = u.get_mut(titan) {
        b.surface.colors = [Vec3::new(0.85, 0.55, 0.2), Vec3::new(0.7, 0.45, 0.15), Vec3::ONE];
    }

    let ura = add_orbiting(u, sun, "Uranus", BodyKind::IceGiant, 17.0, 3.3, 4760.0, ang(7), 0.01);
    if let Some(b) = u.get_mut(ura) {
        b.tilt = 1.71;
        b.surface.colors = [Vec3::new(0.6, 0.85, 0.9), Vec3::new(0.5, 0.78, 0.85), Vec3::ONE];
        b.ring = Some(crate::body::Ring { inner: 4.6, outer: 5.4, color: Vec3::new(0.6, 0.65, 0.7), opacity: 0.4 });
    }
    let nep = add_orbiting(u, sun, "Neptune", BodyKind::IceGiant, 20.0, 3.2, 6660.0, ang(8), 0.03);
    if let Some(b) = u.get_mut(nep) {
        b.surface.colors = [Vec3::new(0.25, 0.42, 0.9), Vec3::new(0.15, 0.3, 0.75), Vec3::ONE];
    }
    let (pos, vel) = orbit_around(u, nep, 6.0, 0.0, 0.4, true);
    let triton = u.new_body("Triton", BodyKind::Ice, pos, vel).with_mass_radius(0.01, 0.5);
    u.add(triton);
    let pluto = add_orbiting(u, sun, "Pluto", BodyKind::Ice, 0.005, 0.4, 8700.0, ang(11), 0.3);
    if let Some(b) = u.get_mut(pluto) {
        b.surface.colors = [Vec3::new(0.85, 0.75, 0.65), Vec3::new(0.55, 0.35, 0.25), Vec3::ONE];
        b.vel *= 0.93; // a bit eccentric
    }
    zero_momentum(u);
    u.event("Welcome to the Solar System! Select a planet, terraform it, or smash something into it.");
}

fn theia(u: &mut Universe) {
    let sun = u.new_body("Sun", BodyKind::Star, DVec3::new(-400.0, 30.0, -300.0), DVec3::ZERO).with_mass_radius(10000.0, 12.0).with_temperature(5800.0);
    let sun_id = u.add(sun);
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
    // The sun is far away and very massive: keep it from pulling the pair apart by making it static-ish.
    if let Some(s) = u.get_mut(sun_id) {
        s.mass = 50.0; // a distant lamp; real gravity would dominate the scene
    }
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
    let mut sun = u.new_body("Distant Sun", BodyKind::Star, DVec3::new(-300.0, 150.0, 200.0), DVec3::ZERO).with_mass_radius(5.0, 12.0).with_temperature(6200.0);
    sun.spin_rate = 0.05;
    u.add(sun);
    let a = u.new_body("Aqua", BodyKind::Earthlike, DVec3::new(-28.0, 0.0, 0.0), DVec3::new(7.5, 0.0, 0.0)).with_mass_radius(45.0, 2.4);
    u.add(a);
    let mut b = u.new_body("Ember", BodyKind::Lava, DVec3::new(28.0, 0.0, 1.0), DVec3::new(-7.5, 0.0, 0.0)).with_mass_radius(40.0, 2.2);
    b.surface.terrain = 0.03;
    u.add(b);
    u.event("Two worlds on a head-on collision course!");
}

fn binary(u: &mut Universe) {
    // Heavy stars + planets beyond ~4x the binary separation (circumbinary
    // orbits closer than ~3.5x are unstable), spaced >= 8 mutual Hill radii apart.
    let m1 = 360_000.0;
    let m2 = 240_000.0;
    let sep = 120.0;
    let r1 = sep * m2 / (m1 + m2);
    let r2 = sep * m1 / (m1 + m2);
    let v_rel = (u.g * (m1 + m2) / sep).sqrt();
    let v1 = v_rel * m2 / (m1 + m2);
    let v2 = v_rel * m1 / (m1 + m2);
    let a = u.new_body("Alpha", BodyKind::Star, DVec3::new(r1, 0.0, 0.0), DVec3::new(0.0, 0.0, -v1)).with_mass_radius(m1, 13.0).with_temperature(9500.0);
    let b = u.new_body("Beta", BodyKind::Star, DVec3::new(-r2, 0.0, 0.0), DVec3::new(0.0, 0.0, v2)).with_mass_radius(m2, 10.0).with_temperature(4300.0);
    u.add(a);
    u.add(b);
    // Circumbinary planets orbit the pair's barycentre.
    let total = m1 + m2;
    let mk = |u: &mut Universe, name: &str, kind: BodyKind, mass: f64, radius: f64, dist: f64, ang: f64| {
        let pos = DVec3::new(ang.cos(), 0.0, ang.sin()) * dist;
        let speed = (u.g * total / dist).sqrt();
        let vel = DVec3::Y.cross(pos).normalize() * speed;
        let b = u.new_body(name, kind, pos, vel).with_mass_radius(mass, radius);
        u.add(b)
    };
    mk(u, "Tatooine", BodyKind::Desert, 6.0, 1.6, 520.0, 0.3);
    mk(u, "Verdant", BodyKind::Earthlike, 35.0, 2.0, 720.0, 5.5);
    let gg = mk(u, "Goliath", BodyKind::GasGiant, 180.0, 5.5, 1250.0, 2.5);
    if let Some(b) = u.get_mut(gg) {
        b.ring = Some(crate::body::Ring { inner: 7.0, outer: 10.0, color: Vec3::new(0.7, 0.6, 0.55), opacity: 0.6 });
        b.tilt = 0.3;
    }
    mk(u, "Frost", BodyKind::Ice, 4.0, 1.3, 2050.0, 4.4);
    zero_momentum(u);
    u.event("Two suns in the sky. Planets orbit the pair together.");
}

fn black_hole_feast(u: &mut Universe) {
    let mut bh = u.new_body("Gargantua", BodyKind::BlackHole, DVec3::ZERO, DVec3::ZERO);
    bh.mass = 30000.0;
    bh.radius = black_hole_radius(bh.mass);
    bh.feeding = 2.0;
    let bh_id = u.add(bh);
    // A star on a plunging orbit that will be tidally disrupted.
    let mut star = u.new_body("Doomed Star", BodyKind::Star, DVec3::new(160.0, 5.0, 0.0), DVec3::new(0.0, 0.0, -6.0)).with_mass_radius(3000.0, 6.0).with_temperature(4800.0);
    star.spin_rate = 0.1;
    u.add(star);
    add_orbiting(u, bh_id, "Miller", BodyKind::Earthlike, 30.0, 1.8, 110.0, 2.0, 0.05);
    add_orbiting(u, bh_id, "Mann", BodyKind::Ice, 8.0, 1.5, 180.0, 4.0, -0.04);
    add_orbiting(u, bh_id, "Edmunds", BodyKind::Desert, 10.0, 1.6, 240.0, 1.0, 0.02);
    let s2 = u.new_body("Wanderer", BodyKind::Rocky, DVec3::new(-90.0, 0.0, 260.0), DVec3::new(2.0, 0.0, -7.0)).with_mass_radius(3.0, 1.1);
    u.add(s2);
    // Glowing gas spiralling around it.
    asteroid_belt(u, DVec3::ZERO, DVec3::ZERO, 30000.0, 40.0, 90.0, 4000, Vec3::new(1.0, 0.55, 0.25) * 1.5);
    u.event("A supermassive black hole. Fly things close and watch them get spaghettified.");
}

fn supernova(u: &mut Universe) {
    let star = u.new_body("Rigel B", BodyKind::Star, DVec3::ZERO, DVec3::ZERO).with_mass_radius(20000.0, 22.0).with_temperature(13000.0);
    let id = u.add(star);
    add_orbiting(u, id, "Cinder", BodyKind::Lava, 8.0, 1.5, 70.0, 0.4, 0.0);
    add_orbiting(u, id, "Oasis", BodyKind::Earthlike, 40.0, 2.1, 140.0, 2.1, 0.02);
    let gg = add_orbiting(u, id, "Titanus", BodyKind::GasGiant, 200.0, 5.5, 230.0, 4.0, -0.03);
    if let Some(b) = u.get_mut(gg) {
        b.ring = Some(crate::body::Ring { inner: 7.0, outer: 11.0, color: Vec3::new(0.7, 0.75, 0.8), opacity: 0.7 });
        b.tilt = 0.4;
    }
    add_orbiting(u, id, "Glacier", BodyKind::Ice, 6.0, 1.3, 330.0, 5.5, 0.05);
    add_orbiting(u, id, "Rust", BodyKind::Desert, 6.0, 1.4, 420.0, 1.0, 0.0);
    zero_momentum(u);
    u.trigger_supernova(id, 20.0);
}

fn galaxy(u: &mut Universe, name: &str, center: DVec3, vel: DVec3, tilt: DQuat, mass: f64, count: usize, radius: f64, warm: Vec3, cool: Vec3) {
    let mut bh = u.new_body(name, BodyKind::BlackHole, center, vel);
    bh.mass = mass;
    bh.radius = black_hole_radius(mass);
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
    asteroid_belt(u, DVec3::ZERO, DVec3::ZERO, 8000.0, 20.0, 260.0, 9000, Vec3::new(0.8, 0.5, 0.3) * 2.5);
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
    u.g = p.gravity;
    u.rng = Rng::new(p.seed);
    let mut rng = Rng::new(p.seed.wrapping_add(7));
    let mut central_mass = 0.0;
    let mut center_id = None;
    match p.center {
        CenterKind::SunLike => {
            let s = u.new_body(&random_name(&mut rng), BodyKind::Star, DVec3::ZERO, DVec3::ZERO).with_mass_radius(400_000.0, 16.0).with_temperature(5800.0);
            central_mass = s.mass;
            center_id = Some(u.add(s));
        }
        CenterKind::RedDwarf => {
            let s = u.new_body(&random_name(&mut rng), BodyKind::Star, DVec3::ZERO, DVec3::ZERO).with_mass_radius(160_000.0, 8.0).with_temperature(3300.0);
            central_mass = s.mass;
            center_id = Some(u.add(s));
        }
        CenterKind::BlueGiant => {
            let s = u.new_body(&random_name(&mut rng), BodyKind::Star, DVec3::ZERO, DVec3::ZERO).with_mass_radius(720_000.0, 26.0).with_temperature(14000.0);
            central_mass = s.mass;
            center_id = Some(u.add(s));
        }
        CenterKind::Binary => {
            let (m1, m2, sep) = (280_000.0, 200_000.0, 100.0);
            let v_rel = (u.g * (m1 + m2) / sep).sqrt();
            let a = u.new_body(&random_name(&mut rng), BodyKind::Star, DVec3::new(sep * m2 / (m1 + m2), 0.0, 0.0), DVec3::new(0.0, 0.0, -v_rel * m2 / (m1 + m2)))
                .with_mass_radius(m1, 10.0)
                .with_temperature(7500.0);
            let b = u.new_body(&random_name(&mut rng), BodyKind::Star, DVec3::new(-sep * m1 / (m1 + m2), 0.0, 0.0), DVec3::new(0.0, 0.0, v_rel * m1 / (m1 + m2)))
                .with_mass_radius(m2, 8.0)
                .with_temperature(4000.0);
            u.add(a);
            u.add(b);
            central_mass = m1 + m2;
        }
        CenterKind::BlackHole => {
            let mut bh = u.new_body(&random_name(&mut rng), BodyKind::BlackHole, DVec3::ZERO, DVec3::ZERO);
            bh.mass = 300_000.0;
            bh.radius = black_hole_radius(bh.mass);
            central_mass = bh.mass;
            center_id = Some(u.add(bh));
            // A black hole gives no light: add a distant companion star as a lamp.
            // ...on a wide circular orbit so it doesn't fall in.
            let lamp_pos = DVec3::new(0.0, 1500.0, -6000.0);
            let lamp_vel = DVec3::Y.cross(lamp_pos).normalize() * (u.g * central_mass / lamp_pos.length()).sqrt();
            let lamp = u.new_body("Companion", BodyKind::Star, lamp_pos, lamp_vel).with_mass_radius(1.0, 40.0).with_temperature(20000.0);
            u.add(lamp);
        }
        CenterKind::Nothing => {
            let lamp = u.new_body("Distant Star", BodyKind::Star, DVec3::new(-1500.0, 800.0, -3000.0), DVec3::ZERO).with_mass_radius(1.0, 30.0).with_temperature(16000.0);
            u.add(lamp);
        }
    }
    let base = match p.center {
        CenterKind::Binary => 420.0, // circumbinary orbits need > ~3.5x the binary separation
        CenterKind::BlueGiant => 300.0,
        CenterKind::RedDwarf => 110.0,
        CenterKind::BlackHole => 250.0,
        _ => 160.0,
    };
    // Previous planet's (distance, mass); the first planet sits at `base`.
    let mut prev: Option<(f64, f64)> = None;
    let mut next_min = base * p.spacing;
    let belt_slot = if p.belt && p.planets > 2 { Some(p.planets / 2) } else { None };
    for k in 0..p.planets {
        if Some(k) == belt_slot && central_mass > 0.0 {
            let inner = prev.map(|(d, _)| d * 1.35).unwrap_or(next_min).max(next_min);
            let outer = inner * 1.3;
            asteroid_belt(u, DVec3::ZERO, DVec3::ZERO, central_mass, inner, outer, 2000, Vec3::new(0.55, 0.5, 0.45));
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
        let (m0, r0) = kind.defaults();
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
