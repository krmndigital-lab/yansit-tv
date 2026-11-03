//! Painting primitives and small components shared by all screens (painter only, no widgets).
#![allow(dead_code)]

use std::sync::Arc;

use egui::epaint::tessellator::{path, Path};
use egui::epaint::text::{LayoutJob, TextFormat, TextWrapping};
use egui::epaint::{
    CornerRadiusF32, Mesh, PathShape, PathStroke, RectShape, RoundedRect, StrokeKind, Vertex,
};
use egui::{
    pos2, vec2, Align, Align2, Color32, FontId, Galley, Painter, Pos2, Rect, Shape, Stroke,
    TextureId, Vec2,
};

use super::icons as ic;
use super::theme::{alpha, col, mix, ty, white};

/// One physical pixel at ppp 2.
pub const PX: f32 = 0.5;

// ------------------------------------------------------------------------------------------ text

pub fn layout(
    p: &Painter,
    s: &str,
    font: FontId,
    color: Color32,
    max_w: f32,
    max_rows: usize,
    spacing: f32,
    lh: Option<f32>,
) -> Arc<Galley> {
    let mut job = LayoutJob::default();
    job.append(
        s,
        0.0,
        TextFormat {
            font_id: font,
            color,
            extra_letter_spacing: spacing,
            line_height: lh,
            ..Default::default()
        },
    );
    job.wrap = TextWrapping {
        max_width: max_w,
        max_rows,
        break_anywhere: false,
        overflow_character: Some('…'),
    };
    job.halign = Align::LEFT;
    p.layout_job(job)
}

/// Draws a galley aligned to `anchor`; returns its box.
pub fn place(p: &Painter, g: Arc<Galley>, anchor: Pos2, align: Align2) -> Rect {
    let rect = align.anchor_size(anchor, g.size());
    let off = g.rect.min.x;
    p.galley(pos2(rect.min.x - off, rect.min.y), g, col::TEXT());
    rect
}

pub fn text(
    p: &Painter,
    anchor: Pos2,
    align: Align2,
    s: &str,
    font: FontId,
    color: Color32,
) -> Rect {
    place(
        p,
        layout(p, s, font, color, f32::INFINITY, 1, 0.0, None),
        anchor,
        align,
    )
}

/// Single line truncated with an ellipsis at `max_w`.
pub fn text_fit(
    p: &Painter,
    anchor: Pos2,
    align: Align2,
    s: &str,
    font: FontId,
    color: Color32,
    max_w: f32,
) -> Rect {
    place(
        p,
        layout(p, s, font, color, max_w.max(1.0), 1, 0.0, None),
        anchor,
        align,
    )
}

pub fn tracked(
    p: &Painter,
    anchor: Pos2,
    align: Align2,
    s: &str,
    font: FontId,
    color: Color32,
    spacing: f32,
) -> Rect {
    place(
        p,
        layout(p, s, font, color, f32::INFINITY, 1, spacing, None),
        anchor,
        align,
    )
}

/// Wrapped paragraph (top-left anchored).
pub fn paragraph(
    p: &Painter,
    pos: Pos2,
    s: &str,
    font: FontId,
    color: Color32,
    max_w: f32,
    max_rows: usize,
) -> Rect {
    let lh = (font.size * 1.45).round();
    place(
        p,
        layout(p, s, font, color, max_w, max_rows, 0.0, Some(lh)),
        pos,
        Align2::LEFT_TOP,
    )
}

pub fn measure(p: &Painter, s: &str, font: FontId) -> Vec2 {
    layout(p, s, font, Color32::WHITE, f32::INFINITY, 1, 0.0, None).size()
}

/// Upper-case tracked label.
/// Width `kicker` will occupy (it draws with letter tracking, so `measure` is too small).
pub fn kicker_width(p: &Painter, s: &str) -> f32 {
    layout(p, s, ty::kicker(), Color32::WHITE, f32::INFINITY, 1, 1.1, None).size().x
}

pub fn kicker(p: &Painter, anchor: Pos2, align: Align2, s: &str, color: Color32) -> Rect {
    tracked(p, anchor, align, s, ty::kicker(), color, 1.1)
}

