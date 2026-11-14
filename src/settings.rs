//! User settings (display, graphics, interface), saved as JSON between runs.
//!
//! The file lives in the platform's config directory:
//! Windows `%APPDATA%\Arthur's Universe Playground\settings.json`,
//! macOS `~/Library/Application Support/Arthur's Universe Playground/settings.json`,
//! Linux `$XDG_CONFIG_HOME/arthurs-universe-playground/settings.json` (or `~/.config/...`).

use std::path::PathBuf;

use serde_json::{json, Value};

/// How finely planets are tessellated, and how close they must be to get the finer meshes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PlanetDetail {
    Low,
    Standard,
    High,
}

impl PlanetDetail {
    pub const ALL: [PlanetDetail; 3] = [PlanetDetail::Low, PlanetDetail::Standard, PlanetDetail::High];

    pub fn label(self) -> &'static str {
        match self {
            PlanetDetail::Low => "Low",
            PlanetDetail::Standard => "Standard",
            PlanetDetail::High => "High",
        }
    }

    fn key(self) -> &'static str {
        match self {
            PlanetDetail::Low => "low",
            PlanetDetail::Standard => "standard",
            PlanetDetail::High => "high",
        }
    }

    fn from_key(s: &str) -> Option<PlanetDetail> {
        PlanetDetail::ALL.into_iter().find(|d| d.key() == s)
    }
}

/// Frame-rate caps offered in the settings (0 = unlimited).
pub const FPS_LIMITS: [u32; 6] = [0, 30, 60, 120, 144, 240];
/// Resolution-scale presets offered as quick picks (the slider allows anything in between).
pub const RENDER_SCALES: [f32; 6] = [0.5, 0.67, 0.75, 1.0, 1.5, 2.0];
pub const RENDER_SCALE_MIN: f32 = 0.5;
pub const RENDER_SCALE_MAX: f32 = 2.0;
pub const UI_SCALE_MIN: f32 = 0.75;
pub const UI_SCALE_MAX: f32 = 1.5;

#[derive(Clone, PartialEq, Debug)]
pub struct Settings {
    // Display
    pub fullscreen: bool,
    pub vsync: bool,
    /// Frames per second cap; 0 = unlimited.
    pub fps_limit: u32,
    /// The 3D scene is drawn at this fraction of the window resolution, then scaled to fit.
    /// Below 1 is faster; above 1 supersamples for smoother edges.
    pub render_scale: f32,
    // Graphics
    pub planet_detail: PlanetDetail,
    pub exposure: f32,
    pub bloom: f32,
    pub stars: f32,
    pub lensing: bool,
    pub trails: bool,
    // Interface
    pub ui_scale: f32,
    pub labels: bool,
    pub show_stats: bool,
    // Sound (each 0..1)
    pub master_volume: f32,
    pub effects_volume: f32,
    pub music_volume: f32,
    pub muted: bool,
    // Gameplay
    /// Forgiving rocket flight: endless fuel, an engine strong enough for any world,
    /// soft landings at any speed and self-levelling steering.
    pub easy_flying: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            fullscreen: false,
            vsync: true,
            fps_limit: 0,
            render_scale: 1.0,
            planet_detail: PlanetDetail::Standard,
            exposure: 1.0,
            bloom: 0.6,
            stars: 1.0,
            lensing: true,
            trails: true,
            ui_scale: 1.0,
            labels: true,
            show_stats: true,
            master_volume: 0.8,
            effects_volume: 0.8,
            music_volume: 0.5,
            muted: false,
            easy_flying: false,
        }
    }
}

impl Settings {
    /// Bring every value back into its valid range (hand-edited or corrupt files).
    pub fn sanitize(&mut self) {
        let d = Settings::default();
        let fix = |v: f32, lo: f32, hi: f32, def: f32| if v.is_finite() { v.clamp(lo, hi) } else { def };
        self.render_scale = fix(self.render_scale, RENDER_SCALE_MIN, RENDER_SCALE_MAX, d.render_scale);
        self.ui_scale = fix(self.ui_scale, UI_SCALE_MIN, UI_SCALE_MAX, d.ui_scale);
        self.exposure = fix(self.exposure, 0.2, 4.0, d.exposure);
        self.bloom = fix(self.bloom, 0.0, 2.0, d.bloom);
        self.stars = fix(self.stars, 0.0, 2.0, d.stars);
        self.fps_limit = self.fps_limit.min(1000);
        self.master_volume = fix(self.master_volume, 0.0, 1.0, d.master_volume);
        self.effects_volume = fix(self.effects_volume, 0.0, 1.0, d.effects_volume);
        self.music_volume = fix(self.music_volume, 0.0, 1.0, d.music_volume);
    }

