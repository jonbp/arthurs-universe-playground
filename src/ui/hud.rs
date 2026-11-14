//! UI for exploring in person: the hangar and walk tool cards, the flight and
//! walking HUDs, the cockpit overlay, the planet-conditions panel, and the
//! "update available" banner.

use super::*;
use crate::conditions::{self, fmt_pressure, fmt_temp, Conditions, Level};
use crate::explore::{FlightInfo, FlightView, WalkInfo};
use crate::rocket::ROCKETS;
use crate::terrain;
use crate::update::Status;

const GOOD: Color32 = Color32::from_rgb(126, 214, 138);
const BODY_TEXT: Color32 = Color32::from_rgb(204, 208, 222);

fn rgb(c: Vec3) -> Color32 {
    let f = |v: f32| (v.clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0) as u8;
    Color32::from_rgb(f(c.x), f(c.y), f(c.z))
}

fn level_color(l: Level) -> Color32 {
    match l {
        Level::Good => GOOD,
        Level::Caution => ACCENT,
        Level::Danger => DANGER,
    }
}

/// A full-width labelled bar: label, track, value text.
fn meter_row(ui: &mut Ui, label: &str, frac: f32, value: &str, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let p = ui.painter();
    p.text(rect.left_center(), Align2::LEFT_CENTER, label, FontId::proportional(12.0), MUTED);
    p.text(rect.right_center(), Align2::RIGHT_CENTER, value, FontId::new(11.5, bold_family()), TEXT);
    let track = Rect::from_min_max(egui::pos2(rect.left() + 92.0, rect.center().y - 3.0), egui::pos2(rect.right() - 58.0, rect.center().y + 3.0));
    p.rect_filled(track, 3, white(18));
    let mut fill = track;
    fill.set_width(track.width() * frac.clamp(0.0, 1.0));
    p.rect_filled(fill, 3, color);
}

/// A stat cell with a bar in it (throttle, fuel).
fn meter_cell(ui: &mut Ui, label: &str, frac: f32, value: &str, color: Color32, w: f32) {
    Frame::new().corner_radius(10).inner_margin(Margin::symmetric(9, 6)).show(ui, |ui| {
        ui.set_width(w - 18.0);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.horizontal(|ui| {
                ui.label(RichText::new(label).size(10.5).color(FAINT));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| ui.label(bold(value, 11.0).color(TEXT)));
            });
            let (track, _) = ui.allocate_exact_size(vec2(ui.available_width(), 13.5), Sense::hover());
            let track = Rect::from_center_size(track.center(), vec2(track.width(), 7.0));
            ui.painter().rect_filled(track, 3, white(18));
            let mut fill = track;
            fill.set_width(track.width() * frac.clamp(0.0, 1.0));
            ui.painter().rect_filled(fill, 3, color);
        });
    });
}

/// "[W][S] pitch   [A][D] turn ..." in small type, wrapping as needed.
fn key_hints(ui: &mut Ui, hints: &[(&[&str], &str)]) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
        for (keys, what) in hints {
            for k in *keys {
                Frame::new().fill(white(10)).stroke(Stroke::new(1.0, white(24))).corner_radius(5).inner_margin(Margin::symmetric(5, 1)).show(ui, |ui| {
                    ui.label(bold(*k, 10.5).color(BODY_TEXT));
                });
            }
            ui.label(RichText::new(*what).size(11.0).color(MUTED));
            ui.add_space(6.0);
        }
    });
}

// ── Tool cards ─────────────────────────────────────────────────────────────

