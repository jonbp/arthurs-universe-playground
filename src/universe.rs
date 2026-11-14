//! The physics sandbox: N-body gravity, collisions (merging / shattering),
//! tidal disruption by black holes, supernovae, particles and visual effects.

use glam::{DVec3, Vec3};

use crate::body::{black_hole_radius, Body, BodyKind, Surface};
use crate::paint::{Brush, PaintStore};
use crate::util::{any_perpendicular, Rng};

pub const P_GRAVITY: u8 = 1;
pub const P_HOT: u8 = 2;
pub const P_DRAG: u8 = 4;
pub const P_IMMORTAL: u8 = 8;
pub const P_GLOW: u8 = 16;

#[derive(Clone, Copy)]
pub struct Particle {
    pub pos: Vec3,
    pub vel: Vec3,
    pub color: Vec3,
    pub size: f32,
    pub life: f32,
    pub max_life: f32,
    pub flags: u8,
    /// Mass this speck carries (rubble from a smash, a shredded world). A body that
    /// swallows it gains the mass. Zero for pure decoration and for galaxy stars.
    pub mass: f32,
}

/// Something worth hearing. The UI layer drains these each frame and plays them,
/// louder the bigger they are and the closer they are to the camera.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sound {
    /// A small body falls into a bigger one.
    Thud,
    /// Worlds smash each other to pieces.
    Smash,
    /// Two stars merge.
    StarMerge,
    /// Two black holes merge.
    BlackHoleMerge,
    Supernova,
    /// The rocket is wrecked.
    Crash,
    LiftOff,
    Land,
    /// A new body appears.
    Pop,
    /// A projectile is hurled.
    Whoosh,
}

#[derive(Clone, Copy, Debug)]
pub struct SoundEvent {
    pub sound: Sound,
    /// Where it happened (None: right here, e.g. your own rocket).
    pub pos: Option<DVec3>,
    /// Rough size in world units: bigger events carry further.
    pub size: f32,
}

/// Pending sounds are capped so a universe that is never listened to (tests,
/// screenshots) doesn't grow the queue without bound.
const MAX_PENDING_SOUNDS: usize = 64;

/// A transient point light + glow (impacts, explosions).
#[derive(Clone)]
pub struct Flash {
    pub pos: DVec3,
    pub color: Vec3,
    pub intensity: f32,
    pub decay: f32,
    pub size: f32,
}

/// Expanding supernova blast front.
#[derive(Clone)]
pub struct Shockwave {
    pub center: DVec3,
    pub radius: f64,
    pub speed: f64,
    pub max_radius: f64,
    pub ejected_mass: f64,
    pub hit: Vec<u64>,
}

/// The player's rocket: a massless craft pulled by every body and pushed by its engine.
/// It is integrated inside the physics step so it stays in sync at any time scale.
#[derive(Clone, Debug)]
pub struct Ship {
    pub pos: DVec3,
    pub vel: DVec3,
    pub acc: DVec3,
    /// Engine acceleration (already scaled by throttle), world space.
    pub thrust: DVec3,
    /// Distance from the ship's centre to its feet.
    pub radius: f64,
    /// Set when the ship touches a body; it then stops moving until the game reacts.
    pub hit: Option<ShipHit>,
}

#[derive(Clone, Copy, Debug)]
pub struct ShipHit {
    pub body: u64,
    /// Speed relative to the (spinning) ground at the moment of contact.
    pub speed: f64,
    /// Unit vector from the body's centre to the contact point.
    pub normal: DVec3,
}

impl Ship {
    pub fn new(pos: DVec3, vel: DVec3, radius: f64) -> Ship {
        Ship { pos, vel, acc: DVec3::ZERO, thrust: DVec3::ZERO, radius, hit: None }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CollisionMode {
    Realistic,
    AlwaysMerge,
    AlwaysShatter,
    PassThrough,
}

impl CollisionMode {
    pub const ALL: [CollisionMode; 4] =
        [CollisionMode::Realistic, CollisionMode::AlwaysMerge, CollisionMode::AlwaysShatter, CollisionMode::PassThrough];
    pub fn label(self) -> &'static str {
        match self {
            CollisionMode::Realistic => "Realistic",
            CollisionMode::AlwaysMerge => "Always merge",
            CollisionMode::AlwaysShatter => "Always shatter",
            CollisionMode::PassThrough => "Ghost (no collisions)",
        }
    }
}

/// Compact copy of what the gravity inner loop needs, so it never touches the
/// (large) `Body` structs.
#[derive(Clone, Copy)]
struct Probe {
    pos: DVec3,
    gm: f64,
    radius: f64,
}

/// Pieces of a shattered world smaller than this are dust (particles), not bodies.
/// Without the cut-off, rubble kept smashing into ever more rubble of the same
/// size, until one collision had filled the whole universe with weightless rocks.
const MIN_FRAGMENT_RADIUS: f64 = 0.12;

/// The most body-on-body pulls ever worked out in one frame, and the fewest the
/// budget is cut to on a slow computer.
pub const MAX_PAIR_BUDGET: usize = 10_000_000;
pub const MIN_PAIR_BUDGET: usize = 400_000;

/// A world smaller than this is not torn in two by a glancing hit: it hits or it misses.
const GRAZE_MIN_RADIUS: f64 = 0.5;
/// A glancing hit only tears a world in two if at least this much of it would sail past.
const GRAZE_MIN_MISS: f64 = 0.15;
/// The part that sails past is slowed by being torn free: it keeps this much of its speed...
const GRAZE_SPEED_KEPT: f64 = 0.7;
/// ...and sheds this much of its mass as loose pieces, thrown outwards at up to
/// about a third of this fraction of the speed of the hit.
const GRAZE_SHED: f64 = 0.4;
const GRAZE_SPRAY: f64 = 0.5;
/// What is left of a world torn in two is called its remnant.
const REMNANT: &str = " remnant";

/// Particles are integrated on several threads above this many per thread.
const PARTICLES_PER_THREAD: usize = 8_192;
const MAX_ATTRACTORS: usize = 16;

#[derive(Clone, Copy)]
enum Contact {
    Touch(usize, usize),
    Tidal { bh: usize, victim: usize },
}

pub struct Universe {
    pub name: String,
    pub bodies: Vec<Body>,
    pub particles: Vec<Particle>,
    pub flashes: Vec<Flash>,
    pub shockwaves: Vec<Shockwave>,
    pub time: f64,
    pub g: f64,
    pub collisions: CollisionMode,
    pub tidal_disruption: bool,
    pub base_dt: f64,
    pub max_particles: usize,
    pub max_bodies: usize,
    /// How many body-on-body pulls may be worked out in one frame. The game tunes
    /// this to the speed of the computer (see `Game::tune_physics`).
    pub pair_budget: usize,
    /// Pulls worked out by the last `update`, and how long its steps took: what the tuning goes by.
    pub step_work: usize,
    pub step_seconds: f64,
    pub trail_len: usize,
    pub trail_interval: f64,
    pub paint: PaintStore,
    pub rng: Rng,
    pub log: Vec<String>,
    /// The rocket being flown, if any (see `explore.rs`).
    pub ship: Option<Ship>,
    /// Sounds since the UI last took them (see [`Sound`]).
    pub sounds: Vec<SoundEvent>,
    next_id: u64,
    /// Simulated time since the populations were last brought up to date.
    population_clock: f64,
    contacts: Vec<Contact>,
    scratch_probes: Vec<Probe>,
    scratch_acc: Vec<DVec3>,
}

impl Universe {
    pub fn new(name: &str, seed: u64) -> Self {
        Universe {
            name: name.to_string(),
            bodies: Vec::new(),
            particles: Vec::new(),
            flashes: Vec::new(),
            shockwaves: Vec::new(),
            time: 0.0,
            g: 1.0,
            collisions: CollisionMode::Realistic,
            tidal_disruption: true,
            base_dt: 0.004,
            max_particles: 150_000,
            max_bodies: 400,
            pair_budget: MAX_PAIR_BUDGET,
            step_work: 0,
            step_seconds: 0.0,
            trail_len: 600,
            trail_interval: 1.0,
            paint: PaintStore::new(),
            rng: Rng::new(seed),
            log: Vec::new(),
            ship: None,
            sounds: Vec::new(),
            next_id: 1,
            population_clock: 0.0,
            contacts: Vec::new(),
            scratch_probes: Vec::new(),
            scratch_acc: Vec::new(),
        }
    }

    /// Clear everything but keep settings and the paint store allocation.
    pub fn reset(&mut self, name: &str) {
        self.name = name.to_string();
        self.bodies.clear();
        self.particles.clear();
        self.flashes.clear();
        self.shockwaves.clear();
        self.time = 0.0;
        self.log.clear();
        self.ship = None;
        self.sounds.clear();
        self.contacts.clear();
        self.paint.reset_all();
    }

    pub fn new_body(&mut self, name: &str, kind: BodyKind, pos: DVec3, vel: DVec3) -> Body {
        let id = self.next_id;
        self.next_id += 1;
        Body::new(id, name, kind, pos, vel, &mut self.rng)
    }

    /// After adding bodies with ids of their own (loading a save): new ids start above them.
    pub fn sync_next_id(&mut self) {
        self.next_id = self.bodies.iter().map(|b| b.id + 1).max().unwrap_or(1).max(self.next_id);
    }

    pub fn add(&mut self, b: Body) -> u64 {
        let id = b.id;
        self.bodies.push(b);
        id
    }

    pub fn index_of(&self, id: u64) -> Option<usize> {
        self.bodies.iter().position(|b| b.id == id && b.alive)
    }

    pub fn get(&self, id: u64) -> Option<&Body> {
        self.bodies.iter().find(|b| b.id == id && b.alive)
    }

    pub fn get_mut(&mut self, id: u64) -> Option<&mut Body> {
        self.bodies.iter_mut().find(|b| b.id == id && b.alive)
    }

    pub fn sound(&mut self, sound: Sound, pos: Option<DVec3>, size: f32) {
        if self.sounds.len() < MAX_PENDING_SOUNDS {
            self.sounds.push(SoundEvent { sound, pos, size });
        }
    }

    pub fn event(&mut self, msg: impl Into<String>) {
        let t = self.time;
        self.log.push(format!("[t={:>6.1}] {}", t, msg.into()));
        if self.log.len() > 60 {
            self.log.remove(0);
        }
    }

    /// The body exerting the strongest pull at `pos` (excluding `exclude`).
    pub fn dominant_attractor(&self, pos: DVec3, exclude: Option<u64>) -> Option<usize> {
        let mut best = None;
        let mut best_a = 0.0;
        for (i, b) in self.bodies.iter().enumerate() {
            if Some(b.id) == exclude || !b.alive {
                continue;
            }
            let d2 = (b.pos - pos).length_squared().max(1e-6);
            let a = b.mass / d2;
            if a > best_a {
                best_a = a;
                best = Some(i);
            }
        }
        best
    }

    /// The body whose neighbourhood `pos` is in: the one with the strongest tidal grip
    /// (mass / distance³), which matches the Hill sphere. Unlike the raw pull used by
    /// [`dominant_attractor`](Self::dominant_attractor), this picks Earth, not the Sun,
    /// for a rocket a few planet-radii above Earth.
    pub fn reference_body(&self, pos: DVec3) -> Option<usize> {
        let mut best = None;
        let mut best_grip = 0.0;
        for (i, b) in self.bodies.iter().enumerate() {
            if !b.alive {
                continue;
            }
            let d = (b.pos - pos).length().max(1e-3);
            let grip = b.mass / (d * d * d);
            if grip > best_grip {
                best_grip = grip;
                best = Some(i);
            }
        }
        best
    }

    /// Velocity for a circular, prograde orbit at `pos` around body `idx`.
    pub fn circular_velocity(&self, pos: DVec3, idx: usize) -> DVec3 {
        let c = &self.bodies[idx];
        let r = pos - c.pos;
        let dist = r.length().max(1e-3);
        let up = DVec3::Y;
        let mut dir = up.cross(r);
        if dir.length_squared() < 1e-9 {
            dir = any_perpendicular(r);
        }
        let speed = (self.g * c.mass / dist).sqrt();
        c.vel + dir.normalize() * speed
    }

    // ------------------------------------------------------------------
    // Simulation
    // ------------------------------------------------------------------

