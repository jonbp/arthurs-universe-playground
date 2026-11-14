//! Saving and loading universes.
//!
//! A save is one file: a short header, the universe as JSON (bodies, physics settings,
//! camera), then the painted surface layers, run-length encoded, then the particles
//! (the stars of a galaxy are particles, so they must survive a save). Unpainted texels are
//! all the same value, so a painted planet costs a few kilobytes rather than 0.5 MB.
//!
//! Saves live in a `saves` folder next to `settings.json`. `autosave.aup` is written
//! when the game closes (and every few minutes), so "Continue" can pick up from there.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use glam::{DVec3, Vec3};
use serde_json::{json, Value};

use crate::body::{Body, BodyKind, Ring, Surface};
use crate::game::Game;
use crate::paint::{PAINT_H, PAINT_W};
use crate::universe::{CollisionMode, Particle};

const MAGIC: &[u8; 8] = b"AUPSAVE\0";
/// Bumped when the format changes incompatibly. Newer versions are refused.
const VERSION: u32 = 1;
pub const EXTENSION: &str = "aup";
pub const AUTOSAVE: &str = "autosave";

/// What the save list shows, read from the JSON header only.
#[derive(Clone, Debug)]
pub struct SaveInfo {
    pub path: PathBuf,
    /// File stem: the slot name used to overwrite or delete it.
    pub slot: String,
    pub name: String,
    pub bodies: usize,
    /// Seconds since the Unix epoch.
    pub saved_at: u64,
    pub autosave: bool,
}

/// `<config dir>/saves`, next to the settings file.
pub fn saves_dir() -> Option<PathBuf> {
    crate::settings::settings_path().and_then(|p| p.parent().map(|d| d.join("saves")))
}

/// A file name for a save called `name`: letters, digits, spaces, dashes and
/// underscores are kept; anything else becomes "_".
pub fn slot_for(name: &str) -> String {
    let s: String = name
        .trim()
        .chars()
        .map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' || c == '_' { c } else { '_' })
        .take(60)
        .collect();
    let s = s.trim().to_string();
    if s.is_empty() || s.eq_ignore_ascii_case(AUTOSAVE) { "Untitled".to_string() } else { s }
}

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// "just now", "5 min ago", "3 h ago", "2 days ago".
pub fn ago(saved_at: u64, now: u64) -> String {
    let s = now.saturating_sub(saved_at);
    match s {
        0..=59 => "just now".into(),
        60..=3599 => format!("{} min ago", s / 60),
        3600..=86_399 => format!("{} h ago", s / 3600),
        86_400..=172_799 => "yesterday".into(),
        _ => format!("{} days ago", s / 86_400),
    }
}

// ── JSON helpers ───────────────────────────────────────────────────────────

fn dv(v: DVec3) -> Value {
    json!([v.x, v.y, v.z])
}
fn fv(v: Vec3) -> Value {
    json!([v.x, v.y, v.z])
}
fn num(v: &Value, k: &str) -> Option<f64> {
    v.get(k).and_then(Value::as_f64).filter(|x| x.is_finite())
}
fn f64_or(v: &Value, k: &str, d: f64) -> f64 {
    num(v, k).unwrap_or(d)
}
fn f32_or(v: &Value, k: &str, d: f32) -> f32 {
    num(v, k).map(|x| x as f32).unwrap_or(d)
}
fn dvec(v: &Value, k: &str) -> Option<DVec3> {
    let a = v.get(k)?.as_array()?;
    let c = |i: usize| a.get(i).and_then(Value::as_f64).filter(|x| x.is_finite());
    Some(DVec3::new(c(0)?, c(1)?, c(2)?))
}
fn fvec(v: &Value, k: &str, d: Vec3) -> Vec3 {
    dvec(v, k).map(|x| x.as_vec3()).unwrap_or(d)
}