pub(super) fn fly_tool(ui: &mut Ui, game: &mut Game) {
    section(ui, "Hangar");
    let all: Vec<usize> = (0..ROCKETS.len()).collect();
    tile_grid(ui, &all, 3, 62.0, |ui, i, size| {
        let r = &ROCKETS[i];
        // Dark hulls would vanish on the dark tile: tint by the brighter of the two colours.
        let tint = if r.accent.length() > 0.6 { r.accent } else { r.body.max(r.accent) };
        if tile(ui, Icon::Rocket, r.name, game.rocket == i, size, Some(rgb(tint.max(Vec3::splat(0.25))))).on_hover_text(r.tagline).clicked() {
            game.rocket = i;
        }
    });
    let r = &ROCKETS[game.rocket.min(ROCKETS.len() - 1)];
    ui.add_space(4.0);
    ui.add(egui::Label::new(RichText::new(r.tagline).size(12.0).color(BODY_TEXT)).wrap());

    section(ui, "Performance");
    meter_row(ui, "Engine power", (r.accel / 50.0) as f32, &format!("{:.1} g", r.accel / 10.0), ACCENT);
    if r.burn_time.is_finite() {
        meter_row(ui, "Fuel tank", (r.burn_time / 30.0) as f32, &format!("{:.0} s", r.burn_time), INFO);
    } else {
        meter_row(ui, "Fuel tank", 1.0, "Endless", INFO);
    }
    meter_row(ui, "Steering", (r.turn_rate / 3.0) as f32, &format!("{:.1}", r.turn_rate), INFO);
    meter_row(ui, "Landing legs", (r.max_landing_speed / 10.0) as f32, &format!("{:.0}", r.max_landing_speed), GOOD);

    ui.add_space(6.0);
    switch_row(ui, "Easy flying", &mut game.settings.easy_flying)
        .on_hover_text("Endless fuel, an engine strong enough for any world, soft landings at any speed, and the rocket keeps itself level. Stars, black holes and gas giants are still dangerous!");
    if game.settings.easy_flying {
        ui.label(RichText::new("Endless fuel · lift off anywhere · no crash landings · self-levelling").size(11.0).color(GOOD));
    }

    ui.add_space(8.0);
    let label = match game.launch_site() {
        Some(b) if terrain::has_solid_surface(b.kind) => format!("Launch from {}", b.name),
        Some(b) => format!("Start in orbit of {}", b.name),
        None => "Launch into open space".to_string(),
    };
    if primary_button(ui, Icon::Rocket, &label).clicked() {
        game.start_flight(None, None);
    }
    ui.add_space(6.0);
    tip(ui, Icon::Target, "Or click any world to launch from that exact spot. Earth's gravity is 1 g: an engine needs more than that to lift off.");
}

pub(super) fn walk_tool(ui: &mut Ui, game: &mut Game) {
    let target = game.selected.and_then(|id| game.universe.get(id)).map(|b| (b.id, b.name.clone(), b.kind));
    match target {
        Some((id, name, kind)) if terrain::has_solid_surface(kind) => {
            section(ui, "Go for a walk");
            if primary_button(ui, Icon::Astronaut, &format!("Walk on {name}")).clicked() {
                game.start_walk(id, None);
            }
            if let Some(b) = game.universe.get(id) {
                let c = conditions::conditions(&game.universe, b);
                conditions_section(ui, &c);
            }
        }
        Some((_, name, kind)) => {
            ui.add_space(4.0);
            let why = if kind.is_stellar() { "can't be walked on" } else { "is all clouds, with no ground to stand on" };
            tip(ui, Icon::Warning, &format!("{name} {why}. Pick a rocky world, a moon or an asteroid."));
        }
        None => {}
    }
    ui.add_space(6.0);
    tip(ui, Icon::Astronaut, "Click a rocky world to stand right where you clicked. Jumps are higher where gravity is weaker: try a moon.");
}

// ── Conditions ─────────────────────────────────────────────────────────────

/// A coloured pill summing up whether people could live there.
fn verdict_pill(ui: &mut Ui, c: &Conditions) {
    let color = level_color(c.level);
    Frame::new().fill(with_alpha(color, 22)).corner_radius(10).inner_margin(Margin::symmetric(10, 7)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
            let icon = if c.level == Level::Good { Icon::Sparkles } else if c.level == Level::Danger { Icon::Warning } else { Icon::Astronaut };
            icon.paint(ui.painter(), r, color);
            ui.add(egui::Label::new(bold(c.verdict, 12.5).color(color)).wrap());
        });
    });
}

fn fact_row(ui: &mut Ui, icon: Icon, label: &str, text: &str) {
    ui.horizontal_top(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(15.0, 17.0), Sense::hover());
        icon.paint(ui.painter(), Rect::from_center_size(r.center(), Vec2::splat(14.0)), MUTED);
        ui.allocate_ui_with_layout(vec2(40.0, 17.0), Layout::left_to_right(Align::Center), |ui| {
            ui.set_min_width(40.0);
            ui.label(RichText::new(label).size(12.0).color(MUTED));
        });
        ui.add(egui::Label::new(RichText::new(text).size(12.0).color(BODY_TEXT)).wrap());
    });
}

