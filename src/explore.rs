//! Exploring the universe in person: flying rockets and walking on worlds.
//!
//! * **Flight** - the rocket is a massless craft integrated by the universe's own
//!   physics step (`Universe::ship`), so it feels the gravity of everything and
//!   stays exact at any time scale. Steering is arcade-style: the keys turn the
//!   rocket directly. Touch down gently and it lands; too fast and it is wrecked.
//! * **Walking** - the astronaut is stored in the body's own rotating coordinates,
//!   so the ground stays still underfoot while the planet spins and orbits.

use glam::{DQuat, DVec3, Quat, Vec3};

use crate::body::{Body, BodyKind};
use crate::camera::{ray_sphere, CameraFrame};
use crate::conditions::{self, Conditions};
use crate::game::Game;
use crate::gpu_types::{GpuParticle, LineVertex};
use crate::rocket::{self, RocketSpec, ROCKETS};
use crate::scene::Polyline;
use crate::terrain;
use crate::universe::{Flash, Ship, ShipHit, Universe, P_GRAVITY, P_HOT};

/// Height of the astronaut's eyes above the ground, in world units.
pub const EYE_HEIGHT: f64 = 0.012;
const WALK_SPEED: f64 = 0.045;
const RUN_FACTOR: f64 = 3.0;
const JUMP_SPEED: f64 = 0.06;
/// How close (along the ground) you must be to climb back into a parked rocket.
const BOARD_RANGE: f64 = 0.15;
/// Time scales chosen when a mode starts: planets are small, so things happen fast.
pub const FLIGHT_TIME_SCALE: f64 = 0.2;
pub const WALK_TIME_SCALE: f64 = 0.1;
const FIRST_PERSON_FOV: f32 = 1.15;

pub enum Mode {
    Sandbox,
    Flight(Flight),
    Walk(Walk),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FlightView {
    /// Outside camera that orbits the rocket.
    Chase,
    /// From the pilot's seat.
    Cockpit,
}

#[derive(Clone, Debug)]
pub enum FlightState {
    Flying,
    /// Standing on a body, at a body-local unit direction (it turns with the ground).
    Landed { body: u64, local: DVec3 },
    /// Wrecked. `anchor` keeps the camera with the body it hit (body id, offset from its centre).
    Destroyed { reason: String, anchor: Option<(u64, DVec3)> },
}

pub struct Flight {
    pub rocket: usize,
    pub state: FlightState,
    pub pos: DVec3,
    pub vel: DVec3,
    /// Rocket space (nose +Y, top +Z) to world space.
    pub orient: DQuat,
    pub throttle: f64,
    /// Fraction of a full tank, 0..1.
    pub fuel: f64,
    pub view: FlightView,
    pub cam_yaw: f64,
    pub cam_pitch: f64,
    pub cam_dist: f64,
    /// Smoothed "up" for the chase camera (away from the nearest world, or +Y in deep space).
    pub cam_up: DVec3,
    /// Hold the nose against the direction of travel, for braking and landing.
    pub brake_assist: bool,
    /// Trying to lift off with an engine too weak for this world.
    pub grounded_by_gravity: bool,
}

pub struct Parked {
    pub rocket: usize,
    pub local: DVec3,
    pub fuel: f64,
}

pub struct Walk {
    pub body: u64,
    /// Where the astronaut stands: body-local unit direction.
    pub local: DVec3,
    /// Facing direction along the ground: body-local unit tangent.
    pub heading: DVec3,
    pub pitch: f64,
    /// Height of the feet above the ground while jumping.
    pub height: f64,
    pub vspeed: f64,
    /// The rocket you arrived in, if any.
    pub parked: Option<Parked>,
}

/// Held-key steering input, sampled once per frame.
#[derive(Default, Clone, Copy)]
struct Controls {
    pitch: f64,
    yaw: f64,
    roll: f64,
    throttle: f64,
    forward: f64,
    strafe: f64,
    turn: f64,
    run: bool,
}

/// Extra things to draw for the current mode.
#[derive(Default)]
pub struct SceneExtras {
    /// Opaque lit triangles (rockets), camera-relative.
    pub solids: Vec<LineVertex>,
    pub glows: Vec<GpuParticle>,
    /// rgb = sky colour, a = how much it covers the stars.
    pub sky: [f32; 4],
    /// xyz = local "up" for the sky gradient, w = sunlight reaching the rocket (0..1).
    pub sky_up: [f32; 4],
}

/// Numbers for the flight HUD.
pub struct FlightInfo {
    pub rocket: &'static RocketSpec,
    pub view: FlightView,
    pub throttle: f64,
    pub fuel: f64,
    pub flying: bool,
    pub landed_on: Option<String>,
    pub destroyed: Option<String>,
    pub near: Option<String>,
    /// Speed relative to the nearest world's ground (or absolute in deep space).
    pub speed: f64,
    /// Positive = climbing.
    pub vertical_speed: f64,
    pub altitude: Option<f64>,
    /// Local gravity in Earth g.
    pub gravity_g: f64,
    /// Direction of travel relative to the nearest world, if moving.
    pub prograde: Option<DVec3>,
    /// Nose angle above the horizon and roll, radians (for the attitude gauge).
    pub pitch: f64,
    pub roll: f64,
    pub brake_assist: bool,
    pub grounded_by_gravity: bool,
    pub safe_landing_speed: f64,
}

/// Numbers for the walking HUD.
pub struct WalkInfo {
    pub body: String,
    pub ground: &'static str,
    pub conditions: Conditions,
    pub local_temp_k: f64,
    pub sky: &'static str,
    pub airborne: f64,
    /// Ground distance to the parked rocket and whether it is close enough to board.
    pub rocket: Option<(f64, bool)>,
}

fn dquat(q: Quat) -> DQuat {
    DQuat::from_xyzw(q.x as f64, q.y as f64, q.z as f64, q.w as f64).normalize()
}

fn body_rot(b: &Body) -> DQuat {
    dquat(b.rotation())
}

/// Any unit vector perpendicular to `v`, chosen to vary smoothly with `v`.
fn tangent(v: DVec3) -> DVec3 {
    let a = if v.z.abs() < 0.9 { DVec3::Z } else { DVec3::X };
    (a - v * a.dot(v)).normalize()
}

fn smoothstep(e0: f64, e1: f64, x: f64) -> f64 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Height of the ground of `b` under world point `pos`, as a distance from its centre.
fn ground_radius(u: &Universe, b: &Body, pos: DVec3) -> f64 {
    match (pos - b.pos).try_normalize() {
        Some(dir) if terrain::has_solid_surface(b.kind) => terrain::surface_radius_world(b, &u.paint, dir),
        _ => b.radius,
    }
}

/// A radius for a starting orbit around body `i` that stays clear of its surface
/// (or accretion disk) and of the orbits of its moons.
fn clear_orbit_radius(u: &Universe, i: usize) -> f64 {
    let b = &u.bodies[i];
    let lowest = if b.kind == BodyKind::BlackHole { 12.0 } else { 1.5 };
    let crowded = |r: f64| {
        u.bodies.iter().any(|m| m.id != b.id && (((m.pos - b.pos).length() - r).abs() < m.radius + 0.5 + r * 0.04))
    };
    (0..16).map(|k| b.radius * lowest * 1.2f64.powi(k)).find(|r| !crowded(*r)).unwrap_or(b.radius * lowest * 2.0)
}

impl Flight {
    fn new(rocket: usize) -> Flight {
        let spec = &ROCKETS[rocket];
        Flight {
            rocket,
            state: FlightState::Flying,
            pos: DVec3::ZERO,
            vel: DVec3::ZERO,
            orient: DQuat::IDENTITY,
            throttle: 0.0,
            fuel: 1.0,
            view: FlightView::Chase,
            cam_yaw: 0.6,
            cam_pitch: 0.25,
            cam_dist: spec.length * 7.0,
            cam_up: DVec3::Y,
            brake_assist: false,
            grounded_by_gravity: false,
        }
    }