fn kind_key(k: BodyKind) -> &'static str {
    match k {
        BodyKind::Star => "star",
        BodyKind::Rocky => "rocky",
        BodyKind::Earthlike => "earthlike",
        BodyKind::Desert => "desert",
        BodyKind::Cloudy => "cloudy",
        BodyKind::GasGiant => "gas_giant",
        BodyKind::IceGiant => "ice_giant",
        BodyKind::Lava => "lava",
        BodyKind::Ice => "ice",
        BodyKind::Asteroid => "asteroid",
        BodyKind::NeutronStar => "neutron_star",
        BodyKind::BlackHole => "black_hole",
    }
}
fn kind_from(s: &str) -> Option<BodyKind> {
    BodyKind::ALL.into_iter().find(|k| kind_key(*k) == s)
}
fn collisions_key(c: CollisionMode) -> &'static str {
    match c {
        CollisionMode::Realistic => "realistic",
        CollisionMode::AlwaysMerge => "merge",
        CollisionMode::AlwaysShatter => "shatter",
        CollisionMode::PassThrough => "pass_through",
    }
}
fn collisions_from(s: &str) -> Option<CollisionMode> {
    CollisionMode::ALL.into_iter().find(|c| collisions_key(*c) == s)
}

fn body_json(b: &Body) -> Value {
    let s = &b.surface;
    json!({
        "id": b.id,
        "name": b.name,
        "kind": kind_key(b.kind),
        "pos": dv(b.pos),
        "vel": dv(b.vel),
        "mass": b.mass,
        "radius": b.radius,
        "surface": {
            "colors": [fv(s.colors[0]), fv(s.colors[1]), fv(s.colors[2])],
            "atmosphere": fv(s.atmosphere),
            "atmo_strength": s.atmo_strength,
            "terrain": s.terrain,
            "sea_level": s.sea_level,
            "clouds": s.clouds,
            "seed": s.seed,
        },
        "temperature": b.temperature,
        "spin_rate": b.spin_rate,
        "spin": b.spin,
        "tilt": b.tilt,
        "ring": b.ring.as_ref().map(|r| json!({ "inner": r.inner, "outer": r.outer, "color": fv(r.color), "opacity": r.opacity })),
        "paint": b.paint_layer,
        "heat": b.heat,
        "ghost": b.ghost,
        "supernova_timer": b.supernova_timer,
        "feeding": b.feeding,
    })
}

/// Rebuild a body. Missing or malformed fields fall back to that kind's defaults, so
/// files from later versions with extra fields still load.
fn body_from(v: &Value, rng: &mut crate::util::Rng) -> Option<(Body, Option<u32>)> {
    let kind = kind_from(v.get("kind")?.as_str()?)?;
    let id = v.get("id")?.as_u64()?;
    let name = v.get("name").and_then(Value::as_str).unwrap_or("");
    let mut b = Body::new(id, name, kind, dvec(v, "pos")?, dvec(v, "vel").unwrap_or(DVec3::ZERO), rng);
    b.mass = f64_or(v, "mass", b.mass).max(1e-6);
    b.radius = f64_or(v, "radius", b.radius).max(1e-6);
    if let Some(s) = v.get("surface") {
        let d = b.surface.clone();
        let colors = s.get("colors").and_then(Value::as_array);
        let color = |i: usize| colors.and_then(|c| c.get(i)).map(|c| fvec(&json!({ "c": c }), "c", d.colors[i])).unwrap_or(d.colors[i]);
        b.surface = Surface {
            colors: [color(0), color(1), color(2)],
            atmosphere: fvec(s, "atmosphere", d.atmosphere),
            atmo_strength: f32_or(s, "atmo_strength", d.atmo_strength),
            terrain: f32_or(s, "terrain", d.terrain),
            sea_level: f32_or(s, "sea_level", d.sea_level),
            clouds: f32_or(s, "clouds", d.clouds),
            seed: f32_or(s, "seed", d.seed),
        };
    }
    b.temperature = f32_or(v, "temperature", b.temperature);
    b.spin_rate = f64_or(v, "spin_rate", b.spin_rate);
    b.spin = f64_or(v, "spin", b.spin);
    b.tilt = f32_or(v, "tilt", b.tilt);
    b.ring = v.get("ring").filter(|r| r.is_object()).map(|r| Ring {
        inner: f32_or(r, "inner", 1.0),
        outer: f32_or(r, "outer", 2.0),
        color: fvec(r, "color", Vec3::new(0.8, 0.75, 0.65)),
        opacity: f32_or(r, "opacity", 0.8),
    });
    b.heat = f32_or(v, "heat", 0.0);
    b.ghost = f64_or(v, "ghost", 0.0);
    b.supernova_timer = num(v, "supernova_timer");
    b.feeding = f32_or(v, "feeding", b.feeding);
    let paint = v.get("paint").and_then(Value::as_u64).map(|l| l as u32);
    Some((b, paint))
}