    /// Advance by `real_dt` seconds of wall time at `time_scale`.
    pub fn update(&mut self, real_dt: f64, time_scale: f64, paused: bool) {
        let real_dt = real_dt.min(0.1);
        self.update_visual_decay(real_dt as f32);
        if paused || time_scale <= 0.0 {
            return;
        }
        let total = real_dt * time_scale;
        let n = self.bodies.len().max(2);
        let pairs = n * (n - 1) / 2;
        // Crowded universes get fewer, longer steps (and past the longest step, time
        // simply runs slower than asked) so that a frame never takes too long.
        let max_steps = (self.pair_budget / pairs).clamp(4, 3000);
        let steps = ((total / self.base_dt).ceil() as usize).clamp(1, max_steps);
        let dt = (total / steps as f64).min(0.05);
        let started = std::time::Instant::now();
        for _ in 0..steps {
            self.step(dt);
        }
        self.step_work = steps * pairs;
        self.step_seconds = started.elapsed().as_secs_f64();
        let simulated = dt * steps as f64;
        self.time += simulated;
        for b in &mut self.bodies {
            b.spin = (b.spin + b.spin_rate * simulated) % std::f64::consts::TAU;
            b.heat = (b.heat - simulated as f32 / 40.0).max(0.0);
            if b.kind == BodyKind::BlackHole {
                b.feeding = (b.feeding - simulated as f32 * 0.03).max(0.35);
            }
        }
        self.update_particles(simulated as f32);
        self.update_supernovae(simulated);
        self.update_shockwaves(simulated);
        self.name_new_moons();
        self.population_clock += simulated;
        if self.population_clock >= crate::population::INTERVAL {
            let dt = std::mem::take(&mut self.population_clock);
            crate::population::update(self, dt);
        }
    }

    /// The people of body `idx` leave it, safely, before something terrible happens to it.
    fn evacuate(&mut self, idx: usize) {
        let b = &mut self.bodies[idx];
        let people = std::mem::take(&mut b.population);
        if people >= 1.0 && !b.name.is_empty() {
            let name = b.name.clone();
            self.event(format!("🚀 The {} people of {name} escaped in their rockets just in time", crate::population::fmt_people(people)));
        }
    }

    /// A label, not physics: when what is left of a torn or shattered world turns
    /// out to be circling a much bigger one, on an orbit that clears the ground and
    /// that nothing heavier will pull it out of, call it what it now is: a moon.
    fn name_new_moons(&mut self) {
        for i in 0..self.bodies.len() {
            let m = &self.bodies[i];
            if !is_leftover(m) || !m.alive || m.kind.is_stellar() || m.ghost > 0.0 || m.radius < 0.3 {
                continue;
            }
            // The world it belongs to: the strongest grip among those far heavier than it.
            let grip = |o: &Body| o.mass / (o.pos - m.pos).length().powi(3).max(1e-9);
            let Some(h) = self.bodies.iter().filter(|o| o.alive && !o.kind.is_stellar() && !is_leftover(o) && o.mass > m.mass * 10.0).max_by(|a, b| grip(a).total_cmp(&grip(b))) else {
                continue;
            };
            let (r, v) = (m.pos - h.pos, m.vel - h.vel);
            let gm = self.g * (h.mass + m.mass);
            let energy = v.length_squared() / 2.0 - gm / r.length();
            if energy >= 0.0 {
                continue;
            }
            let half_width = -gm / (2.0 * energy);
            let oval = (1.0 - r.cross(v).length_squared() / (gm * half_width)).max(0.0).sqrt();
            let (lowest, highest) = (half_width * (1.0 - oval), half_width * (1.0 + oval));
            let hill = self
                .bodies
                .iter()
                .filter(|o| o.alive && o.mass > h.mass)
                .map(|o| (o.pos - h.pos).length() * (h.mass / (3.0 * o.mass)).cbrt())
                .fold(f64::INFINITY, f64::min);
            if lowest < h.radius * 1.1 + m.radius || highest > hill * 0.5 {
                continue;
            }
            // The biggest piece gets the name: wait if a heavier leftover is still circling.
            let bound = |o: &Body| (o.vel - h.vel).length_squared() / 2.0 < self.g * (h.mass + o.mass) / (o.pos - h.pos).length().max(1e-9);
            let heavier = |o: &Body| o.alive && o.id != h.id && o.mass > m.mass && is_leftover(o) && bound(o);
            if self.bodies.iter().any(heavier) {
                continue;
            }
            let name = if h.name.contains("Earth") && !self.bodies.iter().any(|o| o.name == "Moon") { "Moon".to_string() } else { format!("Moon of {}", h.name) };
            if self.bodies.iter().any(|o| o.name == name) {
                continue;
            }
            let was = if m.name.is_empty() { "A leftover piece".to_string() } else { m.name.clone() };
            let message = format!("🌙 {was} has settled into orbit around {}: it is a moon now!", h.name);
            self.bodies[i].name = name;
            self.event(message);
        }
    }

    fn update_visual_decay(&mut self, dt: f32) {
        for b in &mut self.bodies {
            b.flash *= (-dt * 2.5).exp();
            if b.flash < 0.01 {
                b.flash = 0.0;
            }
        }
        for f in &mut self.flashes {
            f.intensity *= (-dt * f.decay).exp();
        }
        self.flashes.retain(|f| f.intensity > 0.02);
    }

    fn step(&mut self, dt: f64) {
        for b in &mut self.bodies {
            b.vel += b.acc * (0.5 * dt);
            b.pos += b.vel * dt;
        }
        if let Some(s) = self.ship.as_mut().filter(|s| s.hit.is_none()) {
            s.vel += s.acc * (0.5 * dt);
            s.pos += s.vel * dt;
        }
        self.compute_accelerations();
        self.step_ship(dt);
        let trail_scale = self.trail_interval;
        let trail_len = self.trail_len;
        for b in &mut self.bodies {
            b.vel += b.acc * (0.5 * dt);
            b.ghost = (b.ghost - dt).max(0.0);
            b.trail_accum += dt;
            // Sample trails relative to each body's own orbital timescale (v/|a| ~ T/2pi),
            // so Mercury and Neptune both draw ~2 full orbits.
            let timescale = b.vel.length() / b.acc.length().max(1e-9);
            let interval = (timescale * std::f64::consts::TAU / 250.0).clamp(0.02, 60.0) * trail_scale;
            if b.trail_accum >= interval {
                b.trail_accum = 0.0;
                b.trail.push_back(b.pos);
                while b.trail.len() > trail_len {
                    b.trail.pop_front();
                }
            }
        }
        if !self.contacts.is_empty() {
            let contacts = std::mem::take(&mut self.contacts);
            for c in &contacts {
                self.resolve_contact(*c);
            }
            self.contacts = contacts;
            self.contacts.clear();
            self.remove_dead();
            // New fragments need an initial acceleration.
            self.compute_accelerations();
            self.contacts.clear();
        }
    }

    /// Gravitational acceleration on a massless point at `pos`.
    pub fn gravity_at(&self, pos: DVec3) -> DVec3 {
        let mut a = DVec3::ZERO;
        for b in &self.bodies {
            let d = b.pos - pos;
            let s = d.length_squared() + 0.0025;
            a += d * (b.mass * self.g / (s * s.sqrt()));
        }
        a
    }

    /// Velocity of the ground of body `b` at world point `at` (orbital motion plus spin).
    pub fn ground_velocity(b: &Body, at: DVec3) -> DVec3 {
        b.vel + (b.axis().as_dvec3() * b.spin_rate).cross(at - b.pos)
    }

    /// Second half of the ship's leapfrog step, then a ground-contact test.
    fn step_ship(&mut self, dt: f64) {
        let Some(mut s) = self.ship.take() else { return };
        if s.hit.is_none() {
            s.acc = self.gravity_at(s.pos) + s.thrust;
            s.vel += s.acc * (0.5 * dt);
            for b in &self.bodies {
                let d = s.pos - b.pos;
                let dist = d.length();
                // Cheap bounding test first; the exact terrain height only when close.
                if dist > b.radius * (1.0 + crate::terrain::max_displacement(b)) + s.radius {
                    continue;
                }
                let normal = d.try_normalize().unwrap_or(DVec3::Y);
                let ground = if crate::terrain::has_solid_surface(b.kind) {
                    crate::terrain::surface_radius_world(b, &self.paint, normal)
                } else {
                    b.radius
                };
                if dist < ground + s.radius {
                    s.pos = b.pos + normal * (ground + s.radius);
                    let speed = (s.vel - Self::ground_velocity(b, s.pos)).length();
                    s.hit = Some(ShipHit { body: b.id, speed, normal });
                    break;
                }
            }
        }
        self.ship = Some(s);
    }

    fn compute_accelerations(&mut self) {
        let n = self.bodies.len();
        let g = self.g;
        self.scratch_probes.clear();
        self.scratch_probes.extend(self.bodies.iter().map(|b| Probe { pos: b.pos, gm: b.mass * g, radius: b.radius }));
        self.scratch_acc.clear();
        self.scratch_acc.resize(n, DVec3::ZERO);
        let tidal = self.tidal_disruption && self.bodies.iter().any(|b| b.kind == BodyKind::BlackHole);
        let bodies = &self.bodies;
        let contacts = &mut self.contacts;
        let pass_through = self.collisions == CollisionMode::PassThrough;
        let probes = &self.scratch_probes[..];
        for i in 0..n {
            let pi = probes[i];
            let (head, tail) = self.scratch_acc.split_at_mut(i + 1);
            let mut ai = DVec3::ZERO;
            for (k, (pj, aj)) in probes[i + 1..].iter().zip(tail.iter_mut()).enumerate() {
                let d = pj.pos - pi.pos;
                let d2 = d.length_squared();
                let s = d2 + 0.0025;
                let inv = 1.0 / (s * s.sqrt());
                ai += d * (pj.gm * inv);
                *aj -= d * (pi.gm * inv);
                // Everything below is the rare path: overlapping bodies or a black hole nearby.
                let rsum = pi.radius + pj.radius;
                if d2 < rsum * rsum {
                    let j = i + 1 + k;
                    let (bi, bj) = (&bodies[i], &bodies[j]);
                    // Fresh fragments pass through other worlds for a moment, and in ghost
                    // mode all worlds do; stars and black holes always swallow.
                    let worlds = !bi.kind.is_stellar() && !bj.kind.is_stellar();
                    let ghosted = (pass_through || bi.ghost > 0.0 || bj.ghost > 0.0) && worlds;
                    // Two worlds whose edges overlap but which are already moving apart
                    // have had their collision: they meet again only if they come back.
                    // (Not when one is deep inside the other: that is a hit, however it got there.)
                    let shallow = d2 > (0.9 * pi.radius.max(pj.radius)).powi(2);
                    let parting = worlds && shallow && d.dot(bj.vel - bi.vel) > 0.0;
                    if !ghosted && !parting {
                        contacts.push(Contact::Touch(i, j));
                    }
                } else if tidal {
                    let j = i + 1 + k;
                    let (ki, kj) = (bodies[i].kind, bodies[j].kind);
                    let (bh, victim) = if ki == BodyKind::BlackHole && kj != BodyKind::BlackHole {
                        (i, j)
                    } else if kj == BodyKind::BlackHole && ki != BodyKind::BlackHole {
                        (j, i)
                    } else {
                        continue;
                    };
                    let (mb, rb) = (bodies[bh].mass, bodies[bh].radius);
                    let v = &bodies[victim];
                    if v.kind == BodyKind::NeutronStar {
                        continue;
                    }
                    let rt = v.radius * (2.0 * mb / v.mass.max(1e-6)).cbrt();
                    if rt > rb * 1.3 && d2 < rt * rt {
                        contacts.push(Contact::Tidal { bh, victim });
                    }
                }
            }
            head[i] += ai;
        }
        for (b, a) in self.bodies.iter_mut().zip(self.scratch_acc.iter()) {
            b.acc = *a;
        }
    }

    fn remove_dead(&mut self) {
        let mut i = 0;
        while i < self.bodies.len() {
            if !self.bodies[i].alive {
                if let Some(l) = self.bodies[i].paint_layer {
                    self.paint.release(l);
                }
                self.bodies.swap_remove(i);
            } else {
                i += 1;
            }
        }
    }

    fn resolve_contact(&mut self, c: Contact) {
        match c {
            Contact::Touch(i, j) => {
                if i >= self.bodies.len() || j >= self.bodies.len() {
                    return;
                }
                if !self.bodies[i].alive || !self.bodies[j].alive {
                    return;
                }
                self.collide(i, j);
            }
            Contact::Tidal { bh, victim } => {
                if bh >= self.bodies.len() || victim >= self.bodies.len() {
                    return;
                }
                if !self.bodies[bh].alive || !self.bodies[victim].alive {
                    return;
                }
                self.spaghettify(bh, victim);
            }
        }
    }

