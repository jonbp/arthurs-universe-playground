//! Arthur's Universe Playground - a Vulkan solar-system sandbox.
//!
//! Change planet surfaces, spawn new worlds on new orbits, smash planets
//! together, and build your own universes with black holes and supernovae.

// Release builds on Windows are GUI-subsystem programs, so no console window opens next
// to the game. When started from a terminal we attach to that terminal's console instead
// (see `attach_parent_console`), so `--help` and log output still show up there.
#![cfg_attr(all(windows, not(debug_assertions), not(test)), windows_subsystem = "windows")]

mod body;
mod camera;
mod conditions;
mod explore;
mod game;
mod gpu_types;
mod icons;
mod mesh;
mod paint;
mod presets;
mod renderer;
mod rocket;
mod scene;
mod audio;
mod save;
mod settings;
mod terrain;
mod ui;
mod universe;
mod update;
mod util;
mod vkctx;

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use glam::Vec2;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Fullscreen, Window, WindowId};

use crate::game::Game;
use crate::renderer::Renderer;
use crate::scene::SceneOptions;
use crate::settings::Settings;
use crate::vkctx::{log, VkContext};

/// Developer / testing options from the command line.
#[derive(Default, Clone)]
struct Options {
    preset: Option<String>,
    screenshot: Option<String>,
    frames: u32,
    time_scale: Option<f64>,
    focus: Option<String>,
    supernova: Option<String>,
    smash: Option<String>,
    paint: Option<String>,
    blackhole: Option<String>,
    hide_ui: bool,
    glyph_test: bool,
    zoom: Option<f64>,
    /// Synthetic UI clicks for automated testing: (x, y) in points, at frame n.
    clicks: Vec<(f32, f32, u32)>,
    /// Start flying from / walking on this body.
    fly: Option<String>,
    walk: Option<String>,
    rocket: Option<usize>,
    cockpit: bool,
    /// Hold these keys down from the first frame (comma separated), e.g. "z" for full throttle.
    hold: Vec<String>,
    /// Open the Saves window at start (for screenshots).
    open_saves: bool,
    no_update_check: bool,
    /// Pretend to be this version when checking for updates (to test the banner).
    assume_version: Option<String>,
}

fn parse_args() -> Options {
    let mut o = Options { frames: 120, ..Default::default() };
    let args: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < args.len() {
        let next = args.get(i + 1).cloned();
        match args[i].as_str() {
            "--preset" => o.preset = next,
            "--screenshot" => o.screenshot = next,
            "--frames" => o.frames = next.and_then(|s| s.parse().ok()).unwrap_or(120),
            "--speed" => o.time_scale = next.and_then(|s| s.parse().ok()),
            "--focus" => o.focus = next,
            "--supernova" => o.supernova = next,
            "--smash" => o.smash = next,
            "--paint" => o.paint = next,
            "--blackhole" => o.blackhole = next,
            "--zoom" => o.zoom = next.and_then(|s| s.parse().ok()),
            "--fly" => o.fly = next,
            "--saves" => {
                o.open_saves = true;
                i += 1;
                continue;
            }
            "--walk" => o.walk = next,
            "--rocket" => o.rocket = next.and_then(|s| s.parse().ok()),
            "--hold" => o.hold = next.map(|s| s.split(',').map(|k| k.trim().to_lowercase()).collect()).unwrap_or_default(),
            "--assume-version" => o.assume_version = next,
            "--cockpit" => {
                o.cockpit = true;
                i += 1;
                continue;
            }
            "--no-update-check" => {
                o.no_update_check = true;
                i += 1;
                continue;
            }
            "--click" => {
                // X,Y@FRAME (points), e.g. --click 1834,54@10
                let parsed = next.as_deref().and_then(|v| {
                    let (xy, f) = v.split_once('@')?;
                    let (x, y) = xy.split_once(',')?;
                    Some((x.parse().ok()?, y.parse().ok()?, f.parse().ok()?))
                });
                match parsed {
                    Some(c) => o.clicks.push(c),
                    None => eprintln!("--click expects X,Y@FRAME"),
                }
            }
            "--glyph-test" | "--icons" => {
                o.glyph_test = true;
                i += 1;
                continue;
            }
            "--hide-ui" => {
                o.hide_ui = true;
                i += 1;
                continue;
            }
            "--help" | "-h" => {
                println!(
                    "Arthur's Universe Playground\n\nDeveloper options:\n  --preset <solar|rogue|theia|headon|binary|blackhole|supernova|galaxy|disk|empty>\n  --focus <body name>   --zoom <distance>   --speed <x>\n  --supernova <star>   --smash <body>   --paint <body>   --blackhole <body>\n  --screenshot <file.bmp> --frames <n>   --hide-ui   --icons (show the icon set)\n  --click <x,y@frame>   inject a UI click (repeatable)\n  --fly <body>   --walk <body>   --rocket <0-5>   --cockpit   --hold <key,key>\n  --saves (open the Saves window)\n  --no-update-check   --assume-version <x.y.z>\n\nSettings are saved to {}  (override with AUP_SETTINGS)",
                    settings::settings_path().map(|p| p.display().to_string()).unwrap_or_else(|| "(nowhere)".into())
                );
                std::process::exit(0);
            }
            _ => {
                i += 1;
                continue;
            }
        }
        i += 2;
    }
    o
}