// ── Paint layers ───────────────────────────────────────────────────────────

/// (count, texel) runs.
fn rle_encode(texels: &[[u8; 4]], out: &mut Vec<u8>) {
    let mut runs: Vec<(u32, [u8; 4])> = Vec::new();
    for t in texels {
        match runs.last_mut() {
            Some((n, last)) if last == t => *n += 1,
            _ => runs.push((1, *t)),
        }
    }
    out.extend_from_slice(&(runs.len() as u32).to_le_bytes());
    for (n, t) in runs {
        out.extend_from_slice(&n.to_le_bytes());
        out.extend_from_slice(&t);
    }
}

struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.at.checked_add(n).filter(|e| *e <= self.data.len()).context("save file is truncated")?;
        let s = &self.data[self.at..end];
        self.at = end;
        Ok(s)
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn vec3(&mut self) -> Result<Vec3> {
        Ok(Vec3::new(self.f32()?, self.f32()?, self.f32()?))
    }
}

const PARTICLE_BYTES: usize = 4 * 12 + 1;

fn write_particles(ps: &[Particle], out: &mut Vec<u8>) {
    out.extend_from_slice(&(ps.len() as u32).to_le_bytes());
    out.reserve(ps.len() * PARTICLE_BYTES);
    for p in ps {
        for f in [p.pos.x, p.pos.y, p.pos.z, p.vel.x, p.vel.y, p.vel.z, p.color.x, p.color.y, p.color.z, p.size, p.life, p.max_life] {
            out.extend_from_slice(&f.to_le_bytes());
        }
        out.push(p.flags);
    }
}

fn read_particles(r: &mut Reader, max: usize) -> Result<Vec<Particle>> {
    let n = r.u32()? as usize;
    if n.saturating_mul(PARTICLE_BYTES) > r.data.len() - r.at {
        bail!("save file is truncated");
    }
    let mut out = Vec::with_capacity(n.min(max));
    for _ in 0..n {
        let p = Particle { pos: r.vec3()?, vel: r.vec3()?, color: r.vec3()?, size: r.f32()?, life: r.f32()?, max_life: r.f32()?, flags: r.take(1)?[0] };
        let finite = p.pos.is_finite() && p.vel.is_finite() && p.color.is_finite() && p.size.is_finite() && p.life.is_finite() && p.max_life.is_finite();
        if finite && out.len() < max {
            out.push(p);
        }
    }
    Ok(out)
}

fn rle_decode(r: &mut Reader) -> Result<Vec<[u8; 4]>> {
    let runs = r.u32()? as usize;
    let mut out = Vec::with_capacity(PAINT_W * PAINT_H);
    for _ in 0..runs {
        let n = r.u32()? as usize;
        let t: [u8; 4] = r.take(4)?.try_into().unwrap();
        if out.len() + n > PAINT_W * PAINT_H {
            bail!("paint layer is larger than expected");
        }
        out.extend(std::iter::repeat_n(t, n));
    }
    if out.len() != PAINT_W * PAINT_H {
        bail!("paint layer is the wrong size");
    }
    Ok(out)
}

// ── Game <-> file ──────────────────────────────────────────────────────────

