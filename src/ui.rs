//! The egui user interface: floating glass panels around the edges of the screen,
//! big vector icons (see `icons.rs`) and tile pickers instead of drop-downs.
//!
//! Layout:
//! ```text
//!  [universe ▾]            [« ▶ » 10× ⏱]          [trails labels lens | ⚙ ⚛ ? ⊘]
//!  [tool ]  [tool options]                              [inspector / settings]
//!  [dock ]
//!  [toasts]                  [ hint ]                                [stats]
//! ```

use egui::{
    vec2, Align, Align2, Color32, CornerRadius, FontFamily, FontId, Frame, Id, Layout, Margin, Popup, PopupCloseBehavior, Pos2,
    Rect, RectAlign, Response, RichText, Sense, Shadow, Stroke, StrokeKind, Ui, Vec2,
};
use glam::{DVec3, Vec3};

use crate::body::{black_hole_radius, BodyKind, Ring};
use crate::camera::CameraFrame;
use crate::game::{Game, Tool};
use crate::icons::Icon;
use crate::paint::Brush;
use crate::presets::{CenterKind, Preset};
use crate::save;
use crate::universe::CollisionMode;

mod hud;
mod options;

// ── Theme ───────────────────────────────────────────────────────────────────

const ACCENT: Color32 = Color32::from_rgb(255, 190, 90);
/// Accent at ~16% over the dark glass (premultiplied).
const ACCENT_BG: Color32 = Color32::from_rgba_premultiplied(41, 30, 14, 41);
const ON_ACCENT: Color32 = Color32::from_rgb(28, 20, 8);
const TEXT: Color32 = Color32::from_rgb(234, 236, 244);
const MUTED: Color32 = Color32::from_rgb(152, 158, 180);
const FAINT: Color32 = Color32::from_rgb(104, 110, 132);
const DANGER: Color32 = Color32::from_rgb(255, 112, 104);
const INFO: Color32 = Color32::from_rgb(120, 200, 255);
const GLASS: Color32 = Color32::from_rgba_premultiplied(13, 15, 26, 232);
const GLASS_SOLID: Color32 = Color32::from_rgb(17, 19, 31);

/// Gap between panels and the window edge.
const EDGE: f32 = 14.0;
/// Height of the top bars (36 px buttons + frame).
const BAR_H: f32 = 50.0;
/// Where the side panels start.
const BELOW_BARS: f32 = EDGE + BAR_H + 12.0;
const DOCK_BTN: f32 = 50.0;
const CARD_W: f32 = 284.0;
const INSPECTOR_W: f32 = 300.0;
const POPUP_W: f32 = 300.0;

fn white(a: u8) -> Color32 {
    Color32::from_white_alpha(a)
}

fn with_alpha(c: Color32, a: u8) -> Color32 {
    let [r, g, b, _] = c.to_array();
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let (a, b) = (a.to_array(), b.to_array());
    let l = |i: usize| (a[i] as f32 + (b[i] as f32 - a[i] as f32) * t).round() as u8;
    Color32::from_rgba_premultiplied(l(0), l(1), l(2), l(3))
}

fn bold_family() -> FontFamily {
    FontFamily::Name("bold".into())
}

/// Semibold Inter text.
fn bold(text: impl Into<String>, size: f32) -> RichText {
    RichText::new(text).family(bold_family()).size(size)
}

fn muted(text: impl Into<String>) -> RichText {
    RichText::new(text).color(MUTED).size(12.0)
}

/// Install Inter (bundled in the binary) as the UI font. egui's default fonts stay
/// in the fallback chain for any odd characters in user-typed names.
fn setup_fonts(ctx: &egui::Context) {
    use std::sync::Arc;
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert("Inter".into(), Arc::new(egui::FontData::from_static(include_bytes!("../assets/fonts/Inter-Regular.ttf"))));
    fonts
        .font_data
        .insert("InterSemiBold".into(), Arc::new(egui::FontData::from_static(include_bytes!("../assets/fonts/Inter-SemiBold.ttf"))));
    let fallback = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    fonts.families.get_mut(&FontFamily::Proportional).unwrap().insert(0, "Inter".into());
    let mut bold = vec!["InterSemiBold".to_string()];
    bold.extend(fallback);
    fonts.families.insert(bold_family(), bold);
    ctx.set_fonts(fonts);
}

pub fn setup_style(ctx: &egui::Context) {
    setup_fonts(ctx);
    let mut v = egui::Visuals::dark();
    v.window_fill = GLASS_SOLID;
    v.panel_fill = GLASS;
    v.window_stroke = Stroke::new(1.0, white(22));
    v.window_corner_radius = CornerRadius::same(16);
    v.menu_corner_radius = CornerRadius::same(14);
    v.window_shadow = Shadow { offset: [0, 10], blur: 32, spread: 0, color: Color32::from_black_alpha(120) };
    v.popup_shadow = Shadow { offset: [0, 8], blur: 24, spread: 0, color: Color32::from_black_alpha(110) };
    v.extreme_bg_color = Color32::from_rgba_premultiplied(8, 8, 12, 120);
    v.faint_bg_color = white(6);
    // egui draws a selected item's text (menu entries, highlighted text) in the selection
    // stroke colour on top of the selection fill: it must contrast with the accent.
    v.selection.bg_fill = ACCENT;
    v.selection.stroke = Stroke::new(1.5, ON_ACCENT);
    v.slider_trailing_fill = true;
    v.handle_shape = egui::style::HandleShape::Circle;
    v.interact_cursor = Some(egui::CursorIcon::PointingHand);

    let w = &mut v.widgets;
    for s in [&mut w.noninteractive, &mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
        s.corner_radius = CornerRadius::same(9);
        s.expansion = 0.0;
    }
    w.noninteractive.bg_fill = GLASS;
    w.noninteractive.weak_bg_fill = Color32::TRANSPARENT;
    w.noninteractive.bg_stroke = Stroke::new(1.0, white(16));
    w.noninteractive.fg_stroke = Stroke::new(1.0, Color32::from_rgb(204, 208, 222));
    // Slider rails and checkbox backgrounds use bg_fill; buttons use weak_bg_fill.
    w.inactive.bg_fill = Color32::from_rgb(50, 54, 72);
    w.inactive.weak_bg_fill = white(12);
    w.inactive.bg_stroke = Stroke::NONE;
    w.inactive.fg_stroke = Stroke::new(2.0, Color32::from_rgb(214, 218, 230));
    w.hovered.bg_fill = Color32::from_rgb(72, 77, 98);
    w.hovered.weak_bg_fill = white(22);
    w.hovered.bg_stroke = Stroke::new(1.0, white(34));
    w.hovered.fg_stroke = Stroke::new(2.0, Color32::WHITE);
    w.active.bg_fill = Color32::from_rgb(88, 93, 116);
    w.active.weak_bg_fill = white(30);
    w.active.bg_stroke = Stroke::new(1.0, ACCENT);
    w.active.fg_stroke = Stroke::new(2.0, Color32::WHITE);
    w.open = w.hovered;
    ctx.set_visuals(v);

    ctx.all_styles_mut(|s| {
        use egui::TextStyle;
        s.text_styles.insert(TextStyle::Body, FontId::proportional(13.5));
        s.text_styles.insert(TextStyle::Button, FontId::proportional(13.5));
        s.text_styles.insert(TextStyle::Small, FontId::proportional(11.5));
        s.text_styles.insert(TextStyle::Heading, FontId::new(18.0, bold_family()));
        s.text_styles.insert(TextStyle::Monospace, FontId::monospace(12.0));
        s.spacing.item_spacing = vec2(8.0, 7.0);
        s.spacing.button_padding = vec2(10.0, 5.0);
        s.spacing.interact_size = vec2(40.0, 24.0);
        s.spacing.slider_rail_height = 4.0;
        s.spacing.menu_margin = Margin::same(8);
        s.spacing.window_margin = Margin::same(16);
        s.spacing.scroll.bar_width = 6.0;
        s.spacing.scroll.floating = true;
    });
}

// ── Building blocks ────────────────────────────────────────────────────────

/// Translucent rounded panel with a soft drop shadow.
fn glass(margin: i8) -> Frame {
    Frame::new()
        .fill(GLASS)
        .stroke(Stroke::new(1.0, white(20)))
        .corner_radius(18)
        .inner_margin(Margin::same(margin))
        .shadow(Shadow { offset: [0, 8], blur: 28, spread: 0, color: Color32::from_black_alpha(100) })
}

fn popup_frame() -> Frame {
    glass(10).fill(GLASS_SOLID).corner_radius(16)
}

/// A panel that floats over the scene, anchored to a screen corner/edge.
fn floating<R>(ctx: &egui::Context, id: &str, anchor: Align2, offset: Vec2, frame: Frame, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Area::new(Id::new(id)).anchor(anchor, offset).show(ctx, |ui| frame.show(ui, add).inner).inner
}

#[derive(Clone, Copy, PartialEq)]
enum Look {
    /// Quiet: icon only, background appears on hover.
    Plain,
    /// On/off option: accent icon on a tinted background when on.
    Toggle(bool),
    /// Solid accent fill when on (the active tool, the play button).
    Primary(bool),
    /// Solid red: a destructive action waiting for a second click.
    Danger,
}

/// A framed single-line text field. egui outlines a focused field with the selection stroke,
/// which is dark (it is also highlighted text's colour), so draw the accent focus ring here.
fn text_field(ui: &mut Ui, field: egui::TextEdit) -> Response {
    let resp = ui.add(field);
    if resp.has_focus() {
        ui.painter().rect_stroke(resp.rect, 9, Stroke::new(1.5, ACCENT), StrokeKind::Inside);
    }
    resp
}

fn hover_t(ui: &Ui, resp: &Response) -> f32 {
    ui.ctx().animate_bool_responsive(resp.id.with("hover"), resp.hovered())
}

/// A square button with a big icon in it.
fn icon_button(ui: &mut Ui, icon: Icon, size: f32, look: Look) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    if ui.is_rect_visible(rect) {
        let t = hover_t(ui, &resp);
        let press = if resp.is_pointer_button_down_on() { 10 } else { 0 };
        let (bg, fg) = match look {
            Look::Primary(true) => (mix(ACCENT, Color32::WHITE, t * 0.25), ON_ACCENT),
            Look::Danger => (mix(DANGER, Color32::WHITE, t * 0.2), ON_ACCENT),
            Look::Toggle(true) => (mix(ACCENT_BG, with_alpha(ACCENT, 70), t * 0.5), ACCENT),
            _ => (white((t * 20.0) as u8 + press), mix(MUTED, TEXT, t)),
        };
        let p = ui.painter();
        p.rect_filled(rect, CornerRadius::same((size * 0.3) as u8), bg);
        let icon_size = (size * 0.5).round();
        icon.paint(p, Rect::from_center_size(rect.center(), Vec2::splat(icon_size)), fg);
    }
    resp
}