fn preset_from_name(n: &str) -> Option<presets::Preset> {
    use presets::Preset::*;
    Some(match n.to_lowercase().as_str() {
        "solar" => SolarSystem,
        "rogue" => RogueBlackHole,
        "theia" => TheiaImpact,
        "headon" => HeadOn,
        "binary" => BinaryStars,
        "blackhole" => BlackHoleFeast,
        "supernova" => Supernova,
        "galaxy" => GalaxyCollision,
        "disk" => Protoplanetary,
        "empty" => Empty,
        _ => return None,
    })
}

struct State {
    window: Arc<Window>,
    renderer: Renderer,
    egui_ctx: egui::Context,
    egui_state: egui_winit::State,
    game: Game,
    sound: audio::SoundBridge,
    frame_data: gpu_types::FrameData,
    extras: explore::SceneExtras,
    last: Instant,
    frame_count: u32,
    fps_timer: f64,
    fps_frames: u32,
    opts: Options,
    /// Settings as last written to disk, and when they started to differ from the live ones.
    saved_settings: Settings,
    settings_dirty_since: Option<Instant>,
    /// Whether settings are read from / written to the settings file (not for automated runs).
    persist_settings: bool,
    /// Fullscreen state we last asked the window for, and when.
    fullscreen: bool,
    fullscreen_changed: Instant,
}

/// Desktop app id (Wayland app_id / X11 WM_CLASS); matches the Linux .desktop file.
#[cfg(all(unix, not(target_os = "macos")))]
const APP_ID: &str = "arthurs-universe-playground";