impl Game {
    /// The universe as it is now, as save-file bytes.
    pub fn save_bytes(&self) -> Vec<u8> {
        let u = &self.universe;
        let bodies: Vec<&Body> = u.bodies.iter().filter(|b| b.alive).collect();
        let c = &self.camera;
        let header = json!({
            "version": VERSION,
            "app_version": env!("CARGO_PKG_VERSION"),
            "saved_at": now(),
            "name": u.name,
            "body_count": bodies.len(),
            "universe": {
                "time": u.time,
                "g": u.g,
                "collisions": collisions_key(u.collisions),
                "tidal_disruption": u.tidal_disruption,
                "base_dt": u.base_dt,
                "max_particles": u.max_particles,
                "trail_len": u.trail_len,
                "trail_interval": u.trail_interval,
            },
            "game": {
                "time_scale": self.saved_time_scale.unwrap_or(self.time_scale),
                "paused": self.paused,
                "selected": self.selected,
            },
            "camera": {
                "target": dv(c.target),
                "yaw": c.yaw,
                "pitch": c.pitch,
                "distance": c.desired_distance,
                "follow": c.follow,
            },
            "bodies": bodies.iter().map(|b| body_json(b)).collect::<Vec<_>>(),
        });
        let json = serde_json::to_vec(&header).expect("save JSON");
        let mut out = Vec::with_capacity(json.len() + 1024);
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&VERSION.to_le_bytes());
        out.extend_from_slice(&(json.len() as u32).to_le_bytes());
        out.extend_from_slice(&json);
        let layers: Vec<u32> = bodies.iter().filter_map(|b| b.paint_layer).filter(|l| u.paint.is_touched(*l)).collect();
        out.extend_from_slice(&(layers.len() as u32).to_le_bytes());
        for l in layers {
            out.extend_from_slice(&l.to_le_bytes());
            rle_encode(&u.paint.layers[l as usize], &mut out);
        }
        write_particles(&u.particles, &mut out);
        out
    }

    /// Replace the current universe with a saved one. On error the current universe is untouched.
    pub fn load_bytes(&mut self, data: &[u8]) -> Result<()> {
        let (header, mut r) = read_header(data)?;
        // Decode everything before touching the game, so a bad file changes nothing.
        let n_layers = r.u32()? as usize;
        let mut layers = Vec::with_capacity(n_layers.min(64));
        for _ in 0..n_layers {
            let key = r.u32()?;
            layers.push((key, rle_decode(&mut r)?));
        }
        let particles = read_particles(&mut r, crate::renderer::MAX_PARTICLES)?;
        let mut rng = crate::util::Rng::new(now());
        let mut bodies = Vec::new();
        for v in header.get("bodies").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]) {
            if let Some(b) = body_from(v, &mut rng) {
                bodies.push(b);
            }
        }

        self.end_trip();
        let name = header.get("name").and_then(Value::as_str).unwrap_or("Saved universe").to_string();
        let u = &mut self.universe;
        u.reset(&name);
        let uv = header.get("universe").cloned().unwrap_or(Value::Null);
        u.time = f64_or(&uv, "time", 0.0);
        u.g = f64_or(&uv, "g", 1.0).clamp(0.01, 10.0);
        u.collisions = uv.get("collisions").and_then(Value::as_str).and_then(collisions_from).unwrap_or(CollisionMode::Realistic);
        u.tidal_disruption = uv.get("tidal_disruption").and_then(Value::as_bool).unwrap_or(true);
        u.base_dt = f64_or(&uv, "base_dt", u.base_dt).clamp(0.0005, 0.02);
        u.max_particles = (f64_or(&uv, "max_particles", u.max_particles as f64) as usize).clamp(1000, crate::renderer::MAX_PARTICLES);
        u.trail_len = (f64_or(&uv, "trail_len", u.trail_len as f64) as usize).clamp(10, 3000);
        u.trail_interval = f64_or(&uv, "trail_interval", 1.0).clamp(0.1, 10.0);
        for (mut b, key) in bodies {
            if let Some(texels) = key.and_then(|k| layers.iter().find(|(l, _)| *l == k)).map(|(_, t)| t) {
                if let Some(l) = u.paint.alloc() {
                    u.paint.set_layer(l, texels);
                    b.paint_layer = Some(l);
                }
            }
            u.add(b);
        }
        u.sync_next_id();
        u.particles = particles;
        u.event(format!("Loaded {name}"));

        let gv = header.get("game").cloned().unwrap_or(Value::Null);
        self.time_scale = f64_or(&gv, "time_scale", 1.0).clamp(0.01, 1000.0);
        self.paused = gv.get("paused").and_then(Value::as_bool).unwrap_or(false);
        self.selected = gv.get("selected").and_then(Value::as_u64).filter(|id| self.universe.get(*id).is_some());
        self.hovered = None;
        let cv = header.get("camera").cloned().unwrap_or(Value::Null);
        let c = &mut self.camera;
        c.target = dvec(&cv, "target").unwrap_or(DVec3::ZERO);
        c.yaw = f64_or(&cv, "yaw", 0.0);
        c.pitch = f64_or(&cv, "pitch", 0.5).clamp(-1.55, 1.55);
        c.desired_distance = f64_or(&cv, "distance", 300.0).clamp(0.01, 1e6);
        c.distance = c.desired_distance;
        c.follow = cv.get("follow").and_then(Value::as_u64).filter(|id| self.universe.get(*id).is_some());
        Ok(())
    }

    /// Save to `dir/<slot>.aup`, writing a temporary file first so a crash mid-write
    /// never leaves a half-written save behind.
    pub fn save_to(&self, dir: &Path, slot: &str) -> Result<PathBuf> {
        std::fs::create_dir_all(dir).with_context(|| format!("couldn't create {}", dir.display()))?;
        let path = dir.join(format!("{slot}.{EXTENSION}"));
        let tmp = dir.join(format!(".{slot}.{EXTENSION}.tmp"));
        std::fs::write(&tmp, self.save_bytes()).with_context(|| format!("couldn't write {}", tmp.display()))?;
        std::fs::rename(&tmp, &path).with_context(|| format!("couldn't write {}", path.display()))?;
        Ok(path)
    }

    pub fn load_from(&mut self, path: &Path) -> Result<()> {
        let data = std::fs::read(path).with_context(|| format!("couldn't read {}", path.display()))?;
        self.load_bytes(&data)
    }
}

