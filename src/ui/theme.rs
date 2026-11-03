//! Design tokens (colours, type scale, spacing, motion) and font installation for Ayna TV.
//!
//! Canvas: 960 x 540 points (1920 x 1080 px at `pixels_per_point = 2`).

use std::sync::Arc;

use egui::epaint::text::{FontTweak, VariationCoords};
use egui::{Color32, FontData, FontDefinitions, FontFamily, FontId};

pub const W: f32 = 960.0;
pub const H: f32 = 540.0;
pub const MX: f32 = 40.0;
pub const MY: f32 = 22.0;

pub const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

pub fn alpha(c: Color32, a: f32) -> Color32 {
    let [r, g, b, _] = c.to_srgba_unmultiplied();
    Color32::from_rgba_unmultiplied(r, g, b, (a.clamp(0.0, 1.0) * 255.0).round() as u8)
}

pub fn white(a: f32) -> Color32 {
    Color32::from_white_alpha((a.clamp(0.0, 1.0) * 255.0).round() as u8)
}

pub fn black(a: f32) -> Color32 {
    Color32::from_black_alpha((a.clamp(0.0, 1.0) * 255.0).round() as u8)
}

pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_premultiplied(
        l(a.r(), b.r()),
        l(a.g(), b.g()),
        l(a.b(), b.b()),
        l(a.a(), b.a()),
    )
}

static DARK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

pub fn set_dark(d: bool) {
    DARK.store(d, std::sync::atomic::Ordering::Relaxed);
}

pub fn is_dark() -> bool {
    DARK.load(std::sync::atomic::Ordering::Relaxed)
}

#[allow(dead_code, non_snake_case)]
pub mod col {
    use super::rgb;
    use egui::Color32;
    macro_rules! c {
        ($name:ident, $dark:expr, $light:expr) => {
            pub fn $name() -> Color32 {
                if super::is_dark() {
                    $dark
                } else {
                    $light
                }
            }
        };
    }
    // Deep midnight obsidian background + glass slate surfaces
    c!(BG, rgb(0x07, 0x0A, 0x12), rgb(0xF6, 0xF7, 0xFA));
    c!(BG2, rgb(0x0F, 0x14, 0x22), rgb(0xFF, 0xFF, 0xFF));
    c!(SURFACE, rgb(0x16, 0x1D, 0x2E), rgb(0xEB, 0xED, 0xF3));
    c!(SURFACE_HI, rgb(0x22, 0x2C, 0x42), rgb(0xDD, 0xE1, 0xEA));
    c!(TEXT, rgb(0xF4, 0xF7, 0xFC), rgb(0x12, 0x15, 0x1E));
    c!(TEXT2, rgb(0xA4, 0xB1, 0xC6), rgb(0x48, 0x50, 0x61));
    c!(TEXT3, rgb(0x64, 0x72, 0x8A), rgb(0x84, 0x8B, 0x9A));
    // Accent gradient: Electric Cyan (#00D2FF) -> Indigo/Violet (#7C5CFF)
    c!(A, rgb(0x00, 0xD2, 0xFF), rgb(0x02, 0x84, 0xC7));
    c!(B, rgb(0x7C, 0x5C, 0xFF), rgb(0x63, 0x66, 0xF1));
    c!(ACCENT, rgb(0x38, 0xBD, 0xF8), rgb(0x02, 0x84, 0xC7));
    c!(GOLD, rgb(0xFF, 0xC5, 0x3D), rgb(0xC2, 0x8A, 0x00));
    c!(OK, rgb(0x10, 0xB9, 0x81), rgb(0x0E, 0x9F, 0x6E));
    c!(WARN, rgb(0xFB, 0xBF, 0x24), rgb(0xB4, 0x6B, 0x00));
    c!(BAD, rgb(0xF8, 0x71, 0x71), rgb(0xD2, 0x2B, 0x2B));
    c!(HEART, rgb(0xFF, 0x5C, 0x8A), rgb(0xE1, 0x1D, 0x5C));
    c!(LIVE, rgb(0x10, 0xB9, 0x81), rgb(0x05, 0x96, 0x69));
    c!(FOCUS_BG, Color32::WHITE, rgb(0x16, 0x19, 0x22));
    c!(FOCUS_FG, Color32::BLACK, Color32::WHITE);