/// A picker tile: icon above a short label. `tint` colours the icon.
fn tile(ui: &mut Ui, icon: Icon, label: &str, selected: bool, size: Vec2, tint: Option<Color32>) -> Response {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    if ui.is_rect_visible(rect) {
        let t = hover_t(ui, &resp);
        let p = ui.painter();
        let (bg, stroke) = if selected {
            (ACCENT_BG, Stroke::new(1.5, ACCENT))
        } else {
            (white(7 + (t * 12.0) as u8), Stroke::new(1.0, white(10 + (t * 22.0) as u8)))
        };
        p.rect(rect, 12, bg, stroke, StrokeKind::Inside);
        let icon_size = (size.y * 0.4).clamp(16.0, 28.0).round();
        let icon_rect = Rect::from_center_size(egui::pos2(rect.center().x, rect.top() + size.y * 0.4), Vec2::splat(icon_size));
        let text_color = if selected { ACCENT } else { mix(MUTED, TEXT, t) };
        icon.paint(p, icon_rect, tint.unwrap_or(text_color));
        p.text(egui::pos2(rect.center().x, rect.bottom() - size.y * 0.19), Align2::CENTER_CENTER, label, FontId::proportional(11.0), text_color);
    }
    resp
}

/// Lay items out as a grid of equal-width tiles.
fn tile_grid<T: Copy>(ui: &mut Ui, items: &[T], cols: usize, height: f32, mut add: impl FnMut(&mut Ui, T, Vec2)) {
    let gap = 6.0;
    let w = ((ui.available_width() - gap * (cols - 1) as f32) / cols as f32).floor();
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing = vec2(gap, gap);
        for row in items.chunks(cols) {
            ui.horizontal(|ui| {
                for &item in row {
                    add(ui, item, vec2(w, height));
                }
            });
        }
    });
}

/// A pill-shaped button with optional icon, text and a chevron. Used for drop-downs.
fn chip(ui: &mut Ui, icon: Option<(Icon, Color32)>, text: &str, min_w: f32, chevron: bool) -> Response {
    let font = FontId::new(13.0, bold_family());
    let galley = ui.painter().layout_no_wrap(text.to_string(), font, TEXT);
    let icon_w = if icon.is_some() { 22.0 } else { 0.0 };
    let chev_w = if chevron { 16.0 } else { 0.0 };
    let w = (galley.size().x + icon_w + chev_w + 20.0).max(min_w);
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 32.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let t = hover_t(ui, &resp);
        let p = ui.painter();
        p.rect(rect, 10, white(10 + (t * 14.0) as u8), Stroke::new(1.0, white(14 + (t * 20.0) as u8)), StrokeKind::Inside);
        let content_w = galley.size().x + icon_w + chev_w;
        let mut x = rect.center().x - content_w / 2.0;
        if let Some((ic, color)) = icon {
            ic.paint(p, Rect::from_min_size(egui::pos2(x, rect.center().y - 9.0), Vec2::splat(18.0)), color);
            x += icon_w;
        }
        let gsize = galley.size();
        p.galley(egui::pos2(x, rect.center().y - gsize.y / 2.0), galley, TEXT);
        x += gsize.x;
        if chevron {
            Icon::ChevronDown.paint(p, Rect::from_min_size(egui::pos2(x + 3.0, rect.center().y - 6.5), Vec2::splat(13.0)), MUTED);
        }
    }
    resp
}

/// Full-width-friendly button: icon and label centred in `size`. `selected` gives the accent look.
fn option_button(ui: &mut Ui, icon: Icon, label: &str, selected: bool, size: Vec2) -> Response {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    if ui.is_rect_visible(rect) {
        let t = hover_t(ui, &resp);
        let p = ui.painter();
        let (bg, stroke, fg) = if selected {
            (ACCENT_BG, Stroke::new(1.5, ACCENT), ACCENT)
        } else {
            (white(8 + (t * 12.0) as u8), Stroke::new(1.0, white(14 + (t * 20.0) as u8)), mix(MUTED, TEXT, t))
        };
        p.rect(rect, 10, bg, stroke, StrokeKind::Inside);
        let galley = p.layout_no_wrap(label.to_string(), FontId::proportional(12.5), fg);
        let total = 16.0 + 7.0 + galley.size().x;
        let x = rect.center().x - total / 2.0;
        icon.paint(p, Rect::from_min_size(egui::pos2(x, rect.center().y - 8.0), Vec2::splat(16.0)), fg);
        p.galley(egui::pos2(x + 23.0, rect.center().y - galley.size().y / 2.0), galley, fg);
    }
    resp
}

/// Wide accent call-to-action button.
fn primary_button(ui: &mut Ui, icon: Icon, text: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let t = hover_t(ui, &resp);
        let p = ui.painter();
        p.rect_filled(rect, 12, mix(ACCENT, Color32::WHITE, t * 0.2));
        let galley = p.layout_no_wrap(text.to_string(), FontId::new(15.0, bold_family()), ON_ACCENT);
        let total = 22.0 + 8.0 + galley.size().x;
        let x = rect.center().x - total / 2.0;
        icon.paint(p, Rect::from_min_size(egui::pos2(x, rect.center().y - 11.0), Vec2::splat(22.0)), ON_ACCENT);
        let gy = rect.center().y - galley.size().y / 2.0;
        p.galley(egui::pos2(x + 30.0, gy), galley, ON_ACCENT);
    }
    resp
}

/// iOS-style switch on the right of a full-width labelled row.
fn switch_row(ui: &mut Ui, label: &str, value: &mut bool) -> Response {
    let (rect, mut resp) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::click());
    if resp.clicked() {
        *value = !*value;
        resp.mark_changed();
    }
    if ui.is_rect_visible(rect) {
        let t = hover_t(ui, &resp);
        let on = ui.ctx().animate_bool_responsive(resp.id.with("on"), *value);
        let p = ui.painter();
        p.text(rect.left_center(), Align2::LEFT_CENTER, label, FontId::proportional(13.0), mix(Color32::from_rgb(204, 208, 222), TEXT, t));
        let track = Rect::from_min_size(egui::pos2(rect.right() - 36.0, rect.center().y - 10.0), vec2(36.0, 20.0));
        p.rect_filled(track, 10, mix(Color32::from_rgb(54, 58, 78), ACCENT, on));
        let knob_x = egui::lerp(track.left() + 10.0..=track.right() - 10.0, on);
        p.circle_filled(egui::pos2(knob_x, track.center().y), 7.5, if on > 0.5 { ON_ACCENT } else { TEXT });
    }
    resp
}

/// Label on the left, slider filling the rest of the row. Only the user's own edits are clamped
/// to the slider's range: a value already outside it (the Sun's mass, say) is shown as-is
/// rather than silently rewritten every frame.
fn slider_row(ui: &mut Ui, label: &str, slider: egui::Slider) -> Response {
    let slider = slider.clamping(egui::SliderClamping::Edits);
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(vec2(84.0, 24.0), Layout::left_to_right(Align::Center), |ui| {
            ui.set_min_width(84.0);
            ui.add(egui::Label::new(muted(label)).truncate());
        });
        fill_slider_width(ui);
        ui.add(slider)
    })
    .inner
}

/// Width of a slider's value box; the slider fills the rest of the row.
const VALUE_W: f32 = 64.0;

/// Make the next slider (plus its value box) exactly fill the remaining row width.
fn fill_slider_width(ui: &mut Ui) {
    let gap = ui.spacing().item_spacing.x;
    ui.spacing_mut().interact_size.x = VALUE_W;
    ui.spacing_mut().button_padding.x = 6.0;
    ui.spacing_mut().slider_width = (ui.available_width() - VALUE_W - gap).max(60.0);
}

/// Small spaced-out caps header separating groups of controls.
fn section(ui: &mut Ui, title: &str) {
    ui.add_space(6.0);
    ui.label(bold(title.to_uppercase(), 10.5).color(FAINT).extra_letter_spacing(1.0));
}

/// A softly tinted hint box with an icon.
fn tip(ui: &mut Ui, icon: Icon, text: &str) {
    Frame::new().fill(with_alpha(INFO, 16)).corner_radius(10).inner_margin(Margin::symmetric(10, 8)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
            icon.paint(ui.painter(), r, INFO);
            ui.add(egui::Label::new(RichText::new(text).size(11.5).color(Color32::from_rgb(178, 210, 236))).wrap());
        });
    });
}

fn keycap(ui: &mut Ui, key: &str) {
    Frame::new()
        .fill(white(12))
        .stroke(Stroke::new(1.0, white(30)))
        .corner_radius(6)
        .inner_margin(Margin::symmetric(7, 2))
        .show(ui, |ui| ui.label(bold(key, 11.5).color(TEXT)));
}

/// Thin vertical divider inside a horizontal bar.
fn divider(ui: &mut Ui) {
    let (r, _) = ui.allocate_exact_size(vec2(1.0, 24.0), Sense::hover());
    ui.painter().rect_filled(r, 0, white(22));
}

/// Icon + title (+ subtitle) header for cards and modals; returns true if the close button was clicked.
fn card_header(ui: &mut Ui, icon: Icon, title: &str, subtitle: Option<&str>, closable: bool) -> bool {
    let mut close = false;
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(Vec2::splat(36.0), Sense::hover());
        ui.painter().rect_filled(r, 10, ACCENT_BG);
        icon.paint(ui.painter(), Rect::from_center_size(r.center(), Vec2::splat(20.0)), ACCENT);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            if subtitle.is_none() {
                ui.add_space(7.0);
            }
            ui.label(bold(title, 15.0).color(TEXT));
            if let Some(s) = subtitle {
                ui.label(RichText::new(s).size(11.5).color(MUTED));
            }
        });
        if closable {
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                close = icon_button(ui, Icon::Close, 28.0, Look::Plain).on_hover_text("Close").clicked();
            });
        }
    });
    ui.add_space(4.0);
    close
}

// ── Icon & label tables ────────────────────────────────────────────────────

fn tool_icon(t: Tool) -> Icon {
    match t {
        Tool::Select => Icon::Cursor,
        Tool::Spawn => Icon::SpawnPlanet,
        Tool::Terraform => Icon::Brush,
        Tool::Smash => Icon::Comet,
        Tool::Fly => Icon::Rocket,
        Tool::Walk => Icon::Astronaut,
    }
}

pub fn kind_icon(k: BodyKind) -> Icon {
    match k {
        BodyKind::Star => Icon::Star,
        BodyKind::Rocky => Icon::Rocky,
        BodyKind::Earthlike => Icon::Globe,
        BodyKind::Desert => Icon::Dunes,
        BodyKind::Cloudy => Icon::Cloud,
        BodyKind::GasGiant => Icon::GasGiant,
        BodyKind::IceGiant => Icon::IceGiant,
        BodyKind::Lava => Icon::Volcano,
        BodyKind::Ice => Icon::Snowflake,
        BodyKind::Asteroid => Icon::Asteroid,
        BodyKind::NeutronStar => Icon::NeutronStar,
        BodyKind::BlackHole => Icon::BlackHole,
    }
}

