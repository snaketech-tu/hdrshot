//! Small line icons drawn with shapes, since the bundled fonts have no icon
//! glyphs. Each draws itself into a square `rect` in `color`.

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, Vec2, pos2};

/// An icon drawn into a square in a color.
pub type Icon = fn(&Painter, Rect, Color32);

fn stroke(rect: Rect, color: Color32) -> Stroke {
    Stroke::new((rect.width() / 9.0).clamp(1.3, 2.4), color)
}

/// A point in `rect`, with `x` and `y` from 0 to 1.
fn at(rect: Rect, x: f32, y: f32) -> Pos2 {
    rect.min + Vec2::new(x * rect.width(), y * rect.height())
}

/// Two sliders: the Settings button.
pub fn settings(painter: &Painter, rect: Rect, color: Color32) {
    let stroke = stroke(rect, color);
    let r = rect.width() * 0.15;
    for (y, knob) in [(0.28, 0.68), (0.72, 0.36)] {
        let y = rect.top() + rect.height() * y;
        let x = rect.left() + rect.width() * knob;
        painter.line_segment([pos2(rect.left(), y), pos2(x - r, y)], stroke);
        painter.line_segment([pos2(x + r, y), pos2(rect.right(), y)], stroke);
        painter.circle_stroke(pos2(x, y), r, stroke);
    }
}

/// A "<": back.
pub fn back(painter: &Painter, rect: Rect, color: Color32) {
    painter.line(
        vec![
            at(rect, 0.62, 0.18),
            at(rect, 0.3, 0.5),
            at(rect, 0.62, 0.82),
        ],
        stroke(rect, color),
    );
}

/// A "+": add.
pub fn plus(painter: &Painter, rect: Rect, color: Color32) {
    let stroke = stroke(rect, color);
    painter.line_segment([at(rect, 0.5, 0.12), at(rect, 0.5, 0.88)], stroke);
    painter.line_segment([at(rect, 0.12, 0.5), at(rect, 0.88, 0.5)], stroke);
}

/// An "x": remove.
pub fn cross(painter: &Painter, rect: Rect, color: Color32) {
    let stroke = stroke(rect, color);
    painter.line_segment([at(rect, 0.22, 0.22), at(rect, 0.78, 0.78)], stroke);
    painter.line_segment([at(rect, 0.78, 0.22), at(rect, 0.22, 0.78)], stroke);
}

/// A folder: show in folder.
pub fn folder(painter: &Painter, rect: Rect, color: Color32) {
    let points = vec![
        at(rect, 0.06, 0.2),
        at(rect, 0.4, 0.2),
        at(rect, 0.5, 0.32),
        at(rect, 0.94, 0.32),
        at(rect, 0.94, 0.84),
        at(rect, 0.06, 0.84),
    ];
    painter.add(Shape::closed_line(points, stroke(rect, color)));
}

/// A box with an arrow leaving it: open.
pub fn open(painter: &Painter, rect: Rect, color: Color32) {
    let stroke = stroke(rect, color);
    painter.line(
        vec![
            at(rect, 0.44, 0.14),
            at(rect, 0.14, 0.14),
            at(rect, 0.14, 0.86),
            at(rect, 0.86, 0.86),
            at(rect, 0.86, 0.56),
        ],
        stroke,
    );
    painter.line_segment([at(rect, 0.46, 0.54), at(rect, 0.88, 0.12)], stroke);
    painter.line(
        vec![
            at(rect, 0.6, 0.12),
            at(rect, 0.88, 0.12),
            at(rect, 0.88, 0.4),
        ],
        stroke,
    );
}