    fn collide(&mut self, i: usize, j: usize) {
        let (ki, kj) = (self.bodies[i].kind, self.bodies[j].kind);
        // Black holes eat everything.
        if ki == BodyKind::BlackHole || kj == BodyKind::BlackHole {
            let (bh, other) = if ki == BodyKind::BlackHole && (kj != BodyKind::BlackHole || self.bodies[i].mass >= self.bodies[j].mass) {
                (i, j)
            } else {
                (j, i)
            };
            if self.bodies[other].kind == BodyKind::BlackHole {
                self.merge_black_holes(bh, other);
            } else {
                self.absorb(bh, other, true);
            }
            return;
        }
        let (big, small) = if self.bodies[i].mass >= self.bodies[j].mass { (i, j) } else { (j, i) };
        if self.bodies[big].kind.is_stellar() || self.bodies[small].kind.is_stellar() {
            // Stars swallow planets; star-star collisions merge with a big flash.
            let star = if self.bodies[big].kind.is_stellar() { big } else { small };
            let other = if star == big { small } else { big };
            if self.bodies[other].kind.is_stellar() {
                self.stellar_merger(big, small);
            } else {
                self.absorb(star, other, true);
            }
            return;
        }
        if self.collisions == CollisionMode::PassThrough {
            return;
        }
        // A glancing hit: only the part of the smaller world that is actually in the
        // way takes part in what follows. The rest carries on past.
        let small = if self.collisions == CollisionMode::Realistic { self.graze(big, small) } else { small };

        let (bb, sb) = (&self.bodies[big], &self.bodies[small]);
        let rel_v = (sb.vel - bb.vel).length();
        let v_esc = (2.0 * self.g * (bb.mass + sb.mass) / (bb.radius + sb.radius)).sqrt().max(1e-6);
        let energy = rel_v / v_esc;
        let ratio = sb.mass / bb.mass;
        // Specific impact energy relative to the catastrophic-disruption threshold
        // (after Leinhardt & Stewart 2012): Q = 0.5 * mu * v^2 / M_tot, Q*_RD ~ k * v_esc^2.
        let m_tot = bb.mass + sb.mass;
        let mu = bb.mass * sb.mass / m_tot;
        let q = 0.5 * mu * rel_v * rel_v / m_tot;
        let q_ratio = q / (0.6 * v_esc * v_esc);
        let merge = match self.collisions {
            CollisionMode::AlwaysMerge => true,
            CollisionMode::AlwaysShatter => false,
            _ => q_ratio < 0.3 || (ratio < 0.02 && energy < 3.0),
        };
        // Pebbles just make a crater rather than shattering into fragments.
        let tiny = sb.radius < 0.25 && ratio < 0.05;
        if merge || tiny {
            self.planet_merge(big, small, energy);
        } else {
            self.planet_shatter(big, small, energy, ratio, q_ratio);
        }
    }

    /// Work out how much of `small` is really in the way of `big`, by following
    /// points spread through it along the direction it is travelling: those whose
    /// path runs into `big` hit it, those whose path clears it sail past.
    ///
    /// If a fair share sails past, `small` is torn in two. It lives on as the part
    /// that missed (slowed by the tearing, and shedding some loose pieces), and a new
    /// body made of the part that hit is returned to take its place in the collision.
    /// Nothing here decides where anything ends up: gravity does that afterwards.
    /// Fast and it flies off, slow and it falls back for another go, and at about
    /// the speed and angle of the real Theia some of it stays in orbit.
    fn graze(&mut self, big: usize, small: usize) -> usize {
        let (b, s) = (self.bodies[big].clone(), self.bodies[small].clone());
        let approach = s.vel - b.vel;
        let Some(dir) = approach.try_normalize() else { return small };
        if s.radius < GRAZE_MIN_RADIUS || self.bodies.len() >= self.max_bodies {
            return small;
        }
        const SAMPLES: usize = 240;
        let (mut hit, mut miss) = (DVec3::ZERO, DVec3::ZERO);
        let mut missed = 0;
        for _ in 0..SAMPLES {
            let p = s.pos + self.rng.unit() * s.radius * self.rng.f64().cbrt();
            let from_centre = p - b.pos;
            // How far to the side of `big`'s centre this point's path passes.
            let aside = (from_centre - dir * from_centre.dot(dir)).length();
            if aside > b.radius {
                miss += p;
                missed += 1;
            } else {
                hit += p;
            }
        }
        let share = missed as f64 / SAMPLES as f64;
        if share < GRAZE_MIN_MISS || missed == SAMPLES {
            return small;
        }
        // It is about to be torn in two: time to go.
        self.evacuate(small);
        let s = Body { population: 0.0, ..s };
        let (hit_at, miss_at) = (hit / (SAMPLES - missed) as f64, miss / missed as f64);
        let miss_mass = s.mass * share;
        let shed = miss_mass * GRAZE_SHED;

        // The part that hit: a new body, right where that part of `small` was.
        let mut part = self.new_body("", s.kind, hit_at, s.vel);
        part.mass = s.mass - miss_mass;
        part.radius = s.radius * (part.mass / s.mass).cbrt();
        part.surface = s.surface.clone();
        part.temperature = s.temperature;
        let part_idx = self.bodies.len();
        self.bodies.push(part);

        // The part that missed carries on as `small` itself, slower than it came.
        let onward = b.vel + approach * GRAZE_SPEED_KEPT;
        let runner = &mut self.bodies[small];
        runner.mass = miss_mass - shed;
        runner.radius = s.radius * (runner.mass / s.mass).cbrt();
        runner.pos = miss_at;
        runner.vel = onward;
        runner.heat = 1.0;
        runner.flash = 1.0;
        // It brushes on past without being counted as hitting again, until its
        // closest approach; after that the two are parting.
        let to_closest = -(miss_at - b.pos).dot(approach) / approach.length_squared();
        runner.ghost = (to_closest * 2.0 + 0.3).clamp(0.5, 4.0);
        let runner = runner.clone();
        // What it lost in speed was spent dragging on `big`.
        self.bodies[big].vel += (s.vel - onward) * (miss_mass / b.mass);
        // Loose pieces come off its outer side.
        let out = (miss_at - b.pos).try_normalize().unwrap_or(dir);
        let outward = (out - dir * out.dot(dir)).try_normalize().unwrap_or(out);
        let count = 3 + (s.radius * 2.0) as usize;
        let first_piece = self.bodies.len();
        self.spawn_fragments(&s, shed, count.min(8), miss_at + outward * runner.radius * 0.5, onward, outward, approach.length() * GRAZE_SPRAY, &b);
        // Throwing them off pushes the remnant back the other way, and whatever
        // crumbled to dust rather than pieces stays part of it.
        let pieces = &self.bodies[first_piece..];
        let thrown: f64 = pieces.iter().map(|f| f.mass).sum();
        let kick: DVec3 = pieces.iter().map(|f| (f.vel - onward) * f.mass).sum();
        let runner = &mut self.bodies[small];
        runner.mass += shed - thrown;
        runner.radius = s.radius * (runner.mass / s.mass).cbrt();
        runner.vel -= kick / runner.mass;
        // A whole world that is torn becomes its remnant; rubble stays rubble.
        let whole = !s.name.is_empty() && !is_leftover(&s);
        if whole {
            runner.name = format!("{}{REMNANT}", s.name);
        }
        if whole && !b.name.is_empty() {
            self.event(format!("💫 {} struck {} a glancing blow and was torn in two", s.name, b.name));
        }
        part_idx
    }

    fn impact_info(&self, big: usize, small: usize) -> (DVec3, DVec3) {
        let (b, s) = (&self.bodies[big], &self.bodies[small]);
        let n = (s.pos - b.pos).try_normalize().unwrap_or(DVec3::Y);
        (b.pos + n * b.radius, n)
    }

    fn ensure_layer(&mut self, idx: usize) -> Option<u32> {
        if !self.bodies[idx].kind.paintable() {
            return None;
        }
        if let Some(l) = self.bodies[idx].paint_layer {
            return Some(l);
        }
        let l = self.paint.alloc()?;
        self.bodies[idx].paint_layer = Some(l);
        Some(l)
    }

    /// Paint a crater (and optional lava) onto `idx` where direction `n` (world) hits.
    pub fn crater(&mut self, idx: usize, n: DVec3, angular_radius: f32, lava: f32) {
        if let Some(layer) = self.ensure_layer(idx) {
            let local = self.bodies[idx].rotation().inverse() * n.as_vec3();
            self.paint.apply(layer, local, angular_radius, Brush::Crater, 1.0);
            self.paint.apply(layer, local, angular_radius * 2.0, Brush::Scorch, 0.4);
            if lava > 0.0 {
                self.paint.apply(layer, local, angular_radius * 0.8, Brush::Lava, lava);
            }
        }
    }

    /// `big` swallows `small`. The result keeps their combined momentum and sits at
    /// their shared centre of mass, so a merger never jolts the rest of the system.
    fn absorb(&mut self, big: usize, small: usize, splash: bool) {
        self.evacuate(small);
        let s = self.bodies[small].clone();
        let (impact, n) = self.impact_info(big, small);
        let b = &mut self.bodies[big];
        let m = b.mass + s.mass;
        b.vel = (b.vel * b.mass + s.vel * s.mass) / m;
        b.pos = (b.pos * b.mass + s.pos * s.mass) / m;
        let old_mass = b.mass;
        b.mass = m;
        match b.kind {
            BodyKind::BlackHole => {
                b.radius = black_hole_radius(m);
                b.feeding = (b.feeding + 1.5).min(4.0);
            }
            BodyKind::Star | BodyKind::NeutronStar => {
                b.radius *= (m / old_mass).cbrt();
                b.flash = (b.flash + 0.6).min(2.0);
            }
            _ => {
                b.radius = (b.radius.powi(3) + s.radius.powi(3)).cbrt();
            }
        }
        // Growing protoplanets earn a name.
        if b.name.is_empty() && !b.kind.is_stellar() && b.mass > 12.0 {
            b.name = format!("Protoplanet {}", b.id);
        }
        let big_name = b.name.clone();
        let big_kind = b.kind;
        let big_vel = b.vel;
        self.bodies[small].alive = false;
        let verb = match big_kind {
            BodyKind::BlackHole => "was swallowed by",
            BodyKind::Star | BodyKind::NeutronStar => "plunged into",
            _ => "merged into",
        };
        if !s.name.is_empty() && !big_name.is_empty() {
            self.event(format!("{} {} {}", s.name, verb, big_name));
        }
        if splash {
            let rel = (s.vel - big_vel).length();
            let count = (300.0 + s.radius * 400.0).min(3000.0) as usize;
            let color = s.debris_color();
            let hot = big_kind == BodyKind::Star;
            for _ in 0..count {
                let dir = (n + self.rng.unit() * 0.9).normalize();
                let speed = rel * self.rng.range(0.05, 0.35) + self.rng.range(0.5, 3.0);
                let c = if hot || self.rng.f32() < 0.4 { Vec3::new(1.0, 0.6, 0.25) * 2.0 } else { color };
                self.emit(
                    impact + self.rng.unit() * s.radius * 0.5,
                    big_vel + dir * speed,
                    c,
                    (s.radius as f32 * 0.12).clamp(0.05, 0.8),
                    self.rng.rangef(1.5, 5.0),
                    P_GRAVITY | P_HOT,
                );
            }
            self.sound(Sound::Thud, Some(impact), s.radius as f32);
            self.flashes.push(Flash {
                pos: impact,
                color: Vec3::new(1.0, 0.75, 0.45),
                intensity: (s.radius as f32 * 0.8).clamp(0.3, 4.0),
                decay: 1.5,
                size: (s.radius * 6.0) as f32,
            });
        }
    }

    fn planet_merge(&mut self, big: usize, small: usize, energy: f64) {
        let (_, n) = self.impact_info(big, small);
        let ratio = (self.bodies[small].radius / self.bodies[big].radius) as f32;
        let mass_ratio = (self.bodies[small].mass / self.bodies[big].mass) as f32;
        self.absorb(big, small, true);
        self.bodies[big].heat = (self.bodies[big].heat + mass_ratio * 4.0 * energy as f32).min(1.0);
        self.bodies[big].flash = (self.bodies[big].flash + ratio).min(1.5);
        self.crater(big, n, (ratio * 1.1).clamp(0.05, 1.3), (energy as f32 * ratio * 2.0).min(1.0));
    }

