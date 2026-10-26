//! The app's look: colors taken from the icon (slate and a bright blue),
//! the Inter typeface, rounded widgets, and a few shared widgets built
//! from them.

use std::sync::Arc;

use egui::{
    Color32, CornerRadius, FontData, FontFamily, FontId, RichText, Sense, Stroke, StrokeKind,
    Theme, Vec2, Visuals,
};

use crate::icons;

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
    /// Secondary text, such as hints and setting labels.
    weak: Color32,
    accent: Color32,
    error: Color32,
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
    weak: Color32::from_rgb(154, 160, 170),
    accent: Color32::from_rgb(31, 142, 241),
    error: Color32::from_rgb(242, 104, 96),
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
    weak: Color32::from_rgb(89, 97, 112),
    accent: Color32::from_rgb(15, 111, 216),
    error: Color32::from_rgb(196, 43, 28),
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
    v.weak_text_color = Some(p.weak);
    v.error_fg_color = p.error;
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
    // Pressed and keyboard-focused widgets get an accent outline, so Tab
    // shows where it is.
    w.active.bg_stroke = Stroke::new(1.5, p.accent);
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

/// The family of [`semibold`].
const SEMIBOLD: &str = "semibold";

/// Inter for all text, in a regular and a semibold weight. Both are
/// subsets with the Latin letters, and digits of equal width so counts
/// don't jiggle; other characters, such as Cyrillic or Chinese file names,
/// come from egui's fonts and the system's.
fn fonts() -> egui::FontDefinitions {
    let mut fonts = egui::FontDefinitions::default();
    for (name, data) in [
        (
            "Inter",
            &include_bytes!("../assets/fonts/Inter-Regular.ttf")[..],
        ),
        (
            "Inter SemiBold",
            &include_bytes!("../assets/fonts/Inter-SemiBold.ttf")[..],
        ),
    ] {
        fonts
            .font_data
            .insert(name.to_owned(), Arc::new(FontData::from_static(data)));
    }
    let proportional = fonts.families.entry(FontFamily::Proportional).or_default();
    let fallbacks = proportional.clone();
    proportional.insert(0, "Inter".to_owned());
    let mut semibold = vec!["Inter SemiBold".to_owned()];
    semibold.extend(fallbacks);
    fonts
        .families
        .insert(FontFamily::Name(SEMIBOLD.into()), semibold);
    fonts
}

/// Inter SemiBold, for titles, headings and the main button.
pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(SEMIBOLD.into()))
}

/// Sets the fonts, colors and spacing for both the dark and light theme;
/// the system's choice decides which one shows.
pub fn apply(ctx: &egui::Context) {
    ctx.set_fonts(fonts());
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

/// A small "i" in a circle, drawn rather than taken from a font, that
/// explains something: hovering it shows `text` as a tooltip, and clicking
/// it shows `text` in a box that stays until the next click elsewhere.
///
/// The box keeps its open state itself rather than in egui's memory, which
/// holds one open popup at a time: opening it there closed the settings
/// menu the icon sits in. It also counts as that menu's open submenu, so
/// the click that closes the box leaves the menu open.
pub fn info_icon(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let size = ui.spacing().interact_size.y.min(16.0);
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::click());
    let popup_id = response.id.with("info");
    let mut open = ui.data(|d| d.get_temp::<bool>(popup_id).unwrap_or(false));
    if response.clicked() {
        open = !open;
    }
    let shown = egui::Popup::from_response(&response)
        .id(popup_id)
        .open_bool(&mut open)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .width(320.0)
        .show(|ui| ui.label(text))
        .is_some();
    if shown {
        egui::containers::menu::MenuState::from_id(ui.ctx(), ui.layer_id().id, |menu| {
            menu.open_item = Some(popup_id);
        });
    }
    ui.data_mut(|d| d.insert_temp(popup_id, open));
    focus_ring(ui, &response, size / 2.0);
    let color = if response.hovered() || open {
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
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    if open {
        response
    } else {
        response.on_hover_text(text)
    }
}

/// The main action: filled with the accent color, white text.
pub fn primary_button(text: &str) -> egui::Button<'static> {
    egui::Button::new(
        RichText::new(text.to_owned())
            .font(semibold(14.5))
            .color(Color32::WHITE),
    )
    .min_size(Vec2::new(150.0, BIG_HEIGHT))
    .corner_radius(8.0)
}

/// A button as large as [`primary_button`], without the accent fill.
pub fn secondary_button(text: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text.to_owned()).font(semibold(14.5)))
        .min_size(Vec2::new(90.0, BIG_HEIGHT))
        .corner_radius(8.0)
}

/// Height of the main button and those beside it.
pub const BIG_HEIGHT: f32 = 34.0;