/// Temperature, gravity, air and water for a world (inspector and walk card).
pub(super) fn conditions_section(ui: &mut Ui, c: &Conditions) {
    section(ui, "Conditions");
    verdict_pill(ui, c);
    let w = ((ui.available_width() - 12.0) / 3.0).floor();
    let temp_color = if c.temp_k > 335.0 { DANGER } else if c.temp_k < 250.0 { INFO } else { GOOD };
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        stat_cell(ui, "Temperature", &fmt_temp(c.temp_k), temp_color, w);
        stat_cell(ui, "Gravity", &format!("{:.2} g", c.gravity_g), TEXT, w);
        stat_cell(ui, "Air pressure", &fmt_pressure(c.pressure_atm), TEXT, w);
    });
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let light = if c.sunlight >= 10.0 { format!("{:.0}×", c.sunlight) } else { format!("{:.0}%", c.sunlight * 100.0) };
        stat_cell(ui, "Sunlight", &light, TEXT, w);
        stat_cell(ui, "Day length", &c.day_length.map(fmt_time).unwrap_or_else(|| "Never ends".into()), TEXT, w);
        stat_cell(ui, "Escape speed", &format!("{:.1}", c.escape_speed), TEXT, w);
    });
    if c.day_k - c.night_k > 15.0 {
        fact_row(ui, Icon::Sun, "Swing", &format!("{} by day, {} at night", fmt_temp(c.day_k), fmt_temp(c.night_k)));
    }
    fact_row(ui, Icon::Cloud, "Air", c.air);
    fact_row(ui, Icon::Waves, "Water", c.water);
}

// ── Flight HUD ─────────────────────────────────────────────────────────────

fn hud_width(ctx: &egui::Context) -> f32 {
    (ctx.content_rect().width() - 2.0 * EDGE - 28.0).min(640.0)
}

/// Icon badge + title + status line; `buttons` are laid out from the right edge.
fn hud_header(ui: &mut Ui, icon: Icon, title: &str, status: &str, status_color: Color32, buttons: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(Vec2::splat(36.0), Sense::hover());
        ui.painter().rect_filled(r, 10, ACCENT_BG);
        icon.paint(ui.painter(), Rect::from_center_size(r.center(), Vec2::splat(20.0)), ACCENT);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.label(bold(title, 14.5).color(TEXT));
            ui.label(RichText::new(status).size(12.0).color(status_color));
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            buttons(ui);
        });
    });
}

