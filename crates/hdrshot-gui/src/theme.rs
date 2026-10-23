//! The app's look: colors taken from the icon (slate and a bright blue),
//! rounded widgets, and a few shared widgets built from them.

use egui::{Color32, CornerRadius, RichText, Sense, Stroke, StrokeKind, Theme, Vec2, Visuals};

/// Height of buttons, segmented choices and setting rows.
pub const ROW_HEIGHT: f32 = 26.0;

const RADIUS: u8 = 7;

struct Palette {
    panel: Color32,
    card: Color32,
    extreme: Color32,
    inactive: Color32,
    hovered: Color32,
    active: Color32,
    border: Color32,
    text: Color32,
    accent: Color32,
}

const DARK: Palette = Palette {
    panel: Color32::from_rgb(21, 24, 30),
    card: Color32::from_rgb(29, 33, 41),
    extreme: Color32::from_rgb(15, 17, 21),
    inactive: Color32::from_rgb(38, 43, 53),
    hovered: Color32::from_rgb(47, 53, 66),
    active: Color32::from_rgb(56, 64, 80),
    border: Color32::from_rgb(44, 50, 61),
    text: Color32::from_rgb(230, 233, 239),
    accent: Color32::from_rgb(31, 142, 241),
};

const LIGHT: Palette = Palette {
    panel: Color32::from_rgb(243, 245, 248),
    card: Color32::from_rgb(255, 255, 255),
    extreme: Color32::from_rgb(255, 255, 255),
    inactive: Color32::from_rgb(232, 236, 241),
    hovered: Color32::from_rgb(221, 227, 234),
    active: Color32::from_rgb(208, 216, 226),
    border: Color32::from_rgb(213, 219, 227),
    text: Color32::from_rgb(27, 31, 39),
    accent: Color32::from_rgb(15, 111, 216),
};

fn visuals(base: Visuals, p: &Palette) -> Visuals {
    let mut v = base;
    let radius = CornerRadius::same(RADIUS);
    v.panel_fill = p.panel;
    v.window_fill = p.card;
    v.faint_bg_color = p.card;
    v.extreme_bg_color = p.extreme;
    v.code_bg_color = p.inactive;
    v.window_stroke = Stroke::new(1.0, p.border);
    v.window_corner_radius = CornerRadius::same(12);
    v.menu_corner_radius = CornerRadius::same(10);
    v.hyperlink_color = p.accent;
    v.selection.bg_fill = p.accent;
    v.selection.stroke = Stroke::new(1.0, Color32::WHITE);
    v.slider_trailing_fill = true;
    v.collapsing_header_frame = false;
    v.indent_has_left_vline = false;

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = p.card;
    w.noninteractive.weak_bg_fill = p.card;
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    for (state, fill) in [
        (&mut w.inactive, p.inactive),
        (&mut w.hovered, p.hovered),
        (&mut w.active, p.active),
        (&mut w.open, p.hovered),
    ] {
        state.bg_fill = fill;
        state.weak_bg_fill = fill;
        state.bg_stroke = Stroke::NONE;
        state.fg_stroke.color = p.text;
        state.expansion = 0.0;
    }
    w.hovered.bg_stroke = Stroke::new(1.0, p.border);
    for state in [
        &mut w.noninteractive,
        &mut w.inactive,
        &mut w.hovered,
        &mut w.active,
        &mut w.open,
    ] {
        state.corner_radius = radius;
    }
    v
}

/// Sets the colors and spacing for both the dark and light theme; the
/// system's choice decides which one shows.
pub fn apply(ctx: &egui::Context) {
    ctx.set_visuals_of(Theme::Dark, visuals(Visuals::dark(), &DARK));
    ctx.set_visuals_of(Theme::Light, visuals(Visuals::light(), &LIGHT));
    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = Vec2::new(8.0, 8.0);
        style.spacing.button_padding = Vec2::new(12.0, 4.0);
        style.spacing.interact_size.y = ROW_HEIGHT - 4.0;
        style.spacing.menu_margin = egui::Margin::same(8);
        style.interaction.selectable_labels = false;
    });
}

/// The accent color of the current theme.
pub fn accent(ui: &egui::Ui) -> Color32 {
    ui.visuals().selection.bg_fill
}

/// A rounded card holding a group of controls.
pub fn card<R>(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let visuals = ui.visuals();
    egui::Frame::new()
        .fill(visuals.faint_bg_color)
        .stroke(visuals.widgets.noninteractive.bg_stroke)
        .corner_radius(10.0)
        .inner_margin(egui::Margin::symmetric(14, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add_contents(ui)
        })
        .inner
}

