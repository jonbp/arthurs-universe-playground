//! The settings panel: display (fullscreen, vsync, frame cap, resolution scale),
//! sound, graphics (planet detail, exposure, glow, stars, lensing, trails), interface and
//! gameplay options.
//! Everything here lives in `game.settings`; main.rs applies and saves it.

use super::*;
use crate::settings::{PlanetDetail, Settings, FPS_LIMITS, RENDER_SCALES, RENDER_SCALE_MAX, RENDER_SCALE_MIN};

const PANEL_W: f32 = 320.0;
/// UI sizes offered. Picks rather than a slider: a slider that resizes itself while you drag it is hard to use.
const UI_SCALES: [f32; 5] = [0.75, 0.9, 1.0, 1.25, 1.5];

fn percent(v: f32) -> String {
    format!("{:.0}%", v * 100.0)
}

/// A labelled segmented control: mutually exclusive picks sharing the row's width.
fn choice_row<T: Copy + PartialEq>(ui: &mut Ui, label: &str, value: &mut T, options: &[T], text: impl Fn(T) -> String) {
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(vec2(84.0, 26.0), Layout::left_to_right(Align::Center), |ui| {
            ui.set_min_width(84.0);
            ui.label(muted(label));
        });
        let (track, _) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::hover());
        ui.painter().rect_filled(track, 8, white(8));
        let w = track.width() / options.len() as f32;
        for (i, &o) in options.iter().enumerate() {
            let r = Rect::from_min_size(egui::pos2(track.left() + w * i as f32, track.top()), vec2(w, track.height())).shrink(2.0);
            let resp = ui.interact(r, ui.id().with(label).with(i), Sense::click());
            let selected = *value == o;
            let t = hover_t(ui, &resp);
            let p = ui.painter();
            if selected {
                p.rect_filled(r, 6, ACCENT);
            } else if t > 0.0 {
                p.rect_filled(r, 6, white((t * 14.0) as u8));
            }
            let color = if selected { ON_ACCENT } else { mix(MUTED, TEXT, t) };
            let font = if selected { FontId::new(12.0, bold_family()) } else { FontId::proportional(12.0) };
            p.text(r.center(), Align2::CENTER_CENTER, text(o), font, color);
            if resp.clicked() {
                *value = o;
            }
        }
    });
}

/// A 0..1 slider shown (and typed) as a percentage.
fn vol(v: &mut f32) -> egui::Slider<'_> {
    egui::Slider::new(v, 0.0..=1.0)
        .custom_formatter(|v, _| format!("{:.0}%", v * 100.0))
        .custom_parser(|t| t.trim().trim_end_matches('%').trim().parse::<f64>().ok().map(|v| v / 100.0))
}

/// Small faint explanatory line under a control.
fn note(ui: &mut Ui, text: impl Into<String>) {
    ui.add(egui::Label::new(RichText::new(text).size(11.0).color(FAINT)).wrap());
}

pub(super) fn settings_panel(ctx: &egui::Context, game: &mut Game, cam: &CameraFrame, scene_px: (u32, u32)) {
    let max_h = (ctx.content_rect().height() - BELOW_BARS - 80.0).max(200.0);
    let mut close = false;
    floating(ctx, "settings_panel", Align2::RIGHT_TOP, vec2(-EDGE, BELOW_BARS), glass(14), |ui| {
        ui.set_width(PANEL_W);
        ui.set_max_height(max_h);
        close = card_header(ui, Icon::Gear, "Settings", Some("Saved automatically"), true);
        egui::ScrollArea::vertical().auto_shrink([false, true]).show(ui, |ui| {
            let s = &mut game.settings;

            section(ui, "Display");
            switch_row(ui, "Fullscreen  (F11)", &mut s.fullscreen);
            switch_row(ui, "VSync", &mut s.vsync).on_hover_text("Match the screen's refresh rate: no tearing, but slightly more input lag");
            choice_row(ui, "Frame limit", &mut s.fps_limit, &FPS_LIMITS, |v| if v == 0 { "Off".into() } else { v.to_string() });
            if s.vsync && s.fps_limit == 0 {
                note(ui, "VSync caps the frame rate at your screen's refresh rate.");
            }

            section(ui, "Sound");
            switch_row(ui, "Mute  (M)", &mut s.muted);
            ui.add_enabled_ui(!s.muted, |ui| {
                slider_row(ui, "Volume", vol(&mut s.master_volume));
                slider_row(ui, "Effects", vol(&mut s.effects_volume)).on_hover_text("Explosions, impacts, the rocket engine and footsteps");
                slider_row(ui, "Music", vol(&mut s.music_volume));
            });

            section(ui, "Resolution");
            slider_row(
                ui,
                "Scale",
                egui::Slider::new(&mut s.render_scale, RENDER_SCALE_MIN..=RENDER_SCALE_MAX)
                    .step_by(0.05)
                    .custom_formatter(|v, _| format!("{:.0}%", v * 100.0))
                    .custom_parser(|t| t.trim().trim_end_matches('%').trim().parse::<f64>().ok().map(|v| v / 100.0)),
            )
            .on_hover_text("Draw the 3D scene at a lower resolution for speed, or a higher one for smoother edges. The interface always stays sharp.");
            choice_row(ui, "Presets", &mut s.render_scale, &RENDER_SCALES, percent);
            let (ww, wh) = (cam.size.x.round() as u32, cam.size.y.round() as u32);
            let effect = if s.render_scale < 0.99 {
                "faster, softer"
            } else if s.render_scale > 1.01 {
                "supersampled: smoother edges, slower"
            } else {
                "native"
            };
            note(ui, format!("Drawing {}×{} for a {ww}×{wh} window ({effect})", scene_px.0, scene_px.1));

            section(ui, "Graphics");
            choice_row(ui, "Planets", &mut s.planet_detail, &PlanetDetail::ALL, |d| d.label().to_string());
            note(
                ui,
                match s.planet_detail {
                    PlanetDetail::Low => "Coarser spheres. Fastest on older or integrated graphics.",
                    PlanetDetail::Standard => "Detailed spheres for worlds that are close up.",
                    PlanetDetail::High => "Detailed spheres for every world you can see well.",
                },
            );
            slider_row(ui, "Exposure", egui::Slider::new(&mut s.exposure, 0.2..=4.0).logarithmic(true));
            slider_row(ui, "Glow", egui::Slider::new(&mut s.bloom, 0.0..=2.0));
            slider_row(ui, "Stars", egui::Slider::new(&mut s.stars, 0.0..=2.0));
            switch_row(ui, "Gravitational lensing", &mut s.lensing);
            switch_row(ui, "Orbit trails  (T)", &mut s.trails);

            section(ui, "Interface");
            choice_row(ui, "UI size", &mut s.ui_scale, &UI_SCALES, percent);
            switch_row(ui, "Labels  (L)", &mut s.labels);
            switch_row(ui, "Bodies, particles and FPS", &mut s.show_stats);

            section(ui, "Gameplay");
            switch_row(ui, "Easy flying", &mut s.easy_flying)
                .on_hover_text("Endless fuel, an engine strong enough for any world, soft landings at any speed, and the rocket keeps itself level");

            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if chip(ui, Some((Icon::Reset, MUTED)), "Reset to defaults", 0.0, false).clicked() {
                    // Leave the window as it is: jumping out of fullscreen would be a surprise.
                    *s = Settings { fullscreen: s.fullscreen, ..Settings::default() };
                }
            });
        });
    });
    if close {
        game.show_settings = false;
    }
}