fn kind_short(k: BodyKind) -> &'static str {
    match k {
        BodyKind::Star => "Star",
        BodyKind::Rocky => "Rocky",
        BodyKind::Earthlike => "Earth",
        BodyKind::Desert => "Desert",
        BodyKind::Cloudy => "Cloudy",
        BodyKind::GasGiant => "Gas giant",
        BodyKind::IceGiant => "Ice giant",
        BodyKind::Lava => "Lava",
        BodyKind::Ice => "Ice",
        BodyKind::Asteroid => "Asteroid",
        BodyKind::NeutronStar => "Neutron",
        BodyKind::BlackHole => "Black hole",
    }
}

fn kind_tint(k: BodyKind) -> Color32 {
    match k {
        BodyKind::Star => Color32::from_rgb(255, 204, 110),
        BodyKind::Rocky => Color32::from_rgb(196, 196, 206),
        BodyKind::Earthlike => Color32::from_rgb(112, 184, 255),
        BodyKind::Desert => Color32::from_rgb(236, 146, 96),
        BodyKind::Cloudy => Color32::from_rgb(238, 214, 156),
        BodyKind::GasGiant => Color32::from_rgb(228, 176, 128),
        BodyKind::IceGiant => Color32::from_rgb(132, 222, 232),
        BodyKind::Lava => Color32::from_rgb(255, 124, 76),
        BodyKind::Ice => Color32::from_rgb(196, 232, 255),
        BodyKind::Asteroid => Color32::from_rgb(178, 168, 156),
        BodyKind::NeutronStar => Color32::from_rgb(182, 204, 255),
        BodyKind::BlackHole => Color32::from_rgb(206, 166, 255),
    }
}

fn brush_info(b: Brush) -> (Icon, &'static str, Color32) {
    match b {
        Brush::Raise => (Icon::Raise, "Raise", Color32::from_rgb(156, 214, 124)),
        Brush::Lower => (Icon::Lower, "Dig", Color32::from_rgb(126, 176, 232)),
        Brush::Crater => (Icon::Crater, "Crater", Color32::from_rgb(196, 186, 176)),
        Brush::Water => (Icon::Waves, "Water", Color32::from_rgb(96, 176, 255)),
        Brush::Lava => (Icon::Volcano, "Lava", Color32::from_rgb(255, 124, 66)),
        Brush::Ice => (Icon::Snowflake, "Ice", Color32::from_rgb(196, 232, 255)),
        Brush::Vegetation => (Icon::Tree, "Forest", Color32::from_rgb(108, 204, 118)),
        Brush::Rock => (Icon::Mountain, "Rock", Color32::from_rgb(184, 174, 164)),
        Brush::Sand => (Icon::Dunes, "Sand", Color32::from_rgb(238, 208, 146)),
        Brush::Scorch => (Icon::Flame, "Scorch", Color32::from_rgb(255, 156, 96)),
        Brush::Erase => (Icon::Eraser, "Restore", Color32::from_rgb(206, 206, 216)),
    }
}

fn preset_info(p: Preset) -> (Icon, &'static str) {
    match p {
        Preset::SolarSystem => (Icon::Planet, "Sun, 8 planets, Pluto and the major moons"),
        Preset::RogueBlackHole => (Icon::Flyby, "A black hole plows through the Solar System"),
        Preset::TheiaImpact => (Icon::Comet, "A Mars-sized world hits the proto-Earth"),
        Preset::HeadOn => (Icon::Burst, "Two worlds collide head-on"),
        Preset::BinaryStars => (Icon::Binary, "Circumbinary planets around two suns"),
        Preset::BlackHoleFeast => (Icon::BlackHole, "A supermassive black hole, a doomed star"),
        Preset::Supernova => (Icon::Supernova, "A blue supergiant explodes after 20 s"),
        Preset::GalaxyCollision => (Icon::Galaxy, "Two ~34k-star galaxies collide"),
        Preset::Protoplanetary => (Icon::Disk, "Planetesimals merge into planets"),
        Preset::Empty => (Icon::Empty, "Build from scratch"),
    }
}

fn center_info(c: CenterKind) -> (Icon, &'static str, Color32) {
    match c {
        CenterKind::SunLike => (Icon::Star, "Sun-like", Color32::from_rgb(255, 210, 120)),
        CenterKind::RedDwarf => (Icon::Star, "Red dwarf", Color32::from_rgb(255, 130, 96)),
        CenterKind::BlueGiant => (Icon::Star, "Blue giant", Color32::from_rgb(140, 190, 255)),
        CenterKind::Binary => (Icon::Binary, "Binary", Color32::from_rgb(255, 204, 140)),
        CenterKind::BlackHole => (Icon::BlackHole, "Black hole", Color32::from_rgb(206, 166, 255)),
        CenterKind::Nothing => (Icon::Empty, "Nothing", MUTED),
    }
}

fn fmt_time(t: f64) -> String {
    if t < 120.0 {
        format!("{t:.0}s")
    } else if t < 7200.0 {
        format!("{:.1}m", t / 60.0)
    } else {
        format!("{:.1}h", t / 3600.0)
    }
}

fn fmt_speed(s: f64) -> String {
    if s >= 9.95 {
        format!("{s:.0}×")
    } else if s >= 1.0 {
        let t = format!("{s:.1}");
        format!("{}×", t.trim_end_matches(".0"))
    } else {
        format!("{s:.2}×")
    }
}

fn fmt_count(n: usize) -> String {
    if n >= 10_000 { format!("{:.1}k", n as f64 / 1000.0) } else { n.to_string() }
}

// ── Top level ──────────────────────────────────────────────────────────────

/// Draw the whole UI. `ppp` = pixels per point, used to convert camera pixels to egui points.
/// `scene_px` = the size the 3D scene is drawn at (window size times the render scale).
pub fn draw(root: &mut Ui, game: &mut Game, cam: &CameraFrame, ppp: f32, scene_px: (u32, u32)) {
    let ctx = root.ctx().clone();
    draw_labels(&ctx, game, cam, ppp);
    hud::scene_overlay(&ctx, game, cam, ppp);
    if !game.show_ui {
        hidden_hint(&ctx, game);
        return;
    }
    if game.glyph_test {
        icon_gallery(&ctx);
    }
    universe_bar(&ctx, game);
    time_bar(&ctx, game);
    view_bar(&ctx, game);
    if game.exploring() {
        // Flying or walking: the tools make way for that mode's own panel.
        hud::flight_hud(&ctx, game);
        hud::walk_hud(&ctx, game);
    } else {
        tool_dock(&ctx, game);
        hint_bar(&ctx, game);
    }
    if game.show_settings {
        // Both live on the right-hand side; the inspector comes back when settings close.
        options::settings_panel(&ctx, game, cam, scene_px);
    } else {
        inspector(&ctx, game);
    }
    event_log(&ctx, game);
    stats_bar(&ctx, game);
    hud::update_banner(&ctx, game);
    custom_modal(&ctx, game);
    saves_modal(&ctx, game);
    help_modal(&ctx, game);
}

fn hidden_hint(ctx: &egui::Context, game: &mut Game) {
    let clicked = floating(ctx, "hidden_hint", Align2::CENTER_TOP, vec2(0.0, EDGE), glass(4).corner_radius(14), |ui| {
        ui.horizontal(|ui| {
            let r = icon_button(ui, Icon::Eye, 30.0, Look::Plain);
            ui.label(RichText::new("Press").color(MUTED).size(12.0));
            keycap(ui, "H");
            ui.label(RichText::new("to show the interface").color(MUTED).size(12.0));
            ui.add_space(6.0);
            r.clicked()
        })
        .inner
    });
    if clicked {
        game.show_ui = true;
    }
}

// ── Top bars ───────────────────────────────────────────────────────────────

fn universe_bar(ctx: &egui::Context, game: &mut Game) {
    floating(ctx, "bar_universe", Align2::LEFT_TOP, vec2(EDGE, EDGE), glass(6), |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            // Brand: the app icon and name.
            let (logo, _) = ui.allocate_exact_size(Vec2::splat(36.0), Sense::hover());
            ui.painter().image(logo_texture(ui.ctx()).id(), logo, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
            // Two-line wordmark keeps the bar narrow.
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.add_space(2.0);
                ui.label(bold("Arthur's", 11.5).color(ACCENT));
                ui.label(bold("Universe Playground", 13.5).color(TEXT));
            });
            ui.add_space(2.0);
            divider(ui);
            ui.add_space(-4.0);
            universe_dropdown(ui, game);
        });
    });
}

/// The app icon. `build.rs` rasterises `assets/AppIcon.svg` to 192×192 premultiplied RGBA.
fn logo_texture(ctx: &egui::Context) -> egui::TextureHandle {
    let id = Id::new("app_logo_texture");
    if let Some(t) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)) {
        return t;
    }
    let image = egui::ColorImage::from_rgba_premultiplied([192, 192], include_bytes!(concat!(env!("OUT_DIR"), "/logo.rgba")));
    let tex = ctx.load_texture("app_logo", image, egui::TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(id, tex.clone()));
    tex
}

/// "Select universe" drop-down showing the current universe; opens the preset menu.
fn universe_dropdown(ui: &mut Ui, game: &mut Game) {
    {
        let caption = ui.painter().layout_no_wrap("Select universe".into(), FontId::proportional(10.5), FAINT);
        let name = ui.painter().layout_no_wrap(game.universe.name.clone(), FontId::new(12.5, bold_family()), TEXT);
        let text_w = caption.size().x.max(name.size().x).min(220.0);
        let (rect, resp) = ui.allocate_exact_size(vec2(10.0 + 18.0 + 10.0 + text_w + 28.0, 36.0), Sense::click());
        let t = hover_t(ui, &resp);
        let p = ui.painter();
        p.rect_filled(rect, 11, white(8 + (t * 14.0) as u8));
        Icon::Galaxy.paint(p, Rect::from_min_size(egui::pos2(rect.left() + 10.0, rect.center().y - 9.0), Vec2::splat(18.0)), ACCENT);
        let tx = rect.left() + 38.0;
        p.galley(egui::pos2(tx, rect.top() + 3.0), caption, FAINT);
        p.with_clip_rect(Rect::from_min_size(egui::pos2(tx, rect.top()), vec2(text_w, 36.0))).galley(egui::pos2(tx, rect.top() + 17.0), name, TEXT);
        Icon::ChevronDown.paint(p, Rect::from_center_size(egui::pos2(rect.right() - 14.0, rect.center().y), Vec2::splat(14.0)), mix(MUTED, TEXT, t));
        let resp = resp.on_hover_text("Load a preset or create your own universe");

        Popup::menu(&resp).width(330.0).gap(10.0).frame(popup_frame()).show(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.label(bold("SELECT UNIVERSE", 10.5).color(FAINT).extra_letter_spacing(1.0));
            ui.add_space(4.0);
            for preset in Preset::ALL {
                let (icon, desc) = preset_info(preset);
                if menu_row(ui, icon, preset.label(), desc, None).clicked() {
                    game.load_preset(preset);
                    ui.close();
                }
            }
            ui.add_space(4.0);
            ui.separator();
            ui.add_space(2.0);
            if menu_row(ui, Icon::Sparkles, "Create your own universe…", "Pick a centre, planets, moons and chaos", Some(ACCENT)).clicked() {
                game.show_custom = true;
                ui.close();
            }
            if game.saves.dir.is_some() {
                let resume = game.saves.autosave_info().map(|s| (s.path.clone(), format!("{} · {}", s.name, save::ago(s.saved_at, save::now()))));
                if let Some((path, desc)) = resume {
                    if menu_row(ui, Icon::Reset, "Continue where you left off", &desc, None).clicked() {
                        game.load_save(&path);
                        ui.close();
                    }
                }
                if menu_row(ui, Icon::Folder, "Saved universes…", "Save this universe, or load one you saved", None).clicked() {
                    game.open_saves();
                    ui.close();
                }
            }
        });
    }
}

