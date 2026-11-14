//! Game state, tools and input handling.

use glam::{DVec3, Vec2, Vec4};

use crate::body::{black_hole_radius, Body, BodyKind, Ring};
use crate::camera::{ray_sphere, Camera, CameraFrame};
use crate::explore::Mode;
use crate::paint::Brush;
use crate::presets::{self, CustomParams, Preset};
use crate::scene::Polyline;
use crate::settings::Settings;
use crate::universe::Universe;
use crate::update::Updater;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Select,
    Spawn,
    Terraform,
    Smash,
    Fly,
    Walk,
}

impl Tool {
    pub const ALL: [Tool; 6] = [Tool::Select, Tool::Spawn, Tool::Terraform, Tool::Smash, Tool::Fly, Tool::Walk];
    pub fn label(self) -> &'static str {
        match self {
            Tool::Select => "Select",
            Tool::Spawn => "Spawn",
            Tool::Terraform => "Terraform",
            Tool::Smash => "Smash",
            Tool::Fly => "Fly",
            Tool::Walk => "Walk",
        }
    }
    pub fn hint(self) -> &'static str {
        match self {
            Tool::Select => "Click a body to select it. Double-click to follow it. Drag to orbit the camera.",
            Tool::Spawn => "Click to place a body in orbit. Click-and-drag to fling it with your own velocity (watch the predicted path).",
            Tool::Terraform => "Paint on a planet's surface: raise continents, dig oceans, add lava, ice or forests.",
            Tool::Smash => "Click a planet to hurl a projectile at it.",
            Tool::Fly => "Pick a rocket, then click a world to launch from that spot.",
            Tool::Walk => "Click a rocky world to stand on it and see what it is like there.",
        }
    }
}

pub struct SpawnSettings {
    pub kind: BodyKind,
    pub mass: f64,
    pub radius: f64,
    pub temperature: f32,
    pub auto_orbit: bool,
    pub retrograde: bool,
    pub rings: bool,
    pub fling_power: f64,
    pub name: String,
}

pub struct BrushSettings {
    pub brush: Brush,
    pub size: f32,
    pub strength: f32,
}

pub struct SmashSettings {
    pub kind: BodyKind,
    pub size: f64,
    pub speed: f64,
}

pub struct Game {
    pub universe: Universe,
    pub camera: Camera,
    pub tool: Tool,
    pub paused: bool,
    pub time_scale: f64,
    pub selected: Option<u64>,
    pub hovered: Option<u64>,
    pub spawn: SpawnSettings,
    pub brush: BrushSettings,
    pub smash: SmashSettings,
    pub show_ui: bool,
    /// Display, graphics and interface options (saved between runs by main.rs).
    pub settings: Settings,
    pub show_settings: bool,
    pub custom: CustomParams,
    pub show_custom: bool,
    pub show_help: bool,
    pub glyph_test: bool,
    pub preset: Preset,
    pub real_time: f64,
    pub fps: f32,
    /// Sandbox, flying a rocket, or walking on a world (see `explore.rs`).
    pub mode: Mode,
    /// The rocket picked in the hangar (index into `rocket::ROCKETS`).
    pub rocket: usize,
    /// Keys currently held down, for steering.
    pub(crate) keys: std::collections::HashSet<String>,
    /// Time scale to go back to when flight / walking ends.
    pub(crate) saved_time_scale: Option<f64>,
    pub updater: Updater,
    spawn_counter: u32,
    // input state
    pub mouse: Vec2,
    left_down: bool,
    right_down: bool,
    middle_down: bool,
    press_pos: Vec2,
    dragged: bool,
    shift: bool,
    last_click: f64,
    spawn_anchor: Option<DVec3>,
    spawn_current: Option<DVec3>,
    painting: bool,
    pub preview: Vec<Polyline>,
}