    pub fn to_json(&self) -> Value {
        // f32 -> f64 would otherwise write 0.6 as 0.6000000238418579.
        let r = |v: f32| (v as f64 * 1000.0).round() / 1000.0;
        json!({
            "fullscreen": self.fullscreen,
            "vsync": self.vsync,
            "fps_limit": self.fps_limit,
            "render_scale": r(self.render_scale),
            "planet_detail": self.planet_detail.key(),
            "exposure": r(self.exposure),
            "bloom": r(self.bloom),
            "stars": r(self.stars),
            "lensing": self.lensing,
            "trails": self.trails,
            "ui_scale": r(self.ui_scale),
            "labels": self.labels,
            "show_stats": self.show_stats,
            "master_volume": r(self.master_volume),
            "effects_volume": r(self.effects_volume),
            "music_volume": r(self.music_volume),
            "muted": self.muted,
            "easy_flying": self.easy_flying,
        })
    }

    /// Read settings from JSON. Missing or malformed entries keep their defaults, so files
    /// from older (or newer) versions still load.
    pub fn from_json(v: &Value) -> Settings {
        let mut s = Settings::default();
        let b = |k: &str, out: &mut bool| {
            if let Some(x) = v.get(k).and_then(Value::as_bool) {
                *out = x;
            }
        };
        let f = |k: &str, out: &mut f32| {
            if let Some(x) = v.get(k).and_then(Value::as_f64) {
                *out = x as f32;
            }
        };
        b("fullscreen", &mut s.fullscreen);
        b("vsync", &mut s.vsync);
        if let Some(x) = v.get("fps_limit").and_then(Value::as_u64) {
            s.fps_limit = x.min(1000) as u32;
        }
        f("render_scale", &mut s.render_scale);
        if let Some(d) = v.get("planet_detail").and_then(Value::as_str).and_then(PlanetDetail::from_key) {
            s.planet_detail = d;
        }
        f("exposure", &mut s.exposure);
        f("bloom", &mut s.bloom);
        f("stars", &mut s.stars);
        b("lensing", &mut s.lensing);
        b("trails", &mut s.trails);
        f("ui_scale", &mut s.ui_scale);
        b("labels", &mut s.labels);
        b("show_stats", &mut s.show_stats);
        f("master_volume", &mut s.master_volume);
        f("effects_volume", &mut s.effects_volume);
        f("music_volume", &mut s.music_volume);
        b("muted", &mut s.muted);
        b("easy_flying", &mut s.easy_flying);
        s.sanitize();
        s
    }

    /// Load the saved settings, or the defaults if there are none (or they can't be read).
    pub fn load() -> Settings {
        let Some(path) = settings_path() else { return Settings::default() };
        match std::fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<Value>(&text) {
                Ok(v) => Settings::from_json(&v),
                Err(e) => {
                    crate::vkctx::log(&format!("Ignoring unreadable settings file {}: {e}", path.display()));
                    Settings::default()
                }
            },
            Err(_) => Settings::default(),
        }
    }

    pub fn save(&self) -> std::io::Result<()> {
        let Some(path) = settings_path() else { return Ok(()) };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = serde_json::to_string_pretty(&self.to_json()).unwrap_or_default();
        // Write then rename, so a crash mid-write can't leave a half-written file.
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, &path)
    }
}

/// Where the settings file lives; `AUP_SETTINGS` overrides it (also used by tests and
/// automated runs that must not touch the real file).
pub fn settings_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("AUP_SETTINGS") {
        return Some(PathBuf::from(p));
    }
    let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    let dir = if cfg!(windows) {
        env("APPDATA")?.join("Arthur's Universe Playground")
    } else if cfg!(target_os = "macos") {
        env("HOME")?.join("Library/Application Support/Arthur's Universe Playground")
    } else {
        env("XDG_CONFIG_HOME").or_else(|| env("HOME").map(|h| h.join(".config")))?.join("arthurs-universe-playground")
    };
    Some(dir.join("settings.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_json() {
        let s = Settings {
            fullscreen: true,
            vsync: false,
            fps_limit: 144,
            render_scale: 0.75,
            planet_detail: PlanetDetail::High,
            exposure: 1.5,
            bloom: 0.2,
            stars: 0.5,
            lensing: false,
            trails: false,
            ui_scale: 1.25,
            labels: false,
            show_stats: false,
            master_volume: 0.25,
            effects_volume: 0.5,
            music_volume: 0.0,
            muted: true,
            easy_flying: true,
        };
        assert_eq!(Settings::from_json(&s.to_json()), s);
    }

    #[test]
    fn missing_and_bad_values_fall_back() {
        let v = serde_json::json!({ "render_scale": 50.0, "ui_scale": "big", "planet_detail": "insane", "vsync": false });
        let s = Settings::from_json(&v);
        assert_eq!(s.render_scale, RENDER_SCALE_MAX);
        assert_eq!(s.ui_scale, 1.0);
        assert_eq!(s.planet_detail, PlanetDetail::Standard);
        assert!(!s.vsync);
        assert!(s.lensing);
    }
}