/// A full-width menu entry: icon, title and a muted one-line description.
fn menu_row(ui: &mut Ui, icon: Icon, title: &str, desc: &str, tint: Option<Color32>) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 42.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let t = hover_t(ui, &resp);
        let p = ui.painter();
        p.rect_filled(rect, 10, white((t * 16.0) as u8));
        let ic = Rect::from_min_size(egui::pos2(rect.left() + 8.0, rect.center().y - 15.0), Vec2::splat(30.0));
        p.rect_filled(ic, 9, white(8 + (t * 6.0) as u8));
        icon.paint(p, ic.shrink(6.0), tint.unwrap_or(mix(MUTED, ACCENT, t)));
        p.text(egui::pos2(rect.left() + 48.0, rect.top() + 6.0), Align2::LEFT_TOP, title, FontId::new(13.0, bold_family()), tint.unwrap_or(TEXT));
        p.text(egui::pos2(rect.left() + 48.0, rect.top() + 23.0), Align2::LEFT_TOP, desc, FontId::proportional(11.0), MUTED);
    }
    resp
}

fn time_bar(ctx: &egui::Context, game: &mut Game) {
    floating(ctx, "bar_time", Align2::CENTER_TOP, vec2(0.0, EDGE), glass(6), |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            if icon_button(ui, Icon::Slower, 36.0, Look::Plain).on_hover_text("Slow down  ( [ or − )").clicked() {
                game.time_scale = (game.time_scale / 2.0).max(0.01);
            }
            let play_icon = if game.paused { Icon::Play } else { Icon::Pause };
            let tip = if game.paused { "Play  (Space)" } else { "Pause  (Space)" };
            if icon_button(ui, play_icon, 36.0, Look::Primary(true)).on_hover_text(tip).clicked() {
                game.paused = !game.paused;
            }
            if icon_button(ui, Icon::Faster, 36.0, Look::Plain).on_hover_text("Speed up  ( ] or + )").clicked() {
                game.time_scale = (game.time_scale * 2.0).min(1000.0);
            }
            ui.add_space(4.0);
            let speed = chip(ui, None, &fmt_speed(game.time_scale), 64.0, true).on_hover_text("Simulation speed");
            Popup::from_toggle_button_response(&speed)
                .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
                .align(RectAlign::BOTTOM)
                .gap(12.0)
                .width(280.0)
                .frame(popup_frame())
                .show(|ui| {
                    ui.set_width(256.0);
                    section(ui, "Simulation speed");
                    fill_slider_width(ui);
                    ui.add(egui::Slider::new(&mut game.time_scale, 0.01..=1000.0).logarithmic(true).suffix("×").max_decimals(2));
                    ui.horizontal(|ui| {
                        for v in [0.1, 1.0, 10.0, 100.0, 500.0] {
                            let sel = (game.time_scale - v).abs() < 1e-6;
                            if ui.selectable_label(sel, fmt_speed(v)).clicked() {
                                game.time_scale = v;
                            }
                        }
                    });
                });
            ui.add_space(6.0);
            divider(ui);
            ui.add_space(6.0);
            let (r, _) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::hover());
            Icon::Clock.paint(ui.painter(), r, MUTED);
            let text = if game.paused {
                bold("Paused", 13.0).color(ACCENT)
            } else {
                RichText::new(fmt_time(game.universe.time)).monospace().size(13.0).color(TEXT)
            };
            ui.add_sized([58.0, 24.0], egui::Label::new(text)).on_hover_text("Simulated time");
            ui.add_space(4.0);
        });
    });
}

fn view_bar(ctx: &egui::Context, game: &mut Game) {
    floating(ctx, "bar_view", Align2::RIGHT_TOP, vec2(-EDGE, EDGE), glass(6), |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            if icon_button(ui, Icon::Orbit, 36.0, Look::Toggle(game.settings.trails)).on_hover_text("Orbit trails  (T)").clicked() {
                game.settings.trails = !game.settings.trails;
            }
            if icon_button(ui, Icon::Tag, 36.0, Look::Toggle(game.settings.labels)).on_hover_text("Labels  (L)").clicked() {
                game.settings.labels = !game.settings.labels;
            }
            if icon_button(ui, Icon::Lens, 36.0, Look::Toggle(game.settings.lensing)).on_hover_text("Gravitational lensing").clicked() {
                game.settings.lensing = !game.settings.lensing;
            }
            ui.add_space(2.0);
            divider(ui);
            ui.add_space(2.0);

            let muted = game.settings.muted;
            let (icon, tip) = if muted { (Icon::SpeakerOff, "Sound is off: click to turn it on  (M)") } else { (Icon::Speaker, "Mute sound  (M)") };
            if icon_button(ui, icon, 36.0, Look::Plain).on_hover_text(tip).clicked() {
                game.settings.muted = !muted;
            }
            if icon_button(ui, Icon::Gear, 36.0, Look::Toggle(game.show_settings)).on_hover_text("Settings: display, sound, graphics, interface").clicked() {
                game.show_settings = !game.show_settings;
            }

            let physics = icon_button(ui, Icon::Atom, 36.0, Look::Plain).on_hover_text("Physics settings");
            settings_popup(&physics).show(|ui| {
                ui.set_width(POPUP_W);
                physics_settings(ui, game);
            });

            if icon_button(ui, Icon::Help, 36.0, Look::Toggle(game.show_help)).on_hover_text("How to play").clicked() {
                game.show_help = !game.show_help;
            }
            if icon_button(ui, Icon::EyeOff, 36.0, Look::Plain).on_hover_text("Hide interface  (H)").clicked() {
                game.show_ui = false;
            }
        });
    });
}

fn settings_popup(resp: &Response) -> Popup<'_> {
    Popup::from_toggle_button_response(resp)
        .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
        .align(RectAlign::BOTTOM_END)
        .gap(12.0)
        .width(POPUP_W)
        .frame(popup_frame().inner_margin(14))
}

fn physics_settings(ui: &mut Ui, game: &mut Game) {
    card_header(ui, Icon::Atom, "Physics", Some("Rules of this universe"), false);
    let u = &mut game.universe;
    slider_row(ui, "Gravity G", egui::Slider::new(&mut u.g, 0.05..=5.0).logarithmic(true));

    section(ui, "Collisions");
    tile_grid(ui, &CollisionMode::ALL, 2, 38.0, |ui, m, size| {
        let (icon, label) = match m {
            CollisionMode::Realistic => (Icon::Target, "Realistic"),
            CollisionMode::AlwaysMerge => (Icon::Implode, "Always merge"),
            CollisionMode::AlwaysShatter => (Icon::Burst, "Always shatter"),
            CollisionMode::PassThrough => (Icon::EyeOff, "Ghost"),
        };
        if option_button(ui, icon, label, u.collisions == m, size).on_hover_text(m.label()).clicked() {
            u.collisions = m;
        }
    });
    ui.add_space(2.0);
    switch_row(ui, "Tidal disruption by black holes", &mut u.tidal_disruption);

    section(ui, "Simulation");
    slider_row(ui, "Trail length", egui::Slider::new(&mut u.trail_len, 10..=3000).logarithmic(true));
    slider_row(ui, "Trail spacing", egui::Slider::new(&mut u.trail_interval, 0.1..=10.0).logarithmic(true).suffix("×"));
    slider_row(ui, "Max particles", egui::Slider::new(&mut u.max_particles, 1000..=crate::renderer::MAX_PARTICLES).logarithmic(true));
    slider_row(ui, "Physics step", egui::Slider::new(&mut u.base_dt, 0.0005..=0.02).logarithmic(true));

    section(ui, "Clean up");
    let mut act = None;
    tile_grid(ui, &[0, 1], 2, 38.0, |ui, i, size| {
        let (icon, label, hint) = if i == 0 {
            (Icon::Particles, "Clear particles", "Remove all debris, dust and nebula particles")
        } else {
            (Icon::Orbit, "Clear trails", "Erase every orbit trail")
        };
        if option_button(ui, icon, label, false, size).on_hover_text(hint).clicked() {
            act = Some(i);
        }
    });
    match act {
        Some(0) => u.particles.clear(),
        Some(_) => {
            for b in &mut u.bodies {
                b.trail.clear();
            }
        }
        None => {}
    }
}

// ── Tools ──────────────────────────────────────────────────────────────────

fn tool_dock(ctx: &egui::Context, game: &mut Game) {
    let open_id = Id::new("tool_card_open");
    let mut card_open = ctx.data_mut(|d| *d.get_temp_mut_or(open_id, true));
    floating(ctx, "tool_dock", Align2::LEFT_TOP, vec2(EDGE, BELOW_BARS), glass(7), |ui| {
        ui.spacing_mut().item_spacing.y = 6.0;
        for (i, t) in Tool::ALL.iter().enumerate() {
            let sel = game.tool == *t;
            let resp = icon_button(ui, tool_icon(*t), DOCK_BTN, Look::Primary(sel));
            // Key number badge in the corner.
            let badge = egui::pos2(resp.rect.right() - 8.0, resp.rect.bottom() - 8.0);
            ui.painter().text(badge, Align2::CENTER_CENTER, format!("{}", i + 1), FontId::new(9.5, bold_family()), if sel { ON_ACCENT } else { FAINT });
            let resp = resp.on_hover_ui(|ui| {
                ui.set_max_width(240.0);
                ui.horizontal(|ui| {
                    ui.label(bold(t.label(), 13.0).color(TEXT));
                    keycap(ui, &format!("{}", i + 1));
                });
                ui.label(RichText::new(t.hint()).size(11.5).color(MUTED));
            });
            if resp.clicked() {
                if sel {
                    card_open = !card_open;
                } else {
                    game.tool = *t;
                    card_open = true;
                }
            }
        }
    });
    ctx.data_mut(|d| d.insert_temp(open_id, card_open));
    if card_open {
        tool_card(ctx, game, &mut card_open);
        ctx.data_mut(|d| d.insert_temp(open_id, card_open));
    }
}

fn tool_card(ctx: &egui::Context, game: &mut Game, open: &mut bool) {
    let x = EDGE + DOCK_BTN + 14.0 + 2.0 + 10.0;
    let max_h = (ctx.content_rect().height() - BELOW_BARS - 210.0).max(200.0);
    floating(ctx, "tool_card", Align2::LEFT_TOP, vec2(x, BELOW_BARS), glass(14), |ui| {
        ui.set_width(CARD_W);
        ui.set_max_height(max_h + 60.0);
        let tool = game.tool;
        if card_header(ui, tool_icon(tool), tool.label(), None, true) {
            *open = false;
        }
        egui::ScrollArea::vertical().max_height(max_h).auto_shrink([false, true]).show(ui, |ui| {
            match tool {
                Tool::Select => select_tool(ui, game),
                Tool::Spawn => spawn_tool(ui, game),
                Tool::Terraform => terraform_tool(ui, game),
                Tool::Smash => smash_tool(ui, game),
                Tool::Fly => hud::fly_tool(ui, game),
                Tool::Walk => hud::walk_tool(ui, game),
            }
        });
    });
}