/// The label of a setting row, vertically centered on the row's controls.
pub fn row_label(ui: &mut egui::Ui, text: &str) {
    ui.allocate_ui_with_layout(
        Vec2::new(0.0, ROW_HEIGHT),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| ui.label(RichText::new(text).color(ui.visuals().weak_text_color())),
    );
}

/// A small "i" in a circle, drawn rather than taken from a font, to hover
/// for an explanation.
pub fn info_icon(ui: &mut egui::Ui) -> egui::Response {
    let size = ui.spacing().interact_size.y.min(16.0);
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
    let color = if response.hovered() {
        ui.visuals().strong_text_color()
    } else {
        ui.visuals().weak_text_color()
    };
    let painter = ui.painter();
    painter.circle_stroke(
        rect.center(),
        size / 2.0 - 1.0,
        egui::Stroke::new(1.2, color),
    );
    let x = rect.center().x;
    painter.circle_filled(
        egui::pos2(x, rect.center().y - size * 0.2),
        size * 0.07,
        color,
    );
    painter.line_segment(
        [
            egui::pos2(x, rect.center().y - size * 0.05),
            egui::pos2(x, rect.center().y + size * 0.25),
        ],
        egui::Stroke::new(1.5, color),
    );
    response
}

/// The main action: filled with the accent color, white text.
pub fn primary_button(text: &str) -> egui::Button<'static> {
    egui::Button::new(
        RichText::new(text.to_owned())
            .strong()
            .size(15.0)
            .color(Color32::WHITE),
    )
    .min_size(Vec2::new(150.0, 34.0))
    .corner_radius(8.0)
}

/// Applies [`primary_button`]'s accent fill, which needs the theme.
pub fn primary(ui: &egui::Ui, button: egui::Button<'static>) -> egui::Button<'static> {
    button.fill(accent(ui))
}

/// One of several choices, shown as a row of joined segments with the
/// chosen one filled. Returns whether the choice changed.
pub fn segmented<T: PartialEq + Copy>(
    ui: &mut egui::Ui,
    value: &mut T,
    options: &[(T, &str, &str)],
) -> bool {
    let font = egui::FontId::proportional(13.5);
    let pad = 12.0;
    let galleys: Vec<_> = options
        .iter()
        .map(|(_, label, _)| {
            ui.painter()
                .layout_no_wrap((*label).to_owned(), font.clone(), Color32::PLACEHOLDER)
        })
        .collect();
    let width: f32 = galleys.iter().map(|g| g.size().x + 2.0 * pad).sum::<f32>() + 4.0;
    let base_id = ui.next_auto_id();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, ROW_HEIGHT), Sense::hover());
    let enabled = ui.is_enabled();
    let visuals = ui.visuals().clone();
    ui.painter().rect(
        rect,
        RADIUS,
        visuals.widgets.inactive.bg_fill,
        Stroke::NONE,
        StrokeKind::Inside,
    );
    let mut changed = false;
    let mut x = rect.left() + 2.0;
    for (index, ((option, _, hint), galley)) in options.iter().zip(galleys).enumerate() {
        let segment = egui::Rect::from_min_size(
            egui::pos2(x, rect.top() + 2.0),
            Vec2::new(galley.size().x + 2.0 * pad, ROW_HEIGHT - 4.0),
        );
        x = segment.right();
        let id = base_id.with(index);
        let response = ui
            .interact(segment, id, Sense::click())
            .on_hover_text(*hint);
        let selected = *value == *option;
        if response.clicked() && !selected {
            *value = *option;
            changed = true;
        }
        let fill = if selected {
            visuals.selection.bg_fill
        } else if response.hovered() && enabled {
            visuals.widgets.hovered.bg_fill
        } else {
            Color32::TRANSPARENT
        };
        let mut text = if selected {
            Color32::WHITE
        } else {
            visuals.text_color()
        };
        if !enabled {
            text = text.gamma_multiply(0.45);
        }
        let painter = ui.painter();
        painter.rect_filled(segment, RADIUS - 2, fill);
        painter.galley_with_override_text_color(
            segment.center() - galley.size() / 2.0,
            galley,
            text,
        );
    }
    changed
}

