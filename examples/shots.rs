//! Google Play screenshots for Android TV, rendered from the REAL egui UI on the host.
//!
//! The TV's `screencap` returns black for both the video plane and the overlay surface, so
//! store images are produced here instead: Mesa surfaceless EGL + an offscreen framebuffer,
//! 1920×1080 PNGs, every supported language, into `docs/store/screenshots/<lang>/`.
//!
//! Content is synthetic and neutral: generic sender names ("iPad", "iPhone"), a procedural
//! landscape as the mirrored picture, a made-up demo track, the RFC 5737-style example IP
//! 192.168.1.20 — nothing from the developer's own network or devices.
//!
//! Run: `cargo run --release --example shots [-- out_dir]`
use egui_glow::glow::{self, HasContext};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use yansit::airplay::{AudioCodec, AudioFormat, AudioPacket, Client, Codec, Metadata, NetStats, Sink, VideoFrame};
use yansit::i18n::{self, Lang, LangPref};
use yansit::media::MediaEngine;
use yansit::ui::{Key, Ui};

const W: u32 = 1920;
const H: u32 = 1080;
const DT: f64 = 1.0 / 60.0;
/// Shown in the header instead of a probed address (documentation-style example LAN IP).
const DEMO_IP: &str = "192.168.1.20";

struct Gl {
    gl: Arc<glow::Context>,
    fbo: glow::Framebuffer,
}

fn gl_setup() -> Gl {
    let egl = unsafe { khronos_egl::DynamicInstance::<khronos_egl::EGL1_5>::load_required().expect("libEGL") };
    // EGL_PLATFORM_SURFACELESS_MESA: no window system needed on the build server.
    let display = unsafe { egl.get_platform_display(0x31DD, khronos_egl::DEFAULT_DISPLAY, &[khronos_egl::ATTRIB_NONE]) }
        .expect("surfaceless display");
    egl.initialize(display).expect("egl init");
    egl.bind_api(khronos_egl::OPENGL_ES_API).unwrap();
    let config = egl
        .choose_first_config(
            display,
            &[khronos_egl::RENDERABLE_TYPE, khronos_egl::OPENGL_ES3_BIT, khronos_egl::SURFACE_TYPE, khronos_egl::PBUFFER_BIT, khronos_egl::NONE],
        )
        .unwrap()
        .expect("config");
    let ctx = egl
        .create_context(display, config, None, &[khronos_egl::CONTEXT_CLIENT_VERSION, 3, khronos_egl::NONE])
        .expect("context");
    egl.make_current(display, None, None, Some(ctx)).expect("make current");
    let gl = Arc::new(unsafe {
        glow::Context::from_loader_function(|name| egl.get_proc_address(name).map_or(std::ptr::null(), |p| p as *const _))
    });
    let fbo = unsafe {
        let fbo = gl.create_framebuffer().unwrap();
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
        let rb = gl.create_renderbuffer().unwrap();
        gl.bind_renderbuffer(glow::RENDERBUFFER, Some(rb));
        gl.renderbuffer_storage(glow::RENDERBUFFER, glow::RGBA8, W as i32, H as i32);
        gl.framebuffer_renderbuffer(glow::FRAMEBUFFER, glow::COLOR_ATTACHMENT0, glow::RENDERBUFFER, Some(rb));
        fbo
    };
    std::mem::forget(egl); // keep the display/context alive for the whole run
    Gl { gl, fbo }
}

/// Procedural "mirrored screen" (dusk landscape over a lake) shown behind the HUD.
fn scene() -> image::RgbImage {
    let (w, h) = (W, H);
    let horizon = h as f32 * 0.62;
    image::RgbImage::from_fn(w, h, |x, y| {
        let (xf, yf) = (x as f32, y as f32);
        let ridge = |amp: f32, freq: f32, ph: f32, base: f32| {
            base - amp * ((xf * freq + ph).sin() * 0.6 + (xf * freq * 2.3 + ph * 1.7).sin() * 0.4).abs()
        };
        let sky_t = (yf / horizon).clamp(0.0, 1.0);
        let mut c = [
            18.0 + 210.0 * sky_t.powf(2.2),
            28.0 + 110.0 * sky_t.powf(1.6),
            70.0 + 40.0 * sky_t,
        ];
        // sun glow
        let (sx, sy) = (w as f32 * 0.68, horizon - 70.0);
        let d = ((xf - sx).powi(2) + (yf - sy).powi(2)).sqrt();
        let glow = (1.0 - d / 420.0).max(0.0).powf(2.0);
        c[0] += 120.0 * glow;
        c[1] += 70.0 * glow;
        c[2] += 20.0 * glow;
        if d < 46.0 {
            c = [255.0, 214.0, 150.0];
        }
        let far = ridge(110.0, 0.0042, 1.3, horizon - 10.0);
        let near = ridge(160.0, 0.0061, 4.1, horizon + 4.0);
        if yf > far && yf <= horizon {
            c = [62.0, 44.0, 78.0];
        }
        if yf > near && yf <= horizon {
            c = [34.0, 24.0, 48.0];
        }
        if yf > horizon {
            // lake: darker mirrored sky with a sun streak
            let m = (yf - horizon) / (h as f32 - horizon);
            let streak = (1.0 - ((xf - sx).abs() / (60.0 + 220.0 * m))).max(0.0) * (1.0 - m);
            c = [
                24.0 + 90.0 * (1.0 - m) + 160.0 * streak,
                30.0 + 50.0 * (1.0 - m) + 110.0 * streak,
                58.0 + 30.0 * (1.0 - m) + 40.0 * streak,
            ];
        }
        image::Rgb([c[0].clamp(0.0, 255.0) as u8, c[1].clamp(0.0, 255.0) as u8, c[2].clamp(0.0, 255.0) as u8])
    })
}