pub(super) fn flight_hud(ctx: &egui::Context, game: &mut Game) {
    let Some(info) = game.flight_info() else { return };
    #[derive(Clone, Copy)]
    enum Act {
        View,
        Brake,
        Easy,
        Walk,
        Exit,
        Relaunch,
    }
    let mut act = None;
    let width = hud_width(ctx);
    floating(ctx, "flight_hud", Align2::CENTER_BOTTOM, vec2(0.0, -EDGE), glass(12), |ui| {
        ui.set_width(width);
        let (status, status_color) = match (&info.destroyed, &info.landed_on, &info.near) {
            (Some(reason), _, _) => (reason.clone(), DANGER),
            (None, Some(name), _) => (format!("Landed on {name}. Hold Shift to power up and lift off."), GOOD),
            (None, None, Some(name)) => (format!("Flying near {name}"), MUTED),
            _ => ("Drifting in open space".to_string(), MUTED),
        };
        hud_header(ui, Icon::Rocket, info.rocket.name, &status, status_color, |ui| {
            if icon_button(ui, Icon::Close, 32.0, Look::Plain).on_hover_text("Back to the sandbox  (Esc)").clicked() {
                act = Some(Act::Exit);
            }
            if info.landed_on.is_some() && icon_button(ui, Icon::Astronaut, 32.0, Look::Plain).on_hover_text("Get out and walk  (G)").clicked() {
                act = Some(Act::Walk);
            }
            if icon_button(ui, Icon::Target, 32.0, Look::Toggle(info.brake_assist))
                .on_hover_text("Brake assist: keep the nose pointing backwards, so the engine slows you down  (R)")
                .clicked()
            {
                act = Some(Act::Brake);
            }
            if icon_button(ui, Icon::Sparkles, 32.0, Look::Toggle(info.easy))
                .on_hover_text("Easy flying: endless fuel, soft landings at any speed, and the rocket keeps itself level  (F)")
                .clicked()
            {
                act = Some(Act::Easy);
            }
            let cockpit = info.view == FlightView::Cockpit;
            let (icon, tip) = if cockpit { (Icon::Rocket, "Switch to the outside view  (V)") } else { (Icon::Seat, "Switch to the cockpit  (V)") };
            if icon_button(ui, icon, 32.0, Look::Toggle(cockpit)).on_hover_text(tip).clicked() {
                act = Some(Act::View);
            }
        });

        if info.destroyed.is_some() {
            ui.add_space(4.0);
            if primary_button(ui, Icon::Rocket, "Launch a new rocket  (Enter)").clicked() {
                act = Some(Act::Relaunch);
            }
            return;
        }

        // The one thing the pilot most needs to know right now.
        let falling = -info.vertical_speed;
        let warning = if info.grounded_by_gravity {
            Some(format!("Too heavy here! Gravity is {:.1} g but this engine only manages {:.1} g.", info.gravity_g, info.rocket.accel / 10.0))
        } else if info.flying && info.fuel <= 0.0 {
            Some("Out of fuel! You are coasting. Land to refill the tank.".to_string())
        } else if info.flying && info.altitude.is_some_and(|a| a < falling.max(0.0) * 1.2) && info.speed > info.safe_landing_speed {
            Some(format!("Too fast to land! Slow down below {:.0} before you touch the ground.", info.safe_landing_speed))
        } else {
            None
        };
        if let Some(text) = &warning {
            ui.add_space(2.0);
            Frame::new().fill(with_alpha(DANGER, 26)).corner_radius(9).inner_margin(Margin::symmetric(10, 5)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(15.0), Sense::hover());
                    Icon::Warning.paint(ui.painter(), r, DANGER);
                    ui.label(bold(text.as_str(), 12.0).color(DANGER));
                });
            });
        }

        ui.add_space(4.0);
        let w = ((ui.available_width() - 5.0 * 6.0) / 6.0).floor();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let fast = info.flying && info.speed > info.safe_landing_speed && info.altitude.is_some_and(|a| a < 1.0);
            stat_cell(ui, "Speed", &format!("{:.2}", info.speed), if fast { ACCENT } else { TEXT }, w);
            stat_cell(ui, "Altitude", &info.altitude.map(|a| format!("{a:.2}")).unwrap_or_else(|| "-".into()), TEXT, w);
            let climb = info.vertical_speed;
            let arrow = if climb > 0.005 { "▲" } else if climb < -0.005 { "▼" } else { "" };
            stat_cell(ui, "Climb", &format!("{arrow} {:.2}", climb.abs()), if climb < -info.safe_landing_speed { DANGER } else { TEXT }, w);
            stat_cell(ui, "Gravity", &format!("{:.2} g", info.gravity_g), TEXT, w);
            meter_cell(ui, "Throttle", info.throttle as f32, &format!("{:.0}%", info.throttle * 100.0), ACCENT, w);
            let fuel_color = if info.fuel < 0.15 { DANGER } else { INFO };
            let fuel_text = if info.rocket.burn_time.is_finite() { format!("{:.0}%", info.fuel * 100.0) } else { "∞".to_string() };
            meter_cell(ui, "Fuel", info.fuel as f32, &fuel_text, fuel_color, w);
        });

        ui.add_space(4.0);
        key_hints(ui, &[
            (&["W", "S"], "tilt"),
            (&["A", "D"], "turn"),
            (&["Q", "E"], "roll"),
            (&["Shift", "Ctrl"], "throttle"),
            (&["Z", "X"], "full / off"),
            (&["R"], "brake"),
            (&["F"], "easy"),
            (&["V"], "view"),
            (&["G"], "walk"),
        ]);
    });
    match act {
        Some(Act::View) => game.key("v"),
        Some(Act::Brake) => game.key("r"),
        Some(Act::Easy) => game.toggle_easy_flying(),
        Some(Act::Walk) => game.toggle_board(),
        Some(Act::Exit) => game.exit_mode(),
        Some(Act::Relaunch) => game.relaunch(),
        None => {}
    }
}

// ── Walking HUD ────────────────────────────────────────────────────────────