/// Title-bar / taskbar icon (Windows, X11), from the logo rasterised by build.rs.
/// macOS ignores this and uses the bundle's AppIcon.icns.
fn window_icon() -> Option<winit::window::Icon> {
    let mut rgba = include_bytes!(concat!(env!("OUT_DIR"), "/logo.rgba")).to_vec();
    for px in rgba.chunks_exact_mut(4) {
        let a = px[3] as u32;
        if a > 0 && a < 255 {
            for c in &mut px[..3] {
                *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    winit::window::Icon::from_rgba(rgba, 192, 192).ok()
}

impl State {
    fn new(event_loop: &ActiveEventLoop, opts: Options) -> Result<State> {
        // Screenshot runs are tests: they start from the defaults and never touch the user's
        // file, unless pointed at a specific one with AUP_SETTINGS.
        let persist_settings = opts.screenshot.is_none() || std::env::var_os("AUP_SETTINGS").is_some();
        let settings = if persist_settings { Settings::load() } else { Settings::default() };
        let attrs = Window::default_attributes()
            .with_title("Arthur's Universe Playground")
            .with_inner_size(LogicalSize::new(1440.0, 900.0))
            .with_min_inner_size(LogicalSize::new(640.0, 400.0))
            .with_window_icon(window_icon())
            .with_fullscreen(settings.fullscreen.then_some(Fullscreen::Borderless(None)));
        #[cfg(all(unix, not(target_os = "macos")))]
        let attrs = {
            use winit::platform::wayland::WindowAttributesExtWayland;
            use winit::platform::x11::WindowAttributesExtX11;
            let attrs = WindowAttributesExtX11::with_name(attrs, APP_ID, APP_ID);
            WindowAttributesExtWayland::with_name(attrs, APP_ID, APP_ID)
        };
        let window = Arc::new(event_loop.create_window(attrs)?);
        let size = window.inner_size();
        let ctx = VkContext::new(window.as_ref())?;
        let mut renderer = Renderer::new(ctx, size.width, size.height)?;
        renderer.set_vsync(settings.vsync);
        renderer.set_render_scale(settings.render_scale);
        let egui_ctx = egui::Context::default();
        ui::setup_style(&egui_ctx);
        // UI size is a setting; egui's own Ctrl +/- zoom would fight it (and '-' slows time).
        egui_ctx.options_mut(|o| o.zoom_with_keyboard = false);
        egui_ctx.set_zoom_factor(settings.ui_scale);
        let egui_state = egui_winit::State::new(
            egui_ctx.clone(),
            egui::ViewportId::ROOT,
            window.as_ref(),
            Some(window.scale_factor() as f32),
            None,
            Some(4096),
        );
        let mut game = Game::new();
        game.settings = settings.clone();
        // Saves sit next to the settings file; automated runs neither read nor write them.
        game.saves.dir = if persist_settings { save::saves_dir() } else { None };
        game.saves.refresh();
        if let Some(p) = opts.preset.as_deref().and_then(preset_from_name) {
            game.load_preset(p);
        }
        if let Some(s) = opts.time_scale {
            game.time_scale = s;
        }
        game.glyph_test = opts.glyph_test;
        if opts.open_saves {
            game.open_saves();
        }
        if opts.hide_ui {
            game.show_ui = false;
        }
        let find = |g: &Game, name: &str| g.universe.bodies.iter().find(|b| b.name.eq_ignore_ascii_case(name)).map(|b| b.id);
        if let Some(n) = &opts.focus {
            if let Some(id) = find(&game, n) {
                game.selected = Some(id);
                game.focus(id);
            }
        }
        if let Some(z) = opts.zoom {
            game.camera.desired_distance = z;
            game.camera.distance = z;
        }
        if let Some(n) = &opts.supernova {
            if let Some(id) = find(&game, n) {
                game.universe.trigger_supernova(id, 1.0);
            }
        }
        if let Some(n) = &opts.blackhole {
            if let Some(id) = find(&game, n) {
                game.collapse_to_black_hole(id);
            }
        }
        if let Some(n) = &opts.paint {
            if let Some(i) = game.universe.bodies.iter().position(|b| b.name.eq_ignore_ascii_case(n)) {
                let layer = game.universe.paint.alloc().unwrap();
                game.universe.bodies[i].paint_layer = Some(layer);
                let p = &mut game.universe.paint;
                use crate::paint::Brush;
                for k in 0..6 {
                    let a = k as f32 * 1.05;
                    let c = glam::Vec3::new(a.cos(), 0.25 * (k as f32 - 2.5) / 2.5, a.sin()).normalize();
                    p.apply(layer, c, 0.35, Brush::Raise, 1.0);
                    p.apply(layer, c, 0.35, Brush::Raise, 1.0);
                }
                p.apply(layer, glam::Vec3::new(1.0, 0.2, 0.3).normalize(), 0.4, Brush::Lava, 1.0);
                p.apply(layer, glam::Vec3::new(-0.3, 0.3, 1.0).normalize(), 0.3, Brush::Crater, 1.0);
                p.apply(layer, glam::Vec3::new(0.2, -0.2, -1.0).normalize(), 0.5, Brush::Ice, 1.0);
            }
        }
        if let Some(n) = &opts.smash {
            if let Some(id) = find(&game, n) {
                game.smash.kind = body::BodyKind::Rocky;
                game.smash.size = 1.5;
                game.smash.speed = 30.0;
                game.launch_at(id, None);
            }
        }
        if let Some(r) = opts.rocket {
            game.rocket = r.min(rocket::ROCKETS.len() - 1);
        }
        if let Some(n) = &opts.fly {
            game.start_flight(find(&game, n), None);
            if let (true, explore::Mode::Flight(f)) = (opts.cockpit, &mut game.mode) {
                f.view = explore::FlightView::Cockpit;
            }
        }
        if let Some(id) = opts.walk.as_ref().and_then(|n| find(&game, n)) {
            game.start_walk(id, None);
        }
        for k in &opts.hold {
            game.key_down(k);
        }
        // One quiet request to GitHub, skipped for automated runs.
        if !opts.no_update_check && (opts.screenshot.is_none() || opts.assume_version.is_some()) && std::env::var_os("AUP_NO_UPDATE_CHECK").is_none() {
            game.updater = update::Updater::start(opts.assume_version.as_deref().unwrap_or(env!("CARGO_PKG_VERSION")));
        }
        let game_fullscreen = game.settings.fullscreen;
        Ok(State {
            window,
            renderer,
            egui_ctx,
            egui_state,
            game,
            // No sound for automated screenshot runs.
            sound: audio::SoundBridge::new(if opts.screenshot.is_some() { None } else { audio::Audio::start() }),
            frame_data: gpu_types::FrameData::new(),
            extras: explore::SceneExtras::default(),
            last: Instant::now(),
            frame_count: 0,
            fps_timer: 0.0,
            fps_frames: 0,
            opts,
            saved_settings: settings,
            settings_dirty_since: None,
            persist_settings,
            fullscreen: game_fullscreen,
            fullscreen_changed: Instant::now(),
        })
    }

    /// Push changed settings to the window, renderer and egui, and save them once they
    /// have stopped changing for a moment (sliders change every frame while dragged).
    fn apply_settings(&mut self) {
        let s = &self.game.settings;
        if s.fullscreen != self.fullscreen {
            self.window.set_fullscreen(s.fullscreen.then_some(Fullscreen::Borderless(None)));
            self.fullscreen = s.fullscreen;
            self.fullscreen_changed = Instant::now();
        }
        self.renderer.set_vsync(s.vsync);
        self.renderer.set_render_scale(s.render_scale);
        if (self.egui_ctx.zoom_factor() - s.ui_scale).abs() > 1e-4 {
            self.egui_ctx.set_zoom_factor(s.ui_scale);
        }
        if !self.persist_settings || *s == self.saved_settings {
            self.settings_dirty_since = None;
            return;
        }
        let since = *self.settings_dirty_since.get_or_insert_with(Instant::now);
        if since.elapsed() > Duration::from_millis(600) {
            self.save_settings();
        }
    }

    fn save_settings(&mut self) {
        if !self.persist_settings || self.game.settings == self.saved_settings {
            return;
        }
        if let Err(e) = self.game.settings.save() {
            log(&format!("Couldn't save settings: {e}"));
        }
        self.saved_settings = self.game.settings.clone();
        self.settings_dirty_since = None;
    }

    /// Hold the frame rate down to the user's cap, if they set one.
    fn limit_frame_rate(&self) {
        let limit = self.game.settings.fps_limit;
        if limit == 0 || self.opts.screenshot.is_some() {
            return;
        }
        let min = Duration::from_secs_f64(1.0 / limit as f64);
        let elapsed = self.last.elapsed();
        if elapsed < min {
            std::thread::sleep(min - elapsed);
        }
    }

    fn frame(&mut self, event_loop: &ActiveEventLoop) -> Result<()> {
        self.apply_settings();
        if let Some(e) = self.game.autosave_if_due() {
            log(&format!("Autosave failed: {e:#}"));
        }
        self.limit_frame_rate();
        let now = Instant::now();
        let dt = (now - self.last).as_secs_f64().min(0.1);
        self.last = now;
        self.fps_timer += dt;
        self.fps_frames += 1;
        if self.fps_timer > 0.5 {
            self.game.fps = (self.fps_frames as f64 / self.fps_timer) as f32;
            self.fps_timer = 0.0;
            self.fps_frames = 0;
        }

        if self.renderer.needs_resize {
            let size = self.window.inner_size();
            if size.width == 0 || size.height == 0 {
                // Minimised: nothing to draw and no vsync to pace us, so don't spin a core.
                std::thread::sleep(std::time::Duration::from_millis(50));
                self.last = Instant::now();
                return Ok(());
            }
            self.renderer.resize(size.width, size.height)?;
        }
        let extent = self.renderer.extent();
        let (w, h) = (extent.width as f32, extent.height as f32);
        let cam = self.game.view_frame(w, h);

        let mut raw = self.egui_state.take_egui_input(&self.window);
        for &(x, y, f) in &self.opts.clicks {
            let pos = egui::pos2(x, y);
            // Like a real click: hover first, then press, then release on later frames.
            let frame = self.frame_count + 1;
            let modifiers = egui::Modifiers::NONE;
            let button = egui::PointerButton::Primary;
            if frame == f {
                raw.events.push(egui::Event::PointerMoved(pos));
            } else if frame == f + 1 {
                raw.events.push(egui::Event::PointerButton { pos, button, pressed: true, modifiers });
            } else if frame == f + 4 {
                raw.events.push(egui::Event::PointerButton { pos, button, pressed: false, modifiers });
            }
        }
        let ppp = self.egui_ctx.pixels_per_point();
        let game = &mut self.game;
        let scene_px = self.renderer.scene_extent();
        let output = self.egui_ctx.run_ui(raw, |ui| ui::draw(ui, game, &cam, ppp, (scene_px.width, scene_px.height)));
        self.egui_state.handle_platform_output(&self.window, output.platform_output);

        let scene_has_pointer = !self.egui_ctx.egui_wants_pointer_input() && !self.egui_ctx.is_pointer_over_egui();
        self.game.track_spawn_drag(&cam);
        self.game.update(dt, &cam, scene_has_pointer);
        let ui_clicked = !scene_has_pointer && self.egui_ctx.input(|i| i.pointer.primary_clicked());
        self.sound.update(&mut self.game, &cam, dt, ui_clicked);

        let cam = self.game.view_frame(w, h);
        self.game.scene_extras(&cam, &mut self.extras);
        let opts = SceneOptions {
            trails: self.game.settings.trails,
            exposure: self.game.settings.exposure,
            bloom: self.game.settings.bloom,
            lensing: self.game.settings.lensing,
            stars: self.game.settings.stars,
            planet_detail: self.game.settings.planet_detail,
            selected: self.game.selected,
            hovered: self.game.hovered,
            time: self.game.real_time as f32,
        };
        scene::build_frame(&mut self.frame_data, &self.game.universe, &cam, &opts, &self.game.preview, &self.extras);
        // Frosted side panels: tell the composite pass where to blur the scene (screen uv).
        let (rects, radius) = ui::take_blur_rects(&self.egui_ctx);
        let win = self.window.inner_size();
        let (ww, wh) = (win.width.max(1) as f32, win.height.max(1) as f32);
        let globals = &mut self.frame_data.globals;
        let n = rects.len().min(gpu_types::MAX_UI_BLUR);
        for (slot, r) in globals.ui_blur.iter_mut().zip(&rects) {
            *slot = [r.min.x * ppp / ww, r.min.y * ppp / wh, r.max.x * ppp / ww, r.max.y * ppp / wh];
        }
        globals.ui_blur_params = [n as f32, radius * ppp, 0.0, 0.0];
        let prims = self.egui_ctx.tessellate(output.shapes, output.pixels_per_point);

        self.frame_count += 1;
        let want_shot = self.opts.screenshot.is_some() && self.frame_count == self.opts.frames;
        if want_shot {
            self.renderer.capture = true;
        }
        let rendered = self.renderer.render(&self.frame_data, &mut self.game.universe.paint, &prims, &output.textures_delta, output.pixels_per_point);
        // The renderer has applied (or deliberately skipped) the texture changes; egui's
        // debug builds assert that a delta is never dropped without being acknowledged.
        let mut textures_delta = output.textures_delta;
        textures_delta.clear();
        let ok = rendered?;
        if !ok {
            self.renderer.needs_resize = true;
        }
        if let (Some((w, h, data)), Some(path)) = (self.renderer.captured.take(), self.opts.screenshot.clone()) {
            write_bmp(&path, w, h, &data, self.renderer.swapchain.format)?;
            log(&format!("Saved screenshot to {path} ({w}x{h}) after {} frames, sim t={:.1}", self.frame_count, self.game.universe.time));
            log(&format!(
                "bodies={} particles={} fps={:.0}",
                self.game.universe.bodies.len(),
                self.game.universe.particles.len(),
                self.game.fps
            ));
            for l in &self.game.universe.log {
                log(&format!("  {l}"));
            }
            event_loop.exit();
        }
        Ok(())
    }
}

/// Write BGRA/RGBA8 pixels as a 24-bit BMP.
fn write_bmp(path: &str, w: u32, h: u32, data: &[u8], format: ash::vk::Format) -> Result<()> {
    let bgra = format == ash::vk::Format::B8G8R8A8_UNORM || format == ash::vk::Format::B8G8R8A8_SRGB;
    let row = (w * 3 + 3) & !3;
    let size = 54 + row * h;
    let mut out = Vec::with_capacity(size as usize);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&54u32.to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(w as i32).to_le_bytes());
    out.extend_from_slice(&(h as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&24u16.to_le_bytes());
    out.extend_from_slice(&[0u8; 24]);
    for y in (0..h).rev() {
        let mut written = 0;
        for x in 0..w {
            let i = ((y * w + x) * 4) as usize;
            let (r, g, b) = if bgra { (data[i + 2], data[i + 1], data[i]) } else { (data[i], data[i + 1], data[i + 2]) };
            out.extend_from_slice(&[b, g, r]);
            written += 3;
        }
        while written < row {
            out.push(0);
            written += 1;
        }
    }
    std::fs::write(path, out)?;
    Ok(())
}

struct App {
    state: Option<State>,
    opts: Options,
    error: Option<String>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }
        match State::new(event_loop, self.opts.clone()) {
            Ok(s) => self.state = Some(s),
            Err(e) => {
                self.error = Some(format!("{e:#}"));
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(st) = self.state.as_mut() else { return };
        let resp = st.egui_state.on_window_event(&st.window, &event);
        let egui_wants_pointer = st.egui_ctx.egui_wants_pointer_input() || st.egui_ctx.is_pointer_over_egui();
        let cam = {
            let e = st.renderer.extent();
            st.game.view_frame(e.width as f32, e.height as f32)
        };
        match event {
            WindowEvent::CloseRequested => {
                // Settings and the autosave are written in `exiting`, which every way of
                // quitting goes through (including Cmd+Q on macOS).
                event_loop.exit();
            }
            WindowEvent::Resized(_) => {
                st.renderer.needs_resize = true;
                // Fullscreen can also be entered or left by the OS (macOS green button,
                // window manager keys): keep the setting in step with the real window, but
                // not while a change we asked for is still animating in.
                let actual = st.window.fullscreen().is_some();
                if actual != st.fullscreen && st.fullscreen_changed.elapsed() > Duration::from_secs(2) {
                    st.fullscreen = actual;
                    st.game.settings.fullscreen = actual;
                }
            }
            WindowEvent::ScaleFactorChanged { .. } => st.renderer.needs_resize = true,
            WindowEvent::RedrawRequested => {
                if let Err(e) = st.frame(event_loop) {
                    self.error = Some(format!("{e:#}"));
                    event_loop.exit();
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                st.game.mouse_move(Vec2::new(position.x as f32, position.y as f32));
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let b = match button {
                    MouseButton::Left => 0,
                    MouseButton::Right => 1,
                    MouseButton::Middle => 2,
                    _ => return,
                };
                let pressed = state == ElementState::Pressed;
                if !pressed || !egui_wants_pointer {
                    st.game.mouse_button(b, pressed, &cam);
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if !egui_wants_pointer {
                    let amount = match delta {
                        MouseScrollDelta::LineDelta(_, y) => y as f64,
                        MouseScrollDelta::PixelDelta(p) => p.y / 40.0,
                    };
                    st.game.scroll(amount);
                }
            }
            WindowEvent::PinchGesture { delta, .. } => {
                if !egui_wants_pointer {
                    st.game.scroll(delta * 8.0);
                }
            }
            WindowEvent::Focused(false) => st.game.focus_lost(),
            WindowEvent::ModifiersChanged(m) => st.game.set_shift(m.state().shift_key()),
            WindowEvent::KeyboardInput { event, .. } => {
                let key = match &event.logical_key {
                    Key::Named(NamedKey::Space) => "space".to_string(),
                    Key::Named(NamedKey::Delete) => "delete".to_string(),
                    Key::Named(NamedKey::Backspace) => "backspace".to_string(),
                    Key::Named(NamedKey::Escape) => "escape".to_string(),
                    Key::Named(NamedKey::Enter) => "enter".to_string(),
                    Key::Named(NamedKey::ArrowUp) => "up".to_string(),
                    Key::Named(NamedKey::ArrowDown) => "down".to_string(),
                    Key::Named(NamedKey::ArrowLeft) => "left".to_string(),
                    Key::Named(NamedKey::ArrowRight) => "right".to_string(),
                    Key::Named(NamedKey::Shift) => "shift".to_string(),
                    Key::Named(NamedKey::Control) => "ctrl".to_string(),
                    Key::Named(NamedKey::F11) => "f11".to_string(),
                    Key::Character(c) => c.to_lowercase(),
                    _ => return,
                };
                // Releases always get through, so a steering key can't stick while the UI has focus.
                if event.state != ElementState::Pressed {
                    st.game.key_up(&key);
                    return;
                }
                // Ignore auto-repeat: holding Space or B must not toggle pause / fire barrages 30x a second.
                if resp.consumed || st.egui_ctx.egui_wants_keyboard_input() || event.repeat {
                    return;
                }
                st.game.key_down(&key);
            }
            _ => {}
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(st) = self.state.as_mut() {
            st.save_settings();
            if let Some(e) = st.game.autosave() {
                log(&format!("Autosave failed: {e:#}"));
            }
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(st) = &self.state {
            st.window.request_redraw();
        }
    }
}

/// A GUI-subsystem program has no console of its own. If we were started from one
/// (cmd, PowerShell, Windows Terminal), borrow it so printed output goes somewhere.
#[cfg(all(windows, not(debug_assertions)))]
fn attach_parent_console() {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn AttachConsole(process_id: u32) -> i32;
    }
    const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
    // Fails harmlessly when launched from Explorer (there is no parent console).
    unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };
}

/// Without a console a fatal error would vanish silently, so show it in a message box.
#[cfg(windows)]
fn error_dialog(message: &str) {
    #[link(name = "user32")]
    unsafe extern "system" {
        fn MessageBoxW(hwnd: *mut std::ffi::c_void, text: *const u16, caption: *const u16, kind: u32) -> i32;
    }
    const MB_OK_ICONERROR: u32 = 0x10;
    let wide = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let (text, caption) = (wide(message), wide("Arthur's Universe Playground"));
    unsafe { MessageBoxW(std::ptr::null_mut(), text.as_ptr(), caption.as_ptr(), MB_OK_ICONERROR) };
}

fn main() {
    #[cfg(all(windows, not(debug_assertions)))]
    attach_parent_console();
    #[cfg(windows)]
    {
        let default_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            default_hook(info);
            error_dialog(&format!("The game crashed:\n\n{info}"));
        }));
    }
    let opts = parse_args();
    let event_loop = EventLoop::new().expect("failed to create event loop");
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App { state: None, opts, error: None };
    if let Err(e) = event_loop.run_app(&mut app) {
        eprintln!("event loop error: {e}");
    }
    // Drop GPU resources before exiting.
    app.state = None;
    if let Some(e) = app.error {
        eprintln!("Arthur's Universe Playground failed: {e}");
        #[cfg(windows)]
        error_dialog(&format!("Arthur's Universe Playground couldn't start or stopped unexpectedly.\n\n{e}"));
        std::process::exit(1);
    }
}