    fn planet_shatter(&mut self, big: usize, small: usize, energy: f64, ratio: f64, q_ratio: f64) {
        self.evacuate(small);
        let (impact, n) = self.impact_info(big, small);
        let b = self.bodies[big].clone();
        let s = self.bodies[small].clone();
        let m_tot = b.mass + s.mass;
        let com_vel = (b.vel * b.mass + s.vel * s.mass) / m_tot;
        let rel_v = (s.vel - b.vel).length();
        let first_fragment = self.bodies.len();
        if !s.name.is_empty() && !b.name.is_empty() {
            self.event(format!("💥 {} smashed into {} at speed {:.1}!", s.name, b.name, rel_v));
        }

        // Largest remnant keeps (1 - Q/2Q*) of the total mass; below ~35% the target is destroyed.
        let largest_remnant = (1.0 - 0.5 * q_ratio).clamp(0.0, 1.0);
        let destroyed = largest_remnant < 0.35 || (self.collisions == CollisionMode::AlwaysShatter && ratio > 0.2);
        let big_destroy = if destroyed { 1.0 } else { (1.0 - largest_remnant).clamp(0.0, 1.0) };

        // Small body always shatters.
        self.bodies[small].alive = false;
        let small_frags = 3 + (s.radius * 3.0) as usize;
        self.spawn_fragments(&s, s.mass * 0.85, small_frags.min(10), s.pos, com_vel, n, rel_v, &b);

        if destroyed {
            // Catastrophic disruption: big body breaks apart too.
            self.evacuate(big);
            self.bodies[big].alive = false;
            let frags = 5 + (b.radius * 2.0) as usize;
            self.spawn_fragments(&b, b.mass * 0.9, frags.min(14), b.pos, com_vel, -n, rel_v * 0.6, &s);
            if !b.name.is_empty() {
                self.event(format!("☠ {} was completely destroyed!", b.name));
            }
        } else {
            // Big body survives, damaged and molten.
            let ejected = ((1.0 - largest_remnant) * m_tot - s.mass * 0.85).clamp(0.0, b.mass * 0.4);
            let bm = &mut self.bodies[big];
            bm.mass = (bm.mass - ejected).max(bm.mass * 0.5) + s.mass * 0.1;
            bm.radius *= (bm.mass / b.mass).cbrt();
            bm.vel = com_vel;
            bm.heat = (bm.heat + 0.25 + big_destroy as f32 * 1.5 + energy as f32 * 0.05).min(1.0);
            bm.flash = 1.5;
            if ejected > 0.05 {
                let k = 2 + (big_destroy * 6.0) as usize;
                let bb = self.bodies[big].clone();
                self.spawn_fragments(&b, ejected, k, impact + n * b.radius * 0.2, com_vel, n, rel_v * 0.5, &bb);
            }
            let ang = ((s.radius / b.radius) as f32 * 1.4).clamp(0.12, 1.4);
            self.crater(big, n, ang, 1.0);
        }

        // Debris cloud and bright flash.
        // (Scaled down for rubble, so a pebble doesn't burst into a planet's worth of sparks.)
        let rubble = s.radius.min(1.0).powi(2);
        let count = ((1500.0 + (s.radius + b.radius * big_destroy) * 900.0) * rubble).min(9000.0) as usize;
        let dc1 = s.debris_color();
        let dc2 = b.debris_color();
        let cloud_start = self.particles.len();
        for k in 0..count {
            let dir = (n * 0.6 + self.rng.unit()).normalize();
            let speed = rel_v * self.rng.range(0.05, 0.7);
            let roll = self.rng.f32();
            let c = if roll < 0.35 {
                Vec3::new(1.0, 0.55, 0.2) * 2.5
            } else if k % 2 == 0 {
                dc1
            } else {
                dc2
            };
            self.emit(
                impact + self.rng.unit() * s.radius,
                com_vel + dir * speed,
                c,
                self.rng.rangef(0.04, 0.22) * (s.radius as f32).max(0.5),
                self.rng.rangef(3.0, 14.0),
                P_GRAVITY | if roll < 0.35 { P_HOT } else { 0 },
            );
        }
        // The pieces and the dust fly apart in random directions, which on its own would
        // hand the wreckage a sideways kick out of nowhere. Shift the pieces together so
        // that, dust included, it all carries on exactly as the two worlds were moving.
        let wreckage: Vec<usize> = (first_fragment..self.bodies.len()).chain(self.bodies[big].alive.then_some(big)).collect();
        let mass: f64 = wreckage.iter().map(|&i| self.bodies[i].mass).sum();
        // What is not in the pieces is in the dust cloud.
        self.give_mass_to_newest(self.particles.len() - cloud_start, (m_tot - mass).max(0.0));
        let cloud = &self.particles[cloud_start..];
        let cloud_mass: f64 = cloud.iter().map(|p| p.mass as f64).sum();
        let cloud_momentum: DVec3 = cloud.iter().map(|p| p.vel.as_dvec3() * p.mass as f64).sum();
        if mass > 0.0 {
            let momentum: DVec3 = wreckage.iter().map(|&i| self.bodies[i].vel * self.bodies[i].mass).sum();
            let shift = (com_vel * (mass + cloud_mass) - cloud_momentum - momentum) / mass;
            for i in wreckage {
                self.bodies[i].vel += shift;
            }
        }
        self.sound(Sound::Smash, Some(impact), (s.radius + b.radius) as f32);
        self.flashes.push(Flash {
            pos: impact,
            color: Vec3::new(1.0, 0.8, 0.55),
            intensity: (2.0 + (s.radius + b.radius) as f32).min(8.0),
            decay: 0.9,
            size: ((s.radius + b.radius) * 8.0) as f32,
        });
    }

    /// Break `src` up into fragments carrying `mass` total.
    #[allow(clippy::too_many_arguments)]
    fn spawn_fragments(&mut self, src: &Body, mass: f64, count: usize, center: DVec3, base_vel: DVec3, n: DVec3, speed: f64, avoid: &Body) {
        let room = self.max_bodies.saturating_sub(self.bodies.len());
        let count = count.min(room);
        if count == 0 {
            return;
        }
        let mut weights: Vec<f64> = (0..count).map(|_| self.rng.range(0.2, 1.0).powi(2)).collect();
        let total: f64 = weights.iter().sum();
        for w in &mut weights {
            *w /= total;
        }
        let density_r = src.radius / src.mass.max(1e-6).cbrt();
        for (k, w) in weights.iter().enumerate() {
            let m = mass * w;
            // Same density as the body it came from: the pieces never take up more room than it did.
            let r = density_r * m.cbrt();
            let dir = (n * 0.8 + self.rng.unit()).normalize();
            let mut pos = center + self.rng.unit() * src.radius * 0.7 + dir * r;
            if r < MIN_FRAGMENT_RADIUS {
                // Too small to be a world of its own: a puff of dust.
                let color = src.debris_color();
                for _ in 0..12 {
                    let v = base_vel + (dir + self.rng.unit() * 0.5) * speed * self.rng.range(0.15, 0.6);
                    self.emit(pos + self.rng.unit() * r, v, color, self.rng.rangef(0.03, 0.08), self.rng.rangef(3.0, 10.0), P_GRAVITY);
                }
                continue;
            }
            // keep fragments out of the surviving body
            let to = pos - avoid.pos;
            let min_d = avoid.radius + r * 1.05;
            if avoid.alive && to.length() < min_d {
                pos = avoid.pos + to.try_normalize().unwrap_or(n) * min_d;
            }
            let v = base_vel + dir * speed * self.rng.range(0.15, 0.6);
            let kind = if r > 0.7 && self.rng.f32() < 0.5 { BodyKind::Lava } else { BodyKind::Asteroid };
            let name = if is_leftover(src) || k > 1 {
                String::new()
            } else {
                format!("{} fragment {}", src.name, k + 1)
            };
            let mut f = self.new_body(&name, kind, pos, v);
            f.mass = m;
            f.radius = r;
            f.surface.colors[0] = (src.debris_color() * 0.7).min(Vec3::ONE);
            f.surface.colors[1] = src.surface.colors[1] * 0.6;
            f.heat = 0.9;
            f.ghost = 1.2;
            f.spin_rate = self.rng.range(-2.0, 2.0);
            self.bodies.push(f);
        }
    }

    fn stellar_merger(&mut self, big: usize, small: usize) {
        let s = self.bodies[small].clone();
        let b = &mut self.bodies[big];
        let m = b.mass + s.mass;
        b.vel = (b.vel * b.mass + s.vel * s.mass) / m;
        b.pos = (b.pos * b.mass + s.pos * s.mass) / m;
        b.temperature = ((b.temperature as f64 * b.mass + s.temperature as f64 * s.mass) / m) as f32 * 1.1;
        if b.kind == BodyKind::Star && s.kind == BodyKind::Star {
            b.radius = (b.radius.powi(3) + s.radius.powi(3)).cbrt();
        } else if b.kind == BodyKind::Star {
            // A star swallowing a (tiny, dense) neutron star barely grows.
            b.radius *= (m / b.mass).cbrt();
        } else {
            // A neutron star stays compact however much it eats; adding the
            // volumes would turn it into a star-sized (and blinding) ball.
            b.radius = (b.radius * (m / b.mass).cbrt()).min(2.0);
        }
        b.mass = m;
        b.flash = 3.0;
        b.update_star_colors();
        let pos = b.pos;
        let vel = b.vel;
        let r = b.radius;
        let name = b.name.clone();
        self.bodies[small].alive = false;
        self.event(format!("🌟 Stellar collision! {} and {} merged", name, s.name));
        for _ in 0..6000 {
            let dir = self.rng.unit();
            let c = if self.rng.f32() < 0.5 { Vec3::new(1.0, 0.8, 0.5) * 3.0 } else { Vec3::new(0.6, 0.7, 1.0) * 2.5 };
            self.emit(pos + dir * r, vel + dir * self.rng.range(5.0, 25.0), c, self.rng.rangef(0.3, 1.2), self.rng.rangef(4.0, 12.0), P_HOT | P_DRAG);
        }
        self.flashes.push(Flash { pos, color: Vec3::new(1.0, 0.9, 0.8), intensity: 15.0, decay: 0.6, size: (r * 20.0) as f32 });
        self.sound(Sound::StarMerge, Some(pos), (r * 4.0) as f32);
    }

    fn merge_black_holes(&mut self, big: usize, small: usize) {
        let s = self.bodies[small].clone();
        let b = &mut self.bodies[big];
        let m = b.mass + s.mass * 0.95; // 5% radiated as gravitational waves!
        b.vel = (b.vel * b.mass + s.vel * s.mass) / (b.mass + s.mass);
        b.pos = (b.pos * b.mass + s.pos * s.mass) / (b.mass + s.mass);
        b.mass = m;
        b.radius = black_hole_radius(m);
        b.feeding = 4.0;
        let pos = b.pos;
        let vel = b.vel;
        let r = b.radius;
        let name = b.name.clone();
        self.bodies[small].alive = false;
        self.event(format!("〰 Black hole merger: {} + {} - gravitational waves ripple out!", name, s.name));
        // gravitational-wave ripple rings
        for ring in 0..3 {
            for k in 0..1500 {
                let a = k as f64 / 1500.0 * std::f64::consts::TAU;
                let dir = DVec3::new(a.cos(), 0.0, a.sin());
                let speed = 30.0 + ring as f64 * 12.0;
                self.emit(pos + dir * r * 2.0, vel + dir * speed, Vec3::new(0.5, 0.6, 1.0) * 1.5, r as f32 * 0.25, 5.0, 0);
            }
        }
        self.flashes.push(Flash { pos, color: Vec3::new(0.7, 0.75, 1.0), intensity: 10.0, decay: 0.8, size: (r * 30.0) as f32 });
        self.sound(Sound::BlackHoleMerge, Some(pos), (r * 6.0) as f32);
    }

    /// Tidal disruption: a body passing too close to a black hole is torn into a stream.
    fn spaghettify(&mut self, bh: usize, victim: usize) {
        self.evacuate(victim);
        let v = self.bodies[victim].clone();
        self.bodies[victim].alive = false;
        let b = &mut self.bodies[bh];
        // The hole swallows about a third of the victim, and that third's momentum with it.
        let eaten = v.mass * 0.3;
        b.vel = (b.vel * b.mass + v.vel * eaten) / (b.mass + eaten);
        b.mass += eaten;
        b.radius = black_hole_radius(b.mass);
        b.feeding = (b.feeding + 2.0).min(5.0);
        let bh_name = b.name.clone();
        if !v.name.is_empty() {
            self.event(format!("🍝 {} was spaghettified by {}!", v.name, bh_name));
        }
        let count = (v.radius * 900.0).clamp(1500.0, 12000.0) as usize;
        let color = v.debris_color();
        let hot = v.kind == BodyKind::Star;
        let stream_start = self.particles.len();
        for _ in 0..count {
            let off = self.rng.unit() * v.radius * self.rng.f64().cbrt();
            let c = if hot || self.rng.f32() < 0.3 { Vec3::new(1.0, 0.65, 0.3) * 2.0 } else { color * 1.3 };
            self.emit(v.pos + off, v.vel + self.rng.unit() * 0.3, c, (v.radius as f32 * 0.05).clamp(0.08, 0.6), self.rng.rangef(30.0, 80.0), P_GRAVITY | P_HOT);
        }
        // The rest of the victim is in the stream, to be eaten bit by bit.
        self.give_mass_to_newest(self.particles.len() - stream_start, v.mass - eaten);
    }

    // ------------------------------------------------------------------
    // Supernovae
    // ------------------------------------------------------------------

    pub fn trigger_supernova(&mut self, id: u64, countdown: f64) -> bool {
        if let Some(b) = self.get_mut(id) {
            if b.kind == BodyKind::Star && b.supernova_timer.is_none() {
                b.supernova_timer = Some(countdown.max(0.5));
                let name = b.name.clone();
                self.event(format!("⚠ {} is collapsing! Supernova imminent", name));
                return true;
            }
        }
        false
    }