pub(super) fn walk_hud(ctx: &egui::Context, game: &mut Game) {
    let Some(info) = game.walk_info() else { return };
    let WalkInfo { body, ground, conditions: c, local_temp_k, sky, airborne, rocket, .. } = &info;
    let (mut board, mut exit) = (false, false);
    let width = hud_width(ctx);
    floating(ctx, "walk_hud", Align2::CENTER_BOTTOM, vec2(0.0, -EDGE), glass(12), |ui| {
        ui.set_width(width);
        let status = if *airborne > 0.0005 { format!("Jumping!  {:.0}× your own height", airborne / crate::explore::EYE_HEIGHT) } else { format!("{ground}  ·  {sky}") };
        hud_header(ui, Icon::Astronaut, &format!("Standing on {body}"), &status, MUTED, |ui| {
            if icon_button(ui, Icon::Close, 32.0, Look::Plain).on_hover_text("Back to the sandbox  (Esc)").clicked() {
                exit = true;
            }
            let (tip, near) = match rocket {
                Some((_, true)) => ("Climb back into your rocket  (G)".to_string(), true),
                Some((d, false)) => (format!("Your rocket is {d:.2} away: walk back to it to climb in"), false),
                None => ("Call a rocket to pick you up  (G)".to_string(), true),
            };
            if icon_button(ui, Icon::Rocket, 32.0, Look::Toggle(near)).on_hover_text(tip).clicked() {
                board = true;
            }
        });
        ui.add_space(4.0);
        let w = ((ui.available_width() - 3.0 * 6.0) / 4.0).floor();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let t = *local_temp_k;
            let temp_color = if t > 335.0 { DANGER } else if t < 250.0 { INFO } else { GOOD };
            stat_cell(ui, "Temperature here", &fmt_temp(t), temp_color, w);
            stat_cell(ui, "Gravity", &format!("{:.2} g", c.gravity_g), TEXT, w);
            stat_cell(ui, "Air pressure", &fmt_pressure(c.pressure_atm), TEXT, w);
            stat_cell(ui, "Sunlight", &format!("{:.0}%", c.sunlight * 100.0), TEXT, w);
        });
        ui.horizontal(|ui| {
            let color = level_color(c.level);
            let (r, _) = ui.allocate_exact_size(Vec2::splat(15.0), Sense::hover());
            let icon = if c.level == Level::Good { Icon::Sparkles } else { Icon::Warning };
            icon.paint(ui.painter(), r, color);
            ui.label(bold(c.verdict, 12.0).color(color));
            ui.label(RichText::new(format!("·  Air: {}  ·  Water: {}", c.air, c.water)).size(11.5).color(MUTED));
        });
        ui.add_space(2.0);
        key_hints(ui, &[
            (&["W", "A", "S", "D"], "walk"),
            (&["Drag"], "look"),
            (&["←", "→"], "turn"),
            (&["Shift"], "run"),
            (&["Space"], "jump"),
            (&["G"], "rocket"),
            (&["Esc"], "leave"),
        ]);
    });
    if exit {
        game.exit_mode();
    } else if board {
        game.toggle_board();
    }
}

// ── Cockpit ────────────────────────────────────────────────────────────────

const HULL: Color32 = Color32::from_rgb(27, 30, 42);
const HULL_EDGE: Color32 = Color32::from_rgb(70, 76, 98);

/// Things painted over the 3D view while exploring: the cockpit window frame and
/// instruments, and the "where am I heading" marker.
pub(super) fn scene_overlay(ctx: &egui::Context, game: &Game, cam: &CameraFrame, ppp: f32) {
    let Some(info) = game.flight_info() else { return };
    if info.destroyed.is_some() {
        return;
    }
    let painter = ctx.layer_painter(egui::LayerId::background());
    let to_pt = |p: glam::Vec2| egui::pos2(p.x / ppp, p.y / ppp);
    // Direction-of-travel marker: a ring with three ticks, where the rocket is heading.
    if let Some(dir) = info.prograde.filter(|_| info.flying) {
        if let Some(sp) = cam.project(cam.eye + dir * 1.0e4) {
            let p = to_pt(sp);
            let s = Stroke::new(1.5, GOOD);
            painter.circle_stroke(p, 9.0, s);
            painter.circle_filled(p, 1.5, GOOD);
            for (dx, dy) in [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0)] {
                painter.line_segment([p + vec2(dx, dy) * 9.0, p + vec2(dx, dy) * 16.0], s);
            }
        }
    }
    if info.view == FlightView::Cockpit {
        cockpit(&painter, ctx.content_rect(), &info);
    }
}