    pub fn shadow() -> f32 {
        if super::is_dark() {
            1.0
        } else {
            0.45
        }
    }
}

pub mod motion {
    pub const FOCUS: f32 = 0.12;
    pub const SLIDE: f32 = 0.18;
    pub const SCROLL: f32 = 0.16;
    pub const FADE: f32 = 0.20;
}

pub mod fam {
    pub const W400: &str = "i400";
    pub const W500: &str = "i500";
    pub const W600: &str = "i600";
    pub const W700: &str = "i700";
    pub const D700: &str = "d700";
    pub const T500: &str = "t500";
    pub const T600: &str = "t600";
    pub const T700: &str = "t700";
    pub const ICON: &str = "icon";
    pub const ICON_FILL: &str = "icon-fill";
}

fn fid(size: f32, family: &str) -> FontId {
    FontId::new(size, FontFamily::Name(family.into()))
}

pub mod ty {
    use super::{fam, fid};
    use egui::FontId;

    pub fn display() -> FontId {
        fid(34.0, fam::D700)
    }
    pub fn h1() -> FontId {
        fid(24.0, fam::D700)
    }
    pub fn h2() -> FontId {
        fid(19.0, fam::W700)
    }
    pub fn title() -> FontId {
        fid(15.5, fam::W600)
    }
    pub fn title_s() -> FontId {
        fid(14.5, fam::W600)
    }
    pub fn body() -> FontId {
        fid(13.5, fam::W400)
    }
    pub fn body_m() -> FontId {
        fid(13.5, fam::W500)
    }
    pub fn small() -> FontId {
        fid(12.5, fam::W500)
    }
    pub fn small_r() -> FontId {
        fid(12.5, fam::W400)
    }
    pub fn caption() -> FontId {
        fid(11.5, fam::W500)
    }
    pub fn kicker() -> FontId {
        fid(10.5, fam::W700)
    }
    pub fn micro() -> FontId {
        fid(10.0, fam::W700)
    }
    pub fn num(size: f32) -> FontId {
        fid(size, fam::T600)
    }
    pub fn num_m(size: f32) -> FontId {
        fid(size, fam::T500)
    }
    pub fn num_b(size: f32) -> FontId {
        fid(size, fam::T700)
    }
    pub fn icon(size: f32) -> FontId {
        fid(size, fam::ICON)
    }
    pub fn icon_fill(size: f32) -> FontId {
        fid(size, fam::ICON_FILL)
    }
    pub fn sized(size: f32, family: &str) -> FontId {
        fid(size, family)
    }
}

static INTER: &[u8] = include_bytes!("../../assets/fonts/InterVariable-om.ttf");
static INTER_TNUM: &[u8] = include_bytes!("../../assets/fonts/InterVariable-tnum-om.ttf");
static SYMBOLS: &[u8] = include_bytes!("../../assets/fonts/MaterialSymbolsRounded-kare.ttf");
/// Full Inter 4.1 variable font (OFL-1.1, see assets/fonts/LICENSE-Inter.txt): fallback for
/// glyphs the size-optimised subset above lacks (Cyrillic, Vietnamese, …), same design + axes.
static INTER_FULL: &[u8] = include_bytes!("../../assets/fonts/InterVariable-full.ttf");

fn variant(blob: &egui::epaint::text::Blob, coords: &[(&[u8; 4], f32)]) -> Arc<FontData> {
    let tweak = FontTweak {
        coords: VariationCoords::new(coords.iter().map(|(t, v)| (*t, *v))),
        ..Default::default()
    };
    Arc::new(FontData::from_blob(blob.clone(), 0).tweak(tweak))
}