    pub fn spec(&self) -> &'static RocketSpec {
        &ROCKETS[self.rocket]
    }

    fn radius(&self) -> f64 {
        self.spec().length * 0.5
    }

    pub fn nose(&self) -> DVec3 {
        self.orient * DVec3::Y
    }

    fn burning(&self) -> bool {
        matches!(self.state, FlightState::Flying) && self.throttle > 0.0 && self.fuel > 0.0
    }

    /// Put a landed rocket on the ground of `b` at `local`, nose to the sky.
    fn sit_on(&mut self, u: &Universe, b: &Body, local: DVec3) {
        let rot = body_rot(b);
        let up = rot * local;
        let ground = terrain::surface_radius(b, &u.paint, local.as_vec3());
        self.pos = b.pos + up * (ground + self.radius());
        self.vel = Universe::ground_velocity(b, self.pos);
        self.orient = (rot * DQuat::from_rotation_arc(DVec3::Y, local)).normalize();
    }

    /// Steering, throttle and lift-off: everything that must happen before the physics step.
    fn pre_sim(&mut self, u: &mut Universe, c: &Controls, dt: f64) {
        let spec = self.spec();
        self.throttle = (self.throttle + c.throttle * dt * 0.9).clamp(0.0, 1.0);
        self.grounded_by_gravity = false;
        match self.state.clone() {
            FlightState::Flying => {
                let a = spec.turn_rate * dt;
                let turn = DQuat::from_rotation_x(c.pitch * a) * DQuat::from_rotation_z(c.yaw * a) * DQuat::from_rotation_y(c.roll * a);
                self.orient = (self.orient * turn).normalize();
                if self.brake_assist && c.pitch == 0.0 && c.yaw == 0.0 {
                    let reference = u.reference_body(self.pos).map(|i| Universe::ground_velocity(&u.bodies[i], self.pos)).unwrap_or(DVec3::ZERO);
                    let rel = self.vel - reference;
                    if rel.length() > 0.05 {
                        let (axis, angle) = DQuat::from_rotation_arc(self.nose(), -rel.normalize()).to_axis_angle();
                        self.orient = (DQuat::from_axis_angle(axis, angle.min(a * 1.5)) * self.orient).normalize();
                    }
                }
                let thrust = if self.burning() { self.nose() * spec.accel * self.throttle } else { DVec3::ZERO };
                let (pos, vel, radius) = (self.pos, self.vel, self.radius());
                u.ship.get_or_insert_with(|| Ship::new(pos, vel, radius)).thrust = thrust;
            }
            FlightState::Landed { body, local } => {
                u.ship = None;
                let Some(i) = u.index_of(body) else {
                    // The world under the rocket is gone: it is adrift.
                    self.state = FlightState::Flying;
                    return;
                };
                let b = u.bodies[i].clone();
                self.sit_on(u, &b, local);
                self.fuel = (self.fuel + dt * 0.15).min(1.0);
                if self.throttle > 0.02 {
                    let up = (self.pos - b.pos).normalize();
                    let weight = -u.gravity_at(self.pos).dot(up);
                    if spec.accel * self.throttle > weight * 1.02 {
                        self.state = FlightState::Flying;
                        self.pos += up * 0.003;
                        // A nudge, not a kick: tiny moons have escape speeds well under 1.
                        self.vel = Universe::ground_velocity(&b, self.pos) + up * 0.02;
                        let mut ship = Ship::new(self.pos, self.vel, self.radius());
                        ship.thrust = up * spec.accel * self.throttle;
                        u.ship = Some(ship);
                        u.event(format!("{} lifted off from {}", spec.name, b.name));
                    } else if self.throttle > 0.98 {
                        self.grounded_by_gravity = true;
                    }
                }
            }
            FlightState::Destroyed { .. } => {
                u.ship = None;
                self.throttle = 0.0;
            }
        }
    }

    /// Read the ship back after the physics step and react to touching the ground.
    fn post_sim(&mut self, u: &mut Universe, dt: f64, sim_dt: f64) {
        let spec = self.spec();
        match self.state.clone() {
            FlightState::Flying => {
                if self.burning() {
                    self.fuel = (self.fuel - self.throttle * sim_dt / spec.burn_time).max(0.0);
                }
                if let Some(s) = u.ship.as_mut() {
                    self.pos = s.pos;
                    self.vel = s.vel;
                    if let Some(hit) = s.hit.take() {
                        self.touch_down(u, hit);
                    }
                }
            }
            // The world has moved on during the step: go with it, or the rocket
            // would trail a frame behind its landing site.
            FlightState::Landed { body, local } => {
                if let Some(b) = u.get(body).cloned() {
                    self.sit_on(u, &b, local);
                }
            }
            FlightState::Destroyed { anchor, .. } => {
                if let Some((b, offset)) = anchor.and_then(|(id, o)| u.get(id).map(|b| (b, o))) {
                    self.pos = b.pos + offset;
                    self.vel = b.vel;
                }
            }
        }
        // Chase camera "up": away from the world we're near, else the system's north.
        let target = u
            .reference_body(self.pos)
            .map(|i| &u.bodies[i])
            .filter(|b| (self.pos - b.pos).length() < b.radius * 5.0)
            .and_then(|b| (self.pos - b.pos).try_normalize())
            .unwrap_or(DVec3::Y);
        self.cam_up = self.cam_up.lerp(target, 1.0 - (-dt * 3.0).exp()).try_normalize().unwrap_or(target);
    }