impl Game {
    pub fn new() -> Game {
        let mut g = Game {
            universe: Universe::new("", 1234),
            camera: Camera::new(),
            tool: Tool::Select,
            paused: false,
            time_scale: 1.0,
            selected: None,
            hovered: None,
            spawn: SpawnSettings {
                kind: BodyKind::Earthlike,
                mass: 40.0,
                radius: 2.0,
                temperature: 5800.0,
                auto_orbit: true,
                retrograde: false,
                rings: false,
                fling_power: 1.0,
                name: String::new(),
            },
            brush: BrushSettings { brush: Brush::Raise, size: 0.25, strength: 0.6 },
            smash: SmashSettings { kind: BodyKind::Asteroid, size: 1.0, speed: 25.0 },
            show_ui: true,
            settings: Settings::default(),
            show_settings: false,
            custom: CustomParams::default(),
            show_custom: false,
            show_help: false,
            glyph_test: false,
            preset: Preset::SolarSystem,
            real_time: 0.0,
            fps: 60.0,
            mode: Mode::Sandbox,
            rocket: 0,
            keys: std::collections::HashSet::new(),
            saved_time_scale: None,
            updater: Updater::disabled(),
            spawn_counter: 0,
            mouse: Vec2::ZERO,
            left_down: false,
            right_down: false,
            middle_down: false,
            press_pos: Vec2::ZERO,
            dragged: false,
            shift: false,
            last_click: -1.0,
            spawn_anchor: None,
            spawn_current: None,
            painting: false,
            preview: Vec::new(),
        };
        g.load_preset(Preset::SolarSystem);
        g
    }

    /// Back to the plain sandbox before the universe is replaced.
    fn end_trip(&mut self) {
        self.mode = Mode::Sandbox;
        self.saved_time_scale = None;
        self.universe.ship = None;
        self.keys.clear();
    }

    pub fn load_preset(&mut self, p: Preset) {
        self.end_trip();
        self.preset = p;
        self.universe.collisions = crate::universe::CollisionMode::Realistic;
        self.universe.trail_interval = 1.0;
        presets::build(&mut self.universe, p);
        self.reset_view(p.camera_distance());
        self.time_scale = match p {
            Preset::GalaxyCollision => 6.0,
            Preset::Protoplanetary => 5.0,
            Preset::SolarSystem | Preset::RogueBlackHole => 2.0,
            _ => 1.0,
        };
        self.paused = false;
    }

    pub fn load_custom(&mut self) {
        self.end_trip();
        let params = self.custom.clone();
        self.universe.collisions = crate::universe::CollisionMode::Realistic;
        presets::build_custom(&mut self.universe, &params);
        let extent = self.universe.bodies.iter().map(|b| b.pos.length()).fold(100.0, f64::max);
        self.reset_view((extent * 1.1).min(3000.0));
        self.paused = false;
        self.time_scale = 1.0;
    }

    fn reset_view(&mut self, dist: f64) {
        self.selected = None;
        self.hovered = None;
        self.camera.follow = None;
        self.camera.target = DVec3::ZERO;
        self.camera.distance = dist * 1.3;
        self.camera.desired_distance = dist;
        self.camera.pitch = 0.5;
    }


    pub fn focus(&mut self, id: u64) {
        if let Some(b) = self.universe.get(id) {
            let (p, r) = (b.pos, b.radius);
            let r = if b.kind == BodyKind::BlackHole { r * 3.0 } else { r };
            // Look at planets from their sunlit side (slightly off-axis so the terminator shows).
            if !b.kind.is_luminous() {
                let star = self
                    .universe
                    .bodies
                    .iter()
                    .filter(|s| s.kind.is_luminous() && s.id != id)
                    .max_by(|a, c| {
                        let la = a.luminosity() as f64 / (a.pos - p).length_squared().max(1.0);
                        let lc = c.luminosity() as f64 / (c.pos - p).length_squared().max(1.0);
                        la.total_cmp(&lc)
                    });
                if let Some(s) = star {
                    let to_star = (s.pos - p).normalize();
                    // camera sits on the lit side: forward = -to_star rotated ~40 degrees
                    let yaw = to_star.x.atan2(to_star.z) + 0.7;
                    self.camera.yaw = yaw;
                    self.camera.pitch = 0.3;
                }
            }
            self.camera.focus_on(id, p, r);
        }
    }

    // ------------------------------------------------------------------
    // Per frame
    // ------------------------------------------------------------------