fn cockpit(painter: &egui::Painter, screen: Rect, info: &FlightInfo) {
    let (w, h) = (screen.width(), screen.height());
    let at = |x: f32, y: f32| egui::pos2(screen.left() + x * w, screen.top() + y * h);
    let fill = |pts: Vec<Pos2>| painter.add(egui::Shape::convex_polygon(pts, HULL, Stroke::NONE));

    // Nose marker in the middle of the window.
    let c = screen.center();
    let s = Stroke::new(1.5, with_alpha(ACCENT, 220));
    painter.circle_stroke(c, 5.0, s);
    for dx in [-1.0f32, 1.0] {
        painter.line_segment([c + vec2(dx * 9.0, 0.0), c + vec2(dx * 22.0, 0.0)], s);
    }

    // Canopy: a top beam and two pillars leaning in towards the nose.
    fill(vec![at(0.0, 0.0), at(1.0, 0.0), at(1.0, 0.04), at(0.0, 0.04)]);
    fill(vec![at(0.0, 0.0), at(0.075, 0.0), at(0.02, 0.9), at(0.0, 0.9)]);
    fill(vec![at(1.0, 0.0), at(1.0, 0.9), at(0.98, 0.9), at(0.925, 0.0)]);
    for (x0, x1) in [(0.075, 0.02), (0.925, 0.98)] {
        painter.line_segment([at(x0, 0.04), at(x1, 0.7)], Stroke::new(1.5, HULL_EDGE));
    }
    painter.line_segment([at(0.07, 0.04), at(0.93, 0.04)], Stroke::new(1.5, HULL_EDGE));

    // Dashboard: a hump across the bottom of the view.
    let top = |x: f32| 0.6 + 0.16 * (2.0 * x - 1.0).powi(2);
    let mut dash: Vec<Pos2> = (0..=40).map(|i| i as f32 / 40.0).map(|x| at(x, top(x))).collect();
    let edge = dash.clone();
    dash.push(at(1.0, 1.0));
    dash.push(at(0.0, 1.0));
    fill(dash);
    painter.add(egui::Shape::line(edge, Stroke::new(2.0, HULL_EDGE)));

    // Attitude ball: sky above, ground below, as the pilot sees the horizon.
    let r = (h * 0.06).clamp(24.0, 64.0);
    let ball = at(0.5, 0.695);
    painter.circle_filled(ball, r + 5.0, Color32::from_rgb(10, 11, 16));
    painter.circle_filled(ball, r, Color32::from_rgb(52, 120, 200));
    let down = vec2(-info.roll.sin() as f32, info.roll.cos() as f32);
    let offset = (info.pitch / std::f64::consts::FRAC_PI_2) as f32 * r;
    let ground: Vec<Pos2> = (0..=72)
        .map(|i| {
            let a = i as f32 / 72.0 * std::f32::consts::TAU;
            vec2(a.cos(), a.sin()) * r
        })
        .filter(|q| q.dot(down) >= offset)
        .map(|q| ball + q)
        .collect();
    if ground.len() >= 3 {
        // The arc is listed in angle order, which may wrap past 0°: start it after the gap.
        let gap = (0..ground.len()).max_by(|a, b| {
            let d = |i: usize| ground[i].distance(ground[(i + 1) % ground.len()]);
            d(*a).total_cmp(&d(*b))
        });
        let start = gap.map(|g| (g + 1) % ground.len()).unwrap_or(0);
        let ordered: Vec<Pos2> = (0..ground.len()).map(|i| ground[(start + i) % ground.len()]).collect();
        painter.add(egui::Shape::convex_polygon(ordered, Color32::from_rgb(134, 92, 52), Stroke::NONE));
    }
    painter.circle_stroke(ball, r, Stroke::new(2.0, HULL_EDGE));
    let wing = Stroke::new(2.0, ACCENT);
    painter.line_segment([ball + vec2(-r * 0.55, 0.0), ball + vec2(-r * 0.18, 0.0)], wing);
    painter.line_segment([ball + vec2(r * 0.18, 0.0), ball + vec2(r * 0.55, 0.0)], wing);
    painter.circle_filled(ball, 2.5, ACCENT);

    // Throttle and fuel columns either side of the ball.
    let column = |x: f32, frac: f64, color: Color32, label: &str| {
        let track = Rect::from_center_size(ball + vec2(x, 0.0), vec2(12.0, r * 1.7));
        painter.rect_filled(track, 4, Color32::from_rgb(10, 11, 16));
        let mut fill = track.shrink(2.0);
        fill.set_top(fill.bottom() - fill.height() * frac.clamp(0.0, 1.0) as f32);
        painter.rect_filled(fill, 3, color);
        painter.text(egui::pos2(track.center().x, track.top() - 4.0), Align2::CENTER_BOTTOM, label, FontId::new(9.5, bold_family()), MUTED);
    };
    column(-r - 26.0, info.throttle, ACCENT, "PWR");
    column(r + 26.0, info.fuel, if info.fuel < 0.15 { DANGER } else { INFO }, "FUEL");

    // Read-outs on the dashboard wings.
    let readout = |x: f32, label: &str, value: String| {
        let p = ball + vec2(x, 0.0);
        painter.text(p + vec2(0.0, -9.0), Align2::CENTER_CENTER, label, FontId::new(9.5, bold_family()), MUTED);
        painter.text(p + vec2(0.0, 7.0), Align2::CENTER_CENTER, value, FontId::monospace(15.0), TEXT);
    };
    readout(-r - 100.0, "SPEED", format!("{:.2}", info.speed));
    readout(r + 100.0, "ALTITUDE", info.altitude.map(|a| format!("{a:.2}")).unwrap_or_else(|| "-".into()));
}