    fn touch_down(&mut self, u: &mut Universe, hit: ShipHit) {
        let spec = self.spec();
        let Some(i) = u.index_of(hit.body) else { return };
        let (name, kind) = (u.bodies[i].name.clone(), u.bodies[i].kind);
        let reason = match kind {
            BodyKind::BlackHole => Some(format!("{} fell into {name} and was never seen again", spec.name)),
            BodyKind::Star | BodyKind::NeutronStar => Some(format!("{} burned up in {name}", spec.name)),
            BodyKind::GasGiant | BodyKind::IceGiant => Some(format!("{} sank into the clouds of {name} and was crushed", spec.name)),
            _ if hit.speed > spec.max_landing_speed => Some(format!("{} crashed into {name} at speed {:.1}", spec.name, hit.speed)),
            _ => None,
        };
        match reason {
            None => {
                let b = u.bodies[i].clone();
                let local = (body_rot(&b).inverse() * hit.normal).normalize();
                self.state = FlightState::Landed { body: hit.body, local };
                self.throttle = 0.0;
                self.sit_on(u, &b, local);
                u.ship = None;
                u.event(format!("{} landed on {name} (touchdown speed {:.1})", spec.name, hit.speed));
            }
            Some(reason) => {
                let b = &u.bodies[i];
                let (anchor, ground_vel) = ((hit.body, self.pos - b.pos), Universe::ground_velocity(b, self.pos));
                let size = spec.length as f32;
                for _ in 0..160 {
                    let dir = (u.rng.unit() + hit.normal * 0.9).normalize();
                    let v = ground_vel + dir * u.rng.range(0.3, 4.0);
                    let c = Vec3::new(1.0, u.rng.rangef(0.35, 0.8), 0.15) * u.rng.rangef(1.0, 3.0);
                    u.emit(self.pos, v, c, size * u.rng.rangef(0.08, 0.35), u.rng.rangef(0.4, 2.5), P_HOT | P_GRAVITY);
                }
                u.flashes.push(Flash { pos: self.pos, color: Vec3::new(1.0, 0.7, 0.4), intensity: 1.5, decay: 2.0, size: size * 25.0 });
                if terrain::has_solid_surface(kind) {
                    let angular = (spec.length * 1.2 / u.bodies[i].radius).clamp(0.015, 0.25) as f32;
                    u.crater(i, hit.normal, angular, 0.0);
                }
                u.ship = None;
                self.state = FlightState::Destroyed { reason: reason.clone(), anchor: Some(anchor) };
                u.event(reason);
            }
        }
    }

    fn frame(&self, u: &Universe, fov: f32, w: f32, h: f32) -> CameraFrame {
        let len = self.spec().length;
        if self.view == FlightView::Cockpit && !matches!(self.state, FlightState::Destroyed { .. }) {
            let eye = self.pos + self.nose() * (len * 0.3);
            return CameraFrame::look(eye, self.nose(), self.orient * DVec3::Z, FIRST_PERSON_FOV, 0.0008, w, h);
        }
        let up = self.cam_up;
        let e1 = tangent(up);
        let e2 = up.cross(e1);
        let (sy, cy) = self.cam_yaw.sin_cos();
        let (sp, cp) = self.cam_pitch.sin_cos();
        let dir = (e1 * cy + e2 * sy) * cp + up * sp;
        let mut eye = self.pos + dir * self.cam_dist;
        // Keep the camera out of the ground.
        if let Some(b) = u.reference_body(self.pos).map(|i| &u.bodies[i]) {
            let d = eye - b.pos;
            let floor = ground_radius(u, b, eye) + 0.004;
            if d.length() < floor {
                eye = b.pos + d.try_normalize().unwrap_or(up) * floor;
            }
        }
        let near = (self.cam_dist * 0.02).clamp(0.0005, 2.0) as f32;
        CameraFrame::look(eye, self.pos - eye, up, fov, near, w, h)
    }
}

impl Walk {
    /// World-space position of the feet, the up direction and the facing direction.
    fn pose(&self, u: &Universe, b: &Body) -> (DVec3, DVec3, DVec3) {
        let rot = body_rot(b);
        let up = rot * self.local;
        let ground = terrain::surface_radius(b, &u.paint, self.local.as_vec3());
        let feet = b.pos + up * (ground + self.height);
        let (sp, cp) = self.pitch.sin_cos();
        (feet, up, rot * (self.heading * cp + self.local * sp))
    }

    fn update(&mut self, u: &Universe, b: &Body, c: &Controls, jump: bool, dt: f64) {
        // Turn, then step along the ground. Angles are distances over the surface radius.
        self.heading = (DQuat::from_axis_angle(self.local, -c.turn * 1.8 * dt) * self.heading).normalize();
        let right = self.heading.cross(self.local);
        let speed = WALK_SPEED * if c.run { RUN_FACTOR } else { 1.0 };
        let step = (self.heading * c.forward + right * c.strafe) * speed * dt;
        let ground = terrain::surface_radius(b, &u.paint, self.local.as_vec3()).max(1e-6);
        self.local = (self.local + step / ground).normalize();
        self.heading = (self.heading - self.local * self.heading.dot(self.local)).try_normalize().unwrap_or_else(|| tangent(self.local));
        // Jumping: the lighter the world, the higher and slower the hop.
        let gravity_g = u.g * b.mass / (b.radius * b.radius).max(1e-9) / 10.0;
        let g = gravity_g.clamp(0.03, 5.0) * 0.25;
        if jump && self.height <= 0.0 {
            self.vspeed = JUMP_SPEED;
        }
        if self.height > 0.0 || self.vspeed > 0.0 {
            self.height += self.vspeed * dt;
            self.vspeed -= g * dt;
            if self.height <= 0.0 {
                self.height = 0.0;
                self.vspeed = 0.0;
            }
        }
    }
}

/// Atmosphere seen from `eye`: sky colour + coverage, and the local up direction.
fn sky_at(u: &Universe, eye: DVec3) -> ([f32; 4], Vec3) {
    let near = u
        .bodies
        .iter()
        .filter(|b| !b.kind.is_stellar() && b.surface.atmo_strength > 0.01)
        .map(|b| (((eye - b.pos).length() - b.radius) / b.radius, b))
        .min_by(|a, b| a.0.total_cmp(&b.0));
    let Some((h, b)) = near.filter(|(h, _)| *h < 0.4) else { return ([0.0; 4], Vec3::Y) };
    let up = (eye - b.pos).try_normalize().unwrap_or(DVec3::Y);
    let sun_height = conditions::sun_direction(u, eye, Some(b.id)).map(|s| s.dot(up)).unwrap_or(-1.0);
    let day = smoothstep(-0.2, 0.25, sun_height);
    let density = (-h.max(0.0) / 0.07).exp();
    let strength = (b.surface.atmo_strength as f64 * 1.2).clamp(0.0, 1.0);
    // Low sun: the sky turns orange.
    let dusk = (1.0 - smoothstep(0.0, 0.35, sun_height.abs())) * 0.6;
    let colour = b.surface.atmosphere.as_dvec3().lerp(DVec3::new(1.0, 0.45, 0.2), dusk) * (0.25 + 0.65 * day);
    let cover = (density * strength * day) as f32;
    ([colour.x as f32, colour.y as f32, colour.z as f32, cover], up.as_vec3())
}

/// Is `pos` in sunlight (not in the shadow of any body)? 1 = lit, 0 = shadowed.
fn sunlight_reaching(u: &Universe, pos: DVec3) -> f32 {
    let Some(sun) = conditions::sun_direction(u, pos, None) else { return 1.0 };
    let shadowed = u.bodies.iter().filter(|b| !b.kind.is_luminous()).any(|b| ray_sphere(pos, sun, b.pos, b.radius * 0.995).is_some() && (b.pos - pos).length() > b.radius * 0.995);
    if shadowed { 0.0 } else { 1.0 }
}

impl Game {
    pub fn held(&self, key: &str) -> bool {
        self.keys.contains(key)
    }