pub fn icon(p: &Painter, c: Pos2, glyph: &str, size: f32, color: Color32) -> Rect {
    text(p, c, Align2::CENTER_CENTER, glyph, ty::icon(size), color)
}

pub fn icon_fill(p: &Painter, c: Pos2, glyph: &str, size: f32, color: Color32) -> Rect {
    text(
        p,
        c,
        Align2::CENTER_CENTER,
        glyph,
        ty::icon_fill(size),
        color,
    )
}

/// Turkish-aware upper-casing (i -> İ, ı -> I).
pub fn tr_upper(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 4);
    for c in s.chars() {
        match c {
            'i' => o.push('İ'),
            'ı' => o.push('I'),
            c => o.extend(c.to_uppercase()),
        }
    }
    o
}

// ---------------------------------------------------------------------------------------- shapes

pub fn rrect(p: &Painter, r: Rect, rad: f32, fill: Color32) {
    p.add(RectShape::filled(r, rad, fill));
}

pub fn rrect_stroke(p: &Painter, r: Rect, rad: f32, w: f32, c: Color32) {
    p.add(RectShape::stroke(
        r,
        rad,
        Stroke::new(w, c),
        StrokeKind::Inside,
    ));
}

/// Feathered rounded rect (shadow / glow).
pub fn soft(p: &Painter, r: Rect, rad: f32, blur: f32, c: Color32) {
    p.add(RectShape::filled(r, rad, c).with_blur_width(blur));
}

pub fn shadow(p: &Painter, r: Rect, rad: f32, strength: f32) {
    let strength = strength * col::shadow();
    if strength > 0.01 {
        soft(
            p,
            r.translate(vec2(0.0, 8.0)).expand(4.0),
            rad + 6.0,
            30.0,
            black(0.55 * strength),
        );
    }
}

fn black(a: f32) -> Color32 {
    super::theme::black(a)
}

pub fn rounded_points(r: Rect, rad: f32) -> Vec<Pos2> {
    let mut pts = Vec::new();
    path::rounded_rectangle(&mut pts, RoundedRect::new(r, CornerRadiusF32::same(rad)));
    pts
}

/// Accent colour along x of `r` (blue -> violet).
pub fn accent_at(r: Rect, pos: Pos2) -> Color32 {
    let t = ((pos.x - r.left()) / r.width().max(1.0)).clamp(0.0, 1.0);
    mix(col::A(), col::B(), t)
}

/// Rounded rect filled with the accent gradient at opacity `a`.
pub fn accent_fill(p: &Painter, r: Rect, rad: f32, a: f32) {
    if a <= 0.01 || r.width() <= 0.5 {
        return;
    }
    let pts = rounded_points(r, rad.min(r.height() * 0.5).min(r.width() * 0.5));
    let mut path = Path::default();
    path.add_line_loop(&pts);
    let mut mesh = Mesh::default();
    path.fill(PX, Color32::WHITE, &mut mesh);
    for v in mesh.vertices.iter_mut() {
        let k = v.color.a() as f32 / 255.0 * a;
        v.color = accent_at(r, v.pos).gamma_multiply(k);
    }
    p.add(Shape::mesh(mesh));
}

/// Gradient (accent) outline + soft glow: the focus treatment. `t` = focus amount 0..1.
pub fn focus_ring(p: &Painter, r: Rect, rad: f32, t: f32) {
    if t <= 0.01 {
        return;
    }
    soft(p, r.expand(3.0), rad + 3.0, 18.0, alpha(col::A(), 0.30 * t));
    let ring = r.expand(1.5);
    let pts = rounded_points(ring, rad + 1.5);
    let rr = ring;
    let a = t;
    let stroke = PathStroke::new_uv(2.5, move |_b, pos| accent_at(rr, pos).gamma_multiply(a));
    p.add(Shape::Path(PathShape {
        points: pts,
        closed: true,
        fill: Color32::TRANSPARENT,
        stroke,
    }));
}