fn kind_grid(ui: &mut Ui, current: &mut BodyKind) -> bool {
    let mut changed = false;
    tile_grid(ui, &BodyKind::ALL, 4, 58.0, |ui, k, size| {
        if tile(ui, kind_icon(k), kind_short(k), *current == k, size, Some(kind_tint(k))).on_hover_text(k.label()).clicked() && *current != k {
            *current = k;
            changed = true;
        }
    });
    changed
}

fn select_tool(ui: &mut Ui, game: &mut Game) {
    section(ui, "Phenomena");
    let mut which = None;
    tile_grid(ui, &[0, 1, 2], 3, 64.0, |ui, i, size| {
        let (icon, label, hint, tint) = match i {
            0 => (Icon::BlackHole, "Black hole", "Drop a black hole at the centre of the view", kind_tint(BodyKind::BlackHole)),
            1 => (Icon::Flyby, "Rogue planet", "Send a rogue gas giant through the view", kind_tint(BodyKind::GasGiant)),
            _ => (Icon::Comet, "Comets", "Shower the selected body with 20 comets", INFO),
        };
        if tile(ui, icon, label, false, size, Some(tint)).on_hover_text(hint).clicked() {
            which = Some(i);
        }
    });
    match which {
        Some(0) => {
            let pos = game.camera.target;
            let mut b = game.universe.new_body("Black Hole", BodyKind::BlackHole, pos, DVec3::ZERO);
            b.mass = 20000.0;
            b.radius = black_hole_radius(b.mass);
            if let Some(i) = game.universe.dominant_attractor(pos, None) {
                b.vel = game.universe.bodies[i].vel;
            }
            let id = game.universe.add(b);
            game.selected = Some(id);
            game.universe.event("A black hole appeared out of nowhere!");
        }
        Some(1) => {
            let target = game.camera.target;
            let dir = game.universe.rng.unit();
            let dir = DVec3::new(dir.x, dir.y * 0.2, dir.z).normalize();
            let start = target + dir * game.camera.distance * 0.9;
            let side = crate::util::any_perpendicular(dir) * game.camera.distance * 0.08;
            let v = (target + side - start).normalize() * 14.0;
            let mut b = game.universe.new_body("Rogue Planet", BodyKind::GasGiant, start, v);
            b.mass = 600.0;
            b.radius = 7.0;
            game.universe.add(b);
            game.universe.event("A rogue gas giant is passing through!");
        }
        Some(_) => {
            if let Some(id) = game.selected {
                game.barrage(id, 20);
            } else {
                game.universe.event("Select a target for the comet shower first");
            }
        }
        None => {}
    }
    ui.add_space(6.0);
    tip(ui, Icon::Supernova, "Click a body to inspect it. Select a star to make it go supernova.");
}

fn spawn_tool(ui: &mut Ui, game: &mut Game) {
    let s = &mut game.spawn;
    section(ui, "Body");
    if kind_grid(ui, &mut s.kind) {
        let (m, r) = s.kind.defaults();
        s.mass = m;
        s.radius = r;
        s.rings = s.kind == BodyKind::GasGiant;
    }
    ui.add_space(4.0);
    text_field(ui, egui::TextEdit::singleline(&mut s.name).hint_text("Name (automatic)").desired_width(f32::INFINITY).margin(Margin::symmetric(10, 6)));
    section(ui, "Properties");
    slider_row(ui, "Mass", egui::Slider::new(&mut s.mass, 0.01..=100000.0).logarithmic(true));
    if s.kind != BodyKind::BlackHole {
        slider_row(ui, "Radius", egui::Slider::new(&mut s.radius, 0.1..=40.0).logarithmic(true));
    }
    if s.kind == BodyKind::Star {
        slider_row(ui, "Temperature", egui::Slider::new(&mut s.temperature, 2500.0..=30000.0).logarithmic(true).suffix(" K"));
    }
    section(ui, "Launch");
    switch_row(ui, "Circular orbit", &mut s.auto_orbit);
    if s.auto_orbit {
        switch_row(ui, "Retrograde", &mut s.retrograde);
    }
    switch_row(ui, "Rings", &mut s.rings);
    slider_row(ui, "Fling power", egui::Slider::new(&mut s.fling_power, 0.1..=5.0).logarithmic(true));
    ui.add_space(6.0);
    tip(ui, Icon::Orbit, "Click to place, drag to fling. The cyan line is the predicted path; it turns red if it will crash.");
}

fn terraform_tool(ui: &mut Ui, game: &mut Game) {
    section(ui, "Brush");
    let current = &mut game.brush.brush;
    tile_grid(ui, &Brush::ALL, 4, 58.0, |ui, b, size| {
        let (icon, label, tint) = brush_info(b);
        if tile(ui, icon, label, *current == b, size, Some(tint)).on_hover_text(b.label()).clicked() {
            *current = b;
        }
    });
    section(ui, "Stroke");
    slider_row(ui, "Size", egui::Slider::new(&mut game.brush.size, 0.03..=1.2));
    slider_row(ui, "Strength", egui::Slider::new(&mut game.brush.strength, 0.05..=1.0));
    ui.add_space(6.0);
    tip(ui, Icon::Globe, "Double-click a planet to follow it, then paint. Raise land on Earth-like worlds for continents; dig for oceans.");
}

fn smash_tool(ui: &mut Ui, game: &mut Game) {
    section(ui, "Projectile");
    kind_grid(ui, &mut game.smash.kind);
    section(ui, "Impact");
    slider_row(ui, "Size", egui::Slider::new(&mut game.smash.size, 0.2..=4.0).logarithmic(true).suffix("×"));
    slider_row(ui, "Speed", egui::Slider::new(&mut game.smash.speed, 2.0..=120.0).logarithmic(true));
    let (m0, _) = game.smash.kind.defaults();
    ui.label(RichText::new(format!("Projectile mass ≈ {:.2}", m0 * game.smash.size.powi(3))).size(11.5).color(FAINT));
    ui.add_space(4.0);
    if let Some(id) = game.selected {
        let mut act = None;
        tile_grid(ui, &[0, 1], 2, 56.0, |ui, i, size| {
            let (icon, label, hint) = if i == 0 {
                (Icon::Target, "Fire at selected", "Launch one projectile at the selected body")
            } else {
                (Icon::Comet, "Asteroid barrage", "Hurl 12 asteroids at the selected body  (B)")
            };
            if tile(ui, icon, label, false, size, Some(ACCENT)).on_hover_text(hint).clicked() {
                act = Some(i);
            }
        });
        match act {
            Some(0) => game.launch_at(id, None),
            Some(_) => game.barrage(id, 12),
            None => {}
        }
    } else {
        tip(ui, Icon::Target, "Click a planet to hurl a projectile at it, or select one to fire from here.");
    }
}

// ── Inspector ──────────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
enum Action {
    Follow,
    Unfollow,
    Moon,
    Circularize,
    Barrage,
    Supernova,
    Collapse,
    Delete,
    ResetPaint,
    Fly,
    Walk,
}

fn stat_cell(ui: &mut Ui, label: &str, value: &str, color: Color32, w: f32) {
    Frame::new().fill(white(7)).corner_radius(10).inner_margin(Margin::symmetric(9, 6)).show(ui, |ui| {
        ui.set_width(w - 18.0);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.label(RichText::new(label).size(10.5).color(FAINT));
            ui.label(bold(value, 13.5).color(color));
        });
    });
}