    fn controls(&self) -> Controls {
        let axis = |pos: &[&str], neg: &[&str]| {
            let on = |keys: &[&str]| keys.iter().any(|k| self.held(k)) as i32 as f64;
            on(pos) - on(neg)
        };
        Controls {
            pitch: axis(&["s", "down"], &["w", "up"]),
            yaw: axis(&["a", "left"], &["d", "right"]),
            roll: axis(&["e"], &["q"]),
            throttle: axis(&["shift"], &["ctrl"]),
            forward: axis(&["w", "up"], &["s", "down"]),
            strafe: axis(&["d"], &["a"]),
            turn: axis(&["right", "e"], &["left", "q"]),
            run: self.held("shift"),
        }
    }

    pub fn exploring(&self) -> bool {
        !matches!(self.mode, Mode::Sandbox)
    }

    fn enter_mode(&mut self, mode: Mode) {
        if self.saved_time_scale.is_none() {
            self.saved_time_scale = Some(self.time_scale);
        }
        self.time_scale = if matches!(mode, Mode::Walk(_)) { WALK_TIME_SCALE } else { FLIGHT_TIME_SCALE };
        self.paused = false;
        self.camera.follow = None;
        self.mode = mode;
    }

    /// A pleasant default spot on `b`: in morning sunshine, a little north of the equator.
    fn sunny_side(&self, b: &Body) -> DVec3 {
        match conditions::sun_direction(&self.universe, b.pos, Some(b.id)) {
            Some(sun) => {
                let side = sun.cross(DVec3::Y).try_normalize().unwrap_or(DVec3::X);
                (sun * 0.75 + side * 0.6 + DVec3::Y * 0.25).normalize()
            }
            None => (self.camera.eye() - b.pos).try_normalize().unwrap_or(DVec3::Y),
        }
    }

    /// Where "Launch" starts from: the selected body, else a sensible default.
    pub fn launch_site(&self) -> Option<&Body> {
        self.selected.or_else(|| self.default_launch_site()).and_then(|id| self.universe.get(id))
    }

    /// A solid world to start from when nothing is selected: an Earth-like one if there is one.
    fn default_launch_site(&self) -> Option<u64> {
        let solid = || self.universe.bodies.iter().filter(|b| terrain::has_solid_surface(b.kind) && !b.name.is_empty());
        solid().find(|b| b.kind == BodyKind::Earthlike).or_else(|| solid().max_by(|a, b| a.mass.total_cmp(&b.mass))).map(|b| b.id)
    }

    /// Start flying the selected rocket. With a solid `body` it starts landed there
    /// (at world direction `dir` from its centre); around anything else it starts in orbit.
    pub fn start_flight(&mut self, body: Option<u64>, dir: Option<DVec3>) {
        let body = body.or(self.selected).or_else(|| self.default_launch_site());
        let mut f = Flight::new(self.rocket.min(ROCKETS.len() - 1));
        let spec = f.spec();
        let cam_target = self.camera.target;
        let sunny = body.and_then(|id| self.universe.get(id)).map(|b| self.sunny_side(b));
        let u = &mut self.universe;
        u.ship = None;
        match body.and_then(|id| u.index_of(id)) {
            Some(i) => {
                let b = u.bodies[i].clone();
                let dir = dir.and_then(|d| d.try_normalize()).or(sunny).unwrap_or(DVec3::Y);
                if terrain::has_solid_surface(b.kind) {
                    let local = (body_rot(&b).inverse() * dir).normalize();
                    f.state = FlightState::Landed { body: b.id, local };
                    f.sit_on(u, &b, local);
                    u.event(format!("{} is ready for lift-off on {}", spec.name, b.name));
                } else {
                    // No ground: start in a circular orbit, clear of the surface (or the accretion disk).
                    let flat = DVec3::new(dir.x, 0.0, dir.z).try_normalize().unwrap_or(DVec3::X);
                    f.pos = b.pos + flat * clear_orbit_radius(u, i);
                    // Look down past the rocket so the world it circles fills the background.
                    f.cam_pitch = 1.0;
                    f.vel = u.circular_velocity(f.pos, i);
                    f.orient = DQuat::from_rotation_arc(DVec3::Y, (f.vel - b.vel).try_normalize().unwrap_or(DVec3::X));
                    u.event(format!("{} is in orbit around {}", spec.name, b.name));
                }
                self.selected = Some(b.id);
            }
            None => {
                // Empty space: drift where the camera is looking.
                f.pos = cam_target;
                if let Some(i) = u.reference_body(f.pos) {
                    let b = &u.bodies[i];
                    let d = f.pos - b.pos;
                    if d.length() < b.radius * 3.0 {
                        f.pos = b.pos + d.try_normalize().unwrap_or(DVec3::X) * b.radius * 3.0;
                    }
                    f.vel = u.circular_velocity(f.pos, i);
                    f.orient = DQuat::from_rotation_arc(DVec3::Y, (f.vel - u.bodies[i].vel).try_normalize().unwrap_or(DVec3::X));
                }
                u.event(format!("{} launched into open space", spec.name));
            }
        }
        f.cam_up = u.reference_body(f.pos).and_then(|i| (f.pos - u.bodies[i].pos).try_normalize()).unwrap_or(DVec3::Y);
        self.enter_mode(Mode::Flight(f));
    }

    /// Step out onto `body` at world direction `dir` from its centre. Returns false
    /// (and says why) if there is no ground there.
    pub fn start_walk(&mut self, body: u64, dir: Option<DVec3>) -> bool {
        let Some(b) = self.universe.get(body).cloned() else { return false };
        if !terrain::has_solid_surface(b.kind) {
            let why = match b.kind {
                BodyKind::Star | BodyKind::NeutronStar => "is far too hot to stand on",
                BodyKind::BlackHole => "has no surface at all",
                _ => "is all clouds: there is no ground to stand on",
            };
            self.universe.event(format!("{} {why}", b.name));
            return false;
        }
        let dir = dir.and_then(|d| d.try_normalize()).unwrap_or_else(|| self.sunny_side(&b));
        let local = (body_rot(&b).inverse() * dir).normalize();
        self.universe.ship = None;
        self.universe.event(format!("You are standing on {}", b.name));
        self.selected = Some(body);
        self.enter_mode(Mode::Walk(Walk { body, local, heading: tangent(local), pitch: 0.0, height: 0.0, vspeed: 0.0, parked: None }));
        true
    }

    /// Leave flight / walking and go back to the sandbox camera.
    pub fn exit_mode(&mut self) {
        let mode = std::mem::replace(&mut self.mode, Mode::Sandbox);
        self.universe.ship = None;
        self.keys.clear();
        if let Some(ts) = self.saved_time_scale.take() {
            self.time_scale = ts;
        }
        let (pos, near) = match &mode {
            Mode::Sandbox => return,
            Mode::Flight(f) => (f.pos, self.universe.reference_body(f.pos).map(|i| self.universe.bodies[i].id)),
            Mode::Walk(w) => (self.universe.get(w.body).map(|b| b.pos).unwrap_or(self.camera.target), Some(w.body)),
        };
        self.camera.target = pos;
        match near.filter(|id| self.universe.get(*id).is_some_and(|b| (b.pos - pos).length() < b.radius * 30.0)) {
            Some(id) => {
                self.selected = Some(id);
                self.focus(id);
            }
            None => self.camera.desired_distance = self.camera.desired_distance.clamp(5.0, 400.0),
        }
    }