/// Vertical gradient with an eased ramp.
pub fn v_gradient(p: &Painter, r: Rect, top: Color32, bottom: Color32, bands: usize) {
    let mut m = Mesh::default();
    let n = bands.max(1);
    for i in 0..=n {
        let t = i as f32 / n as f32;
        let e = t * t * (3.0 - 2.0 * t);
        let y = r.top() + r.height() * t;
        let c = mix(top, bottom, e);
        m.colored_vertex(pos2(r.left(), y), c);
        m.colored_vertex(pos2(r.right(), y), c);
    }
    for i in 0..n as u32 {
        let a = i * 2;
        m.add_triangle(a, a + 1, a + 2);
        m.add_triangle(a + 2, a + 1, a + 3);
    }
    p.add(Shape::mesh(m));
}

pub fn h_gradient(p: &Painter, r: Rect, left: Color32, right: Color32, bands: usize) {
    let mut m = Mesh::default();
    let n = bands.max(1);
    for i in 0..=n {
        let t = i as f32 / n as f32;
        let e = t * t * (3.0 - 2.0 * t);
        let x = r.left() + r.width() * t;
        let c = mix(left, right, e);
        m.colored_vertex(pos2(x, r.top()), c);
        m.colored_vertex(pos2(x, r.bottom()), c);
    }
    for i in 0..n as u32 {
        let a = i * 2;
        m.add_triangle(a, a + 1, a + 2);
        m.add_triangle(a + 2, a + 1, a + 3);
    }
    p.add(Shape::mesh(m));
}

pub fn gradient_quad(p: &Painter, r: Rect, tl: Color32, tr: Color32, bl: Color32, br: Color32) {
    let mut m = Mesh::default();
    m.colored_vertex(r.left_top(), tl);
    m.colored_vertex(r.right_top(), tr);
    m.colored_vertex(r.left_bottom(), bl);
    m.colored_vertex(r.right_bottom(), br);
    m.add_triangle(0, 1, 2);
    m.add_triangle(2, 1, 3);
    p.add(Shape::mesh(m));
}

// ---------------------------------------------------------------------------------------- images

/// Textured rounded rect.
pub fn image(p: &Painter, r: Rect, tex: TextureId, uv: Rect, rad: f32, tint: Color32) {
    p.add(RectShape::filled(r, rad, tint).with_texture(tex, uv));
}

/// UV rect that covers `dst` with an image of size `src` (centre crop; `bias_y` vertical anchor).
pub fn cover_uv(src: [usize; 2], dst: Vec2, bias_y: f32) -> Rect {
    let (sw, sh) = (src[0].max(1) as f32, src[1].max(1) as f32);
    let sa = sw / sh;
    let da = dst.x / dst.y.max(1.0);
    if sa > da {
        let w = da / sa;
        let x0 = (1.0 - w) * 0.5;
        Rect::from_min_max(pos2(x0, 0.0), pos2(x0 + w, 1.0))
    } else {
        let h = sa / da;
        let y0 = (1.0 - h) * bias_y;
        Rect::from_min_max(pos2(0.0, y0), pos2(1.0, y0 + h))
    }
}

/// Largest rect of aspect `src` inside `dst` (centred) - logos.
pub fn contain(src: [usize; 2], dst: Rect) -> Rect {
    let (sw, sh) = (src[0].max(1) as f32, src[1].max(1) as f32);
    let s = (dst.width() / sw).min(dst.height() / sh);
    Rect::from_center_size(dst.center(), vec2(sw * s, sh * s))
}

/// Image whose opacity follows `mask(u, v)` - feathered hero / backdrop art.
pub fn image_masked(
    p: &Painter,
    r: Rect,
    tex: TextureId,
    uv: Rect,
    cols: usize,
    rows: usize,
    a: f32,
    mask: impl Fn(f32, f32) -> f32,
) {
    let mut m = Mesh::with_texture(tex);
    for j in 0..=rows {
        for i in 0..=cols {
            let u = i as f32 / cols as f32;
            let v = j as f32 / rows as f32;
            let k = (mask(u, v) * a).clamp(0.0, 1.0);
            m.vertices.push(Vertex {
                pos: pos2(r.left() + r.width() * u, r.top() + r.height() * v),
                uv: pos2(uv.left() + uv.width() * u, uv.top() + uv.height() * v),
                color: Color32::from_white_alpha((k * 255.0).round() as u8),
            });
        }
    }
    let stride = (cols + 1) as u32;
    for j in 0..rows as u32 {
        for i in 0..cols as u32 {
            let a = j * stride + i;
            m.add_triangle(a, a + 1, a + stride);
            m.add_triangle(a + stride, a + 1, a + stride + 1);
        }
    }
    p.add(Shape::mesh(m));
}