/// How often the game autosaves while running, in real seconds.
pub const AUTOSAVE_EVERY: f64 = 180.0;

/// The Saves window and where saves go.
#[derive(Default)]
pub struct Saves {
    /// Where saves are kept. None turns saving off (automated screenshot runs).
    pub dir: Option<PathBuf>,
    pub list: Vec<SaveInfo>,
    pub show: bool,
    /// Name typed into the "Save as" box.
    pub name: String,
    /// A save the Delete button was pressed on once: the second press deletes it.
    pub confirm_delete: Option<PathBuf>,
    /// Last result to show in the window: (text, is_error).
    pub message: Option<(String, bool)>,
    pub last_autosave: f64,
}

impl Saves {
    pub fn refresh(&mut self) {
        self.list = self.dir.as_deref().map(list).unwrap_or_default();
    }

    pub fn autosave_info(&self) -> Option<&SaveInfo> {
        self.list.iter().find(|s| s.autosave)
    }
}

impl Game {
    pub fn open_saves(&mut self) {
        self.saves.refresh();
        self.saves.name = self.universe.name.clone();
        self.saves.confirm_delete = None;
        self.saves.message = None;
        self.saves.show = true;
    }

    /// Save under the name typed in the Saves window (overwriting a save of the same name).
    pub fn save_as(&mut self, name: &str) {
        let Some(dir) = self.saves.dir.clone() else { return };
        let name = name.trim();
        if !name.is_empty() {
            self.universe.name = name.to_string();
        }
        self.saves.message = Some(match self.save_to(&dir, &slot_for(name)) {
            Ok(_) => {
                self.universe.event(format!("Saved {}", self.universe.name));
                (format!("Saved \u{201c}{}\u{201d}", self.universe.name), false)
            }
            Err(e) => (format!("Couldn't save: {e:#}"), true),
        });
        self.saves.refresh();
    }

    pub fn load_save(&mut self, path: &Path) {
        match self.load_from(path) {
            Ok(()) => {
                self.saves.show = false;
                self.saves.message = None;
            }
            Err(e) => self.saves.message = Some((format!("Couldn't load that save: {e:#}"), true)),
        }
    }

    pub fn delete_save(&mut self, path: &Path) {
        self.saves.confirm_delete = None;
        if let Err(e) = std::fs::remove_file(path) {
            self.saves.message = Some((format!("Couldn't delete it: {e}"), true));
        }
        self.saves.refresh();
    }

    /// Write `autosave.aup` (on quit and every few minutes). Errors are only logged:
    /// an autosave must never get in the way of playing.
    pub fn autosave(&mut self) -> Option<anyhow::Error> {
        self.saves.last_autosave = self.real_time;
        let dir = self.saves.dir.clone()?;
        self.save_to(&dir, AUTOSAVE).err()
    }