/// A rounded button summing up one group of settings, with a chevron
/// pointing up while its menu (`open`) shows. Accented ones stand out, for
/// settings that differ from the usual.
pub fn chip(ui: &mut egui::Ui, text: &str, open: bool, accented: bool) -> egui::Response {
    let font = egui::FontId::proportional(12.5);
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font, Color32::PLACEHOLDER);
    let chevron = 8.0;
    let size = Vec2::new(galley.size().x + chevron + 24.0, ROW_HEIGHT + 2.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let visuals = ui.visuals();
    let enabled = ui.is_enabled();
    let accent = visuals.selection.bg_fill;
    let (fill, stroke) = if open || accented {
        (accent.gamma_multiply(0.16), Stroke::new(1.0, accent))
    } else if response.hovered() && enabled {
        (
            visuals.widgets.hovered.bg_fill,
            visuals.widgets.noninteractive.bg_stroke,
        )
    } else {
        (
            visuals.faint_bg_color,
            visuals.widgets.noninteractive.bg_stroke,
        )
    };
    let mut text_color = visuals.text_color();
    let mut weak = visuals.weak_text_color();
    if !enabled {
        text_color = text_color.gamma_multiply(0.45);
        weak = weak.gamma_multiply(0.45);
    }
    let painter = ui.painter();
    painter.rect(rect, size.y / 2.0, fill, stroke, StrokeKind::Inside);
    let text_pos = egui::pos2(rect.left() + 10.0, rect.center().y - galley.size().y / 2.0);
    painter.galley_with_override_text_color(text_pos, galley, text_color);
    let center = egui::pos2(rect.right() - 8.0 - chevron / 2.0, rect.center().y);
    paint_chevron(painter, center, chevron, open, Stroke::new(1.5, weak));
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A small "v" `size` points wide, centered on `center`, or "^" when `up`.
pub fn paint_chevron(
    painter: &egui::Painter,
    center: egui::Pos2,
    size: f32,
    up: bool,
    stroke: Stroke,
) {
    let h = size / 4.0;
    let (edge, tip) = if up { (h, -h) } else { (-h, h) };
    painter.line(
        vec![
            center + Vec2::new(-size / 2.0, edge),
            center + Vec2::new(0.0, tip),
            center + Vec2::new(size / 2.0, edge),
        ],
        stroke,
    );
}

/// A square button with a drawn icon, as the bundled fonts have no icon
/// glyphs. `paint` draws it in the given rectangle and color.
pub fn icon_button(
    ui: &mut egui::Ui,
    paint: impl FnOnce(&egui::Painter, egui::Rect, Color32),
) -> egui::Response {
    let size = Vec2::splat(ROW_HEIGHT + 4.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let visuals = ui.visuals();
    let fill = if response.hovered() {
        visuals.widgets.hovered.bg_fill
    } else {
        visuals.widgets.inactive.bg_fill
    };
    let color = if response.hovered() {
        visuals.strong_text_color()
    } else {
        visuals.text_color()
    };
    ui.painter().rect_filled(rect, RADIUS, fill);
    paint(ui.painter(), rect.shrink(8.0), color);
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Two sliders, the icon of the Settings button.
pub fn paint_settings(painter: &egui::Painter, rect: egui::Rect, color: Color32) {
    let stroke = Stroke::new(1.6, color);
    let r = rect.width() * 0.15;
    for (y, knob) in [(0.28, 0.68), (0.72, 0.36)] {
        let y = rect.top() + rect.height() * y;
        let x = rect.left() + rect.width() * knob;
        painter.line_segment([egui::pos2(rect.left(), y), egui::pos2(x - r, y)], stroke);
        painter.line_segment([egui::pos2(x + r, y), egui::pos2(rect.right(), y)], stroke);
        painter.circle_stroke(egui::pos2(x, y), r, stroke);
    }
}

/// A "<", the icon of the Back button.
pub fn paint_back(painter: &egui::Painter, rect: egui::Rect, color: Color32) {
    let c = rect.center();
    let h = rect.height() * 0.32;
    painter.line(
        vec![
            c + Vec2::new(h * 0.45, -h),
            c + Vec2::new(-h * 0.55, 0.0),
            c + Vec2::new(h * 0.45, h),
        ],
        Stroke::new(1.8, color),
    );
}

/// The heading above a card of settings.
pub fn section(ui: &mut egui::Ui, text: &str) {
    ui.add_space(6.0);
    ui.label(
        RichText::new(text)
            .size(12.5)
            .strong()
            .color(ui.visuals().weak_text_color()),
    );
}

/// A small rounded label with a tinted background, for a file's status.
pub fn pill(ui: &mut egui::Ui, text: &str, color: Color32) -> egui::Response {
    let font = egui::FontId::proportional(12.0);
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font, color);
    let size = galley.size() + Vec2::new(16.0, 6.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, size.y / 2.0, color.gamma_multiply(0.16));
    painter.galley(rect.center() - galley.size() / 2.0, galley, color);
    response
}