    fn update_supernovae(&mut self, dt: f64) {
        let mut explode = Vec::new();
        for (i, b) in self.bodies.iter_mut().enumerate() {
            // Only stars explode: drop the countdown if the body was turned into
            // something else (collapsed into a black hole, retyped in the inspector).
            if b.kind != BodyKind::Star {
                b.supernova_timer = None;
                continue;
            }
            if let Some(t) = b.supernova_timer.as_mut() {
                *t -= dt;
                // pre-supernova pulsations
                if *t < 2.0 {
                    let pulse = ((*t * 12.0).sin() * 0.5 + 0.5) as f32;
                    b.flash = b.flash.max((2.0 - *t as f32) * 0.6 * pulse);
                }
                if *t <= 0.0 {
                    explode.push(i);
                }
            }
        }
        for i in explode {
            self.explode_star(i);
        }
    }

    fn explode_star(&mut self, idx: usize) {
        let s = self.bodies[idx].clone();
        let massive = s.mass >= 15000.0;
        let remnant_mass = if massive { s.mass * 0.3 } else { s.mass * 0.14 };
        let ejected = s.mass - remnant_mass;
        {
            let b = &mut self.bodies[idx];
            b.supernova_timer = None;
            b.mass = remnant_mass;
            b.flash = 3.0;
            let mut rng = Rng::new(b.id ^ 0xABCD);
            if massive {
                b.kind = BodyKind::BlackHole;
                b.radius = black_hole_radius(remnant_mass);
                b.surface = Surface::for_kind(BodyKind::BlackHole, &mut rng);
                b.feeding = 3.0;
            } else {
                b.kind = BodyKind::NeutronStar;
                b.radius = 0.8;
                b.surface = Surface::for_kind(BodyKind::NeutronStar, &mut rng);
                b.spin_rate = 25.0;
            }
            b.name = format!("{} remnant", s.name);
        }
        self.event(format!(
            "🔆 SUPERNOVA! {} exploded, leaving a {}",
            s.name,
            if massive { "black hole" } else { "neutron star" }
        ));

        // Expanding, glowing ejecta shell that slows into a nebula.
        let palette = [
            Vec3::new(1.0, 0.95, 0.9) * 1.6,
            Vec3::new(1.0, 0.45, 0.15) * 1.3,
            Vec3::new(1.0, 0.2, 0.3) * 1.1,
            Vec3::new(0.3, 0.9, 0.9) * 1.0,
            Vec3::new(0.6, 0.35, 1.0) * 1.1,
        ];
        let shell = 16000;
        let base_speed = 35.0 + s.radius * 1.5;
        for k in 0..shell {
            let dir = self.rng.unit();
            // filamentary structure: clump speeds by direction
            let lumpy = 0.75 + 0.5 * ((dir.x * 7.0).sin() * (dir.y * 5.0).cos() * (dir.z * 6.0).sin()).abs();
            let speed = base_speed * lumpy * self.rng.range(0.6, 1.2);
            let c = palette[k % palette.len()];
            let life = self.rng.rangef(25.0, 60.0);
            self.emit(s.pos + dir * s.radius, s.vel + dir * speed, c, self.rng.rangef(0.5, 1.8), life, P_DRAG);
        }
        // fast bright blast front
        for _ in 0..5000 {
            let dir = self.rng.unit();
            self.emit(s.pos + dir * s.radius, s.vel + dir * self.rng.range(90.0, 130.0), Vec3::new(0.9, 0.95, 1.0) * 2.0, 0.9, self.rng.rangef(2.0, 4.0), 0);
        }
        self.flashes.push(Flash { pos: s.pos, color: Vec3::new(0.85, 0.9, 1.0), intensity: 30.0, decay: 0.7, size: (s.radius * 14.0) as f32 });
        self.sound(Sound::Supernova, Some(s.pos), (s.radius * 20.0) as f32);
        self.shockwaves.push(Shockwave {
            center: s.pos,
            radius: s.radius,
            speed: 110.0,
            max_radius: 2500.0,
            ejected_mass: ejected,
            hit: vec![s.id],
        });
    }

    fn update_shockwaves(&mut self, dt: f64) {
        let mut hits: Vec<(usize, f64, DVec3, f64)> = Vec::new();
        for w in &mut self.shockwaves {
            let r0 = w.radius;
            w.radius += w.speed * dt;
            for (i, b) in self.bodies.iter().enumerate() {
                if w.hit.contains(&b.id) {
                    continue;
                }
                let d = (b.pos - w.center).length();
                if d <= w.radius && d >= r0 - b.radius - 5.0 {
                    w.hit.push(b.id);
                    let dir = (b.pos - w.center).try_normalize().unwrap_or(DVec3::X);
                    hits.push((i, d, dir, w.ejected_mass));
                }
            }
        }
        self.shockwaves.retain(|w| w.radius < w.max_radius);
        for (i, d, dir, ej) in hits {
            if i >= self.bodies.len() || !self.bodies[i].alive {
                continue;
            }
            let b = &self.bodies[i];
            if b.kind == BodyKind::BlackHole {
                continue;
            }
            let dv = 0.1 * ej / d.max(5.0) / b.mass.max(0.1).cbrt();
            let close = d < 30.0 + ej.sqrt() * 0.25;
            if close && b.kind.paintable() && b.radius < 8.0 {
                // vaporised: shatter into molten debris
                self.evacuate(i);
                let src = self.bodies[i].clone();
                self.bodies[i].alive = false;
                if !src.name.is_empty() {
                    self.event(format!("☄ {} was vaporised by the blast", src.name));
                }
                let dummy = self.new_body("", BodyKind::Asteroid, DVec3::splat(1e9), DVec3::ZERO);
                self.spawn_fragments(&src, src.mass * 0.6, 6, src.pos, src.vel + dir * dv, dir, dv.max(5.0), &dummy);
                continue;
            }
            let b = &mut self.bodies[i];
            b.vel += dir * dv;
            b.heat = (b.heat + (300.0 / d as f32).min(1.0)).min(1.0);
            b.flash = 0.8;
            if b.kind.paintable() {
                let strength = (250.0 / d as f32).clamp(0.2, 1.0);
                if let Some(layer) = self.ensure_layer(i) {
                    let b = &self.bodies[i];
                    let local = b.rotation().inverse() * (-dir).as_vec3();
                    self.paint.apply(layer, local, 1.5, Brush::Scorch, strength);
                    if strength > 0.5 {
                        self.paint.apply(layer, local, 0.8, Brush::Lava, strength * 0.7);
                    }
                }
            }
        }
        self.remove_dead();
    }

    // ------------------------------------------------------------------
    // Particles
    // ------------------------------------------------------------------

    pub fn emit(&mut self, pos: DVec3, vel: DVec3, color: Vec3, size: f32, life: f32, flags: u8) {
        if self.particles.len() >= self.max_particles {
            return;
        }
        self.particles.push(Particle { pos: pos.as_vec3(), vel: vel.as_vec3(), color, size, life, max_life: life, flags, mass: 0.0 });
    }

    /// Share `mass` out among the `count` particles emitted most recently, so that
    /// whatever lands on a world (or falls into a black hole) is added to it.
    fn give_mass_to_newest(&mut self, count: usize, mass: f64) {
        let n = count.min(self.particles.len());
        if n == 0 || mass <= 0.0 {
            return;
        }
        let each = (mass / n as f64) as f32;
        let from = self.particles.len() - n;
        for p in &mut self.particles[from..] {
            p.mass = each;
        }
    }

    fn update_particles(&mut self, dt: f32) {
        if self.particles.is_empty() {
            return;
        }
        // The most massive bodies act as attractors / absorbers for particles.
        let mut idx: Vec<usize> = (0..self.bodies.len()).collect();
        idx.sort_by(|a, b| self.bodies[*b].mass.total_cmp(&self.bodies[*a].mass));
        idx.truncate(MAX_ATTRACTORS);
        let attractors: Vec<Attractor> = idx
            .iter()
            .map(|&i| {
                let b = &self.bodies[i];
                Attractor { pos: b.pos.as_vec3(), vel: b.vel.as_vec3(), gm: (b.mass * self.g) as f32, r2: (b.radius * b.radius) as f32 }
            })
            .collect();
        let n = self.particles.len();
        // Big clouds get fewer steps each, so the frame rate holds up.
        let max_steps = ((PARTICLE_WORK / (n * attractors.len().max(1)).max(1)) as u32).clamp(8, PARTICLE_MAX_STEPS);
        let threads = if n >= 2 * PARTICLES_PER_THREAD { worker_threads().min(n / PARTICLES_PER_THREAD) } else { 1 };
        let fed = if threads <= 1 {
            integrate_particles(&mut self.particles, &attractors, dt, max_steps)
        } else {
            // Particles don't interact with each other, so chunks are independent
            // and the result is identical to the single-threaded one.
            let chunk = n.div_ceil(threads);
            let attractors = &attractors[..];
            std::thread::scope(|scope| {
                let workers: Vec<_> =
                    self.particles.chunks_mut(chunk).map(|c| scope.spawn(move || integrate_particles(c, attractors, dt, max_steps))).collect();
                let mut fed = Fed::default();
                for w in workers {
                    fed.add(&w.join().expect("particle worker panicked"));
                }
                fed
            })
        };
        self.particles.retain(|p| p.life > 0.0);
        for (k, &i) in idx.iter().enumerate() {
            if fed.count[k] == 0 {
                continue;
            }
            let b = &mut self.bodies[i];
            if b.kind == BodyKind::BlackHole {
                b.feeding = (b.feeding + fed.count[k] as f32 * 0.002).min(5.0);
            }
            // Dust with mass makes whatever it lands on a little heavier (and nudges it).
            let gained = fed.mass[k] as f64;
            if gained > 0.0 {
                let m = b.mass + gained;
                b.vel = (b.vel * b.mass + fed.momentum[k].as_dvec3()) / m;
                match b.kind {
                    BodyKind::BlackHole => b.radius = black_hole_radius(m),
                    BodyKind::NeutronStar => {}
                    _ => b.radius *= (m / b.mass).cbrt(),
                }
                b.mass = m;
            }
        }
    }

    // ------------------------------------------------------------------
    // Prediction
    // ------------------------------------------------------------------

    /// Predict the path of a massless test body launched at pos/vel.
    /// Returns sampled points and the index of the body it would hit (if any).
    pub fn predict(&self, pos: DVec3, vel: DVec3, horizon: f64, steps: usize) -> (Vec<DVec3>, Option<usize>) {
        self.predict_relative(pos, vel, horizon, steps, None)
    }

    /// Like [`predict`](Self::predict), but with `relative_to` (a body index) the path is
    /// drawn as seen from that body: where the craft will be around it, placed at the
    /// body's current position. That is the useful picture when flying near a planet
    /// that is itself racing around its star.
    pub fn predict_relative(&self, pos: DVec3, vel: DVec3, horizon: f64, steps: usize, relative_to: Option<usize>) -> (Vec<DVec3>, Option<usize>) {
        let mut idx: Vec<usize> = (0..self.bodies.len()).collect();
        idx.sort_by(|a, b| self.bodies[*b].mass.total_cmp(&self.bodies[*a].mass));
        idx.truncate(24);
        let frame = relative_to.filter(|r| *r < self.bodies.len()).map(|r| match idx.iter().position(|i| *i == r) {
            Some(k) => k,
            None => {
                // A small moon outside the heaviest 24: it still has to be simulated.
                if idx.len() >= 24 {
                    idx.pop();
                }
                idx.push(r);
                idx.len() - 1
            }
        });
        let mut bp: Vec<DVec3> = idx.iter().map(|&i| self.bodies[i].pos).collect();
        let mut bv: Vec<DVec3> = idx.iter().map(|&i| self.bodies[i].vel).collect();
        let bm: Vec<f64> = idx.iter().map(|&i| self.bodies[i].mass * self.g).collect();
        let br: Vec<f64> = idx.iter().map(|&i| self.bodies[i].radius).collect();
        let n = bp.len();
        let mut p = pos;
        let mut v = vel;
        let dt = horizon / steps as f64;
        let mut out = Vec::with_capacity(steps / 4 + 2);
        out.push(p);
        let frame_start = frame.map(|k| bp[k]);
        // Shift a point by how far the reference body has moved since the start.
        let seen = |p: DVec3, bp: &[DVec3]| match (frame, frame_start) {
            (Some(k), Some(start)) => p - (bp[k] - start),
            _ => p,
        };
        let mut ba = vec![DVec3::ZERO; n];
        let accel = |bp: &[DVec3], ba: &mut [DVec3]| {
            for a in ba.iter_mut() {
                *a = DVec3::ZERO;
            }
            for i in 0..n {
                for j in (i + 1)..n {
                    let d = bp[j] - bp[i];
                    let s = d.length_squared() + 0.0025;
                    let inv = 1.0 / (s * s.sqrt());
                    ba[i] += d * (bm[j] * inv);
                    ba[j] -= d * (bm[i] * inv);
                }
            }
        };
        let test_acc = |bp: &[DVec3], p: DVec3| {
            let mut a = DVec3::ZERO;
            for i in 0..n {
                let d = bp[i] - p;
                let s = d.length_squared() + 0.0025;
                a += d * (bm[i] / (s * s.sqrt()));
            }
            a
        };
        accel(&bp, &mut ba);
        let mut pa = test_acc(&bp, p);
        for step in 0..steps {
            for i in 0..n {
                bv[i] += ba[i] * (0.5 * dt);
                bp[i] += bv[i] * dt;
            }
            v += pa * (0.5 * dt);
            p += v * dt;
            accel(&bp, &mut ba);
            pa = test_acc(&bp, p);
            for i in 0..n {
                bv[i] += ba[i] * (0.5 * dt);
            }
            v += pa * (0.5 * dt);
            if step % 4 == 0 {
                out.push(seen(p, &bp));
            }
            for i in 0..n {
                if (bp[i] - p).length_squared() < br[i] * br[i] {
                    out.push(seen(p, &bp));
                    return (out, Some(idx[i]));
                }
            }
        }
        (out, None)
    }

}

#[derive(Clone, Copy)]
struct Attractor {
    /// Where it is at the end of the frame, and how fast it is moving.
    pos: Vec3,
    vel: Vec3,
    gm: f32,
    /// Radius squared: particles inside are absorbed.
    r2: f32,
}

fn worker_threads() -> usize {
    static N: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *N.get_or_init(|| std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).min(8))
}