/// A wide button-shaped progress bar: the accent fills it from the left as
/// `fraction` grows, and `text` shows white over the filled part.
pub fn progress_bar(ui: &mut egui::Ui, width: f32, fraction: f32, text: &str) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, BIG_HEIGHT), Sense::hover());
    let accent = accent(ui);
    let text_color = ui.visuals().strong_text_color();
    let painter = ui.painter();
    painter.rect_filled(rect, 8.0, accent.gamma_multiply(0.22));
    let split = rect.left() + rect.width() * fraction.clamp(0.0, 1.0);
    let filled = rect.with_max_x(split);
    if filled.width() > 0.5 {
        painter
            .with_clip_rect(filled)
            .rect_filled(rect, 8.0, accent);
    }
    let galley = painter.layout_no_wrap(text.to_owned(), semibold(14.0), Color32::PLACEHOLDER);
    let pos = rect.center() - galley.size() / 2.0;
    painter
        .with_clip_rect(filled)
        .galley_with_override_text_color(pos, galley.clone(), Color32::WHITE);
    painter
        .with_clip_rect(rect.with_min_x(split))
        .galley_with_override_text_color(pos, galley, text_color);
    response
}

/// The outline around a custom widget that has keyboard focus.
pub fn focus_ring(ui: &egui::Ui, response: &egui::Response, radius: f32) {
    if response.has_focus() {
        ui.painter().rect_stroke(
            response.rect.expand(2.0),
            radius + 2.0,
            Stroke::new(1.5, accent(ui)),
            StrokeKind::Outside,
        );
    }
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
        focus_ring(ui, &response, f32::from(RADIUS - 2));
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
    let font = egui::FontId::proportional(12.0);
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font, Color32::PLACEHOLDER);
    let chevron = 8.0;
    let size = Vec2::new(galley.size().x + chevron + 21.0, ROW_HEIGHT + 2.0);
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
    let text_pos = egui::pos2(rect.left() + 9.0, rect.center().y - galley.size().y / 2.0);
    painter.galley_with_override_text_color(text_pos, galley, text_color);
    let center = egui::pos2(rect.right() - 7.0 - chevron / 2.0, rect.center().y);
    icons::chevron(painter, center, chevron, open, Stroke::new(1.5, weak));
    focus_ring(ui, &response, size.y / 2.0);
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A square button with a drawn icon (see [`icons`]), as the bundled
/// fonts have no icon glyphs.
pub fn icon_button(ui: &mut egui::Ui, paint: icons::Icon) -> egui::Response {
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
    focus_ring(ui, &response, f32::from(RADIUS));
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A small icon button without a background until hovered, for the
/// actions on a file's row.
pub fn small_icon_button(ui: &mut egui::Ui, paint: icons::Icon) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(24.0), Sense::click());
    let visuals = ui.visuals();
    let color = if response.hovered() {
        ui.painter()
            .rect_filled(rect, 6.0, visuals.widgets.hovered.bg_fill);
        visuals.strong_text_color()
    } else {
        visuals.text_color()
    };
    paint(ui.painter(), rect.shrink(6.0), color);
    focus_ring(ui, &response, 6.0);
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A button with a drawn icon before its text.
pub fn icon_text_button(ui: &mut egui::Ui, paint: icons::Icon, text: &str) -> egui::Response {
    let font = FontId::proportional(13.5);
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font, Color32::PLACEHOLDER);
    let icon = 12.0;
    let size = Vec2::new(galley.size().x + icon + 6.0 + 20.0, ROW_HEIGHT);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let visuals = ui.visuals();
    let enabled = ui.is_enabled();
    let fill = if response.hovered() && enabled {
        visuals.widgets.hovered.bg_fill
    } else {
        visuals.widgets.inactive.bg_fill
    };
    let mut color = visuals.text_color();
    if !enabled {
        color = color.gamma_multiply(0.45);
    }
    let painter = ui.painter();
    painter.rect_filled(rect, RADIUS, fill);
    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(rect.left() + 10.0 + icon / 2.0, rect.center().y),
        Vec2::splat(icon),
    );
    paint(painter, icon_rect, color);
    let text_pos = egui::pos2(
        icon_rect.right() + 6.0,
        rect.center().y - galley.size().y / 2.0,
    );
    painter.galley_with_override_text_color(text_pos, galley, color);
    focus_ring(ui, &response, f32::from(RADIUS));
    response
}

/// The heading above a card of settings.
pub fn section(ui: &mut egui::Ui, text: &str) {
    ui.add_space(6.0);
    ui.label(
        RichText::new(text)
            .font(semibold(12.5))
            .color(ui.visuals().weak_text_color()),
    );
}