/// Deterministic pleasant colour pair for a seed string (placeholders).
pub fn seed_colors(seed: &str) -> (Color32, Color32) {
    let mut h: u32 = 2166136261;
    for b in seed.bytes() {
        h = (h ^ b as u32).wrapping_mul(16777619);
    }
    let hue = (h % 360) as f32 / 360.0;
    (hsv(hue, 0.55, 0.42), hsv((hue + 0.09) % 1.0, 0.62, 0.20))
}

pub fn hsv(h: f32, s: f32, v: f32) -> Color32 {
    let i = (h * 6.0).floor();
    let f = h * 6.0 - i;
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - f * s), v * (1.0 - (1.0 - f) * s));
    let (r, g, b) = match i as i32 % 6 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    Color32::from_rgb((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
}

/// Initials of a name ("TRT 1 HD" -> "T1", "Kanal D" -> "KD").
pub fn initials(name: &str) -> String {
    let mut out = String::new();
    for w in name
        .split(|c: char| c.is_whitespace() || c == '-' || c == '|' || c == ':')
        .filter(|w| !w.is_empty())
    {
        let up = tr_upper(w);
        if matches!(up.as_str(), "HD" | "FHD" | "SD" | "UHD" | "4K" | "TR") {
            continue;
        }
        if let Some(c) = w.chars().next() {
            out.push_str(&tr_upper(&c.to_string()));
        }
        if out.chars().count() >= 2 {
            break;
        }
    }
    if out.is_empty() {
        out.push('?');
    }
    out
}

/// Gradient tile with initials (until the real image arrives).
pub fn placeholder(p: &Painter, r: Rect, rad: f32, seed: &str, label_size: f32, a: f32) {
    let (c1, c2) = seed_colors(seed);
    let pts = rounded_points(r, rad);
    let mut path = Path::default();
    path.add_line_loop(&pts);
    let mut mesh = Mesh::default();
    path.fill(PX, Color32::WHITE, &mut mesh);
    for v in mesh.vertices.iter_mut() {
        let t = (((v.pos.x - r.left()) / r.width().max(1.0)) * 0.35
            + ((v.pos.y - r.top()) / r.height().max(1.0)) * 0.65)
            .clamp(0.0, 1.0);
        let k = v.color.a() as f32 / 255.0 * a;
        v.color = mix(c1, c2, t).gamma_multiply(k);
    }
    p.add(Shape::mesh(mesh));
    rrect_stroke(p, r, rad, 1.0, white(0.06 * a));
    if label_size > 0.0 {
        let s = initials(seed);
        text(
            p,
            r.center(),
            Align2::CENTER_CENTER,
            &s,
            ty::sized(label_size, super::theme::fam::W700),
            white(0.82 * a),
        );
    }
}

// ---------------------------------------------------------------------------------------- components

/// Thin progress bar with accent fill (`t` 0..1).
pub fn progress(p: &Painter, r: Rect, t: f32, track: Color32) {
    let rad = r.height() * 0.5;
    rrect(p, r, rad, track);
    let t = t.clamp(0.0, 1.0);
    if t > 0.0 {
        let f = Rect::from_min_size(r.min, vec2((r.width() * t).max(r.height()), r.height()));
        accent_fill(p, f, rad, 1.0);
    }
}

/// Small rounded badge (left-centre anchored); returns its rect.
pub fn badge(p: &Painter, lc: Pos2, label: &str, fg: Color32, bg: Color32) -> Rect {
    let font = ty::micro();
    let tw = measure(p, label, font.clone()).x;
    let h = 18.0;
    let r = Rect::from_min_size(pos2(lc.x, lc.y - h * 0.5), vec2(tw + 12.0, h));
    rrect(p, r, 5.0, bg);
    text(
        p,
        pos2(r.left() + 6.0, r.center().y),
        Align2::LEFT_CENTER,
        label,
        font,
        fg,
    );
    r
}

/// Outlined badge (quality tags).
pub fn tag(p: &Painter, lc: Pos2, label: &str, a: f32) -> Rect {
    let font = ty::micro();
    let tw = measure(p, label, font.clone()).x;
    let h = 18.0;
    let r = Rect::from_min_size(pos2(lc.x, lc.y - h * 0.5), vec2(tw + 12.0, h));
    rrect(p, r, 5.0, white(0.10 * a));
    rrect_stroke(p, r, 5.0, 1.0, white(0.22 * a));
    text(
        p,
        pos2(r.left() + 6.0, r.center().y),
        Align2::LEFT_CENTER,
        label,
        font,
        white(0.92 * a),
    );
    r
}

/// Rating chip "★ 7.8".
pub fn rating(p: &Painter, lc: Pos2, v: f32, a: f32) -> Rect {
    if v <= 0.0 {
        return Rect::from_min_size(lc, Vec2::ZERO);
    }
    let s = format!("{v:.1}");
    let font = ty::num_b(11.0);
    let tw = measure(p, &s, font.clone()).x;
    let h = 18.0;
    let r = Rect::from_min_size(pos2(lc.x, lc.y - h * 0.5), vec2(tw + 26.0, h));
    rrect(p, r, 5.0, black(0.55 * a));
    icon_fill(
        p,
        pos2(r.left() + 9.5, r.center().y),
        ic::STAR,
        12.0,
        alpha(col::GOLD(), a),
    );
    text(
        p,
        pos2(r.left() + 18.0, r.center().y),
        Align2::LEFT_CENTER,
        &s,
        font,
        white(0.95 * a),
    );
    r
}

/// Button kinds for `button`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Btn {
    Primary,
    Normal,
}