fn inspector(ctx: &egui::Context, game: &mut Game) {
    let Some(id) = game.selected else { return };
    let Some(idx) = game.universe.index_of(id) else { return };
    let mut close = false;
    let mut action: Option<Action> = None;
    let mut orphaned_layer: Option<u32> = None;
    let attractor = game.universe.dominant_attractor(game.universe.bodies[idx].pos, Some(id));
    let orbit = attractor.map(|a| {
        let c = &game.universe.bodies[a];
        let b = &game.universe.bodies[idx];
        let d = (b.pos - c.pos).length();
        let v = (b.vel - c.vel).length();
        // vis-viva semi-major axis -> period
        let mu = game.universe.g * (c.mass + b.mass);
        let energy = v * v / 2.0 - mu / d;
        let period = (energy < 0.0).then(|| {
            let a_axis = -mu / (2.0 * energy);
            std::f64::consts::TAU * (a_axis.powi(3) / mu).sqrt()
        });
        (c.name.clone(), d, v, period)
    });
    let following = game.camera.follow == Some(id);
    // While flying or walking the mode's panel sits along the bottom: leave it room.
    let exploring = game.exploring();
    let max_h = (ctx.content_rect().height() - BELOW_BARS - if exploring { 230.0 } else { 70.0 }).max(200.0);
    let conditions = crate::conditions::conditions(&game.universe, &game.universe.bodies[idx]);
    let solid = crate::terrain::has_solid_surface(game.universe.bodies[idx].kind);

    floating(ctx, "inspector", Align2::RIGHT_TOP, vec2(-EDGE, BELOW_BARS), glass(14), |ui| {
        ui.set_width(INSPECTOR_W);
        // Areas reuse last frame's size as max_rect; lift it so the scroll area can grow.
        ui.set_max_height(max_h);
        let b = &mut game.universe.bodies[idx];

        // Header: kind badge, editable name, kind picker, close.
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(Vec2::splat(44.0), Sense::hover());
            ui.painter().rect_filled(r, 12, with_alpha(kind_tint(b.kind), 30));
            kind_icon(b.kind).paint(ui.painter(), Rect::from_center_size(r.center(), Vec2::splat(26.0)), kind_tint(b.kind));
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.add(
                    egui::TextEdit::singleline(&mut b.name)
                        .frame(Frame::NONE)
                        .margin(Margin::ZERO)
                        .font(FontId::new(17.0, bold_family()))
                        .text_color(TEXT)
                        .desired_width(INSPECTOR_W - 44.0 - 50.0),
                );
                let resp = ui
                    .horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 3.0;
                        let l = ui.add(egui::Label::new(RichText::new(b.kind.label()).size(12.0).color(MUTED)).sense(Sense::click()));
                        let (r, c) = ui.allocate_exact_size(Vec2::splat(12.0), Sense::click());
                        Icon::ChevronDown.paint(ui.painter(), r, MUTED);
                        l.union(c)
                    })
                    .inner
                    .on_hover_text("Change type");
                let mut kind = b.kind;
                Popup::from_toggle_button_response(&resp).width(290.0).gap(8.0).frame(popup_frame()).show(|ui| {
                    ui.set_width(290.0);
                    section(ui, "Change type");
                    kind_grid(ui, &mut kind);
                });
                if kind != b.kind {
                    let mut rng = crate::util::Rng::new(b.id.wrapping_mul(31) + kind as u64);
                    b.kind = kind;
                    b.surface = crate::body::Surface::for_kind(kind, &mut rng);
                    // Only stars can be on a supernova countdown, and stars / black
                    // holes have no paintable surface: give the paint layer back.
                    b.supernova_timer = None;
                    if !kind.paintable() {
                        orphaned_layer = b.paint_layer.take();
                    }
                    if kind == BodyKind::BlackHole {
                        b.radius = black_hole_radius(b.mass);
                        b.feeding = 1.0;
                    }
                    if kind == BodyKind::Star {
                        b.update_star_colors();
                    }
                }
            });
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                close = icon_button(ui, Icon::Close, 28.0, Look::Plain).on_hover_text("Deselect  (Esc)").clicked();
            });
        });
        ui.add_space(6.0);

        egui::ScrollArea::vertical().max_height(max_h - 70.0).auto_shrink([false, true]).show(ui, |ui| {
            // Orbit summary.
            match &orbit {
                Some((name, d, v, period)) => {
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(Vec2::splat(15.0), Sense::hover());
                        Icon::Orbit.paint(ui.painter(), r, MUTED);
                        ui.label(RichText::new("Orbiting").size(12.0).color(MUTED));
                        ui.label(bold(name, 12.5).color(TEXT));
                    });
                    let w = ((ui.available_width() - 12.0) / 3.0).floor();
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        stat_cell(ui, "Distance", &format!("{d:.1}"), TEXT, w);
                        stat_cell(ui, "Speed", &format!("{v:.2}"), TEXT, w);
                        match period {
                            Some(p) => stat_cell(ui, "Period", &fmt_time(*p), TEXT, w),
                            None => stat_cell(ui, "Period", "Escaping", DANGER, w),
                        }
                    });
                }
                None => {
                    ui.label(RichText::new("Drifting freely").size(12.0).color(MUTED));
                }
            }

            // Actions.
            section(ui, "Actions");
            let mut items: Vec<(Icon, String, &str, Action, Option<Color32>, bool)> = vec![
                if following {
                    (Icon::Focus, "Following".into(), "Stop following  (F / Esc)", Action::Unfollow, None, true)
                } else {
                    (Icon::Focus, "Follow".into(), "Keep the camera on it  (F)", Action::Follow, None, false)
                },
                (Icon::Moon, "Add moon".into(), "Put a new moon in orbit around it", Action::Moon, None, false),
                (Icon::CircleOrbit, "Circularise".into(), "Make its orbit circular", Action::Circularize, None, false),
                (Icon::Comet, "Barrage".into(), "Hurl 12 asteroids at it  (B)", Action::Barrage, None, false),
            ];
            if exploring {
                // The camera belongs to the rocket / astronaut for now.
                items.remove(0);
            } else {
                let (label, hint) = if solid { ("Fly from here", "Launch the selected rocket from its surface") } else { ("Fly in orbit", "Start the selected rocket in orbit around it") };
                items.push((Icon::Rocket, label.into(), hint, Action::Fly, Some(INFO), false));
                if solid {
                    items.push((Icon::Astronaut, "Walk here".into(), "Stand on its surface and look around", Action::Walk, Some(INFO), false));
                }
            }
            if b.kind == BodyKind::Star {
                match b.supernova_timer {
                    Some(t) => items.push((Icon::Warning, format!("Boom in {t:.1}s"), "It's going to blow!", Action::Supernova, Some(DANGER), true)),
                    None => items.push((Icon::Supernova, "Supernova".into(), "Make this star explode", Action::Supernova, Some(Color32::from_rgb(255, 160, 120)), false)),
                }
            }
            if b.kind != BodyKind::BlackHole {
                items.push((Icon::Implode, "Collapse".into(), "Collapse it into a black hole", Action::Collapse, Some(kind_tint(BodyKind::BlackHole)), false));
            }
            items.push((Icon::Trash, "Delete".into(), "Remove it  (Del)", Action::Delete, Some(DANGER), false));
            let supernova_pending = b.supernova_timer.is_some();
            tile_grid(ui, &items.iter().enumerate().map(|(i, _)| i).collect::<Vec<_>>(), 3, 58.0, |ui, i, size| {
                let (icon, label, hint, act, tint, sel) = &items[i];
                if tile(ui, *icon, label, *sel, size, *tint).on_hover_text(*hint).clicked() {
                    if matches!(act, Action::Supernova) && supernova_pending {
                        return;
                    }
                    action = Some(*act);
                }
            });

            hud::conditions_section(ui, &conditions);

            // Physical properties.
            section(ui, "Physical");
            slider_row(ui, "Mass", egui::Slider::new(&mut b.mass, 0.001..=1_000_000.0).logarithmic(true));
            if b.kind == BodyKind::BlackHole {
                b.radius = black_hole_radius(b.mass);
                slider_row(ui, "Accretion", egui::Slider::new(&mut b.feeding, 0.0..=5.0));
            } else {
                slider_row(ui, "Radius", egui::Slider::new(&mut b.radius, 0.05..=60.0).logarithmic(true));
            }
            if b.kind == BodyKind::Star && slider_row(ui, "Temperature", egui::Slider::new(&mut b.temperature, 2500.0..=30000.0).logarithmic(true).suffix(" K")).changed() {
                b.update_star_colors();
            }
            slider_row(ui, "Spin", egui::Slider::new(&mut b.spin_rate, -3.0..=3.0));
            slider_row(ui, "Axial tilt", egui::Slider::new(&mut b.tilt, -std::f32::consts::PI..=std::f32::consts::PI));

            if !b.kind.is_stellar() {
                section(ui, "Surface");
                let s = &mut b.surface;
                ui.horizontal(|ui| {
                    ui.allocate_ui_with_layout(vec2(84.0, 24.0), Layout::left_to_right(Align::Center), |ui| {
                        ui.set_min_width(84.0);
                        ui.label(muted("Colours"));
                    });
                    for c in &mut s.colors {
                        v3_edit(ui, c);
                    }
                });
                ui.horizontal(|ui| {
                    ui.allocate_ui_with_layout(vec2(84.0, 24.0), Layout::left_to_right(Align::Center), |ui| {
                        ui.set_min_width(84.0);
                        ui.label(muted("Atmosphere"));
                    });
                    v3_edit(ui, &mut s.atmosphere);
                    fill_slider_width(ui);
                    ui.add(egui::Slider::new(&mut s.atmo_strength, 0.0..=2.0).clamping(egui::SliderClamping::Edits));
                });
                slider_row(ui, "Terrain seed", egui::Slider::new(&mut s.seed, 0.0..=10.0));
                slider_row(ui, "Terrain height", egui::Slider::new(&mut s.terrain, 0.0..=0.2));
                if b.kind == BodyKind::Earthlike {
                    slider_row(ui, "Sea level", egui::Slider::new(&mut s.sea_level, -1.0..=1.0));
                    slider_row(ui, "Clouds", egui::Slider::new(&mut s.clouds, 0.0..=1.5));
                }
                slider_row(ui, "Molten heat", egui::Slider::new(&mut b.heat, 0.0..=1.0));
                let mut has_ring = b.ring.is_some();
                if switch_row(ui, "Rings", &mut has_ring).changed() {
                    b.ring = has_ring.then(|| {
                        let r = b.radius as f32;
                        Ring { inner: r * 1.3, outer: r * 2.2, color: Vec3::new(0.8, 0.75, 0.65), opacity: 0.8 }
                    });
                }
                if let Some(r) = &mut b.ring {
                    slider_row(ui, "Ring inner", egui::Slider::new(&mut r.inner, 0.1..=100.0).logarithmic(true));
                    slider_row(ui, "Ring outer", egui::Slider::new(&mut r.outer, 0.1..=150.0).logarithmic(true));
                    ui.horizontal(|ui| {
                        ui.allocate_ui_with_layout(vec2(84.0, 24.0), Layout::left_to_right(Align::Center), |ui| {
                            ui.set_min_width(84.0);
                            ui.label(muted("Ring colour"));
                        });
                        v3_edit(ui, &mut r.color);
                        fill_slider_width(ui);
                        ui.add(egui::Slider::new(&mut r.opacity, 0.0..=1.0).clamping(egui::SliderClamping::Edits));
                    });
                    if r.outer < r.inner + 0.1 {
                        r.outer = r.inner + 0.1;
                    }
                }
                if b.paint_layer.is_some() {
                    ui.add_space(4.0);
                    let resp = chip(ui, Some((Icon::Reset, MUTED)), "Reset painted surface", ui.available_width(), false);
                    if resp.clicked() {
                        action = Some(Action::ResetPaint);
                    }
                }
            }
        });
    });

    if close {
        game.selected = None;
    }
    if let Some(l) = orphaned_layer {
        game.universe.paint.release(l);
    }
    match action {
        Some(Action::Follow) => game.focus(id),
        Some(Action::Unfollow) => game.camera.follow = None,
        Some(Action::Moon) => game.add_moon(id),
        Some(Action::Circularize) => game.circularize(id),
        Some(Action::Barrage) => game.barrage(id, 12),
        Some(Action::Supernova) => {
            game.universe.trigger_supernova(id, 3.0);
        }
        Some(Action::Collapse) => game.collapse_to_black_hole(id),
        Some(Action::Delete) => game.delete(id),
        Some(Action::Fly) => game.start_flight(Some(id), None),
        Some(Action::Walk) => {
            game.start_walk(id, None);
        }
        Some(Action::ResetPaint) => {
            if let Some(i) = game.universe.index_of(id) {
                if let Some(l) = game.universe.bodies[i].paint_layer.take() {
                    game.universe.paint.release(l);
                }
            }
        }
        None => {}
    }
}

fn v3_edit(ui: &mut Ui, v: &mut Vec3) -> Response {
    let mut c = [v.x, v.y, v.z];
    let r = ui.color_edit_button_rgb(&mut c);
    *v = Vec3::from(c);
    r
}

// ── Bottom row ─────────────────────────────────────────────────────────────

/// Split a log line "[t=  12.3] message" into ("12.3", "message").
fn split_log_line(line: &str) -> (&str, &str) {
    match line.strip_prefix("[t=").and_then(|r| r.split_once("] ")) {
        Some((t, msg)) => (t.trim(), msg),
        None => ("", line),
    }
}

const LOG_W: f32 = 430.0;
const LOG_ROWS: f32 = 6.0;