    /// G: climb out of a landed rocket, or back into one.
    pub fn toggle_board(&mut self) {
        match std::mem::replace(&mut self.mode, Mode::Sandbox) {
            Mode::Flight(f) => match f.state {
                FlightState::Landed { body, local } => {
                    // Step out a few paces from the rocket, looking away from it.
                    let heading = tangent(local);
                    let ground = self.universe.get(body).map(|b| b.radius).unwrap_or(1.0);
                    let feet = (local + heading * (f.spec().length * 0.9 / ground)).normalize();
                    let heading = (heading - feet * heading.dot(feet)).normalize();
                    let parked = Parked { rocket: f.rocket, local, fuel: f.fuel };
                    let name = self.universe.get(body).map(|b| b.name.clone()).unwrap_or_default();
                    self.universe.event(format!("You stepped out onto {name}"));
                    self.selected = Some(body);
                    self.mode = Mode::Walk(Walk { body, local: feet, heading, pitch: 0.0, height: 0.0, vspeed: 0.0, parked: Some(parked) });
                    self.time_scale = WALK_TIME_SCALE;
                }
                _ => {
                    if matches!(f.state, FlightState::Flying) {
                        self.universe.event("Land first, then you can get out and walk");
                    }
                    self.mode = Mode::Flight(f);
                }
            },
            Mode::Walk(w) => {
                let radius = self.universe.get(w.body).map(|b| b.radius).unwrap_or(1.0);
                let (rocket, local, fuel) = match &w.parked {
                    Some(p) if p.local.angle_between(w.local) * radius > BOARD_RANGE => {
                        self.universe.event("Your rocket is too far away: walk back to it");
                        self.mode = Mode::Walk(w);
                        return;
                    }
                    Some(p) => (p.rocket, p.local, p.fuel),
                    // No rocket here yet: one comes to pick you up.
                    None => (self.rocket.min(ROCKETS.len() - 1), w.local, 1.0),
                };
                let mut f = Flight::new(rocket);
                f.fuel = fuel;
                f.state = FlightState::Landed { body: w.body, local };
                if let Some(b) = self.universe.get(w.body).cloned() {
                    f.sit_on(&self.universe, &b, local);
                    f.cam_up = (f.pos - b.pos).try_normalize().unwrap_or(DVec3::Y);
                }
                self.universe.event(format!("You climbed aboard {}", f.spec().name));
                self.mode = Mode::Flight(f);
                self.time_scale = FLIGHT_TIME_SCALE;
            }
            Mode::Sandbox => {}
        }
    }

    /// After a crash: a fresh rocket, back where the last one came down if possible.
    pub fn relaunch(&mut self) {
        let site = match &self.mode {
            Mode::Flight(Flight { state: FlightState::Destroyed { anchor: Some((id, offset)), .. }, .. }) => Some((*id, *offset)),
            _ => None,
        };
        match site.filter(|(id, _)| self.universe.get(*id).is_some_and(|b| terrain::has_solid_surface(b.kind))) {
            Some((id, offset)) => self.start_flight(Some(id), Some(offset)),
            None => self.start_flight(None, None),
        }
    }

    /// Key presses while flying or walking. Returns true if the key was used.
    pub(crate) fn mode_key(&mut self, key: &str) -> bool {
        if !self.exploring() {
            return false;
        }
        match key {
            "escape" => self.exit_mode(),
            "g" => self.toggle_board(),
            "enter" => {
                if matches!(&self.mode, Mode::Flight(f) if matches!(f.state, FlightState::Destroyed { .. })) {
                    self.relaunch();
                }
            }
            "space" => match &mut self.mode {
                // Jumping is handled with the held key; flying keeps Space as pause.
                Mode::Walk(_) => {}
                _ => return false,
            },
            _ => match &mut self.mode {
                Mode::Flight(f) => match key {
                    "v" | "c" => f.view = if f.view == FlightView::Chase { FlightView::Cockpit } else { FlightView::Chase },
                    "z" => f.throttle = 1.0,
                    "x" => f.throttle = 0.0,
                    "r" => f.brake_assist = !f.brake_assist,
                    // Steering keys and tool keys do nothing as presses.
                    "w" | "a" | "s" | "d" | "q" | "e" | "f" | "b" | "1" | "2" | "3" | "4" | "5" | "6" | "delete" | "backspace" => {}
                    _ => return false,
                },
                Mode::Walk(_) => match key {
                    "w" | "a" | "s" | "d" | "q" | "e" | "f" | "b" | "1" | "2" | "3" | "4" | "5" | "6" | "delete" | "backspace" => {}
                    _ => return false,
                },
                Mode::Sandbox => return false,
            },
        }
        true
    }

    /// Mouse drag: orbit the chase camera, or look around on foot.
    pub(crate) fn mode_look(&mut self, dx: f64, dy: f64) {
        match &mut self.mode {
            Mode::Flight(f) => {
                f.cam_yaw -= dx * 0.005;
                f.cam_pitch = (f.cam_pitch + dy * 0.005).clamp(-1.5, 1.5);
            }
            Mode::Walk(w) => {
                w.heading = (DQuat::from_axis_angle(w.local, -dx * 0.004) * w.heading).normalize();
                w.pitch = (w.pitch - dy * 0.004).clamp(-1.45, 1.45);
            }
            Mode::Sandbox => {}
        }
    }

    pub(crate) fn mode_scroll(&mut self, amount: f64) {
        if let Mode::Flight(f) = &mut self.mode {
            let len = f.spec().length;
            f.cam_dist = (f.cam_dist * (1.0 - amount * 0.12).clamp(0.3, 3.0)).clamp(len * 2.5, len * 40_000.0);
        }
    }

    pub(crate) fn mode_pre_sim(&mut self, dt: f64) {
        let c = self.controls();
        if let Mode::Flight(f) = &mut self.mode {
            f.pre_sim(&mut self.universe, &c, dt);
        }
    }

    pub(crate) fn mode_post_sim(&mut self, dt: f64, sim_dt: f64) {
        let c = self.controls();
        let jump = self.held("space");
        let mut ground_lost = None;
        match &mut self.mode {
            Mode::Flight(f) => f.post_sim(&mut self.universe, dt, sim_dt),
            Mode::Walk(w) => match self.universe.get(w.body).filter(|b| terrain::has_solid_surface(b.kind)) {
                Some(b) => w.update(&self.universe, b, &c, jump, dt),
                None => ground_lost = Some(()),
            },
            Mode::Sandbox => {}
        }
        if ground_lost.is_some() {
            self.universe.event("The ground vanished from under your feet!");
            self.exit_mode();
        }
        self.update_flight_path();
    }