pub fn button_width(p: &Painter, glyph: Option<&str>, label: &str) -> f32 {
    let tw = measure(p, label, ty::title()).x;
    tw + if glyph.is_some() { 28.0 } else { 0.0 } + 44.0
}

/// Pill button; `f` focus 0..1. Focused = white fill with dark text (strongest contrast on TV).
pub fn button(p: &Painter, r: Rect, glyph: Option<&str>, label: &str, f: f32, kind: Btn) {
    let r = Rect::from_center_size(r.center(), r.size() * (1.0 + 0.05 * f));
    let rad = r.height() * 0.5;
    shadow(p, r, rad, 0.8 * f);
    if f > 0.01 {
        soft(p, r.expand(3.0), rad + 3.0, 16.0, alpha(col::A(), 0.35 * f));
    }
    match kind {
        Btn::Primary => {
            accent_fill(p, r, rad, 1.0);
            rrect(p, r, rad, white(0.92 * f));
        }
        Btn::Normal => {
            rrect(p, r, rad, mix(white(0.12), Color32::WHITE, f));
        }
    }
    let fg = mix(col::TEXT(), col::BG(), f);
    let font = ty::title();
    let iw = if glyph.is_some() { 28.0 } else { 0.0 };
    // Long (translated) labels ellipsize inside the pill instead of spilling out of it.
    let tw = measure(p, label, font.clone()).x.min((r.width() - iw - 24.0).max(20.0));
    let x0 = r.center().x - (tw + iw) * 0.5;
    if let Some(g) = glyph {
        icon_fill(p, pos2(x0 + 11.0, r.center().y), g, 22.0, fg);
    }
    text_fit(
        p,
        pos2(x0 + iw, r.center().y),
        Align2::LEFT_CENTER,
        label,
        font,
        fg,
        tw,
    );
}