/// Terminal-style event log in the bottom-left corner. Collapses to a one-line pill.
fn event_log(ctx: &egui::Context, game: &Game) {
    let log = &game.universe.log;
    if log.is_empty() {
        return;
    }
    // While exploring, the bottom edge belongs to the flight / walking panel:
    // the log moves up to where the tool dock normally is.
    let (anchor, offset) = if game.exploring() { (Align2::LEFT_TOP, vec2(EDGE, BELOW_BARS)) } else { (Align2::LEFT_BOTTOM, vec2(EDGE, -EDGE)) };
    let open_id = Id::new("event_log_open");
    let mut open = ctx.data_mut(|d| *d.get_temp_mut_or(open_id, true));
    let mono = FontId::monospace(11.5);
    let (_, latest) = split_log_line(log.last().map(String::as_str).unwrap_or(""));

    if !open {
        // Collapsed: icon, newest message, expand chevron. The whole pill is the button.
        let frame = glass(0).corner_radius(12).inner_margin(Margin::symmetric(10, 6));
        let clicked = floating(ctx, "event_log", anchor, offset, frame, |ui| {
            let resp = ui
                .horizontal(|ui| {
                    ui.set_max_width(LOG_W - 20.0);
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(15.0), Sense::hover());
                    Icon::Terminal.paint(ui.painter(), r, ACCENT);
                    ui.add_space(2.0);
                    ui.add(egui::Label::new(RichText::new(latest).font(mono.clone()).color(Color32::from_rgb(204, 208, 222))).truncate());
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
                    Icon::ChevronUp.paint(ui.painter(), r, MUTED);
                })
                .response;
            ui.interact(resp.rect, Id::new("event_log_expand"), Sense::click()).on_hover_text("Show event log").clicked()
        });
        if clicked {
            open = true;
        }
    } else {
        let frame = glass(0).corner_radius(14).inner_margin(Margin { left: 10, right: 6, top: 6, bottom: 10 });
        floating(ctx, "event_log", anchor, offset, frame, |ui| {
            ui.set_width(LOG_W);
            // Areas reuse last frame's size as max_rect; lift it so the console can reach full height.
            ui.set_max_height(400.0);
            // Header: icon, title, count, collapse.
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(Vec2::splat(15.0), Sense::hover());
                Icon::Terminal.paint(ui.painter(), r, ACCENT);
                ui.label(bold("Event log", 12.0).color(TEXT));
                ui.label(RichText::new(format!("{}", log.len())).font(FontId::monospace(10.5)).color(FAINT));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if icon_button(ui, Icon::ChevronDown, 24.0, Look::Plain).on_hover_text("Collapse").clicked() {
                        open = false;
                    }
                });
            });
            ui.add_space(2.0);
            // Console body.
            let row_h = ui.fonts_mut(|f| f.row_height(&mono)) + 5.0;
            Frame::new().fill(Color32::from_black_alpha(110)).corner_radius(9).inner_margin(Margin::symmetric(9, 6)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                let body_h = row_h * (log.len() as f32).min(LOG_ROWS);
                egui::ScrollArea::vertical()
                    .min_scrolled_height(body_h)
                    .max_height(body_h)
                    .stick_to_bottom(true)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        let n = log.len();
                        let text_x = 46.0 + 8.0 + 14.0;
                        for (i, line) in log.iter().enumerate() {
                            let (t, msg) = split_log_line(line);
                            let newest = i + 1 == n;
                            // Painted rows: exact line height, no widget minimums, clipped at the edge.
                            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), row_h), Sense::hover());
                            if !ui.is_rect_visible(rect) {
                                continue;
                            }
                            let p = ui.painter_at(rect);
                            let y = rect.center().y;
                            p.text(egui::pos2(rect.left() + 46.0, y), Align2::RIGHT_CENTER, t, mono.clone(), FAINT);
                            p.text(egui::pos2(rect.left() + 54.0, y), Align2::LEFT_CENTER, "›", mono.clone(), if newest { ACCENT } else { FAINT });
                            let galley = p.layout_no_wrap(msg.to_string(), mono.clone(), if newest { TEXT } else { MUTED });
                            let truncated = galley.size().x > rect.width() - text_x;
                            p.galley(egui::pos2(rect.left() + text_x, y - galley.size().y / 2.0), galley, TEXT);
                            if truncated {
                                resp.on_hover_text(msg);
                            }
                        }
                    });
            });
        });
    }
    ctx.data_mut(|d| d.insert_temp(open_id, open));
}

fn hint_bar(ctx: &egui::Context, game: &mut Game) {
    let following = game.camera.follow.and_then(|f| game.universe.index_of(f)).map(|i| game.universe.bodies[i].name.clone());
    let mut stop = false;
    if let Some(name) = &following {
        floating(ctx, "hint_bar", Align2::CENTER_BOTTOM, vec2(0.0, -EDGE), glass(4).corner_radius(14), |ui| {
            ui.horizontal(|ui| {
                ui.add_space(6.0);
                let (r, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
                Icon::Focus.paint(ui.painter(), r, ACCENT);
                ui.label(RichText::new("Following").size(12.0).color(MUTED));
                ui.label(bold(name, 12.5).color(TEXT));
                ui.add_space(4.0);
                stop = icon_button(ui, Icon::Close, 26.0, Look::Plain).on_hover_text("Stop following  (Esc)").clicked();
            });
        });
    } else {
        // Top-aligned so the icon lines up with the first line of text, even when it wraps.
        let frame = glass(0).corner_radius(14).inner_margin(Margin::symmetric(14, 9));
        floating(ctx, "hint_bar", Align2::CENTER_BOTTOM, vec2(0.0, -EDGE), frame, |ui| {
            ui.horizontal_top(|ui| {
                let font = FontId::proportional(12.0);
                let line_h = ui.fonts_mut(|f| f.row_height(&font));
                let (r, _) = ui.allocate_exact_size(vec2(16.0, line_h), Sense::hover());
                tool_icon(game.tool).paint(ui.painter(), Rect::from_center_size(r.center(), Vec2::splat(15.0)), ACCENT);
                ui.vertical(|ui| {
                    ui.set_max_width(520.0);
                    ui.add(egui::Label::new(RichText::new(game.tool.hint()).font(font).color(Color32::from_rgb(196, 200, 216))).wrap());
                });
            });
        });
    }
    if stop {
        game.camera.follow = None;
    }
}

fn stats_bar(ctx: &egui::Context, game: &Game) {
    if !game.settings.show_stats {
        return;
    }
    // Not enough room beside the flight / walking panel on narrow windows.
    if game.exploring() && ctx.content_rect().width() < 1150.0 {
        return;
    }
    egui::Area::new(Id::new("stats_bar")).anchor(Align2::RIGHT_BOTTOM, vec2(-EDGE, -EDGE)).interactable(false).show(ctx, |ui| {
        glass(4).corner_radius(14).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add_space(6.0);
                for (icon, text) in [
                    (Icon::Planet, fmt_count(game.universe.bodies.len())),
                    (Icon::Particles, fmt_count(game.universe.particles.len())),
                    (Icon::Activity, format!("{:.0}", game.fps)),
                ] {
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(15.0), Sense::hover());
                    icon.paint(ui.painter(), r, FAINT);
                    ui.add_sized([40.0, 24.0], egui::Label::new(RichText::new(text).monospace().size(12.0).color(MUTED)));
                }
            });
        });
    });
}

// ── Modals ─────────────────────────────────────────────────────────────────

fn modal(id: &str) -> egui::Modal {
    egui::Modal::new(Id::new(id)).frame(glass(20).fill(GLASS_SOLID).corner_radius(20)).backdrop_color(Color32::from_black_alpha(110))
}

fn custom_modal(ctx: &egui::Context, game: &mut Game) {
    if !game.show_custom {
        return;
    }
    let mut create = false;
    let mut close = false;
    let real_time = game.real_time;
    let resp = modal("custom_modal").show(ctx, |ui| {
        ui.set_width(380.0);
        let p = &mut game.custom;
        close = card_header(ui, Icon::Sparkles, "Create your own universe", Some("Design a new system from scratch"), true);
        text_field(ui, egui::TextEdit::singleline(&mut p.name).hint_text("Universe name").desired_width(f32::INFINITY).margin(Margin::symmetric(10, 7)));
        section(ui, "Centre");
        tile_grid(ui, &CenterKind::ALL, 3, 60.0, |ui, c, size| {
            let (icon, label, tint) = center_info(c);
            if tile(ui, icon, label, p.center == c, size, Some(tint)).on_hover_text(c.label()).clicked() {
                p.center = c;
            }
        });
        section(ui, "System");
        slider_row(ui, "Planets", egui::Slider::new(&mut p.planets, 0..=14));
        slider_row(ui, "Spacing", egui::Slider::new(&mut p.spacing, 0.6..=2.0));
        slider_row(ui, "Gravity", egui::Slider::new(&mut p.gravity, 0.2..=3.0));
        slider_row(ui, "Chaos", egui::Slider::new(&mut p.chaos, 0.0..=1.0)).on_hover_text("How unstable the orbits are");
        section(ui, "Extras");
        switch_row(ui, "Moons", &mut p.moons);
        switch_row(ui, "Asteroid belt", &mut p.belt);
        switch_row(ui, "Ringed giants", &mut p.rings);
        ui.horizontal(|ui| {
            ui.allocate_ui_with_layout(vec2(84.0, 28.0), Layout::left_to_right(Align::Center), |ui| {
                ui.set_min_width(84.0);
                ui.label(muted("Seed"));
            });
            ui.add(egui::DragValue::new(&mut p.seed).speed(1.0));
            if icon_button(ui, Icon::Dice, 30.0, Look::Plain).on_hover_text("Random seed").clicked() {
                p.seed = (real_time * 1000.0) as u64 ^ 0x5DEECE66D;
            }
        });
        ui.add_space(10.0);
        if primary_button(ui, Icon::Rocket, "Create universe").clicked() {
            create = true;
        }
    });
    if create {
        game.load_custom();
        game.show_custom = false;
    } else if close || resp.should_close() {
        game.show_custom = false;
    }
}

fn saves_modal(ctx: &egui::Context, game: &mut Game) {
    if !game.saves.show {
        return;
    }
    enum Act {
        Save,
        Load(std::path::PathBuf),
        Delete(std::path::PathBuf),
    }
    let mut act = None;
    let mut close = false;
    let now = save::now();
    let resp = modal("saves_modal").show(ctx, |ui| {
        ui.set_width(400.0);
        close = card_header(ui, Icon::Save, "Saved universes", Some("Keep your worlds and come back to them later"), true);
        let saves = &mut game.saves;

        section(ui, "Save this universe");
        let field = text_field(ui, egui::TextEdit::singleline(&mut saves.name).hint_text("Name this save").desired_width(f32::INFINITY).margin(Margin::symmetric(10, 7)));
        let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let slot = save::slot_for(&saves.name);
        if let Some(old) = saves.list.iter().find(|s| !s.autosave && s.slot == slot) {
            ui.label(RichText::new(format!("This replaces the save called \u{201c}{}\u{201d}.", old.name)).size(11.5).color(MUTED));
        }
        ui.add_space(4.0);
        if primary_button(ui, Icon::Save, "Save").clicked() || enter {
            act = Some(Act::Save);
        }
        if let Some((text, error)) = &saves.message {
            ui.add_space(4.0);
            ui.label(RichText::new(text.as_str()).size(12.0).color(if *error { DANGER } else { Color32::from_rgb(126, 214, 138) }));
        }

        section(ui, "Your saves");
        if saves.list.is_empty() {
            tip(ui, Icon::Folder, "Nothing saved yet. Your universe is also saved automatically when you close the game.");
        }
        egui::ScrollArea::vertical().max_height(300.0).auto_shrink([false, true]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            for s in &saves.list {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    let w = ui.available_width() - 40.0;
                    let title = if s.autosave { format!("{}  (autosave)", s.name) } else { s.name.clone() };
                    let worlds = if s.bodies == 1 { "1 body".to_string() } else { format!("{} bodies", s.bodies) };
                    let desc = format!("{worlds} · saved {}", save::ago(s.saved_at, now));
                    let icon = if s.autosave { Icon::Reset } else { Icon::Galaxy };
                    let row = ui.allocate_ui(vec2(w, 42.0), |ui| menu_row(ui, icon, &title, &desc, None)).inner;
                    if row.on_hover_text("Load this universe").clicked() {
                        act = Some(Act::Load(s.path.clone()));
                    }
                    let confirming = saves.confirm_delete.as_ref() == Some(&s.path);
                    let (look, hint) = if confirming { (Look::Danger, "Click again to delete it for good") } else { (Look::Plain, "Delete this save") };
                    if icon_button(ui, Icon::Trash, 34.0, look).on_hover_text(hint).clicked() {
                        act = Some(Act::Delete(s.path.clone()));
                    }
                });
            }
        });
    });
    match act {
        Some(Act::Save) => {
            let name = game.saves.name.clone();
            game.save_as(&name);
        }
        Some(Act::Load(path)) => game.load_save(&path),
        Some(Act::Delete(path)) => {
            if game.saves.confirm_delete.as_ref() == Some(&path) {
                game.delete_save(&path);
            } else {
                game.saves.confirm_delete = Some(path);
            }
        }
        None => {}
    }
    if close || resp.should_close() {
        game.saves.show = false;
    }
}