    pub fn update(&mut self, dt: f64, cam: &CameraFrame, scene_has_pointer: bool) {
        self.real_time += dt;
        self.preview.clear();
        self.mode_pre_sim(dt);
        let before = self.universe.time;
        self.universe.update(dt, self.time_scale, self.paused);
        self.mode_post_sim(dt, self.universe.time - before);
        if let Some(id) = self.selected {
            if self.universe.get(id).is_none() {
                self.selected = None;
            }
        }
        if !self.exploring() {
            let follow_pos = self.camera.follow.and_then(|id| self.universe.get(id)).map(|b| b.pos);
            self.camera.update(dt, follow_pos);
        }

        self.hovered = if scene_has_pointer { self.pick(cam, self.mouse) } else { None };

        // Continuous terraforming while the mouse is held on a planet.
        if self.painting && self.left_down {
            self.paint_at(cam, dt as f32);
        }
        self.update_preview(cam, scene_has_pointer);
    }

    fn update_preview(&mut self, cam: &CameraFrame, scene_has_pointer: bool) {
        if self.exploring() || self.tool != Tool::Spawn || !scene_has_pointer && self.spawn_anchor.is_none() {
            return;
        }
        let anchor = match self.spawn_anchor.or_else(|| self.plane_hit(cam, self.mouse)) {
            Some(a) => a,
            None => return,
        };
        let vel = self.spawn_velocity(anchor);
        // Prediction horizon ~ one orbit around the dominant attractor.
        let horizon = match self.universe.dominant_attractor(anchor, None) {
            Some(i) => {
                let c = &self.universe.bodies[i];
                let r = (anchor - c.pos).length().max(1.0);
                let v = (vel - c.vel).length().max(0.1);
                (std::f64::consts::TAU * r / v * 1.05).clamp(5.0, 8000.0)
            }
            None => 120.0,
        };
        let (pts, hit) = self.universe.predict(anchor, vel, horizon, 2400);
        let color = if hit.is_some() { Vec4::new(1.0, 0.35, 0.25, 0.9) } else { Vec4::new(0.4, 0.9, 1.0, 0.8) };
        self.preview.push(Polyline { points: pts, color, dashed: false, fade: true });
        if let (Some(a), Some(c)) = (self.spawn_anchor, self.spawn_current) {
            self.preview.push(Polyline { points: vec![a, c], color: Vec4::new(1.0, 0.85, 0.3, 1.0), dashed: false, fade: false });
        }
    }

    /// Point on the horizontal plane through the camera target under the cursor.
    pub fn plane_hit(&self, cam: &CameraFrame, px: Vec2) -> Option<DVec3> {
        let dir = cam.ray(px);
        let plane_y = self.camera.target.y;
        if dir.y.abs() < 1e-4 {
            return None;
        }
        let t = (plane_y - cam.eye.y) / dir.y;
        if t <= 0.0 || t > 1e6 {
            return None;
        }
        Some(cam.eye + dir * t)
    }

    fn spawn_velocity(&self, anchor: DVec3) -> DVec3 {
        let att = self.universe.dominant_attractor(anchor, None);
        let mut base = match att {
            Some(i) if self.spawn.auto_orbit => {
                let v = self.universe.circular_velocity(anchor, i);
                if self.spawn.retrograde {
                    let cv = self.universe.bodies[i].vel;
                    cv - (v - cv)
                } else {
                    v
                }
            }
            Some(i) => self.universe.bodies[i].vel,
            None => DVec3::ZERO,
        };
        if let (Some(a), Some(c)) = (self.spawn_anchor, self.spawn_current) {
            let drag = c - a;
            let scale = 0.12 * self.spawn.fling_power;
            base += drag * scale;
        }
        base
    }

    pub fn pick(&self, cam: &CameraFrame, px: Vec2) -> Option<u64> {
        let dir = cam.ray(px);
        let mut best: Option<(f64, u64)> = None;
        for b in &self.universe.bodies {
            let mut hit = ray_sphere(cam.eye, dir, b.pos, b.radius.max(0.01));
            if hit.is_none() {
                if let Some(sp) = cam.project(b.pos) {
                    let rpx = cam.screen_radius(b.pos, b.radius).max(9.0);
                    if sp.distance(px) < rpx && (!b.name.is_empty() || rpx > 9.0) {
                        hit = Some((b.pos - cam.eye).length());
                    }
                }
            }
            if let Some(t) = hit {
                if best.map(|(bt, _)| t < bt).unwrap_or(true) {
                    best = Some((t, b.id));
                }
            }
        }
        best.map(|(_, id)| id)
    }