/// Arc spinner.
pub fn spinner(p: &Painter, c: Pos2, rad: f32, t: f64, color: Color32, w: f32) {
    let start = (t * 4.6) as f32;
    let sweep = 1.2 + 0.9 * ((t * 2.2).sin() as f32 + 1.0);
    let n = 24;
    let pts: Vec<Pos2> = (0..=n)
        .map(|i| {
            let a = start + sweep * i as f32 / n as f32;
            c + vec2(a.cos(), a.sin()) * rad
        })
        .collect();
    p.circle_stroke(c, rad, Stroke::new(w, alpha(color, 0.14)));
    p.circle_filled(pts[0], w * 0.5, color);
    p.circle_filled(pts[n], w * 0.5, color);
    p.add(Shape::line(pts, Stroke::new(w, color)));
}

/// Translucent dark glass card.
pub fn glass(p: &Painter, r: Rect, rad: f32, a: f32) {
    shadow(p, r, rad, 0.5 * a);
    if super::theme::is_dark() {
        rrect(p, r, rad, Color32::from_rgba_premultiplied(9, 12, 20, (205.0 * a) as u8));
        rrect_stroke(p, r, rad, 1.0, white(0.08 * a));
    } else {
        rrect(p, r, rad, alpha(Color32::WHITE, 0.94 * a));
        rrect_stroke(p, r, rad, 1.0, alpha(Color32::BLACK, 0.06 * a));
    }
}

/// Remote hints at the bottom right: [(key label, action)].
pub fn hints(p: &Painter, rb: Pos2, items: &[(&str, &str)], a: f32) {
    let font = ty::caption();
    let mut x = rb.x;
    for (key, label) in items.iter().rev() {
        let r = text(
            p,
            pos2(x, rb.y),
            Align2::RIGHT_CENTER,
            label,
            font.clone(),
            alpha(col::TEXT2(), a),
        );
        x = r.left() - 6.0;
        let is_icon = key.chars().next().is_some_and(|c| (c as u32) >= 0xE000);
        let kw = if is_icon {
            22.0
        } else {
            measure(p, key, ty::micro()).x + 12.0
        };
        let kr = Rect::from_min_max(pos2(x - kw.max(22.0), rb.y - 11.0), pos2(x, rb.y + 11.0));
        rrect(p, kr, 6.0, alpha(col::TEXT(), 0.11 * a));
        if is_icon {
            icon_fill(p, kr.center(), key, 15.0, alpha(col::TEXT(), 0.9 * a));
        } else {
            text(
                p,
                kr.center(),
                Align2::CENTER_CENTER,
                key,
                ty::micro(),
                alpha(col::TEXT(), 0.9 * a),
            );
        }
        x = kr.left() - 18.0;
    }
}

/// Filled triangle pointing right / left (play / skip glyphs without an icon font entry).
pub fn triangle(p: &Painter, c: Pos2, size: f32, right: bool, color: Color32) {
    let s = size * 0.5;
    let pts = if right {
        vec![
            pos2(c.x - s * 0.8, c.y - s),
            pos2(c.x + s, c.y),
            pos2(c.x - s * 0.8, c.y + s),
        ]
    } else {
        vec![
            pos2(c.x + s * 0.8, c.y - s),
            pos2(c.x - s, c.y),
            pos2(c.x + s * 0.8, c.y + s),
        ]
    };
    p.add(Shape::convex_polygon(pts, color, Stroke::NONE));
}

/// Countdown ring: full circle track with a `f` (0..1) accent arc from 12 o'clock.
pub fn spinner_ring(p: &Painter, c: Pos2, rad: f32, f: f32) {
    p.circle_stroke(c, rad, Stroke::new(2.5, white(0.14)));
    if f > 0.001 {
        let n = ((f * 40.0) as usize).max(2);
        let a0 = -std::f32::consts::FRAC_PI_2;
        let pts: Vec<Pos2> = (0..=n)
            .map(|i| {
                let a = a0 + std::f32::consts::TAU * f * i as f32 / n as f32;
                c + vec2(a.cos(), a.sin()) * rad
            })
            .collect();
        p.add(Shape::line(pts, Stroke::new(2.5, col::ACCENT())));
    }
}