/// What each attractor swallowed while the particles were being moved.
#[derive(Clone, Copy, Default)]
struct Fed {
    count: [u32; MAX_ATTRACTORS],
    mass: [f32; MAX_ATTRACTORS],
    /// Mass times velocity of what was swallowed.
    momentum: [Vec3; MAX_ATTRACTORS],
}

impl Fed {
    fn add(&mut self, other: &Fed) {
        for k in 0..MAX_ATTRACTORS {
            self.count[k] += other.count[k];
            self.mass[k] += other.mass[k];
            self.momentum[k] += other.momentum[k];
        }
    }
}

/// The longest step a particle takes while it is cheap to take short ones (slow game speeds).
const PARTICLE_STEP: f32 = 0.02;
/// At faster game speeds a particle's step is this fraction of its orbital timescale
/// (about a hundred steps per orbit), so dust close to a heavy body takes many short
/// steps and far-off dust a few long ones.
const PARTICLE_ORBIT_FRACTION: f32 = 0.06;
/// ...but never more than this many steps in one frame,
const PARTICLE_MAX_STEPS: u32 = 48;
/// and fewer when there is a lot of dust: about this many particle-attractor pulls a frame.
const PARTICLE_WORK: usize = 2_500_000;

/// Advance a slice of particles by `dt`. Returns what each attractor swallowed.
///
/// Each particle picks its own step length as it goes. One fixed step for all of
/// them made galaxies and accretion disks fly apart when time was sped up: the dust
/// nearest the centre was taking only a handful of steps per orbit.
fn integrate_particles(particles: &mut [Particle], attractors: &[Attractor], dt: f32, max_steps: u32) -> Fed {
    const DRAG: f32 = 0.12;
    let mut fed = Fed::default();
    for p in particles {
        if p.flags & P_GRAVITY != 0 {
            // Time still to cover this frame, and the steps left to do it in.
            let mut left = dt;
            let mut steps_left = max_steps;
            'steps: while left > 0.0 {
                let mut acc = Vec3::ZERO;
                // Strongest G*mass / distance^3 felt: the square of the orbital rate.
                let mut grip = 0.0f32;
                for (k, a) in attractors.iter().enumerate() {
                    // The attractor moves during the frame too: use where it is right now.
                    let d = a.pos - a.vel * left - p.pos;
                    let d2 = d.length_squared();
                    if d2 < a.r2 {
                        // Swallowed: count it once and stop integrating it.
                        p.life = -1.0;
                        fed.count[k] += 1;
                        fed.mass[k] += p.mass;
                        fed.momentum[k] += p.vel * p.mass;
                        break 'steps;
                    }
                    let s = d2 + 0.05;
                    let w = a.gm / (s * s.sqrt());
                    acc += d * w;
                    grip = grip.max(w);
                }
                let h = if left <= PARTICLE_STEP || grip <= 0.0 {
                    left
                } else {
                    // Split what is left of the frame into equal steps no longer than the
                    // ideal: a steady orbit then gets the same step every time, which keeps it steady.
                    let ideal = (PARTICLE_ORBIT_FRACTION / grip.sqrt()).max(PARTICLE_STEP);
                    left / (left / ideal).ceil().clamp(1.0, steps_left as f32)
                };
                p.vel += acc * h;
                if p.flags & P_DRAG != 0 {
                    p.vel *= (-DRAG * h).exp();
                }
                p.pos += p.vel * h;
                left -= h;
                steps_left = steps_left.saturating_sub(1).max(1);
            }
        } else if p.flags & P_DRAG != 0 {
            // Coasting to a stop: the exact answer in one go.
            let slowed = (-DRAG * dt).exp();
            p.pos += p.vel * ((1.0 - slowed) / DRAG);
            p.vel *= slowed;
        } else {
            p.pos += p.vel * dt;
        }
        if p.flags & P_IMMORTAL == 0 {
            p.life -= dt;
        }
    }
    fed
}