/// An eye: watching.
pub fn eye(painter: &Painter, rect: Rect, color: Color32) {
    let stroke = stroke(rect, color);
    let c = rect.center();
    let (w, h) = (rect.width() / 2.0, rect.height() * 0.3);
    let mut points = Vec::new();
    for i in 0..=12 {
        let t = i as f32 / 12.0 * std::f32::consts::PI;
        points.push(c + Vec2::new(-w * t.cos(), -h * t.sin()));
    }
    for i in 1..12 {
        let t = i as f32 / 12.0 * std::f32::consts::PI;
        points.push(c + Vec2::new(w * t.cos(), h * t.sin()));
    }
    painter.add(Shape::closed_line(points, stroke));
    painter.circle_stroke(c, rect.width() * 0.13, stroke);
}

/// A picture with a mountain and a sun: images.
pub fn image(painter: &Painter, rect: Rect, color: Color32) {
    let stroke = stroke(rect, color);
    let frame = Rect::from_min_max(at(rect, 0.06, 0.16), at(rect, 0.94, 0.84));
    painter.rect_stroke(frame, rect.width() * 0.1, stroke, egui::StrokeKind::Middle);
    painter.line(
        vec![
            at(rect, 0.06, 0.7),
            at(rect, 0.32, 0.48),
            at(rect, 0.5, 0.62),
            at(rect, 0.64, 0.52),
            at(rect, 0.94, 0.74),
        ],
        stroke,
    );
    painter.circle_stroke(at(rect, 0.7, 0.34), rect.width() * 0.07, stroke);
}

/// An arrow pointing down onto a line: drop.
pub fn drop(painter: &Painter, rect: Rect, color: Color32) {
    let stroke = stroke(rect, color);
    painter.line_segment([at(rect, 0.5, 0.08), at(rect, 0.5, 0.68)], stroke);
    painter.line(
        vec![
            at(rect, 0.24, 0.44),
            at(rect, 0.5, 0.7),
            at(rect, 0.76, 0.44),
        ],
        stroke,
    );
    painter.line_segment([at(rect, 0.18, 0.9), at(rect, 0.82, 0.9)], stroke);
}

/// A "v", or "^" when `up`, `size` points wide.
pub fn chevron(painter: &Painter, center: Pos2, size: f32, up: bool, stroke: Stroke) {
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

/// A filled circle with a white check: done.
pub fn done(painter: &Painter, center: Pos2, radius: f32, fill: Color32) {
    painter.circle_filled(center, radius, fill);
    let r = radius;
    painter.line(
        vec![
            center + Vec2::new(-0.45 * r, 0.02 * r),
            center + Vec2::new(-0.13 * r, 0.33 * r),
            center + Vec2::new(0.45 * r, -0.3 * r),
        ],
        Stroke::new(r * 0.22, Color32::WHITE),
    );
}

/// A filled circle with a white "!": failed.
pub fn failed(painter: &Painter, center: Pos2, radius: f32, fill: Color32) {
    painter.circle_filled(center, radius, fill);
    let r = radius;
    painter.line_segment(
        [
            center + Vec2::new(0.0, -0.5 * r),
            center + Vec2::new(0.0, 0.12 * r),
        ],
        Stroke::new(r * 0.22, Color32::WHITE),
    );
    painter.circle_filled(center + Vec2::new(0.0, 0.45 * r), r * 0.13, Color32::WHITE);
}

/// A dotted circle: waiting.
pub fn waiting(painter: &Painter, center: Pos2, radius: f32, color: Color32) {
    for i in 0..12 {
        let t = i as f32 / 12.0 * std::f32::consts::TAU;
        painter.circle_filled(
            center + radius * Vec2::new(t.cos(), t.sin()),
            radius * 0.1,
            color,
        );
    }
}

/// A circle with a dash: skipped, as the files were already there.
pub fn skipped(painter: &Painter, center: Pos2, radius: f32, color: Color32) {
    let stroke = Stroke::new(radius * 0.16, color);
    painter.circle_stroke(center, radius * 0.92, stroke);
    painter.line_segment(
        [
            center - Vec2::new(0.42 * radius, 0.0),
            center + Vec2::new(0.42 * radius, 0.0),
        ],
        Stroke::new(radius * 0.2, color),
    );
}