    /// Called once a frame: autosave when it is due.
    pub fn autosave_if_due(&mut self) -> Option<anyhow::Error> {
        if self.real_time - self.saves.last_autosave < AUTOSAVE_EVERY {
            return None;
        }
        self.autosave()
    }
}

fn read_header(data: &[u8]) -> Result<(Value, Reader<'_>)> {
    let mut r = Reader { data, at: 0 };
    if r.take(8)? != MAGIC {
        bail!("not a saved universe");
    }
    let version = r.u32()?;
    if version > VERSION {
        bail!("saved by a newer version of the game");
    }
    let len = r.u32()? as usize;
    let header: Value = serde_json::from_slice(r.take(len)?).context("save file is damaged")?;
    Ok((header, r))
}

/// Every save in `dir`, autosave first, then newest first. Unreadable files are skipped.
pub fn list(dir: &Path) -> Vec<SaveInfo> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut saves: Vec<SaveInfo> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == EXTENSION))
        .filter_map(|path| {
            // Only the header is needed: read the start of the file, not the paint.
            let data = read_prefix(&path).ok()?;
            let (h, _) = read_header(&data).ok()?;
            let slot = path.file_stem()?.to_string_lossy().to_string();
            Some(SaveInfo {
                autosave: slot == AUTOSAVE,
                slot,
                name: h.get("name").and_then(Value::as_str).unwrap_or("Untitled").to_string(),
                bodies: h.get("body_count").and_then(Value::as_u64).unwrap_or(0) as usize,
                saved_at: h.get("saved_at").and_then(Value::as_u64).unwrap_or(0),
                path,
            })
        })
        .collect();
    saves.sort_by(|a, b| b.autosave.cmp(&a.autosave).then(b.saved_at.cmp(&a.saved_at)));
    saves
}