/// Debris with no name of its own yet: nameless rubble, or what is left of a broken world.
fn is_leftover(b: &Body) -> bool {
    b.name.is_empty() || b.name.ends_with(REMNANT) || b.name.contains(" fragment ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::presets::{self, Preset};

    fn two_body(u: &mut Universe, dist: f64) -> (u64, u64) {
        let s = u.new_body("Star", BodyKind::Star, DVec3::ZERO, DVec3::ZERO).with_mass_radius(10000.0, 10.0);
        let sid = u.add(s);
        let v = (u.g * 10000.0 / dist).sqrt();
        let p = u.new_body("Planet", BodyKind::Rocky, DVec3::new(dist, 0.0, 0.0), DVec3::new(0.0, 0.0, -v)).with_mass_radius(1.0, 1.0);
        let pid = u.add(p);
        (sid, pid)
    }

    #[test]
    fn circular_orbit_stays_circular() {
        let mut u = Universe::new("t", 1);
        let (_, pid) = two_body(&mut u, 80.0);
        let period = std::f64::consts::TAU * 80.0 / (10000.0f64 / 80.0).sqrt();
        // simulate ~3 orbits
        let steps = (3.0 * period / (1.0 / 60.0)) as usize;
        for _ in 0..steps {
            u.update(1.0 / 60.0, 1.0, false);
        }
        let p = u.get(pid).unwrap();
        let r = p.pos.length();
        assert!((r - 80.0).abs() < 0.8, "orbit radius drifted to {r}");
    }

    #[test]
    fn momentum_conserved_in_merge() {
        let mut u = Universe::new("t", 2);
        u.collisions = CollisionMode::AlwaysMerge;
        let a = u.new_body("A", BodyKind::Rocky, DVec3::new(-3.0, 0.0, 0.0), DVec3::new(1.0, 0.0, 0.0)).with_mass_radius(10.0, 1.0);
        let b = u.new_body("B", BodyKind::Rocky, DVec3::new(3.0, 0.0, 0.0), DVec3::new(-1.0, 0.0, 0.5)).with_mass_radius(5.0, 1.0);
        u.add(a);
        u.add(b);
        let p0: DVec3 = u.bodies.iter().map(|b| b.vel * b.mass).sum();
        for _ in 0..300 {
            u.update(1.0 / 60.0, 1.0, false);
        }
        assert_eq!(u.bodies.len(), 1, "bodies should have merged");
        let m = &u.bodies[0];
        assert!((m.mass - 15.0).abs() < 1e-9);
        assert!((m.vel * m.mass - p0).length() < 1e-6, "momentum not conserved");
    }

    #[test]
    fn fast_impact_shatters() {
        let mut u = Universe::new("t", 3);
        let a = u.new_body("A", BodyKind::Earthlike, DVec3::new(-10.0, 0.0, 0.0), DVec3::new(15.0, 0.0, 0.0)).with_mass_radius(40.0, 2.0);
        let b = u.new_body("B", BodyKind::Rocky, DVec3::new(10.0, 0.0, 0.0), DVec3::new(-15.0, 0.0, 0.0)).with_mass_radius(35.0, 2.0);
        u.add(a);
        u.add(b);
        for _ in 0..120 {
            u.update(1.0 / 60.0, 1.0, false);
        }
        assert!(u.bodies.len() > 2, "expected fragments, got {}", u.bodies.len());
        assert!(!u.particles.is_empty(), "expected debris particles");
        assert!(u.bodies.iter().all(|b| b.pos.is_finite() && b.vel.is_finite()));
    }

    #[test]
    fn supernova_leaves_remnant() {
        let mut u = Universe::new("t", 4);
        let s = u.new_body("Big", BodyKind::Star, DVec3::ZERO, DVec3::ZERO).with_mass_radius(20000.0, 20.0);
        let id = u.add(s);
        let s2 = u.new_body("Small", BodyKind::Star, DVec3::new(5000.0, 0.0, 0.0), DVec3::ZERO).with_mass_radius(8000.0, 10.0);
        let id2 = u.add(s2);
        assert!(u.trigger_supernova(id, 0.5));
        assert!(u.trigger_supernova(id2, 0.5));
        for _ in 0..90 {
            u.update(1.0 / 60.0, 1.0, false);
        }
        assert_eq!(u.get(id).unwrap().kind, BodyKind::BlackHole);
        assert_eq!(u.get(id2).unwrap().kind, BodyKind::NeutronStar);
        assert!(u.particles.len() > 10000);
    }

    #[test]
    fn black_hole_spaghettifies() {
        let mut u = Universe::new("t", 5);
        let mut bh = u.new_body("BH", BodyKind::BlackHole, DVec3::ZERO, DVec3::ZERO);
        bh.mass = 30000.0;
        bh.radius = black_hole_radius(bh.mass);
        u.add(bh);
        let p = u.new_body("Victim", BodyKind::Earthlike, DVec3::new(60.0, 0.0, 0.0), DVec3::ZERO).with_mass_radius(40.0, 2.0);
        u.add(p);
        for _ in 0..600 {
            u.update(1.0 / 60.0, 1.0, false);
        }
        assert_eq!(u.bodies.len(), 1);
        assert!(u.log.iter().any(|l| l.contains("spaghettified")));
    }

    #[test]
    fn collapsed_star_does_not_go_supernova() {
        let mut u = Universe::new("t", 8);
        let s = u.new_body("Big", BodyKind::Star, DVec3::ZERO, DVec3::ZERO).with_mass_radius(20000.0, 20.0);
        let id = u.add(s);
        assert!(u.trigger_supernova(id, 0.5));
        // ...then it gets turned into a black hole before the countdown ends.
        let b = u.get_mut(id).unwrap();
        b.kind = BodyKind::BlackHole;
        let mass = b.mass;
        for _ in 0..90 {
            u.update(1.0 / 60.0, 1.0, false);
        }
        let b = u.get(id).unwrap();
        assert_eq!(b.mass, mass, "a black hole must not explode");
        assert!(b.supernova_timer.is_none());
        assert!(u.shockwaves.is_empty());
    }

    #[test]
    fn neutron_star_stays_compact_after_eating_a_star() {
        let mut u = Universe::new("t", 9);
        let ns = u.new_body("Pulsar", BodyKind::NeutronStar, DVec3::ZERO, DVec3::ZERO).with_mass_radius(9000.0, 0.8);
        let id = u.add(ns);
        let s = u.new_body("Star", BodyKind::Star, DVec3::new(5.0, 0.0, 0.0), DVec3::ZERO).with_mass_radius(3000.0, 8.0);
        u.add(s);
        for _ in 0..10 {
            u.update(1.0 / 60.0, 1.0, false);
        }
        assert_eq!(u.bodies.len(), 1);
        let b = u.get(id).unwrap();
        assert!((b.mass - 12000.0).abs() < 1e-6);
        assert!(b.radius < 2.5, "neutron star ballooned to radius {}", b.radius);
    }

    #[test]
    fn particles_integrate_identically_on_many_threads() {
        let attractors = [
            Attractor { pos: Vec3::ZERO, vel: Vec3::ZERO, gm: 5000.0, r2: 4.0 },
            Attractor { pos: Vec3::new(60.0, 0.0, 0.0), vel: Vec3::ZERO, gm: 800.0, r2: 1.0 },
        ];
        let rng = Rng::new(11);
        let make = || -> Vec<Particle> {
            (0..PARTICLES_PER_THREAD * 3 + 17)
                .map(|k| Particle {
                    pos: (rng.unit() * rng.range(1.0, 90.0)).as_vec3(),
                    vel: (rng.unit() * 6.0).as_vec3(),
                    color: Vec3::ONE,
                    size: 0.1,
                    life: 5.0,
                    max_life: 5.0,
                    mass: 0.0,
                    flags: if k % 3 == 0 { P_GRAVITY | P_DRAG } else { P_GRAVITY },
                })
                .collect()
        };
        let mut u = Universe::new("t", 12);
        let a = u.new_body("A", BodyKind::BlackHole, DVec3::ZERO, DVec3::ZERO).with_mass_radius(5000.0, 2.0);
        let b = u.new_body("B", BodyKind::Rocky, DVec3::new(60.0, 0.0, 0.0), DVec3::ZERO).with_mass_radius(800.0, 1.0);
        u.add(a);
        u.add(b);
        u.particles = make();
        let mut reference = u.particles.clone();
        let fed = integrate_particles(&mut reference, &attractors, 0.05, PARTICLE_MAX_STEPS);
        reference.retain(|p| p.life > 0.0);
        assert!(fed.count[0] > 0, "expected some particles to fall into the hole");
        u.update_particles(0.05);
        assert_eq!(u.particles.len(), reference.len());
        assert!(u.particles.iter().zip(&reference).all(|(x, y)| x.pos == y.pos && x.vel == y.vel && x.life == y.life));
    }

    #[test]
    fn merged_body_sits_at_the_centre_of_mass() {
        let mut u = Universe::new("t", 2);
        u.collisions = CollisionMode::AlwaysMerge;
        let a = u.new_body("A", BodyKind::Rocky, DVec3::new(-3.0, 0.0, 0.0), DVec3::new(1.0, 0.0, 0.0)).with_mass_radius(10.0, 1.0);
        let b = u.new_body("B", BodyKind::Rocky, DVec3::new(3.0, 0.0, 0.0), DVec3::new(-1.0, 0.0, 0.5)).with_mass_radius(8.0, 1.0);
        u.add(a);
        u.add(b);
        let centre = |u: &Universe| u.bodies.iter().map(|b| b.pos * b.mass).sum::<DVec3>() / 18.0;
        let drift = u.bodies.iter().map(|b| b.vel * b.mass).sum::<DVec3>() / 18.0;
        let start = centre(&u);
        for _ in 0..300 {
            u.update(1.0 / 60.0, 1.0, false);
        }
        assert_eq!(u.bodies.len(), 1, "bodies should have merged");
        // Nothing outside pushed on the pair, so their centre of mass just kept drifting.
        let expected = start + drift * u.time;
        assert!((centre(&u) - expected).length() < 1e-6, "the merged body jumped {} away from the centre of mass", (centre(&u) - expected).length());
    }

    #[test]
    fn shattering_conserves_momentum() {
        let mut u = Universe::new("t", 3);
        u.collisions = CollisionMode::AlwaysShatter;
        let a = u.new_body("A", BodyKind::Earthlike, DVec3::new(-10.0, 0.0, 0.0), DVec3::new(15.0, 2.0, 0.0)).with_mass_radius(60.0, 2.3);
        let b = u.new_body("B", BodyKind::Rocky, DVec3::new(10.0, 0.0, 0.5), DVec3::new(-15.0, 0.0, 0.0)).with_mass_radius(20.0, 1.6);
        u.add(a);
        u.add(b);
        let drift = u.bodies.iter().map(|b| b.vel * b.mass).sum::<DVec3>() / 80.0;
        // Look the moment they break up (later smashes among the pieces shed more dust).
        for _ in 0..120 {
            u.update(1.0 / 60.0, 1.0, false);
            if u.bodies.len() > 2 {
                break;
            }
        }
        assert!(u.bodies.len() > 2, "expected fragments, got {}", u.bodies.len());
        // Pieces and dust together still weigh what the two worlds did...
        let dust = |u: &Universe| u.particles.iter().map(|p| p.mass as f64).sum::<f64>();
        let mass = u.bodies.iter().map(|b| b.mass).sum::<f64>() + dust(&u);
        assert!((mass - 80.0).abs() < 0.01, "80 units of world became {mass}");
        assert!(dust(&u) > 1.0, "some of it should be dust");
        // ...and keep moving as the pair did (near enough: dust is pulled by the
        // pieces without pulling back, which costs a hair in the frame of the smash).
        let momentum = u.bodies.iter().map(|b| b.vel * b.mass).sum::<DVec3>() + u.particles.iter().map(|p| p.vel.as_dvec3() * p.mass as f64).sum::<DVec3>();
        let now = momentum / mass;
        assert!((now - drift).length() < 0.02, "wreckage drifts at {now:?}, the pair was drifting at {drift:?}");
    }

    #[test]
    fn rubble_grinds_to_dust_instead_of_multiplying() {
        let mut u = Universe::new("t", 4);
        let a = u.new_body("", BodyKind::Asteroid, DVec3::new(-2.0, 0.0, 0.0), DVec3::new(10.0, 0.0, 0.0)).with_mass_radius(0.01, 0.15);
        let b = u.new_body("", BodyKind::Asteroid, DVec3::new(2.0, 0.0, 0.0), DVec3::new(-10.0, 0.0, 0.0)).with_mass_radius(0.01, 0.15);
        u.add(a);
        u.add(b);
        let volume = |u: &Universe| u.bodies.iter().map(|b| b.radius.powi(3)).sum::<f64>();
        let before = volume(&u);
        for _ in 0..60 {
            u.update(1.0 / 60.0, 1.0, false);
        }
        assert!(u.bodies.len() <= 2, "two pebbles became {} bodies", u.bodies.len());
        assert!(volume(&u) <= before * 1.001, "the pieces are bigger than what broke");
        assert!(!u.particles.is_empty(), "expected a puff of dust");
    }

    #[test]
    fn planet_smash_does_not_fill_the_universe_with_rubble() {
        // At some speeds this used to set off a chain reaction: fragments shattered
        // into more fragments until the body limit was full of weightless rocks.
        let mut u = Universe::new("t", 1);
        presets::build(&mut u, Preset::HeadOn);
        let mut most = 0;
        for _ in 0..120 {
            u.update(1.0 / 60.0, 100.0, false);
            most = most.max(u.bodies.len());
        }
        // A few dozen pieces at the height of it, settling to a handful.
        assert!(most < 200, "the smash grew to {most} bodies");
        assert!(u.bodies.len() < 40, "{} bodies were still left at the end", u.bodies.len());
        assert!(u.bodies.iter().all(|b| b.mass > 1e-6), "weightless fragments were left behind");
    }

    #[test]
    fn black_hole_takes_the_momentum_of_what_it_eats() {
        let mut u = Universe::new("t", 5);
        let mut bh = u.new_body("BH", BodyKind::BlackHole, DVec3::ZERO, DVec3::ZERO);
        bh.mass = 30000.0;
        bh.radius = black_hole_radius(bh.mass);
        let id = u.add(bh);
        // Already inside the tidal radius, crossing sideways.
        let p = u.new_body("Victim", BodyKind::Earthlike, DVec3::new(15.0, 0.0, 0.0), DVec3::new(0.0, 0.0, 20.0)).with_mass_radius(40.0, 2.0);
        u.add(p);
        u.update(0.004, 1.0, false);
        assert_eq!(u.bodies.len(), 1);
        let b = u.get(id).unwrap();
        // It swallows 30% of the victim: 12 units of mass moving at 20.
        assert!((b.vel.z * b.mass - 240.0).abs() < 1.0, "sideways momentum {}", b.vel.z * b.mass);
    }

    #[test]
    fn dust_orbits_survive_fast_forward() {
        // A ring of dust close to a star, watched at 100x speed for many orbits.
        let mut u = Universe::new("t", 6);
        let s = u.new_body("Star", BodyKind::Star, DVec3::ZERO, DVec3::ZERO).with_mass_radius(10000.0, 5.0);
        u.add(s);
        let r = 15.0;
        for k in 0..100 {
            let a = k as f64 / 100.0 * std::f64::consts::TAU;
            let v = (10000.0f64 / r).sqrt();
            u.emit(DVec3::new(a.cos(), 0.0, a.sin()) * r, DVec3::new(-a.sin(), 0.0, a.cos()) * v, Vec3::ONE, 0.1, 1.0, P_GRAVITY | P_IMMORTAL);
        }
        for _ in 0..360 {
            u.update(1.0 / 60.0, 100.0, false);
        }
        assert_eq!(u.particles.len(), 100);
        for p in &u.particles {
            let d = p.pos.length() as f64;
            assert!((d / r - 1.0).abs() < 0.05, "dust wandered from radius {r} to {d:.1}");
        }
    }

    #[test]
    fn dust_that_lands_makes_a_world_heavier() {
        let mut u = Universe::new("t", 13);
        let p = u.new_body("World", BodyKind::Rocky, DVec3::ZERO, DVec3::ZERO).with_mass_radius(40.0, 2.0);
        let id = u.add(p);
        // A clump of rubble weighing 2 in all, falling in from one side at speed 3.
        for k in 0..100 {
            u.emit(DVec3::new(4.0, k as f64 * 0.01, 0.0), DVec3::new(-3.0, 0.0, 0.0), Vec3::ONE, 0.1, 30.0, P_GRAVITY);
        }
        u.give_mass_to_newest(100, 2.0);
        for _ in 0..120 {
            u.update(1.0 / 60.0, 1.0, false);
        }
        assert!(u.particles.is_empty(), "the rubble should have landed");
        let b = u.get(id).unwrap();
        assert!((b.mass - 42.0).abs() < 1e-3, "40 + 2 of rubble made {}", b.mass);
        assert!(b.radius > 2.0, "a heavier world is a bigger one");
        assert!(b.vel.x < -0.1, "the rubble should have nudged it along, got {:?}", b.vel);
    }

    #[test]
    fn black_hole_eats_the_rest_of_its_victim_over_time() {
        let mut u = Universe::new("t", 5);
        let mut bh = u.new_body("BH", BodyKind::BlackHole, DVec3::ZERO, DVec3::ZERO);
        bh.mass = 30000.0;
        bh.radius = black_hole_radius(bh.mass);
        let id = u.add(bh);
        let p = u.new_body("Victim", BodyKind::Earthlike, DVec3::new(15.0, 0.0, 0.0), DVec3::ZERO).with_mass_radius(40.0, 2.0);
        u.add(p);
        u.update(0.004, 1.0, false);
        // Torn apart: a third swallowed at once, the rest still there as a stream.
        let stream: f64 = u.particles.iter().map(|p| p.mass as f64).sum();
        assert!((u.get(id).unwrap().mass - 30012.0).abs() < 1e-6);
        assert!((stream - 28.0).abs() < 0.01, "the stream should weigh 28, not {stream}");
        for _ in 0..600 {
            u.update(1.0 / 60.0, 1.0, false);
        }
        let eaten = u.get(id).unwrap().mass - 30012.0;
        assert!(eaten > 10.0, "the hole should have fed on the stream, but only gained {eaten}");
    }

    /// A world of 60 and one of 14, as in the Theia preset but alone in space.
    fn two_worlds(u: &mut Universe, offset: f64, speed: f64) -> (u64, u64) {
        let a = u.new_body("Earth", BodyKind::Earthlike, DVec3::ZERO, DVec3::ZERO).with_mass_radius(60.0, 2.6);
        let b = u.new_body("Theia", BodyKind::Desert, DVec3::new(32.0, 0.0, offset), DVec3::new(-speed, 0.0, 0.0)).with_mass_radius(14.0, 1.7);
        (u.add(a), u.add(b))
    }

    fn weigh(u: &Universe) -> (f64, DVec3) {
        let dust = u.particles.iter().map(|p| (p.mass as f64, p.vel.as_dvec3()));
        u.bodies.iter().map(|b| (b.mass, b.vel)).chain(dust).fold((0.0, DVec3::ZERO), |(m, p), (bm, bv)| (m + bm, p + bv * bm))
    }

    #[test]
    fn glancing_blow_tears_a_world_in_two() {
        let mut u = Universe::new("t", 14);
        let (earth, theia) = two_worlds(&mut u, 5.85, 4.75);
        let (mass, momentum) = weigh(&u);
        while u.bodies.len() == 2 {
            u.update(1.0 / 60.0, 1.0, false);
            assert!(u.time < 30.0, "they never met");
        }
        // Part of Theia has sunk into the Earth; the rest carries on, with pieces coming off it.
        let (e, t) = (u.get(earth).unwrap(), u.get(theia).unwrap());
        assert!(e.mass > 60.1 && e.mass < 73.0, "Earth should have taken part of Theia, it weighs {}", e.mass);
        assert!(t.mass > 1.0 && t.mass < 13.0, "part of Theia should carry on, it weighs {}", t.mass);
        assert_eq!(t.name, "Theia remnant");
        assert!(u.bodies.len() > 2, "loose pieces should have come off");
        // Nothing appears or vanishes in the tearing.
        let (mass_after, momentum_after) = weigh(&u);
        assert!((mass_after - mass).abs() < 1e-6, "{mass} units of world became {mass_after}");
        assert!((momentum_after - momentum).length() < 0.05, "momentum went from {momentum:?} to {momentum_after:?}");
    }

    #[test]
    fn head_on_hit_tears_nothing_off() {
        let mut u = Universe::new("t", 15);
        two_worlds(&mut u, 0.0, 2.0);
        for _ in 0..900 {
            u.update(1.0 / 60.0, 1.0, false);
        }
        assert_eq!(u.bodies.len(), 1, "a slow dead-centre hit is a plain merger");
        assert!((u.bodies[0].mass - 74.0).abs() < 1e-6);
    }

    #[test]
    fn fast_glancing_blow_is_a_hit_and_run() {
        let mut u = Universe::new("t", 17);
        let (earth, theia) = two_worlds(&mut u, 5.0, 14.0);
        for _ in 0..600 {
            u.update(1.0 / 60.0, 1.0, false);
        }
        // Far too fast to be caught: what is left of Theia leaves for good.
        let (e, t) = (u.get(earth).unwrap(), u.get(theia).expect("the remnant should survive"));
        let (r, v) = (t.pos - e.pos, t.vel - e.vel);
        assert!(v.length_squared() / 2.0 > u.g * (e.mass + t.mass) / r.length(), "the remnant should be escaping");
        assert!(r.length() > 30.0);
    }

    /// Is there a proper moon round `host`: a fair size, bound, and staying close?
    fn has_moon(u: &Universe, host: &str) -> bool {
        let Some(e) = u.bodies.iter().find(|b| b.name == host) else { return false };
        u.bodies.iter().any(|m| {
            let (r, v) = (m.pos - e.pos, m.vel - e.vel);
            let energy = v.length_squared() / 2.0 - u.g * (e.mass + m.mass) / r.length();
            m.name.starts_with("Moon") && m.mass > 0.5 && energy < 0.0 && r.length() < 40.0
        })
    }

    #[test]
    fn glancing_blows_sometimes_leave_a_moon() {
        // Nothing is placed in orbit by hand, so it depends on how the pieces fly:
        // with the right kind of hit a moon is common, but never certain.
        let moons = (0..24u64)
            .filter(|seed| {
                let mut u = Universe::new("t", 100 + seed * 7919);
                two_worlds(&mut u, 5.85, 4.25);
                for _ in 0..3600 {
                    u.update(1.0 / 60.0, 10.0, false);
                }
                has_moon(&u, "Earth")
            })
            .count();
        assert!((6..24).contains(&moons), "{moons} of 24 glancing blows left a moon");
    }

    #[test]
    fn theia_preset_leaves_the_earth_a_moon_at_any_speed() {
        // The preset fixes its dice, so that this one does work every time: at any
        // game speed, and with frames of uneven length like the real game's.
        let uneven = Rng::new(5);
        for (speed, steady) in [(1.0, true), (6.0, true), (40.0, true), (150.0, true), (2.0, false), (6.0, false), (30.0, false)] {
            let mut u = Universe::new("t", speed as u64);
            // (As in the game, the universe has been used before the preset is loaded.)
            presets::build(&mut u, Preset::HeadOn);
            for _ in 0..200 {
                u.update(1.0 / 60.0, 4.0, false);
            }
            presets::build(&mut u, Preset::TheiaImpact);
            while u.time < 600.0 {
                u.update(if steady { 1.0 / 60.0 } else { uneven.range(0.01, 0.035) }, speed, false);
            }
            assert!(has_moon(&u, "Proto-Earth"), "no proper Moon at {speed}x (steady frames: {steady}): {:?}", u.log);
            let e = u.bodies.iter().find(|b| b.name == "Proto-Earth").expect("the Proto-Earth should survive");
            // Nobody puts it there: this is where gravity leaves what was torn off Theia.
            let m = u.bodies.iter().find(|b| b.name == "Moon").unwrap_or_else(|| panic!("no Moon at {speed}x: {:?}", u.log));
            let (r, v) = (m.pos - e.pos, m.vel - e.vel);
            assert!(m.mass > 0.5, "at {speed}x the Moon is a pebble of {}", m.mass);
            assert!(r.length() > e.radius + m.radius, "at {speed}x the Moon is in the ground");
            assert!(v.length_squared() / 2.0 < u.g * (e.mass + m.mass) / r.length(), "at {speed}x the Moon is escaping");
        }
    }

    #[test]
    fn crowded_universe_stays_within_its_step_budget() {
        let mut u = Universe::new("t", 16);
        let s = u.new_body("S", BodyKind::Star, DVec3::ZERO, DVec3::ZERO).with_mass_radius(10000.0, 10.0);
        u.add(s);
        for k in 0..299 {
            let (r, a) = (60.0 + k as f64 * 3.0, k as f64 * 2.4);
            let v = (10000.0f64 / r).sqrt();
            let b = u.new_body("", BodyKind::Asteroid, DVec3::new(r * a.cos(), 0.0, r * a.sin()), DVec3::new(-a.sin(), 0.0, a.cos()) * v).with_mass_radius(0.5, 0.5);
            u.add(b);
        }
        u.pair_budget = 1_000_000;
        u.update(1.0 / 60.0, 100.0, false);
        assert!(u.step_work > 0 && u.step_work <= 1_000_000, "did {} pulls on a budget of a million", u.step_work);
        // It can't keep up with 100x on that budget, so time runs slower rather than the frame longer.
        assert!(u.time > 0.5 && u.time < 100.0 / 60.0, "simulated {} s", u.time);
        assert!(u.bodies.iter().all(|b| b.pos.is_finite()));
    }

    #[test]
    fn prediction_detects_impact() {
        let mut u = Universe::new("t", 6);
        let s = u.new_body("Star", BodyKind::Star, DVec3::ZERO, DVec3::ZERO).with_mass_radius(10000.0, 10.0);
        u.add(s);
        let (pts, hit) = u.predict(DVec3::new(100.0, 0.0, 0.0), DVec3::ZERO, 100.0, 2000);
        assert!(hit.is_some(), "falling straight in should hit the star");
        assert!(pts.len() > 2);
        let v = (10000.0f64 / 100.0).sqrt();
        let (_, hit) = u.predict(DVec3::new(100.0, 0.0, 0.0), DVec3::new(0.0, 0.0, v), 70.0, 2000);
        assert!(hit.is_none(), "circular orbit should not hit");
    }

    #[test]
    fn paint_brushes_modify_layer() {
        let mut p = PaintStore::new();
        let l = p.alloc().unwrap();
        p.apply(l, Vec3::Y, 0.3, Brush::Raise, 1.0);
        let top = p.layers[l as usize][crate::paint::PAINT_W / 2];
        assert!(top[0] > 128, "raise brush should lift the north pole");
        p.apply(l, Vec3::X, 0.3, Brush::Lava, 1.0);
        // texel at (theta=0, equator) -> u = 0.5, v = 0.5
        let i = (crate::paint::PAINT_H / 2) * crate::paint::PAINT_W + crate::paint::PAINT_W / 2;
        let t = p.layers[l as usize][i];
        assert_eq!(t[1] / 32, 2, "material should be lava");
        assert!(t[2] > 0);
        p.release(l);
        assert_eq!(p.layers[l as usize][i], crate::paint::NEUTRAL);
    }

    #[test]
    fn all_presets_build_and_run() {
        let mut u = Universe::new("t", 7);
        for p in Preset::ALL {
            presets::build(&mut u, p);
            for _ in 0..30 {
                u.update(1.0 / 60.0, 5.0, false);
            }
            assert!(u.bodies.iter().all(|b| b.pos.is_finite()), "{p:?} produced NaN");
        }
        let params = presets::CustomParams { chaos: 0.5, ..Default::default() };
        for c in presets::CenterKind::ALL {
            let params = presets::CustomParams { center: c, ..params.clone() };
            presets::build_custom(&mut u, &params);
            for _ in 0..30 {
                u.update(1.0 / 60.0, 5.0, false);
            }
            assert!(u.bodies.iter().all(|b| b.pos.is_finite()), "{c:?} produced NaN");
        }
    }
}

#[cfg(test)]
mod stability {
    use super::*;
    use crate::presets::{self, Preset};

    /// The Solar System preset must stay intact: no lost moons, no planet
    /// crossings, and the asteroid belt keeps its shape.
    fn run(time_scale: f64, sim_seconds: f64) {
        let mut u = Universe::new("t", 1);
        presets::build(&mut u, Preset::SolarSystem);
        let parent = |n: &str| match n {
            "Moon" => Some("Earth"),
            "Phobos" | "Deimos" => Some("Mars"),
            "Io" | "Europa" | "Ganymede" | "Callisto" => Some("Jupiter"),
            "Titan" => Some("Saturn"),
            "Triton" => Some("Neptune"),
            _ => None,
        };
        let names: Vec<String> = u.bodies.iter().map(|b| b.name.clone()).collect();
        let id = |u: &Universe, n: &str| u.bodies.iter().find(|b| b.name == n).map(|b| b.id).unwrap();
        // (name, id, parent id or sun, initial distance, min, max)
        let sun = id(&u, "Sun");
        let mut track: Vec<(String, u64, u64, f64, f64, f64)> = names
            .iter()
            .filter(|n| n.as_str() != "Sun")
            .map(|n| {
                let me = id(&u, n);
                let p = parent(n).map(|p| id(&u, p)).unwrap_or(sun);
                let d = (u.get(me).unwrap().pos - u.get(p).unwrap().pos).length();
                (n.clone(), me, p, d, d, d)
            })
            .collect();
        let belt0 = u.particles.len();
        let frames = (sim_seconds / time_scale * 60.0) as usize;
        for _ in 0..frames {
            u.update(1.0 / 60.0, time_scale, false);
            for t in track.iter_mut() {
                let (Some(m), Some(p)) = (u.get(t.1), u.get(t.2)) else { panic!("{} was lost: {:?}", t.0, u.log) };
                let d = (m.pos - p.pos).length();
                t.4 = t.4.min(d);
                t.5 = t.5.max(d);
            }
        }
        for (name, _, _, d0, lo, hi) in &track {
            println!("{name:9} start {d0:8.1}  min {lo:8.1}  max {hi:8.1}");
            let tol = if parent(name).is_some() { 0.35 } else if name == "Pluto" { 0.3 } else { 0.12 };
            assert!(*lo > d0 * (1.0 - tol) && *hi < d0 * (1.0 + tol), "{name} orbit drifted: start {d0:.1} range {lo:.1}..{hi:.1}");
        }
        let in_belt = u
            .particles
            .iter()
            .filter(|p| {
                let r = (p.pos.as_dvec3() - u.get(sun).unwrap().pos).length();
                (880.0..1420.0).contains(&r)
            })
            .count();
        println!("belt: {in_belt}/{belt0} still in the belt");
        assert!(in_belt as f64 > belt0 as f64 * 0.95, "asteroid belt dispersed: {in_belt}/{belt0}");
    }

    /// Every body that started in the system must still exist after `sim_seconds`.
    fn survivors(u: &mut Universe, time_scale: f64, sim_seconds: f64) -> Vec<String> {
        let names: Vec<(u64, String)> = u.bodies.iter().map(|b| (b.id, b.name.clone())).collect();
        let frames = (sim_seconds / time_scale * 60.0) as usize;
        for _ in 0..frames {
            u.update(1.0 / 60.0, time_scale, false);
        }
        names.into_iter().filter(|(id, _)| u.get(*id).is_none()).map(|(_, n)| n).collect()
    }

    #[test]
    fn binary_preset_is_stable() {
        let mut u = Universe::new("t", 1);
        presets::build(&mut u, Preset::BinaryStars);
        let lost = survivors(&mut u, 20.0, 3000.0);
        assert!(lost.is_empty(), "lost {lost:?}: {:?}", u.log);
    }

    #[test]
    fn generated_universes_are_stable() {
        for center in presets::CenterKind::ALL {
            if center == presets::CenterKind::Nothing {
                continue; // rogue planets drift by design
            }
            for seed in [1u64, 42, 777, 2024] {
                let mut u = Universe::new("t", 1);
                let params = presets::CustomParams { center, seed, planets: 9, ..Default::default() };
                presets::build_custom(&mut u, &params);
                let lost = survivors(&mut u, 30.0, 1200.0);
                assert!(lost.is_empty(), "{center:?} seed {seed}: lost {lost:?}\n{:?}", u.log);
            }
        }
    }

    #[test]
    fn solar_system_is_stable_at_normal_speed() {
        // ~7 Earth years
        run(4.0, 600.0);
    }

    #[test]
    fn solar_system_is_stable_with_time_warp() {
        // ~45 Earth years / a sizeable chunk of a Jupiter orbit, at heavy time warp
        run(100.0, 4000.0);
    }
}