pub fn font_definitions() -> FontDefinitions {
    let mut defs = FontDefinitions::empty();
    let inter: egui::epaint::text::Blob = Arc::new(INTER);
    let tnum: egui::epaint::text::Blob = Arc::new(INTER_TNUM);
    let sym: egui::epaint::text::Blob = Arc::new(SYMBOLS);
    let full: egui::epaint::text::Blob = Arc::new(INTER_FULL);
    let faces: [(&str, f32, f32); 5] = [
        (fam::W400, 400.0, 14.0),
        (fam::W500, 500.0, 14.0),
        (fam::W600, 600.0, 14.0),
        (fam::W700, 700.0, 14.0),
        (fam::D700, 700.0, 32.0),
    ];
    for (name, wght, opsz) in faces {
        defs.font_data.insert(
            name.into(),
            variant(&inter, &[(b"wght", wght), (b"opsz", opsz)]),
        );
        defs.font_data.insert(
            format!("{name}-x"),
            variant(&full, &[(b"wght", wght), (b"opsz", opsz)]),
        );
    }
    for (name, wght) in [(fam::T500, 500.0), (fam::T600, 600.0), (fam::T700, 700.0)] {
        defs.font_data.insert(
            format!("{name}-d"),
            variant(&tnum, &[(b"wght", wght), (b"opsz", 14.0)]),
        );
    }
    defs.font_data.insert(
        fam::ICON.into(),
        variant(&sym, &[(b"FILL", 0.0), (b"wght", 400.0)]),
    );
    defs.font_data.insert(
        fam::ICON_FILL.into(),
        variant(&sym, &[(b"FILL", 1.0), (b"wght", 400.0)]),
    );
    // Primary face(s), then the full Inter at the same weight for missing glyphs, then icons.
    let chain = |primary: &[&str], weight: &str| -> Vec<String> {
        let mut v: Vec<String> = primary.iter().map(|s| s.to_string()).collect();
        v.push(format!("{weight}-x"));
        v.push(fam::ICON.into());
        v
    };
    defs.families
        .insert(FontFamily::Proportional, chain(&[fam::W400], fam::W400));
    defs.families
        .insert(FontFamily::Monospace, chain(&["t500-d", fam::W500], fam::W500));
    for (name, _, _) in faces {
        defs.families
            .insert(FontFamily::Name(name.into()), chain(&[name], name));
    }
    defs.families.insert(
        FontFamily::Name(fam::T500.into()),
        chain(&["t500-d", fam::W500], fam::W500),
    );
    defs.families.insert(
        FontFamily::Name(fam::T600.into()),
        chain(&["t600-d", fam::W600], fam::W600),
    );
    defs.families.insert(
        FontFamily::Name(fam::T700.into()),
        chain(&["t700-d", fam::W700], fam::W700),
    );
    defs.families.insert(
        FontFamily::Name(fam::ICON.into()),
        vec![fam::ICON.into()],
    );
    defs.families.insert(
        FontFamily::Name(fam::ICON_FILL.into()),
        vec![fam::ICON_FILL.into()],
    );
    defs
}

pub fn install(ctx: &egui::Context) {
    let key = egui::Id::NULL.with("yansit-ui-installed");
    if ctx.data(|d| d.get_temp::<bool>(key)).unwrap_or(false) {
        return;
    }
    ctx.set_fonts(font_definitions());
    let mut v = egui::Visuals::dark();
    v.panel_fill = Color32::TRANSPARENT;
    v.window_fill = Color32::TRANSPARENT;
    v.override_text_color = Some(col::TEXT());
    ctx.set_visuals(v);
    ctx.global_style_mut(|s| {
        s.interaction.selectable_labels = false;
        s.animation_time = motion::FOCUS;
    });
    ctx.data_mut(|d| d.insert_temp(key, true));
}
