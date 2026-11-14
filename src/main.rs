//! Arthur's Universe Playground - a Vulkan solar-system sandbox.
//!
//! Change planet surfaces, spawn new worlds on new orbits, smash planets
//! together, and build your own universes with black holes and supernovae.

mod body;
mod camera;
mod game;
mod gpu_types;
mod icons;
mod mesh;
mod paint;
mod presets;
mod renderer;
mod scene;
mod ui;
mod universe;
mod util;
mod vkctx;

use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use glam::Vec2;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

use crate::game::Game;
use crate::renderer::Renderer;
use crate::scene::SceneOptions;
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
                    "Arthur's Universe Playground\n\nDeveloper options:\n  --preset <solar|rogue|theia|headon|binary|blackhole|supernova|galaxy|disk|empty>\n  --focus <body name>   --zoom <distance>   --speed <x>\n  --supernova <star>   --smash <body>   --paint <body>   --blackhole <body>\n  --screenshot <file.bmp> --frames <n>   --hide-ui   --icons (show the icon set)\n  --click <x,y@frame>   inject a UI click (repeatable)"
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
    last: Instant,
    frame_count: u32,
    fps_timer: f64,
    fps_frames: u32,
    opts: Options,
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
        let attrs = Window::default_attributes()
            .with_title("Arthur's Universe Playground")
            .with_inner_size(LogicalSize::new(1440.0, 900.0))
            .with_min_inner_size(LogicalSize::new(640.0, 400.0))
            .with_window_icon(window_icon());
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
        let renderer = Renderer::new(ctx, size.width, size.height)?;
        let egui_ctx = egui::Context::default();
        ui::setup_style(&egui_ctx);
        let egui_state = egui_winit::State::new(
            egui_ctx.clone(),
            egui::ViewportId::ROOT,
            window.as_ref(),
            Some(window.scale_factor() as f32),
            None,
            Some(4096),
        );
        let mut game = Game::new();
        if let Some(p) = opts.preset.as_deref().and_then(preset_from_name) {
            game.load_preset(p);
        }
        if let Some(s) = opts.time_scale {
            game.time_scale = s;
        }
        game.glyph_test = opts.glyph_test;
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
        Ok(State {
            window,
            renderer,
            egui_ctx,
            egui_state,
            game,
            last: Instant::now(),
            frame_count: 0,
            fps_timer: 0.0,
            fps_frames: 0,
            opts,
        })
    }

    fn frame(&mut self, event_loop: &ActiveEventLoop) -> Result<()> {
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
                return Ok(());
            }
            self.renderer.resize(size.width, size.height)?;
        }
        let extent = self.renderer.extent();
        let (w, h) = (extent.width as f32, extent.height as f32);
        let cam = self.game.camera.frame(w, h);

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
        let output = self.egui_ctx.run_ui(raw, |ui| ui::draw(ui, game, &cam, ppp));
        self.egui_state.handle_platform_output(&self.window, output.platform_output);

        let scene_has_pointer = !self.egui_ctx.egui_wants_pointer_input() && !self.egui_ctx.is_pointer_over_egui();
        self.game.track_spawn_drag(&cam);
        self.game.update(dt, &cam, scene_has_pointer);

        let cam = self.game.camera.frame(w, h);
        let opts = SceneOptions {
            trails: self.game.show_trails,
            exposure: self.game.exposure,
            bloom: self.game.bloom,
            lensing: self.game.lensing,
            stars: self.game.stars,
            selected: self.game.selected,
            hovered: self.game.hovered,
            time: self.game.real_time as f32,
        };
        let frame = scene::build_frame(&self.game.universe, &cam, &opts, &self.game.preview);
        let prims = self.egui_ctx.tessellate(output.shapes, output.pixels_per_point);

        self.frame_count += 1;
        let want_shot = self.opts.screenshot.is_some() && self.frame_count == self.opts.frames;
        if want_shot {
            self.renderer.capture = true;
        }
        let ok = self.renderer.render(&frame, &mut self.game.universe.paint, &prims, &output.textures_delta, output.pixels_per_point)?;
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
            st.game.camera.frame(e.width as f32, e.height as f32)
        };
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => st.renderer.needs_resize = true,
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
            WindowEvent::ModifiersChanged(m) => st.game.set_shift(m.state().shift_key()),
            WindowEvent::KeyboardInput { event, .. } => {
                if resp.consumed || st.egui_ctx.egui_wants_keyboard_input() || event.state != ElementState::Pressed {
                    return;
                }
                let key = match &event.logical_key {
                    Key::Named(NamedKey::Space) => "space".to_string(),
                    Key::Named(NamedKey::Delete) => "delete".to_string(),
                    Key::Named(NamedKey::Backspace) => "backspace".to_string(),
                    Key::Named(NamedKey::Escape) => "escape".to_string(),
                    Key::Character(c) => c.to_lowercase(),
                    _ => return,
                };
                st.game.key(&key);
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(st) = &self.state {
            st.window.request_redraw();
        }
    }
}

fn main() {
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
        std::process::exit(1);
    }
}