    fn paint_at(&mut self, cam: &CameraFrame, dt: f32) {
        let dir = cam.ray(self.mouse);
        let mut best: Option<(f64, usize)> = None;
        for (i, b) in self.universe.bodies.iter().enumerate() {
            if !b.kind.paintable() {
                continue;
            }
            if let Some(t) = ray_sphere(cam.eye, dir, b.pos, b.radius * 1.02) {
                if best.map(|(bt, _)| t < bt).unwrap_or(true) {
                    best = Some((t, i));
                }
            }
        }
        let Some((t, i)) = best else { return };
        let layer = {
            let b = &self.universe.bodies[i];
            match b.paint_layer {
                Some(l) => l,
                None => match self.universe.paint.alloc() {
                    Some(l) => {
                        self.universe.bodies[i].paint_layer = Some(l);
                        l
                    }
                    None => return,
                },
            }
        };
        let b = &self.universe.bodies[i];
        let hit = cam.eye + dir * t;
        let local = b.rotation().inverse() * (hit - b.pos).normalize().as_vec3();
        let strength = (self.brush.strength * dt * 6.0).min(1.0);
        let strength = if self.brush.brush == Brush::Crater { self.brush.strength } else { strength };
        self.universe.paint.apply(layer, local, self.brush.size, self.brush.brush, strength);
        if self.brush.brush == Brush::Crater {
            // craters are one-shot per click
            self.painting = false;
        }
    }

    // ------------------------------------------------------------------
    // Input
    // ------------------------------------------------------------------

    pub fn set_shift(&mut self, s: bool) {
        self.shift = s;
    }

    pub fn mouse_move(&mut self, p: Vec2) {
        let delta = p - self.mouse;
        self.mouse = p;
        if (p - self.press_pos).length() > 4.0 && (self.left_down || self.right_down || self.middle_down) {
            self.dragged = true;
        }
        if self.exploring() {
            if self.dragged && (self.left_down || self.right_down) {
                self.mode_look(delta.x as f64, delta.y as f64);
            }
            return;
        }
        let orbiting = self.right_down || (self.left_down && !self.shift && self.tool != Tool::Spawn && !self.painting);
        let panning = self.middle_down || (self.left_down && self.shift && self.spawn_anchor.is_none());
        if panning {
            self.camera.pan(delta.x as f64, delta.y as f64);
        } else if orbiting && self.dragged {
            self.camera.rotate(delta.x as f64, delta.y as f64);
        }
    }

    pub fn mouse_button(&mut self, button: u8, pressed: bool, cam: &CameraFrame) {
        match (button, pressed) {
            (0, true) => {
                self.left_down = true;
                self.press_pos = self.mouse;
                self.dragged = false;
                match self.tool {
                    _ if self.exploring() => {}
                    Tool::Spawn if !self.shift => {
                        self.spawn_anchor = self.plane_hit(cam, self.mouse);
                        self.spawn_current = self.spawn_anchor;
                    }
                    Tool::Terraform if !self.shift => {
                        self.painting = self.pick_paintable(cam).is_some();
                    }
                    _ => {}
                }
            }
            (0, false) => {
                // A release whose press went to the UI (or another window) is not a
                // scene click: without this, pressing any panel button would also
                // click "through" it and deselect / fire at whatever is behind.
                if !self.left_down {
                    return;
                }
                self.left_down = false;
                if self.spawn_anchor.is_some() {
                    self.finish_spawn(cam);
                } else if !self.dragged {
                    self.click(cam);
                }
                self.painting = false;
            }
            (1, p) => {
                self.right_down = p;
                if p {
                    self.press_pos = self.mouse;
                    self.dragged = false;
                }
            }
            (2, p) => {
                self.middle_down = p;
                if p {
                    self.press_pos = self.mouse;
                }
            }
            _ => {}
        }
    }

    /// The window lost focus: forget held buttons, since their releases won't arrive.
    pub fn focus_lost(&mut self) {
        self.left_down = false;
        self.right_down = false;
        self.middle_down = false;
        self.shift = false;
        self.painting = false;
        self.spawn_anchor = None;
        self.spawn_current = None;
        self.keys.clear();
    }