    /// Predicted path of the rocket around the world it is nearest to.
    fn update_flight_path(&mut self) {
        let Mode::Flight(f) = &self.mode else { return };
        if !matches!(f.state, FlightState::Flying) || !self.show_trails {
            return;
        }
        let u = &self.universe;
        let Some(i) = u.reference_body(f.pos) else { return };
        let b = &u.bodies[i];
        let r = (f.pos - b.pos).length().max(0.1);
        let v = (f.vel - b.vel).length().max(0.3);
        let horizon = (std::f64::consts::TAU * r / v * 1.05).clamp(1.0, 900.0);
        let (points, hit) = u.predict_relative(f.pos, f.vel, horizon, 720, Some(i));
        let color = if hit.is_some() { glam::Vec4::new(1.0, 0.5, 0.3, 0.75) } else { glam::Vec4::new(0.4, 0.9, 1.0, 0.7) };
        self.preview.push(Polyline { points, color, dashed: false, fade: true });
    }

    /// The camera for this frame, whatever the mode.
    pub fn view_frame(&self, w: f32, h: f32) -> CameraFrame {
        match &self.mode {
            Mode::Sandbox => self.camera.frame(w, h),
            Mode::Flight(f) => f.frame(&self.universe, self.camera.fov_y, w, h),
            Mode::Walk(walk) => match self.universe.get(walk.body) {
                Some(b) => {
                    let (feet, up, forward) = walk.pose(&self.universe, b);
                    CameraFrame::look(feet + up * EYE_HEIGHT, forward, up, FIRST_PERSON_FOV, 0.0008, w, h)
                }
                None => self.camera.frame(w, h),
            },
        }
    }

    /// Rockets, engine glow and the sky for this frame.
    pub fn scene_extras(&self, cam: &CameraFrame, out: &mut SceneExtras) {
        out.solids.clear();
        out.glows.clear();
        let u = &self.universe;
        let (sky, up) = sky_at(u, cam.eye);
        out.sky = sky;
        let mut lit = 1.0;
        let time = self.real_time as f32;
        match &self.mode {
            Mode::Flight(f) if !matches!(f.state, FlightState::Destroyed { .. }) => {
                let spec = f.spec();
                lit = sunlight_reaching(u, f.pos);
                let power = if f.burning() { f.throttle as f32 } else { 0.0 };
                if f.view == FlightView::Chase {
                    rocket::build_mesh(spec, &mut out.solids, cam.rel(f.pos), f.orient.as_quat(), power, time);
                }
                if power > 0.0 {
                    let tail = cam.rel(f.pos - f.nose() * spec.length * 0.6);
                    let c = spec.flame * (0.6 + power);
                    out.glows.push(GpuParticle { pos_size: [tail.x, tail.y, tail.z, spec.length as f32 * (0.6 + power)], color: [c.x, c.y, c.z, 1.0] });
                }
            }
            Mode::Walk(w) => {
                if let (Some(p), Some(b)) = (&w.parked, u.get(w.body)) {
                    let spec = &ROCKETS[p.rocket];
                    let rot = body_rot(b);
                    let pos = b.pos + rot * p.local * (terrain::surface_radius(b, &u.paint, p.local.as_vec3()) + spec.length * 0.5);
                    lit = sunlight_reaching(u, pos);
                    let orient = rot * DQuat::from_rotation_arc(DVec3::Y, p.local);
                    rocket::build_mesh(spec, &mut out.solids, cam.rel(pos), orient.as_quat(), 0.0, time);
                }
            }
            _ => {}
        }
        out.sky_up = [up.x, up.y, up.z, lit];
    }

    pub fn flight_info(&self) -> Option<FlightInfo> {
        let Mode::Flight(f) = &self.mode else { return None };
        let u = &self.universe;
        let spec = f.spec();
        let near = u.reference_body(f.pos).map(|i| &u.bodies[i]);
        let reference = near.map(|b| Universe::ground_velocity(b, f.pos)).unwrap_or(DVec3::ZERO);
        // Far from any surface, ground spin is meaningless: measure against the body itself.
        let reference = match near {
            Some(b) if (f.pos - b.pos).length() > b.radius * 3.0 => b.vel,
            _ => reference,
        };
        let rel = f.vel - reference;
        let up = near.and_then(|b| (f.pos - b.pos).try_normalize()).unwrap_or(DVec3::Y);
        let altitude = near.map(|b| ((f.pos - b.pos).length() - ground_radius(u, b, f.pos) - f.radius()).max(0.0));
        let nose = f.nose();
        let right = f.orient * DVec3::X;
        let top = f.orient * DVec3::Z;
        Some(FlightInfo {
            rocket: spec,
            view: f.view,
            throttle: f.throttle,
            fuel: f.fuel,
            flying: matches!(f.state, FlightState::Flying),
            landed_on: match &f.state {
                FlightState::Landed { body, .. } => u.get(*body).map(|b| b.name.clone()),
                _ => None,
            },
            destroyed: match &f.state {
                FlightState::Destroyed { reason, .. } => Some(reason.clone()),
                _ => None,
            },
            near: near.map(|b| b.name.clone()),
            speed: rel.length(),
            vertical_speed: rel.dot(up),
            altitude,
            gravity_g: u.gravity_at(f.pos).length() / 10.0,
            prograde: (rel.length() > 0.05).then(|| rel.normalize()),
            pitch: nose.dot(up).clamp(-1.0, 1.0).asin(),
            roll: right.dot(up).atan2(top.dot(up)),
            brake_assist: f.brake_assist,
            grounded_by_gravity: f.grounded_by_gravity,
            safe_landing_speed: spec.max_landing_speed,
        })
    }

