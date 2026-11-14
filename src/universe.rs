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
}

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
    pub trail_len: usize,
    pub trail_interval: f64,
    pub paint: PaintStore,
    pub rng: Rng,
    pub log: Vec<String>,
    next_id: u64,
    contacts: Vec<Contact>,
    scratch_pos: Vec<DVec3>,
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
            trail_len: 600,
            trail_interval: 1.0,
            paint: PaintStore::new(),
            rng: Rng::new(seed),
            log: Vec::new(),
            next_id: 1,
            contacts: Vec::new(),
            scratch_pos: Vec::new(),
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
        self.paint.reset_all();
    }

    pub fn new_body(&mut self, name: &str, kind: BodyKind, pos: DVec3, vel: DVec3) -> Body {
        let id = self.next_id;
        self.next_id += 1;
        Body::new(id, name, kind, pos, vel, &mut self.rng)
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
        let max_steps = (10_000_000 / pairs).clamp(4, 3000);
        let steps = ((total / self.base_dt).ceil() as usize).clamp(1, max_steps);
        let dt = (total / steps as f64).min(0.05);
        for _ in 0..steps {
            self.step(dt);
        }
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
        self.compute_accelerations();
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

    fn compute_accelerations(&mut self) {
        let n = self.bodies.len();
        self.scratch_pos.clear();
        self.scratch_pos.extend(self.bodies.iter().map(|b| b.pos));
        self.scratch_acc.clear();
        self.scratch_acc.resize(n, DVec3::ZERO);
        let g = self.g;
        let check_collisions = true;
        for i in 0..n {
            let pi = self.scratch_pos[i];
            let mi = self.bodies[i].mass * g;
            let ri = self.bodies[i].radius;
            let ki = self.bodies[i].kind;
            let gi = self.bodies[i].ghost;
            let mut ai = DVec3::ZERO;
            for j in (i + 1)..n {
                let d = self.scratch_pos[j] - pi;
                let d2 = d.length_squared();
                let s = d2 + 0.0025;
                let inv = 1.0 / (s * s.sqrt());
                let bj = &self.bodies[j];
                ai += d * (bj.mass * g * inv);
                self.scratch_acc[j] -= d * (mi * inv);
                if !check_collisions {
                    continue;
                }
                let rsum = ri + bj.radius;
                if d2 < rsum * rsum {
                    let ghosted = (gi > 0.0 || bj.ghost > 0.0) && !ki.is_stellar() && !bj.kind.is_stellar();
                    if !ghosted {
                        self.contacts.push(Contact::Touch(i, j));
                    }
                } else if self.tidal_disruption {
                    let (bh, victim) = if ki == BodyKind::BlackHole && bj.kind != BodyKind::BlackHole {
                        (i, j)
                    } else if bj.kind == BodyKind::BlackHole && ki != BodyKind::BlackHole {
                        (j, i)
                    } else {
                        continue;
                    };
                    let (mb, rb) = (self.bodies[bh].mass, self.bodies[bh].radius);
                    let v = &self.bodies[victim];
                    if v.kind == BodyKind::NeutronStar {
                        continue;
                    }
                    let rt = v.radius * (2.0 * mb / v.mass.max(1e-6)).cbrt();
                    if rt > rb * 1.3 && d2 < rt * rt {
                        self.contacts.push(Contact::Tidal { bh, victim });
                    }
                }
            }
            self.scratch_acc[i] += ai;
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

    /// `big` swallows `small` (momentum conserving).
    fn absorb(&mut self, big: usize, small: usize, splash: bool) {
        let s = self.bodies[small].clone();
        let (impact, n) = self.impact_info(big, small);
        let b = &mut self.bodies[big];
        let m = b.mass + s.mass;
        b.vel = (b.vel * b.mass + s.vel * s.mass) / m;
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
        let (impact, n) = self.impact_info(big, small);
        let b = self.bodies[big].clone();
        let s = self.bodies[small].clone();
        let m_tot = b.mass + s.mass;
        let com_vel = (b.vel * b.mass + s.vel * s.mass) / m_tot;
        let rel_v = (s.vel - b.vel).length();
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
        let count = (1500.0 + (s.radius + b.radius * big_destroy) * 900.0).min(9000.0) as usize;
        let dc1 = s.debris_color();
        let dc2 = b.debris_color();
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
            let r = (density_r * m.cbrt()).max(0.12);
            let dir = (n * 0.8 + self.rng.unit()).normalize();
            let mut pos = center + self.rng.unit() * src.radius * 0.7 + dir * r;
            // keep fragments out of the surviving body
            let to = pos - avoid.pos;
            let min_d = avoid.radius + r * 1.05;
            if avoid.alive && to.length() < min_d {
                pos = avoid.pos + to.try_normalize().unwrap_or(n) * min_d;
            }
            let v = base_vel + dir * speed * self.rng.range(0.15, 0.6);
            let kind = if r > 0.7 && self.rng.f32() < 0.5 { BodyKind::Lava } else { BodyKind::Asteroid };
            let name = if src.name.is_empty() || src.name.contains("fragment") || k > 1 {
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
        b.temperature = ((b.temperature as f64 * b.mass + s.temperature as f64 * s.mass) / m) as f32 * 1.1;
        b.radius = (b.radius.powi(3) + s.radius.powi(3)).cbrt();
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
    }

    fn merge_black_holes(&mut self, big: usize, small: usize) {
        let s = self.bodies[small].clone();
        let b = &mut self.bodies[big];
        let m = b.mass + s.mass * 0.95; // 5% radiated as gravitational waves!
        b.vel = (b.vel * b.mass + s.vel * s.mass) / (b.mass + s.mass);
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
    }

    /// Tidal disruption: a body passing too close to a black hole is torn into a stream.
    fn spaghettify(&mut self, bh: usize, victim: usize) {
        let v = self.bodies[victim].clone();
        self.bodies[victim].alive = false;
        let b = &mut self.bodies[bh];
        b.mass += v.mass * 0.3;
        b.radius = black_hole_radius(b.mass);
        b.feeding = (b.feeding + 2.0).min(5.0);
        let bh_name = b.name.clone();
        if !v.name.is_empty() {
            self.event(format!("🍝 {} was spaghettified by {}!", v.name, bh_name));
        }
        let count = (v.radius * 900.0).clamp(1500.0, 12000.0) as usize;
        let color = v.debris_color();
        let hot = v.kind == BodyKind::Star;
        for _ in 0..count {
            let off = self.rng.unit() * v.radius * self.rng.f64().cbrt();
            let c = if hot || self.rng.f32() < 0.3 { Vec3::new(1.0, 0.65, 0.3) * 2.0 } else { color * 1.3 };
            self.emit(v.pos + off, v.vel + self.rng.unit() * 0.3, c, (v.radius as f32 * 0.05).clamp(0.08, 0.6), self.rng.rangef(30.0, 80.0), P_GRAVITY | P_HOT);
        }
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
                let src = b.clone();
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
        self.particles.push(Particle { pos: pos.as_vec3(), vel: vel.as_vec3(), color, size, life, max_life: life, flags });
    }

    fn update_particles(&mut self, dt: f32) {
        if self.particles.is_empty() {
            return;
        }
        // The most massive bodies act as attractors / absorbers for particles.
        let mut idx: Vec<usize> = (0..self.bodies.len()).collect();
        idx.sort_by(|a, b| self.bodies[*b].mass.partial_cmp(&self.bodies[*a].mass).unwrap());
        idx.truncate(16);
        let attractors: Vec<(Vec3, f32, f32, usize)> = idx
            .iter()
            .map(|&i| {
                let b = &self.bodies[i];
                (b.pos.as_vec3(), (b.mass * self.g) as f32, (b.radius * b.radius) as f32, i)
            })
            .collect();
        let sub = ((dt / 0.02).ceil() as usize).clamp(1, 8);
        let h = dt / sub as f32;
        let drag = (-0.12 * h).exp();
        let mut fed = vec![0u32; attractors.len()];
        for p in &mut self.particles {
            for _ in 0..sub {
                if p.flags & P_GRAVITY != 0 {
                    let mut acc = Vec3::ZERO;
                    for (k, (apos, gm, r2, _)) in attractors.iter().enumerate() {
                        let d = *apos - p.pos;
                        let d2 = d.length_squared();
                        if d2 < *r2 {
                            p.life = -1.0;
                            fed[k] += 1;
                            break;
                        }
                        let s = d2 + 0.05;
                        acc += d * (*gm / (s * s.sqrt()));
                    }
                    p.vel += acc * h;
                }
                if p.flags & P_DRAG != 0 {
                    p.vel *= drag;
                }
                p.pos += p.vel * h;
            }
            if p.flags & P_IMMORTAL == 0 {
                p.life -= dt;
            }
        }
        self.particles.retain(|p| p.life > 0.0);
        for (k, n) in fed.iter().enumerate() {
            if *n > 0 {
                let b = &mut self.bodies[attractors[k].3];
                if b.kind == BodyKind::BlackHole {
                    b.feeding = (b.feeding + *n as f32 * 0.002).min(5.0);
                }
            }
        }
    }

    // ------------------------------------------------------------------
    // Prediction
    // ------------------------------------------------------------------

    /// Predict the path of a massless test body launched at pos/vel.
    /// Returns sampled points and the index of the body it would hit (if any).
    pub fn predict(&self, pos: DVec3, vel: DVec3, horizon: f64, steps: usize) -> (Vec<DVec3>, Option<usize>) {
        let mut idx: Vec<usize> = (0..self.bodies.len()).collect();
        idx.sort_by(|a, b| self.bodies[*b].mass.partial_cmp(&self.bodies[*a].mass).unwrap());
        idx.truncate(24);
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
                out.push(p);
            }
            for i in 0..n {
                if (bp[i] - p).length_squared() < br[i] * br[i] {
                    out.push(p);
                    return (out, Some(idx[i]));
                }
            }
        }
        (out, None)
    }

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