fn help_modal(ctx: &egui::Context, game: &mut Game) {
    if !game.show_help {
        return;
    }
    let mut close = false;
    let resp = modal("help_modal").show(ctx, |ui| {
        ui.set_width(460.0);
        close = card_header(ui, Icon::Help, "How to play", Some("Build, break and bend your own universe"), true);
        let rows = |ui: &mut Ui, id: &str, rows: &[(&[&str], &str)]| {
            egui::Grid::new(id).num_columns(2).spacing([14.0, 7.0]).min_col_width(160.0).show(ui, |ui| {
                for (keys, what) in rows {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        for k in *keys {
                            keycap(ui, k);
                        }
                    });
                    ui.label(RichText::new(*what).size(12.5).color(Color32::from_rgb(204, 208, 222)));
                    ui.end_row();
                }
            });
        };
        section(ui, "Camera");
        rows(ui, "help_cam", &[
            (&["Right-drag"], "Orbit (left-drag works in Select)"),
            (&["Scroll"], "Zoom (pinch on a trackpad)"),
            (&["Middle-drag"], "Pan (or Shift + drag)"),
            (&["Double-click", "F"], "Follow a body"),
        ]);
        section(ui, "Tools");
        tile_grid(ui, &Tool::ALL, 6, 58.0, |ui, t, size| {
            tile(ui, tool_icon(t), t.label(), false, size, Some(ACCENT)).on_hover_text(t.hint());
        });
        ui.add_space(2.0);
        rows(ui, "help_tools", &[(&["1", "2", "3", "4", "5", "6"], "Pick a tool")]);
        section(ui, "Flying and walking");
        rows(ui, "help_explore", &[
            (&["W", "A", "S", "D"], "Steer the rocket / walk"),
            (&["Shift", "Ctrl"], "Throttle up / down (Z full, X off)"),
            (&["V"], "Cockpit or outside view"),
            (&["R"], "Brake assist for landing"),
            (&["G"], "Get out and walk / climb back in"),
            (&["Space"], "Jump (on foot)"),
        ]);
        section(ui, "Keys");
        rows(ui, "help_keys", &[
            (&["Space"], "Pause / play"),
            (&["[", "]"], "Slow down / speed up time"),
            (&["Del"], "Delete the selection"),
            (&["B"], "Asteroid barrage at the selection"),
            (&["T", "L"], "Toggle trails / labels"),
            (&["H"], "Hide the interface"),
            (&["F11"], "Fullscreen"),
            (&["Esc"], "Deselect and stop following"),
        ]);
        section(ui, "Ideas");
        for idea in [
            "Throw Jupiter into the Sun",
            "Turn the Sun into a black hole and watch the planets",
            "Make the Sun go supernova",
            "Paint new continents on Earth, then hit it with an asteroid",
            "Load Galaxy Collision and set the speed to 50×",
            "Fly to the Moon, land, and see how high you can jump",
        ] {
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
                Icon::Sparkles.paint(ui.painter(), r, ACCENT);
                ui.label(RichText::new(idea).size(12.5).color(Color32::from_rgb(204, 208, 222)));
            });
        }
        hud::version_footer(ui, game);
    });
    if close || resp.should_close() {
        game.show_help = false;
    }
}

// ── Scene overlays ─────────────────────────────────────────────────────────

/// Names, selection rings and markers drawn behind the panels.
fn draw_labels(ctx: &egui::Context, game: &Game, cam: &CameraFrame, ppp: f32) {
    let painter = ctx.layer_painter(egui::LayerId::background());
    let to_pt = |p: glam::Vec2| egui::pos2(p.x / ppp, p.y / ppp);
    // Biggest bodies get first claim on label space; overlapping labels are skipped.
    let mut order: Vec<&crate::body::Body> = game.universe.bodies.iter().collect();
    order.sort_by(|a, b| {
        let ka = (game.selected == Some(a.id), a.mass);
        let kb = (game.selected == Some(b.id), b.mass);
        kb.partial_cmp(&ka).unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut placed: Vec<Rect> = Vec::new();
    let sandbox = !game.exploring();
    for b in order {
        // Standing on it or skimming it: its name and ring would just float in the ground.
        if !sandbox && (cam.eye - b.pos).length() < b.radius * 1.5 {
            continue;
        }
        let Some(sp) = cam.project(b.pos) else { continue };
        let r_px = cam.screen_radius(b.pos, b.radius);
        let p = to_pt(sp);
        let r = r_px / ppp;
        let selected = game.selected == Some(b.id);
        let hovered = game.hovered == Some(b.id);
        if selected {
            selection_ring(&painter, p, r + 7.0, ctx.input(|i| i.time) as f32);
        } else if hovered {
            painter.circle_stroke(p, r + 5.0, Stroke::new(1.0, with_alpha(INFO, 200)));
        }
        if !game.settings.labels || b.name.is_empty() {
            continue;
        }
        // Tiny distant planets get a marker so you can find them.
        if r < 2.0 && !b.kind.is_luminous() {
            painter.circle_stroke(p, 3.0, Stroke::new(1.0, white(70)));
        }
        let is_moon = b.mass < 1.0 && !selected && !hovered;
        if is_moon && r < 3.0 && sandbox && game.camera.distance > 80.0 {
            continue;
        }
        let at = egui::pos2(p.x, p.y + r + 6.0);
        let font = if selected { FontId::new(12.5, bold_family()) } else { FontId::proportional(12.0) };
        let galley = painter.layout_no_wrap(b.name.clone(), font, if selected { ACCENT } else { white(185) });
        let rect = Rect::from_min_size(egui::pos2(at.x - galley.size().x / 2.0, at.y), galley.size()).expand2(vec2(6.0, 2.0));
        if !selected && placed.iter().any(|o| o.intersects(rect)) {
            continue;
        }
        placed.push(rect);
        if selected {
            painter.rect_filled(rect, 8, Color32::from_rgba_premultiplied(10, 10, 16, 170));
        }
        painter.galley(rect.min + vec2(6.0, 2.0), galley, TEXT);
    }
    // Spawn anchor marker
    if game.tool == Tool::Spawn {
        if let Some(pl) = game.preview.first() {
            if let Some(sp) = pl.points.first().and_then(|p| cam.project(*p)) {
                painter.circle_stroke(to_pt(sp), 6.0, Stroke::new(1.5, Color32::from_rgb(120, 230, 255)));
            }
        }
    }
}

/// Four rotating corner arcs around the selected body.
fn selection_ring(painter: &egui::Painter, c: Pos2, r: f32, time: f32) {
    let stroke = Stroke::new(1.75, ACCENT);
    for k in 0..4 {
        let a0 = time * 0.6 + k as f32 * std::f32::consts::FRAC_PI_2 + 0.25;
        let pts: Vec<Pos2> = (0..=10).map(|i| {
            let a = a0 + i as f32 * 0.1;
            c + vec2(a.cos(), a.sin()) * r
        }).collect();
        painter.add(egui::Shape::line(pts, stroke));
    }
}

/// Developer aid (`--glyph-test` / `--icons`): every icon at a large and a small size.
fn icon_gallery(ctx: &egui::Context) {
    egui::Window::new("Icon set").anchor(Align2::CENTER_CENTER, [0.0, 0.0]).collapsible(false).resizable(false).show(ctx, |ui| {
        for row in Icon::ALL.chunks(11) {
            ui.horizontal(|ui| {
                for icon in row {
                    ui.vertical(|ui| {
                        ui.set_width(64.0);
                        let (r, _) = ui.allocate_exact_size(vec2(64.0, 36.0), Sense::hover());
                        icon.paint(ui.painter(), Rect::from_center_size(r.center() - vec2(10.0, 0.0), Vec2::splat(32.0)), TEXT);
                        icon.paint(ui.painter(), Rect::from_center_size(r.center() + vec2(20.0, 8.0), Vec2::splat(16.0)), ACCENT);
                        ui.label(RichText::new(icon.name()).size(9.5).color(MUTED));
                    });
                }
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;


    /// Selected menu entries and highlighted text are drawn in the selection stroke colour on
    /// the selection fill; both were the accent yellow, which made "1×" in the speed menu vanish.
    #[test]
    fn selected_text_contrasts_with_the_selection_fill() {
        fn luminance(c: Color32) -> f32 {
            let lin = |v: u8| egui::ecolor::linear_f32_from_gamma_u8(v);
            0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
        }
        let ctx = egui::Context::default();
        setup_style(&ctx);
        let sel = ctx.global_style().visuals.selection;
        let (a, b) = (luminance(sel.bg_fill), luminance(sel.stroke.color));
        let ratio = (a.max(b) + 0.05) / (a.min(b) + 0.05);
        assert!(ratio >= 4.5, "contrast {ratio:.2}:1");
    }

    /// Selecting a body opens the inspector; merely showing it must not change the body.
    /// (The Sun is heavier than the Mass slider's old maximum, and egui's default clamping
    /// halved it every frame, flinging every planet out of orbit.)
    #[test]
    fn inspecting_the_sun_leaves_its_mass_alone() {
        let mut game = Game::new();
        game.load_preset(Preset::SolarSystem);
        let sun = game.universe.bodies.iter().find(|b| b.name == "Sun").unwrap();
        let (id, mass, radius) = (sun.id, sun.mass, sun.radius);
        game.selected = Some(id);
        let ctx = egui::Context::default();
        setup_style(&ctx);
        for _ in 0..3 {
            let _ = ctx.run_ui(egui::RawInput::default(), |ui| inspector(ui.ctx(), &mut game));
        }
        let sun = game.universe.get(id).unwrap();
        assert_eq!(sun.mass, mass);
        assert_eq!(sun.radius, radius);
    }
}