    pub fn walk_info(&self) -> Option<WalkInfo> {
        let Mode::Walk(w) = &self.mode else { return None };
        let u = &self.universe;
        let b = u.get(w.body)?;
        let cond = conditions::conditions(u, b);
        let ground = terrain::ground(b, &u.paint, w.local.as_vec3());
        let up = body_rot(b) * w.local;
        let sun = conditions::sun_direction(u, b.pos, Some(b.id));
        let sun_height = sun.map(|s| s.dot(up)).unwrap_or(-1.0);
        let sky = match sun {
            None => "Starless dark",
            Some(_) if sun_height > 0.2 => "Daytime",
            Some(_) if sun_height > -0.1 => "Sun on the horizon",
            Some(_) => "Night",
        };
        Some(WalkInfo {
            body: b.name.clone(),
            ground: terrain::ground_name(b, &ground),
            local_temp_k: cond.local_temperature(sun_height, w.local.y),
            conditions: cond,
            sky,
            airborne: w.height,
            rocket: w.parked.as_ref().map(|p| {
                let d = p.local.angle_between(w.local) * b.radius;
                (d, d <= BOARD_RANGE)
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::presets::Preset;

    fn game() -> Game {
        let mut g = Game::new();
        g.load_preset(Preset::SolarSystem);
        g
    }

    fn id(g: &Game, name: &str) -> u64 {
        g.universe.bodies.iter().find(|b| b.name == name).unwrap().id
    }

    fn run(g: &mut Game, seconds: f64) {
        let cam = g.view_frame(1600.0, 1000.0);
        for _ in 0..(seconds * 60.0) as usize {
            g.update(1.0 / 60.0, &cam, false);
        }
    }

    fn flight(g: &Game) -> &Flight {
        match &g.mode {
            Mode::Flight(f) => f,
            _ => panic!("not flying"),
        }
    }

    fn altitude(g: &Game) -> f64 {
        g.flight_info().unwrap().altitude.unwrap()
    }

    #[test]
    fn rocket_sits_on_the_ground_and_rides_the_planet() {
        let mut g = game();
        let earth = id(&g, "Earth");
        g.start_flight(Some(earth), Some(DVec3::new(1.0, 0.3, 0.2)));
        assert!(matches!(flight(&g).state, FlightState::Landed { .. }));
        assert_eq!(g.time_scale, FLIGHT_TIME_SCALE);
        let start = g.universe.get(earth).unwrap().pos;
        run(&mut g, 2.0);
        // Earth has moved a long way along its orbit, and the rocket went with it.
        let b = g.universe.get(earth).unwrap();
        assert!((b.pos - start).length() > 5.0);
        let feet = (flight(&g).pos - b.pos).length() - terrain::surface_radius_world(b, &g.universe.paint, flight(&g).pos - b.pos) - flight(&g).radius();
        assert!(feet.abs() < 1e-9, "feet are {feet} off the ground");
        assert!(matches!(flight(&g).state, FlightState::Landed { .. }));
        // Nose points away from the planet.
        let up = (flight(&g).pos - b.pos).normalize();
        assert!(flight(&g).nose().dot(up) > 0.999);
    }

    #[test]
    fn full_throttle_lifts_off_and_burns_fuel() {
        let mut g = game();
        let earth = id(&g, "Earth");
        g.start_flight(Some(earth), None);
        g.key_down("z");
        g.key_up("z");
        run(&mut g, 2.0);
        let f = flight(&g);
        assert!(matches!(f.state, FlightState::Flying), "{:?}", f.state);
        assert!(f.fuel < 1.0);
        assert!(altitude(&g) > 0.2, "altitude {}", altitude(&g));
        assert!(g.flight_info().unwrap().vertical_speed > 0.0);
        assert!(g.universe.log.iter().any(|l| l.contains("lifted off from Earth")));
        assert!(!g.preview.is_empty(), "a predicted path is drawn while flying");
    }

    #[test]
    fn weak_engines_cannot_leave_heavy_worlds() {
        let mut g = game();
        g.rocket = ROCKETS.iter().position(|r| r.name == "Ion Glider").unwrap();
        let earth = id(&g, "Earth");
        g.start_flight(Some(earth), None);
        g.key_down("z");
        run(&mut g, 1.0);
        assert!(matches!(flight(&g).state, FlightState::Landed { .. }));
        assert!(g.flight_info().unwrap().grounded_by_gravity);
        // ...but the Moon's gravity is no problem.
        let moon = id(&g, "Moon");
        g.start_flight(Some(moon), None);
        g.key_down("z");
        run(&mut g, 1.0);
        assert!(matches!(flight(&g).state, FlightState::Flying));
    }

    #[test]
    fn falling_back_fast_wrecks_the_rocket_and_enter_relaunches() {
        let mut g = game();
        let earth = id(&g, "Earth");
        g.start_flight(Some(earth), None);
        g.key_down("z");
        g.key_up("z");
        run(&mut g, 1.0);
        g.key_down("x");
        g.key_up("x");
        assert!(matches!(flight(&g).state, FlightState::Flying));
        // Dive straight back at the ground, far faster than the landing legs can take.
        let b = g.universe.get(earth).unwrap().clone();
        let ship = g.universe.ship.as_mut().unwrap();
        ship.vel = b.vel - (ship.pos - b.pos).normalize() * 9.0;
        run(&mut g, 10.0);
        let f = flight(&g);
        assert!(matches!(f.state, FlightState::Destroyed { .. }), "{:?} alt {}", f.state, altitude(&g));
        assert!(g.universe.ship.is_none());
        assert!(g.universe.log.iter().any(|l| l.contains("crashed into Earth")), "{:?}", g.universe.log);
        // The wreck stays with the planet.
        let before = (flight(&g).pos - g.universe.get(earth).unwrap().pos).length();
        run(&mut g, 1.0);
        let after = (flight(&g).pos - g.universe.get(earth).unwrap().pos).length();
        assert!((before - after).abs() < 1e-6);
        g.key_down("enter");
        assert!(matches!(flight(&g).state, FlightState::Landed { .. }));
        assert_eq!(flight(&g).fuel, 1.0);
    }

    #[test]
    fn gentle_touchdown_on_the_moon_lands() {
        let mut g = game();
        let moon = id(&g, "Moon");
        g.rocket = ROCKETS.iter().position(|r| r.name == "Moonhopper").unwrap();
        g.start_flight(Some(moon), None);
        g.key_down("z");
        g.key_up("z");
        run(&mut g, 0.15);
        assert!(matches!(flight(&g).state, FlightState::Flying));
        g.key_down("x");
        g.key_up("x");
        assert!(altitude(&g) > 0.0);
        run(&mut g, 60.0);
        // A short hop: it comes back down well within what the legs can take.
        assert!(matches!(flight(&g).state, FlightState::Landed { .. }), "{:?}", flight(&g).state);
        assert!(g.universe.log.iter().any(|l| l.contains("landed on Moon")), "{:?}", g.universe.log);
        // Landed rockets refuel.
        run(&mut g, 8.0);
        assert_eq!(flight(&g).fuel, 1.0);
    }

    #[test]
    fn gas_giants_and_stars_start_in_orbit_and_swallow_rockets() {
        let mut g = game();
        let jupiter = id(&g, "Jupiter");
        g.start_flight(Some(jupiter), None);
        assert!(matches!(flight(&g).state, FlightState::Flying));
        let r0 = (flight(&g).pos - g.universe.get(jupiter).unwrap().pos).length();
        run(&mut g, 6.0);
        let r1 = (flight(&g).pos - g.universe.get(jupiter).unwrap().pos).length();
        assert!((r1 - r0).abs() / r0 < 0.05, "circular orbit drifted: {r0} -> {r1}");
        // Dive straight in (fast, so no moon wanders into the way).
        let b = g.universe.get(jupiter).unwrap().clone();
        let s = g.universe.ship.as_mut().unwrap();
        s.vel = b.vel - (s.pos - b.pos).normalize() * 25.0;
        run(&mut g, 10.0);
        assert!(matches!(&flight(&g).state, FlightState::Destroyed { reason, .. } if reason.contains("clouds of Jupiter")), "{:?}", flight(&g).state);
    }

    #[test]
    fn steering_turns_the_nose_and_views_toggle() {
        let mut g = game();
        g.start_flight(Some(id(&g, "Jupiter")), None);
        let nose = flight(&g).nose();
        g.key_down("a");
        run(&mut g, 0.5);
        g.key_up("a");
        let turned = flight(&g).nose();
        assert!(nose.angle_between(turned) > 0.5, "turned {}", nose.angle_between(turned));
        run(&mut g, 0.5);
        assert!(flight(&g).nose().angle_between(turned) < 1e-6, "no drift once the key is released");
        assert_eq!(flight(&g).view, FlightView::Chase);
        g.key_down("v");
        assert_eq!(flight(&g).view, FlightView::Cockpit);
        // The cockpit camera looks along the nose.
        let cam = g.view_frame(1600.0, 1000.0);
        assert!(cam.fwd.dot(flight(&g).nose().as_vec3()) > 0.999);
    }

    #[test]
    fn walking_moves_over_the_ground_at_eye_height() {
        let mut g = game();
        let mars = id(&g, "Mars");
        assert!(g.start_walk(mars, Some(DVec3::new(0.2, 0.1, 1.0))));
        assert_eq!(g.time_scale, WALK_TIME_SCALE);
        let eye_height = |g: &Game| {
            let cam = g.view_frame(1600.0, 1000.0);
            let b = g.universe.get(mars).unwrap();
            (cam.eye - b.pos).length() - terrain::surface_radius_world(b, &g.universe.paint, cam.eye - b.pos)
        };
        // (The test re-derives the spot through an f32 rotation, hence the tolerance.)
        assert!((eye_height(&g) - EYE_HEIGHT).abs() < 1e-5);
        let start = match &g.mode {
            Mode::Walk(w) => w.local,
            _ => unreachable!(),
        };
        g.key_down("w");
        run(&mut g, 4.0);
        g.key_up("w");
        let Mode::Walk(w) = &g.mode else { panic!("not walking") };
        let walked = start.angle_between(w.local) * g.universe.get(mars).unwrap().radius;
        assert!((walked - WALK_SPEED * 4.0).abs() < 0.02, "walked {walked}");
        assert!(w.heading.dot(w.local).abs() < 1e-9, "heading stays level with the ground");
        assert!((eye_height(&g) - EYE_HEIGHT).abs() < 1e-5, "still standing on the terrain");
        let info = g.walk_info().unwrap();
        assert_eq!(info.body, "Mars");
        assert!(info.conditions.temp_k < 260.0);
    }

    #[test]
    fn jumps_are_higher_on_lighter_worlds() {
        let peak = |name: &str| {
            let mut g = game();
            let body = id(&g, name);
            g.start_walk(body, None);
            g.key_down("space");
            run(&mut g, 0.1);
            g.key_up("space");
            let mut peak: f64 = 0.0;
            let cam = g.view_frame(1600.0, 1000.0);
            for _ in 0..3000 {
                g.update(1.0 / 60.0, &cam, false);
                let Mode::Walk(w) = &g.mode else { panic!() };
                peak = peak.max(w.height);
            }
            let Mode::Walk(w) = &g.mode else { panic!() };
            assert_eq!(w.height, 0.0, "came back down on {name}");
            peak
        };
        let (earth, moon) = (peak("Earth"), peak("Moon"));
        assert!(earth > 0.003 && earth < EYE_HEIGHT, "earth jump {earth}");
        assert!(moon > earth * 4.0, "moon {moon} vs earth {earth}");
    }

    #[test]
    fn cannot_walk_on_clouds_or_stars() {
        let mut g = game();
        assert!(!g.start_walk(id(&g, "Jupiter"), None));
        assert!(!g.start_walk(id(&g, "Sun"), None));
        assert!(!g.exploring());
        assert!(g.universe.log.iter().any(|l| l.contains("no ground to stand on")));
    }

    #[test]
    fn get_out_walk_and_climb_back_in() {
        let mut g = game();
        let moon = id(&g, "Moon");
        g.start_flight(Some(moon), None);
        flight_mut(&mut g).fuel = 0.5;
        g.key_down("g");
        g.key_up("g");
        let Mode::Walk(w) = &g.mode else { panic!("should be walking") };
        assert!(w.parked.is_some());
        let info = g.walk_info().unwrap();
        assert!(info.rocket.unwrap().1, "the rocket is right there");
        // The parked rocket is drawn.
        let cam = g.view_frame(1600.0, 1000.0);
        let mut extras = SceneExtras::default();
        g.scene_extras(&cam, &mut extras);
        assert!(!extras.solids.is_empty());
        // Wander off too far to board.
        g.key_down("w");
        g.key_down("shift");
        run(&mut g, 4.0);
        g.key_up("w");
        g.key_up("shift");
        g.key_down("g");
        g.key_up("g");
        assert!(matches!(g.mode, Mode::Walk(_)), "too far to board");
        // Walk back and climb in.
        g.key_down("s");
        g.key_down("shift");
        run(&mut g, 4.0);
        g.key_up("s");
        g.key_up("shift");
        g.key_down("g");
        let f = flight(&g);
        assert!(matches!(f.state, FlightState::Landed { .. }));
        assert!((f.fuel - 0.5).abs() < 1e-9, "the tank is as it was left");
    }

    fn flight_mut(g: &mut Game) -> &mut Flight {
        match &mut g.mode {
            Mode::Flight(f) => f,
            _ => panic!("not flying"),
        }
    }

    #[test]
    fn escape_returns_to_the_sandbox_and_restores_time() {
        let mut g = game();
        g.time_scale = 8.0;
        let earth = id(&g, "Earth");
        g.start_flight(Some(earth), None);
        g.key_down("g");
        assert!(matches!(g.mode, Mode::Walk(_)));
        g.key_down("escape");
        assert!(!g.exploring());
        assert_eq!(g.time_scale, 8.0);
        assert_eq!(g.camera.follow, Some(earth));
        assert!(g.universe.ship.is_none());
    }

    #[test]
    fn loading_a_universe_ends_the_trip() {
        let mut g = game();
        g.start_flight(Some(id(&g, "Jupiter")), None);
        run(&mut g, 0.1);
        assert!(g.universe.ship.is_some());
        g.load_preset(Preset::BinaryStars);
        assert!(!g.exploring());
        assert!(g.universe.ship.is_none());
    }

    #[test]
    fn deleting_the_ground_ends_the_walk() {
        let mut g = game();
        let mars = id(&g, "Mars");
        g.start_walk(mars, None);
        g.delete(mars);
        run(&mut g, 0.1);
        assert!(!g.exploring());
    }

    #[test]
    fn sky_is_blue_by_day_and_clear_in_space() {
        let g = game();
        let earth = g.universe.bodies.iter().find(|b| b.name == "Earth").unwrap();
        let to_sun = (-earth.pos).normalize();
        let (day, up) = sky_at(&g.universe, earth.pos + to_sun * earth.radius * 1.01);
        assert!(day[3] > 0.7 && day[2] > day[0], "day sky {day:?}");
        assert!(up.dot(to_sun.as_vec3()) > 0.99);
        let (night, _) = sky_at(&g.universe, earth.pos - to_sun * earth.radius * 1.01);
        assert!(night[3] < 0.05, "night sky {night:?}");
        let (space, _) = sky_at(&g.universe, earth.pos + to_sun * earth.radius * 3.0);
        assert_eq!(space[3], 0.0);
        // Night side of the planet is in shadow; the day side is not.
        assert_eq!(sunlight_reaching(&g.universe, earth.pos - to_sun * earth.radius * 1.05), 0.0);
        assert_eq!(sunlight_reaching(&g.universe, earth.pos + to_sun * earth.radius * 1.05), 1.0);
    }
}