/// The magic, version and JSON header of a save, without the paint layers after it.
fn read_prefix(path: &Path) -> Result<Vec<u8>> {
    use std::io::Read;
    let mut f = std::fs::File::open(path)?;
    let mut head = [0u8; 16];
    f.read_exact(&mut head)?;
    let len = u32::from_le_bytes(head[12..16].try_into().unwrap()) as usize;
    let mut data = head.to_vec();
    data.resize(16 + len, 0);
    f.read_exact(&mut data[16..])?;
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paint::Brush;
    use crate::presets::Preset;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("aup-save-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn a_painted_universe_round_trips() {
        let mut g = Game::new();
        g.load_preset(Preset::SolarSystem);
        let earth = g.universe.bodies.iter().position(|b| b.name == "Earth").unwrap();
        let layer = g.universe.paint.alloc().unwrap();
        g.universe.bodies[earth].paint_layer = Some(layer);
        g.universe.paint.apply(layer, Vec3::X, 0.4, Brush::Lava, 1.0);
        g.universe.bodies[earth].name = "Arthur's Earth".into();
        g.universe.bodies[earth].ring = Some(Ring { inner: 3.0, outer: 5.0, color: Vec3::ONE, opacity: 0.5 });
        g.universe.time = 42.0;
        g.time_scale = 8.0;
        let painted = g.universe.paint.layers[layer as usize].clone();
        let before: Vec<(u64, String, DVec3, f64)> = g.universe.bodies.iter().map(|b| (b.id, b.name.clone(), b.pos, b.mass)).collect();

        let bytes = g.save_bytes();
        assert!(bytes.len() < 400_000, "save is {} bytes", bytes.len());

        let mut h = Game::new();
        h.load_preset(Preset::BinaryStars);
        h.load_bytes(&bytes).unwrap();
        let after: Vec<(u64, String, DVec3, f64)> = h.universe.bodies.iter().map(|b| (b.id, b.name.clone(), b.pos, b.mass)).collect();
        assert_eq!(before, after);
        assert_eq!(h.universe.time, 42.0);
        assert_eq!(h.time_scale, 8.0);
        assert_eq!(h.universe.name, g.universe.name);
        let e = h.universe.bodies.iter().find(|b| b.name == "Arthur's Earth").unwrap();
        assert_eq!(e.ring.as_ref().unwrap().outer, 5.0);
        let l = e.paint_layer.expect("paint layer restored");
        assert_eq!(h.universe.paint.layers[l as usize], painted);
        assert!(h.universe.paint.dirty[l as usize], "uploaded to the GPU on the next frame");
        // New bodies don't reuse a loaded id.
        let new_id = h.universe.new_body("x", BodyKind::Rocky, DVec3::ZERO, DVec3::ZERO).id;
        assert!(h.universe.bodies.iter().all(|b| b.id != new_id));
        // It still simulates.
        let cam = h.camera.frame(1600.0, 1000.0);
        h.update(1.0 / 60.0, &cam, false);
    }

    #[test]
    fn galaxy_stars_survive_a_save() {
        let mut g = Game::new();
        g.load_preset(Preset::GalaxyCollision);
        let n = g.universe.particles.len();
        assert!(n > 10_000);
        let bytes = g.save_bytes();
        let mut h = Game::new();
        h.load_bytes(&bytes).unwrap();
        assert_eq!(h.universe.particles.len(), n);
        let (a, b) = (&g.universe.particles[n / 2], &h.universe.particles[n / 2]);
        assert_eq!((a.pos, a.vel, a.flags), (b.pos, b.vel, b.flags));
    }

    #[test]
    fn bad_files_are_refused_and_change_nothing() {
        let mut g = Game::new();
        let good = g.save_bytes();
        let name = g.universe.name.clone();
        assert!(g.load_bytes(b"hello").is_err());
        assert!(g.load_bytes(&good[..good.len() / 2]).is_err());
        let mut newer = good.clone();
        newer[8..12].copy_from_slice(&99u32.to_le_bytes());
        assert!(g.load_bytes(&newer).unwrap_err().to_string().contains("newer version"));
        assert_eq!(g.universe.name, name);
        assert!(!g.universe.bodies.is_empty());
    }

    #[test]
    fn saves_are_listed_newest_first_with_autosave_on_top() {
        let dir = scratch("list");
        let mut g = Game::new();
        g.save_to(&dir, AUTOSAVE).unwrap();
        g.universe.name = "Mine".into();
        g.save_to(&dir, &slot_for("Mine")).unwrap();
        std::fs::write(dir.join("junk.aup"), b"nope").unwrap();
        let saves = list(&dir);
        assert_eq!(saves.len(), 2, "{saves:?}");
        assert!(saves[0].autosave);
        assert_eq!(saves[1].name, "Mine");
        assert_eq!(saves[1].bodies, g.universe.bodies.len());
        g.load_preset(Preset::Empty);
        g.load_from(&saves[1].path).unwrap();
        assert_eq!(g.universe.name, "Mine");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_as_autosave_and_delete() {
        let dir = scratch("flow");
        let mut g = Game::new();
        g.saves.dir = Some(dir.clone());
        g.open_saves();
        assert!(g.saves.list.is_empty());
        g.save_as("Arthur's Planets");
        assert_eq!(g.universe.name, "Arthur's Planets");
        assert_eq!(g.saves.message.as_ref().map(|m| m.1), Some(false), "{:?}", g.saves.message);
        assert!(g.autosave_if_due().is_none());
        assert!(g.saves.autosave_info().is_none(), "not due yet");
        g.real_time += AUTOSAVE_EVERY + 1.0;
        assert!(g.autosave_if_due().is_none());
        g.saves.refresh();
        assert_eq!(g.saves.list.len(), 2);
        assert!(g.saves.autosave_info().is_some());
        let mine = g.saves.list[1].path.clone();
        g.load_preset(Preset::Supernova);
        g.load_save(&mine);
        assert_eq!(g.universe.name, "Arthur's Planets");
        assert!(!g.saves.show);
        g.delete_save(&mine);
        assert_eq!(g.saves.list.len(), 1);
        // No folder (screenshot runs): saving is simply off.
        let mut off = Game::new();
        assert!(off.autosave().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn slot_names_are_safe_file_names() {
        assert_eq!(slot_for("Arthur's World"), "Arthur_s World");
        assert_eq!(slot_for("../../etc"), "______etc");
        assert_eq!(slot_for("   "), "Untitled");
        assert_eq!(slot_for("autosave"), "Untitled");
        assert_eq!(ago(1000, 1030), "just now");
        assert_eq!(ago(0, 7200), "2 h ago");
    }
}