/// Procedural album cover (PNG bytes) for the now-playing screen.
fn cover_png() -> Vec<u8> {
    let img = image::RgbImage::from_fn(600, 600, |x, y| {
        let (u, v) = (x as f32 / 600.0, y as f32 / 600.0);
        let r = (((u - 0.35).powi(2) + (v - 0.4).powi(2)).sqrt() * 9.0).sin() * 0.5 + 0.5;
        image::Rgb([
            (40.0 + 200.0 * u * r) as u8,
            (30.0 + 120.0 * (1.0 - v)) as u8,
            (90.0 + 150.0 * v * (1.0 - r * 0.5)) as u8,
        ])
    });
    let mut out = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).unwrap();
    out
}

struct Shooter {
    gl: Gl,
    painter: egui_glow::Painter,
    ui: Ui,
    engine: Arc<MediaEngine>,
    t: f64,
    textures: egui::TexturesDelta,
    dir: PathBuf,
}

impl Shooter {
    fn new(gl: Gl, lang: Lang, dir: PathBuf, state: &Path) -> Self {
        let _ = std::fs::remove_dir_all(state);
        std::fs::create_dir_all(state).unwrap();
        // The device language drives the first-run receiver name and the UI ("Auto").
        i18n::set_device_tag(lang.code());
        i18n::apply(LangPref::Auto);
        let engine = MediaEngine::new(state.to_path_buf());
        let mut ui = Ui::new(engine.clone());
        ui.freeze_ip_for_screenshots(DEMO_IP);
        let painter = egui_glow::Painter::new(gl.gl.clone(), "", None, true).expect("painter");
        Shooter { gl, painter, ui, engine, t: 10.0, textures: Default::default(), dir }
    }

    /// Advance the UI (animations settle) and optionally paint + save.
    fn step(&mut self, keys: &[Key], secs: f64) {
        let n = (secs / DT).round().max(1.0) as usize;
        for i in 0..n {
            let ks: &[Key] = if i == 0 { keys } else { &[] };
            let mut uf = self.ui.frame_at([W, H], ks, self.t);
            self.textures.append(std::mem::take(&mut uf.out.textures_delta));
            self.t += DT;
        }
    }

    fn shot(&mut self, name: &str, background: Option<&image::RgbImage>) {
        let mut uf = self.ui.frame_at([W, H], &[], self.t);
        self.textures.append(std::mem::take(&mut uf.out.textures_delta));
        self.t += DT;
        let prims = self.ui.ctx().tessellate(std::mem::take(&mut uf.out.shapes), uf.out.pixels_per_point);
        unsafe {
            self.gl.gl.bind_framebuffer(glow::FRAMEBUFFER, Some(self.gl.fbo));
            self.gl.gl.viewport(0, 0, W as i32, H as i32);
            self.gl.gl.disable(glow::SCISSOR_TEST);
            self.gl.gl.clear_color(0.0, 0.0, 0.0, 0.0);
            self.gl.gl.clear(glow::COLOR_BUFFER_BIT);
        }
        self.painter.paint_and_update_textures([W, H], uf.out.pixels_per_point, &prims, &mut self.textures);
        let (w, h) = (W as usize, H as usize);
        let mut px = vec![0u8; w * h * 4];
        unsafe {
            self.gl.gl.bind_framebuffer(glow::FRAMEBUFFER, Some(self.gl.fbo));
            self.gl
                .gl
                .read_pixels(0, 0, w as i32, h as i32, glow::RGBA, glow::UNSIGNED_BYTE, glow::PixelPackData::Slice(Some(&mut px)));
        }
        // GL rows are bottom-up; egui paints premultiplied alpha -> composite over the
        // "video" (or black, like the TV's hardware plane behind the overlay).
        let mut out = image::RgbImage::new(W, H);
        for y in 0..h {
            for x in 0..w {
                let i = ((h - 1 - y) * w + x) * 4;
                let a = px[i + 3] as f32 / 255.0;
                let bg = background.map_or([0u8; 3], |b| b.get_pixel(x as u32, y as u32).0);
                let mut c = [0u8; 3];
                for k in 0..3 {
                    c[k] = (px[i + k] as f32 + bg[k] as f32 * (1.0 - a)).round().clamp(0.0, 255.0) as u8;
                }
                out.put_pixel(x as u32, y as u32, image::Rgb(c));
            }
        }
        let path = self.dir.join(format!("{name}.png"));
        out.save(&path).unwrap();
        println!("wrote {}", path.display());
    }