    pub fn track_spawn_drag(&mut self, cam: &CameraFrame) {
        if self.spawn_anchor.is_some() && self.left_down {
            if let Some(p) = self.plane_hit(cam, self.mouse) {
                self.spawn_current = Some(p);
            }
        }
    }

    fn pick_paintable(&self, cam: &CameraFrame) -> Option<u64> {
        let dir = cam.ray(self.mouse);
        self.universe
            .bodies
            .iter()
            .filter(|b| b.kind.paintable())
            .filter_map(|b| ray_sphere(cam.eye, dir, b.pos, b.radius * 1.02).map(|t| (t, b.id)))
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, id)| id)
    }

    fn click(&mut self, cam: &CameraFrame) {
        let hit = self.pick(cam, self.mouse);
        let double = self.real_time - self.last_click < 0.35;
        self.last_click = self.real_time;
        match self.tool {
            // While flying or walking, a click just picks a target to inspect.
            _ if self.exploring() => {
                if let Some(id) = hit {
                    self.selected = Some(id);
                }
            }
            Tool::Smash => {
                if let Some(id) = hit {
                    self.launch_at(id, None);
                }
            }
            Tool::Fly | Tool::Walk => {
                let Some(id) = hit else { return };
                // The exact spot under the cursor, if the click landed on the globe itself.
                let dir = self.universe.get(id).and_then(|b| {
                    let ray = cam.ray(self.mouse);
                    ray_sphere(cam.eye, ray, b.pos, b.radius).map(|t| cam.eye + ray * t - b.pos)
                });
                self.selected = Some(id);
                if self.tool == Tool::Fly {
                    self.start_flight(Some(id), dir);
                } else {
                    self.start_walk(id, dir);
                }
            }
            _ => {
                if let Some(id) = hit {
                    self.selected = Some(id);
                    if double {
                        self.focus(id);
                    }
                } else if self.tool == Tool::Select {
                    self.selected = None;
                }
            }
        }
    }

    fn finish_spawn(&mut self, cam: &CameraFrame) {
        let Some(anchor) = self.spawn_anchor.take() else { return };
        self.track_spawn_drag(cam);
        // velocity uses the anchor/current pair, so restore anchor for the calculation
        self.spawn_anchor = Some(anchor);
        let vel = self.spawn_velocity(anchor);
        self.spawn_anchor = None;
        self.spawn_current = None;
        let id = self.spawn_body(anchor, vel);
        self.selected = Some(id);
    }

    pub fn make_spawn_body(&mut self, pos: DVec3, vel: DVec3) -> Body {
        self.spawn_counter += 1;
        let s = &self.spawn;
        let name = if s.name.trim().is_empty() {
            let prefix = match s.kind {
                BodyKind::Star => "Arthur's Star",
                BodyKind::BlackHole => "Arthur's Black Hole",
                BodyKind::NeutronStar => "Arthur's Pulsar",
                BodyKind::Asteroid => "Arthur's Asteroid",
                _ => "Arthur's World",
            };
            format!("{prefix} {}", self.spawn_counter)
        } else {
            s.name.clone()
        };
        let (kind, mass, radius, temp, rings) = (s.kind, s.mass, s.radius, s.temperature, s.rings);
        let mut b = self.universe.new_body(&name, kind, pos, vel);
        b.mass = mass;
        b.radius = if kind == BodyKind::BlackHole { black_hole_radius(mass) } else { radius };
        if kind == BodyKind::Star {
            b.temperature = temp;
            b.update_star_colors();
        }
        if rings {
            let r = b.radius as f32;
            b.ring = Some(Ring { inner: r * 1.3, outer: r * 2.2, color: glam::Vec3::new(0.8, 0.75, 0.65), opacity: 0.8 });
        }
        b
    }

    pub fn spawn_body(&mut self, pos: DVec3, vel: DVec3) -> u64 {
        let b = self.make_spawn_body(pos, vel);
        let name = b.name.clone();
        let id = self.universe.add(b);
        self.universe.event(format!("Spawned {name}"));
        id
    }

    /// Hurl a projectile at the target. If `from_dir` is None the projectile comes from the camera side.
    pub fn launch_at(&mut self, target: u64, from_dir: Option<DVec3>) {
        let Some(t) = self.universe.get(target) else { return };
        let (tpos, tvel, tr) = (t.pos, t.vel, t.radius);
        let kind = self.smash.kind;
        let (m0, r0) = kind.defaults();
        let size = self.smash.size;
        let mass = m0 * size.powi(3);
        let radius = if kind == BodyKind::BlackHole { black_hole_radius(mass) } else { r0 * size };
        let dir = from_dir.unwrap_or_else(|| {
            let cam_dir = (tpos - self.camera.eye()).normalize();
            // slight random offset so impacts aren't all dead-centre
            (cam_dir + self.universe.rng.unit() * 0.08).normalize()
        });
        let start = tpos - dir * (tr * 4.0 + radius * 3.0 + 12.0);
        let v = tvel + dir * self.smash.speed;
        self.spawn_counter += 1;
        let name = format!("Projectile {}", self.spawn_counter);
        let mut b = self.universe.new_body(&name, kind, start, v);
        b.mass = mass;
        b.radius = radius;
        if kind == BodyKind::Star {
            b.temperature = 4500.0;
            b.update_star_colors();
        }
        let tname = self.universe.get(target).map(|b| b.name.clone()).unwrap_or_default();
        self.universe.add(b);
        self.universe.event(format!("Launched a {} at {}!", kind.label().to_lowercase(), tname));
    }

    pub fn barrage(&mut self, target: u64, count: usize) {
        let saved = (self.smash.kind, self.smash.size);
        self.smash.kind = BodyKind::Asteroid;
        for _ in 0..count {
            self.smash.size = self.universe.rng.range(0.5, 1.6);
            let mut d = self.universe.rng.unit();
            d.y *= 0.4;
            self.launch_at(target, Some(d.normalize()));
        }
        self.smash.kind = saved.0;
        self.smash.size = saved.1;
    }

    pub fn delete(&mut self, id: u64) {
        if let Some(i) = self.universe.index_of(id) {
            let b = self.universe.bodies.remove(i);
            if let Some(l) = b.paint_layer {
                self.universe.paint.release(l);
            }
            self.universe.event(format!("Deleted {}", b.name));
        }
        if self.selected == Some(id) {
            self.selected = None;
        }
        if self.camera.follow == Some(id) {
            self.camera.follow = None;
        }
    }

    pub fn collapse_to_black_hole(&mut self, id: u64) {
        let Some(i) = self.universe.index_of(id) else { return };
        let b = &mut self.universe.bodies[i];
        let name = b.name.clone();
        if b.mass < 3000.0 {
            b.mass = 3000.0; // needs to be heavy enough to be interesting
        }
        b.kind = BodyKind::BlackHole;
        b.radius = black_hole_radius(b.mass);
        b.surface.colors = [glam::Vec3::ZERO; 3];
        b.surface.atmo_strength = 0.0;
        b.ring = None;
        b.feeding = 2.0;
        b.flash = 2.0;
        b.supernova_timer = None;
        if let Some(l) = b.paint_layer.take() {
            self.universe.paint.release(l);
        }
        self.universe.event(format!("{name} collapsed into a black hole!"));
    }

    pub fn add_moon(&mut self, id: u64) {
        let Some(p) = self.universe.get(id) else { return };
        let (ppos, pvel, pmass, pr, pname) = (p.pos, p.vel, p.mass, p.radius, p.name.clone());
        let dist = pr * self.universe.rng.range(2.2, 3.5) + 1.0;
        let ang = self.universe.rng.range(0.0, std::f64::consts::TAU);
        let off = DVec3::new(ang.cos(), 0.0, ang.sin()) * dist;
        let speed = (self.universe.g * pmass / dist).sqrt();
        let vel = pvel + DVec3::Y.cross(off).normalize() * speed;
        let kind = [BodyKind::Rocky, BodyKind::Ice, BodyKind::Lava][self.universe.rng.below(3)];
        let mut m = self.universe.new_body(&format!("{pname} moon"), kind, ppos + off, vel);
        m.mass = (pmass * 0.01).max(0.01);
        m.radius = (pr * 0.27).max(0.2);
        self.universe.add(m);
        self.universe.event(format!("{pname} gained a new moon"));
    }

    pub fn circularize(&mut self, id: u64) {
        let Some(i) = self.universe.index_of(id) else { return };
        let pos = self.universe.bodies[i].pos;
        if let Some(a) = self.universe.dominant_attractor(pos, Some(id)) {
            let v = self.universe.circular_velocity(pos, a);
            self.universe.bodies[i].vel = v;
        }
    }

    /// A key went down (not auto-repeat): remember it for steering, then run its action.
    pub fn key_down(&mut self, key: &str) {
        self.keys.insert(key.to_string());
        self.key(key);
    }

    pub fn key_up(&mut self, key: &str) {
        self.keys.remove(key);
    }

    pub fn key(&mut self, key: &str) {
        if self.mode_key(key) {
            return;
        }
        match key {
            "space" => self.paused = !self.paused,
            "1" => self.tool = Tool::Select,
            "2" => self.tool = Tool::Spawn,
            "3" => self.tool = Tool::Terraform,
            "4" => self.tool = Tool::Smash,
            "5" => self.tool = Tool::Fly,
            "6" => self.tool = Tool::Walk,
            "f" => {
                if let Some(id) = self.selected {
                    self.focus(id);
                }
            }
            "delete" | "backspace" => {
                if let Some(id) = self.selected {
                    self.delete(id);
                }
            }
            "escape" => {
                self.selected = None;
                self.camera.follow = None;
                self.spawn_anchor = None;
                self.spawn_current = None;
            }
            "=" | "+" | "]" => self.time_scale = (self.time_scale * 2.0).min(1000.0),
            "-" | "[" => self.time_scale = (self.time_scale / 2.0).max(0.01),
            "h" => self.show_ui = !self.show_ui,
            "f11" => self.settings.fullscreen = !self.settings.fullscreen,
            "t" => self.settings.trails = !self.settings.trails,
            "l" => self.settings.labels = !self.settings.labels,
            "b" => {
                if let Some(id) = self.selected {
                    self.barrage(id, 12);
                }
            }
            _ => {}
        }
    }

    pub fn scroll(&mut self, amount: f64) {
        if self.exploring() {
            self.mode_scroll(amount);
        } else {
            self.camera.zoom(amount);
        }
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    /// Drive the game through the same input entry points the window uses.
    fn setup(preset: Preset) -> (Game, CameraFrame) {
        let mut g = Game::new();
        g.load_preset(preset);
        g.camera.distance = g.camera.desired_distance;
        let cam = g.camera.frame(1600.0, 1000.0);
        (g, cam)
    }

    fn click(g: &mut Game, cam: &CameraFrame, p: Vec2) {
        g.mouse_move(p);
        g.mouse_button(0, true, cam);
        g.mouse_button(0, false, cam);
    }

    #[test]
    fn spawn_click_creates_orbiting_body() {
        let (mut g, cam) = setup(Preset::SolarSystem);
        g.tool = Tool::Spawn;
        let n = g.universe.bodies.len();
        // a point on the plane away from the sun
        click(&mut g, &cam, Vec2::new(1100.0, 650.0));
        assert_eq!(g.universe.bodies.len(), n + 1);
        let b = g.universe.bodies.last().unwrap();
        assert!(b.name.starts_with("Arthur's World"));
        // auto-orbit gives roughly circular speed around the sun
        let r = b.pos.length();
        let v = b.vel.length();
        assert!((v - (crate::presets::SUN_MASS / r).sqrt()).abs() / v < 0.25, "v={v} r={r}");
        assert_eq!(g.selected, Some(b.id));
    }

    #[test]
    fn spawn_drag_flings_with_extra_velocity() {
        let (mut g, cam) = setup(Preset::Empty);
        g.tool = Tool::Spawn;
        g.mouse_move(Vec2::new(800.0, 500.0));
        g.mouse_button(0, true, &cam);
        g.mouse_move(Vec2::new(1000.0, 500.0));
        g.track_spawn_drag(&cam);
        g.update(1.0 / 60.0, &cam, true);
        assert!(!g.preview.is_empty(), "trajectory preview should be shown while dragging");
        g.mouse_button(0, false, &cam);
        let b = g.universe.bodies.last().expect("body spawned");
        assert!(b.vel.length() > 1.0, "fling should give velocity");
    }

    #[test]
    fn terraform_drag_paints_planet() {
        let (mut g, _) = setup(Preset::SolarSystem);
        let earth = g.universe.bodies.iter().find(|b| b.name == "Earth").unwrap().id;
        g.paused = true;
        g.focus(earth);
        for _ in 0..200 {
            let cam = g.camera.frame(1600.0, 1000.0);
            g.update(1.0 / 60.0, &cam, true);
        }
        let cam = g.camera.frame(1600.0, 1000.0);
        let center = cam.project(g.universe.get(earth).unwrap().pos).unwrap();
        g.tool = Tool::Terraform;
        g.brush.brush = Brush::Lava;
        g.mouse_move(center);
        g.mouse_button(0, true, &cam);
        for _ in 0..10 {
            g.update(1.0 / 60.0, &cam, true);
        }
        g.mouse_button(0, false, &cam);
        let layer = g.universe.get(earth).unwrap().paint_layer.expect("layer allocated");
        let painted = g.universe.paint.layers[layer as usize].iter().filter(|t| t[1] / 32 == 2 && t[2] > 0).count();
        assert!(painted > 100, "only {painted} lava texels");
    }

    #[test]
    fn smash_click_launches_projectile_that_hits() {
        let (mut g, _) = setup(Preset::SolarSystem);
        let jup = g.universe.bodies.iter().find(|b| b.name == "Jupiter").unwrap().id;
        g.focus(jup);
        for _ in 0..200 {
            let cam = g.camera.frame(1600.0, 1000.0);
            g.paused = true;
            g.update(1.0 / 60.0, &cam, true);
        }
        g.paused = false;
        let cam = g.camera.frame(1600.0, 1000.0);
        let p = cam.project(g.universe.get(jup).unwrap().pos).unwrap();
        g.tool = Tool::Smash;
        let n = g.universe.bodies.len();
        click(&mut g, &cam, p);
        assert_eq!(g.universe.bodies.len(), n + 1, "projectile launched");
        for _ in 0..240 {
            let cam = g.camera.frame(1600.0, 1000.0);
            g.update(1.0 / 60.0, &cam, false);
        }
        assert!(g.universe.log.iter().any(|l| l.contains("Jupiter") && (l.contains("merged") || l.contains("smashed"))), "{:?}", g.universe.log);
    }

    #[test]
    fn release_without_press_is_not_a_click() {
        // Pressing a UI button swallows the press, but the release still reaches the game.
        let (mut g, cam) = setup(Preset::SolarSystem);
        let sun = g.universe.bodies.iter().find(|b| b.name == "Sun").unwrap();
        let (p, id) = (cam.project(sun.pos).unwrap(), sun.id);
        click(&mut g, &cam, p);
        assert_eq!(g.selected, Some(id));
        g.mouse_move(Vec2::new(5.0, 5.0));
        g.mouse_button(0, false, &cam);
        assert_eq!(g.selected, Some(id), "a bare release must not deselect");
        g.tool = Tool::Smash;
        let n = g.universe.bodies.len();
        g.mouse_move(p);
        g.mouse_button(0, false, &cam);
        assert_eq!(g.universe.bodies.len(), n, "a bare release must not fire a projectile");
    }

    #[test]
    fn collapse_cancels_supernova() {
        let (mut g, _) = setup(Preset::Supernova);
        let id = g.universe.bodies.iter().find(|b| b.supernova_timer.is_some()).unwrap().id;
        g.collapse_to_black_hole(id);
        assert!(g.universe.get(id).unwrap().supernova_timer.is_none());
    }

    #[test]
    fn select_click_picks_body() {
        let (mut g, cam) = setup(Preset::SolarSystem);
        let sun = g.universe.bodies.iter().find(|b| b.name == "Sun").unwrap();
        let p = cam.project(sun.pos).unwrap();
        let id = sun.id;
        click(&mut g, &cam, p);
        assert_eq!(g.selected, Some(id));
        click(&mut g, &cam, Vec2::new(5.0, 5.0));
        assert_eq!(g.selected, None);
    }
}