// ── Updates ────────────────────────────────────────────────────────────────

/// "A new version is available" pill under the time bar, with a download button.
pub(super) fn update_banner(ctx: &egui::Context, game: &mut Game) {
    if game.updater.dismissed {
        return;
    }
    let Status::Available(release) = game.updater.status() else { return };
    let (mut download, mut dismiss) = (false, false);
    floating(ctx, "update_banner", Align2::CENTER_TOP, vec2(0.0, BELOW_BARS), glass(6).corner_radius(15), |ui| {
        ui.horizontal(|ui| {
            ui.add_space(6.0);
            let (r, _) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::hover());
            Icon::Sparkles.paint(ui.painter(), r, ACCENT);
            ui.label(bold(format!("Version {} is out", release.version), 13.0).color(TEXT));
            ui.label(RichText::new(format!("you have {}", game.updater.current_version())).size(12.0).color(MUTED));
            ui.add_space(4.0);
            let hint = if release.asset_url.is_some() { "Download the new version in your browser" } else { "Open the download page in your browser" };
            download = chip(ui, Some((Icon::Download, ACCENT)), "Download", 0.0, false).on_hover_text(hint).clicked();
            dismiss = icon_button(ui, Icon::Close, 28.0, Look::Plain).on_hover_text("Not now").clicked();
        });
    });
    if download {
        ctx.open_url(egui::OpenUrl::new_tab(release.download_url()));
    }
    if dismiss {
        game.updater.dismissed = true;
    }
}

/// Version line at the bottom of the help window.
pub(super) fn version_footer(ui: &mut Ui, game: &mut Game) {
    ui.add_space(8.0);
    ui.separator();
    let status = game.updater.status();
    let mut check = false;
    let mut open = None;
    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("Version {}", game.updater.current_version())).size(12.0).color(MUTED));
        match &status {
            Status::Disabled => {}
            Status::Checking => {
                ui.label(RichText::new("·  checking for updates…").size(12.0).color(FAINT));
            }
            Status::UpToDate => {
                ui.label(RichText::new("·  you have the latest version").size(12.0).color(GOOD));
            }
            Status::Available(r) => {
                ui.label(RichText::new(format!("·  version {} is available", r.version)).size(12.0).color(ACCENT));
            }
            Status::Failed(_) => {
                ui.label(RichText::new("·  couldn't check for updates").size(12.0).color(FAINT));
            }
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| match &status {
            Status::Available(r) => {
                if chip(ui, Some((Icon::Download, ACCENT)), "Download", 0.0, false).clicked() {
                    open = Some(r.download_url().to_string());
                }
            }
            Status::Checking => {}
            _ => {
                if chip(ui, Some((Icon::Reset, MUTED)), "Check for updates", 0.0, false).clicked() {
                    check = true;
                }
            }
        });
    });
    if check {
        game.updater.check();
    }
    if let Some(url) = open {
        ui.ctx().open_url(egui::OpenUrl::new_tab(url));
    }
}