    fn finish(mut self) -> Gl {
        self.painter.destroy();
        self.gl
    }
}

fn main() {
    let out_root = std::env::args().nth(1).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("docs/store/screenshots"));
    let state_root = std::env::temp_dir().join("yansit-shots-state");
    let bg = scene();
    let cover = cover_png();
    let mut gl = gl_setup();

    for lang in i18n::ALL {
        let dir = out_root.join(lang.code());
        std::fs::create_dir_all(&dir).unwrap();
        // Store images show the paid-tier prices the owner chose ($10/yr, $20 once).
        yansit::billing::set_state(false, false, false, "$10.00".into(), "$20.00".into(), true);
        let mut s = Shooter::new(gl, lang, dir, &state_root.join(lang.code()));

        // 01: ready to receive (settings panel focused on the Language row).
        s.step(&[], 1.2);
        s.shot("01-ready", None);

        // 02: mirroring with the live telemetry HUD over the mirrored picture.
        let ipad = Client { name: "iPad".into(), model: "iPad Pro".into(), ..Default::default() };
        s.engine.session_start(&ipad);
        s.engine.video_format(Codec::H265, &[], 2732, 2048);
        let t0 = std::time::Instant::now();
        for i in 0..46u32 {
            while t0.elapsed() < Duration::from_micros(16_667 * i as u64) {
                std::thread::sleep(Duration::from_micros(500));
            }
            s.engine.video_frame(VideoFrame { data: vec![0u8; 34_000], key: i == 0, pts_local: 0, arrival: 0 });
        }
        s.step(&[], 0.9);
        s.shot("02-mirroring", Some(&bg));
        s.engine.session_end();
        s.step(&[], 0.6);

        // 03: AirPlay music with cover art, metadata and progress.
        let iphone = Client { name: "iPhone".into(), model: "iPhone".into(), ..Default::default() };
        s.engine.session_start(&iphone);
        s.engine.audio_start(&AudioFormat {
            codec: AudioCodec::Alac,
            spf: 352,
            sample_rate: 44_100,
            channels: 2,
            using_screen: false,
            is_media: true,
        });
        for seq in 0..1840u32 {
            s.engine.audio_packet(AudioPacket { data: vec![0u8; 16], rtp: seq * 352, seq: seq as u16, pts_local: 0, arrival: 0 });
        }
        s.engine.metadata(&Metadata { title: "Sunrise Drive".into(), artist: "Ayna Demo".into(), album: "Living Room Sessions".into() });
        s.engine.cover_art(&cover);
        s.engine.volume(-7.5);
        s.engine.progress(74.0, 213.0);
        s.engine.net_stats(&NetStats { ntp_delay_ms: 3.2, ntp_offset_valid: true });
        s.step(&[], 1.2);
        s.shot("03-audio", None);
        s.engine.session_end();
        s.step(&[], 0.6);

        // 04: Ayna Pro sheet (the Pro row is the first, already focused row).
        s.step(&[Key::Ok], 1.0);
        s.shot("04-pro", None);
        s.step(&[Key::Back], 0.4);

        // 05: on-screen AirPlay PIN for a new device.
        s.engine.pin("4827");
        s.step(&[], 0.9);
        s.shot("05-pin", None);
        s.engine.pin("");
        s.step(&[], 0.4);

        // 06: About sheet (GPL notice, source link, credits) — for review, not a store image.
        // Up from the first row (Ayna Pro) wraps to the last row (About).
        s.step(&[Key::Up], 0.4);
        s.step(&[Key::Ok], 1.0);
        s.shot("06-about", None);
        s.step(&[Key::Back], 0.3);

        gl = s.finish();
    }
}
