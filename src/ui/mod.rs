//! Ayna TV UI (`egui 0.37`, 960x540 logical canvas at `pixels_per_point = 2.0`).
//!
//! Renders:
//! 1. **Dashboard / Ready Screen**: Modern dark glassmorphism TV interface with live receiver
//!    status card, iPad/iPhone/Mac connection guide, active audio player card (with album cover
//!    art, metadata & progress bar when streaming RAOP audio), and a 10-row D-pad Settings panel.
//! 2. **Zero-Overhead Mirroring Mode**: When mirroring video is active (`StreamMode::Mirroring`)
//!    and the HUD is hidden, sets `UiFrame::visible = false` so no GL swap occurs over the
//!    hardware video plane. Pressing OK/Info/Menu toggles a sleek live telemetry HUD bar.
//! 3. **PIN Modal**: Large 4-digit on-screen code modal when PIN authentication is requested.

pub mod anim;
pub mod draw;
pub mod icons;
pub mod theme;

use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::{
    pos2, vec2, Align2, Color32, ColorImage, Context, FullOutput, Painter, Pos2, RawInput, Rect,
    Stroke, TextureHandle, TextureOptions,
};

use crate::i18n::{self, t, tf, K};
use crate::media::{LiveSnapshot, MediaEngine, ResPreset, Settings, StreamMode};
use anim::{k, Anims};
use draw as d;
use theme::{alpha, black, col, mix, motion, rgb, ty, white, H, W};

/// Remote / keyboard key events delivered to the UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Left,
    Right,
    Ok,
    Hold,
    Back,
    Menu,
    Info,
    PlayPause,
    Play,
    Pause,
    Stop,
    Next,
    Prev,
    Forward,
    Rewind,
    ChUp,
    ChDown,
    Backspace,
    Digit(u8),
    Char(char),
}

pub struct UiFrame {
    pub out: FullOutput,
    /// When `false` (fullscreen mirroring with HUD hidden), the caller skips `eglSwapBuffers`
    /// after clearing once so the hardware video plane runs with zero GL compositor overhead.
    pub visible: bool,
    pub repaint_after: Duration,
}

/// Rows of the right-hand settings panel, top to bottom. Language is first so a user who
/// lands in the wrong language finds it immediately (its label always also says "Language").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Row {
    Language,
    DeviceName,
    Resolution,
    Fps,
    Codec,
    Latency,
    Color,
    Scaling,
    AudioBuffer,
    Pin,
    AutoWake,
    Pro,
    About,
}

const ROWS: [Row; 13] = [
    Row::Pro,
    Row::Language,
    Row::DeviceName,
    Row::Resolution,
    Row::Fps,
    Row::Codec,
    Row::Latency,
    Row::Color,
    Row::Scaling,
    Row::AudioBuffer,
    Row::Pin,
    Row::AutoWake,
    Row::About,
];

/// Public source repository shown in About (GPL-3.0 §6). Set at build time with
/// `YANSIT_SOURCE_URL=https://… cargo build`; without it About points to the Play listing.
const SOURCE_URL: Option<&str> = option_env!("YANSIT_SOURCE_URL");
const NUM_SETTINGS: usize = ROWS.len();

/// Buttons of the Ayna Pro sheet (which ones show depends on the entitlement state).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UpBtn {
    Yearly,
    Lifetime,
    Restore,
    Manage,
    Close,
}

pub struct Ui {
    ctx: Context,
    engine: Arc<MediaEngine>,
    an: Anims,
    t0: Instant,
    local_ip: String,
    last_ip_check: Instant,
    /// Selected row in the right-hand settings panel (`0..NUM_SETTINGS`).
    sel_row: usize,
    /// Selected button in the mirroring HUD (`0..4`).
    hud_btn: usize,
    /// Timestamp (`an.now()` seconds) until which the mirroring HUD stays visible.
    hud_until: f64,
    /// User opened the full dashboard overlay while mirroring is active.
    dashboard_over_video: bool,
    last_mode: StreamMode,
    cover_tex: Option<(u64, TextureHandle)>,
    toast: Option<(String, f64)>,
    disconnect_req: bool,
    exit_req: bool,
    /// Ayna Pro upgrade sheet open, and the selected button (0 yearly, 1 lifetime, 2 restore, 3 close).
    upgrade_open: bool,
    upgrade_sel: usize,
    /// About sheet (licence, source code, credits, trademarks) is open.
    about_open: bool,
    /// Review / rating prompt modal after first successful session.
    review_open: bool,
    review_focus: usize,
    session_start_time: Option<f64>,
    /// Screenshot/demo mode: keep a neutral example IP instead of probing the real network
    /// (store screenshots must not show the developer's own LAN address).
    ip_frozen: bool,
}

impl Ui {
    pub fn new(engine: Arc<MediaEngine>) -> Self {
        let ctx = Context::default();
        theme::install(&ctx);
        Self {
            ctx,
            engine,
            an: Anims::default(),
            t0: Instant::now(),
            local_ip: detect_local_ipv4(),
            last_ip_check: Instant::now(),
            sel_row: 0,
            hud_btn: 0,
            hud_until: 0.0,
            dashboard_over_video: false,
            last_mode: StreamMode::Idle,
            cover_tex: None,
            toast: None,
            disconnect_req: false,
            exit_req: false,
            upgrade_open: false,
            upgrade_sel: 0,
            about_open: false,
            review_open: false,
            review_focus: 0,
            session_start_time: None,
            ip_frozen: false,
        }
    }

    /// For store screenshots (`examples/shots.rs`): show `ip` and stop probing the network.
    pub fn freeze_ip_for_screenshots(&mut self, ip: &str) {
        self.local_ip = ip.to_string();
        self.ip_frozen = true;
    }

    pub fn take_disconnect_request(&mut self) -> bool {
        std::mem::replace(&mut self.disconnect_req, false)
    }

    pub fn take_exit(&mut self) -> bool {
        std::mem::replace(&mut self.exit_req, false)
    }

    pub fn ctx(&self) -> &Context {
        &self.ctx
    }

    pub fn frame(&mut self, screen_px: [u32; 2], keys: &[Key]) -> UiFrame {
        self.frame_at(screen_px, keys, self.t0.elapsed().as_secs_f64())
    }

    pub fn frame_at(&mut self, screen_px: [u32; 2], keys: &[Key], now: f64) -> UiFrame {
        if !self.ip_frozen && self.last_ip_check.elapsed() > Duration::from_secs(6) {
            self.local_ip = detect_local_ipv4();
            self.last_ip_check = Instant::now();
        }

        let snap = self.engine.snapshot();
        if snap.mode != self.last_mode {
            if snap.mode == StreamMode::Mirroring {
                // Show HUD briefly for 4 seconds when mirroring starts
                self.hud_until = now + 4.0;
                self.dashboard_over_video = false;
                self.hud_btn = 0;
                self.session_start_time = Some(now);
            } else {
                // A mirror session just ended. After the first real session (>= 30 s) show the
                // neutral rating prompt once, in place of that session's ad; otherwise the free
                // tier gets an interstitial *between* sessions (never over live video).
                if self.last_mode == StreamMode::Mirroring {
                    let duration = self.session_start_time.take().map(|t| now - t).unwrap_or(0.0);
                    if duration >= 30.0 && !crate::billing::is_review_prompted() {
                        crate::billing::mark_review_prompted();
                        self.review_open = true;
                        self.review_focus = 0;
                    } else if duration >= 20.0 {
                        // Not after a failed / instantly dropped connection.
                        crate::billing::request_interstitial();
                    }
                }
                self.dashboard_over_video = false;
            }
            self.last_mode = snap.mode;
        }

        for &key in keys {
            self.handle_key(key, &snap, now);
        }

        let hud_open =
            snap.mode == StreamMode::Mirroring && (now < self.hud_until || snap.paused);
        let pin_open = !snap.active_pin.is_empty();
        let show_dashboard = snap.mode != StreamMode::Mirroring || self.dashboard_over_video;
        let visible = show_dashboard || hud_open || pin_open || self.upgrade_open || self.about_open || self.review_open;

        let [pw, ph] = screen_px;
        let ppp = ((pw as f32 / W).min(ph as f32 / H)).max(0.5);
        let raw = RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(W, H))),
            time: Some(now),
            ..Default::default()
        };

        self.an.begin_frame(now);
        let ctx = self.ctx.clone();
        let out = ctx.run_ui(raw, |ui| {
            theme::install(ui.ctx());
            ui.ctx().set_pixels_per_point(ppp);
            let p = ui.ctx().layer_painter(egui::LayerId::background());
            if show_dashboard {
                self.draw_dashboard(&p, &snap, now);
            } else if hud_open {
                self.draw_mirror_hud(&p, &snap, now);
            }
            if pin_open {
                self.draw_pin_modal(&p, &snap.active_pin, now);
            }
            if self.upgrade_open {
                self.draw_upgrade_modal(&p, now);
            }
            if self.about_open {
                self.draw_about_modal(&p);
            }
            if self.review_open {
                self.draw_review_modal(&p, now);
            }
            if let Some((ref msg, until)) = self.toast {
                if now < until {
                    self.draw_toast(&p, msg, (until - now).min(1.0) as f32);
                }
            }
        });
        self.an.end_frame();

        let repaint_after = if !visible {
            Duration::from_millis(250)
        } else if self.an.busy() || hud_open {
            Duration::ZERO
        } else if snap.mode != StreamMode::Idle {
            Duration::from_millis(200)
        } else {
            Duration::from_millis(500)
        };

        UiFrame {
            out,
            visible,
            repaint_after,
        }
    }

    fn handle_key(&mut self, key: Key, snap: &LiveSnapshot, now: f64) {
        if self.about_open {
            if matches!(key, Key::Ok | Key::Back) {
                self.about_open = false;
            }
            return;
        }
        if self.review_open {
            self.handle_review_key(key, now);
            return;
        }
        if self.upgrade_open {
            self.handle_upgrade_key(key, now);
            return;
        }
        // While mirroring fullscreen (and dashboard is not open), keys control the HUD
        if snap.mode == StreamMode::Mirroring && !self.dashboard_over_video {
            let hud_visible = now < self.hud_until || snap.paused;
            if !hud_visible {
                match key {
                    Key::Back => {
                        self.hud_until = now + 5.0;
                        self.hud_btn = 1; // Focus "Disconnect" for quick exit
                    }
                    Key::Menu | Key::Info | Key::Ok | Key::Hold | Key::Up | Key::Down
                    | Key::Left | Key::Right => {
                        self.hud_until = now + 5.0;
                    }
                    Key::Stop => {
                        self.disconnect_req = true;
                    }
                    _ => {}
                }
                return;
            }

            // HUD is currently visible
            self.hud_until = now + 5.0;
            match key {
                Key::Left => {
                    self.hud_btn = (self.hud_btn + 3) % 4;
                }
                Key::Right => {
                    self.hud_btn = (self.hud_btn + 1) % 4;
                }
                Key::Down | Key::Back => {
                    self.hud_until = 0.0;
                }
                Key::Menu | Key::Info => {
                    self.dashboard_over_video = true;
                }
                Key::Ok => match self.hud_btn {
                    0 => {
                        let mut s = self.engine.settings();
                        s.stretch_fill = !s.stretch_fill;
                        let label = tf(
                            K::LabelValue,
                            &[t(K::Picture), t(if s.stretch_fill { K::ScaleFill } else { K::ScaleFit })],
                        );
                        self.engine.update_settings(s);
                        self.toast = Some((label, now + 2.2));
                    }
                    1 => {
                        self.disconnect_req = true;
                        self.hud_until = 0.0;
                    }
                    2 => {
                        self.dashboard_over_video = true;
                    }
                    _ => {
                        self.hud_until = 0.0;
                    }
                },
                _ => {}
            }
            return;
        }

        // Dashboard / Settings navigation
        match key {
            Key::Up => {
                self.sel_row = (self.sel_row + NUM_SETTINGS - 1) % NUM_SETTINGS;
            }
            Key::Down => {
                self.sel_row = (self.sel_row + 1) % NUM_SETTINGS;
            }
            Key::Left => {
                self.adjust_setting(ROWS[self.sel_row], false, now);
            }
            Key::Right | Key::Ok => {
                self.adjust_setting(ROWS[self.sel_row], true, now);
            }
            Key::Back => {
                if self.dashboard_over_video && snap.mode == StreamMode::Mirroring {
                    self.dashboard_over_video = false;
                    self.hud_until = 0.0;
                } else if snap.mode == StreamMode::AudioOnly {
                    self.disconnect_req = true;
                } else {
                    self.exit_req = true;
                }
            }
            Key::Stop => {
                if snap.mode != StreamMode::Idle {
                    self.disconnect_req = true;
                }
            }
            _ => {}
        }
    }

    fn adjust_setting(&mut self, row: Row, forward: bool, now: f64) {
        let mut s = self.engine.settings();
        let msg: String = match row {
            Row::Language => {
                s.language = s.language.step(forward);
                let lang = i18n::apply(s.language);
                // A built-in receiver name moves to the new language too (custom names stay).
                if let Some(name) = i18n::localize_preset(&s.device_name, lang) {
                    s.device_name = name.to_string();
                }
                tf(K::LabelValue, &[t(K::RowLanguage), lang.native_name()])
            }
            Row::DeviceName => {
                let presets = i18n::device_name_presets(i18n::current());
                let next = match presets.iter().position(|&n| n == s.device_name) {
                    Some(i) if forward => (i + 1) % presets.len(),
                    Some(i) => (i + presets.len() - 1) % presets.len(),
                    None => 0,
                };
                s.device_name = presets[next].to_string();
                tf(K::ToastDeviceName, &[&s.device_name])
            }
            Row::Resolution => {
                // Step from what the receiver really advertises (free tier: max 1080p).
                let cur = s.effective_res();
                let cand = if forward { cur.next() } else { cur.prev() };
                if cand == ResPreset::Uhd4K && !crate::billing::is_pro() {
                    // 4K is a Ayna Pro feature: show the upgrade sheet instead of switching.
                    self.open_upgrade();
                    return;
                }
                s.res_preset = cand;
                tf(K::LabelValue, &[t(K::RowResolution), s.res_preset.label()])
            }
            Row::Fps => {
                s.max_fps = if s.max_fps >= 60 { 30 } else { 60 };
                tf(K::ToastFps, &[&s.max_fps.to_string()])
            }
            Row::Codec => {
                s.h265_enabled = !s.h265_enabled;
                t(if s.h265_enabled { K::ToastHevcOn } else { K::ToastH264Only }).into()
            }
            Row::Latency => {
                s.low_latency = !s.low_latency;
                t(if s.low_latency { K::ToastLatencyUltra } else { K::ToastLatencyBalanced }).into()
            }
            Row::Color => {
                s.enforce_sdr = !s.enforce_sdr;
                t(if s.enforce_sdr { K::ToastSdrOn } else { K::ToastColorAuto }).into()
            }
            Row::Scaling => {
                s.stretch_fill = !s.stretch_fill;
                tf(
                    K::LabelValue,
                    &[t(K::RowScaling), t(if s.stretch_fill { K::ScaleFill } else { K::ScaleFit })],
                )
            }
            Row::AudioBuffer => {
                const BUFS: [u32; 4] = [20, 40, 80, 120];
                let idx = BUFS.iter().position(|&b| b == s.audio_buffer_ms).unwrap_or(0);
                let next = if forward {
                    (idx + 1) % BUFS.len()
                } else {
                    (idx + BUFS.len() - 1) % BUFS.len()
                };
                s.audio_buffer_ms = BUFS[next];
                tf(K::LabelValue, &[t(K::RowAudioBuffer), &format!("{} ms", s.audio_buffer_ms)])
            }
            Row::Pin => {
                s.require_pin = !s.require_pin;
                t(if s.require_pin { K::ToastPinOn } else { K::ToastPinOff }).into()
            }
            Row::AutoWake => {
                s.auto_wake = !s.auto_wake;
                t(if s.auto_wake { K::ToastAutoWakeOn } else { K::ToastAutoWakeOff }).into()
            }
            Row::Pro => {
                self.open_upgrade();
                return;
            }
            Row::About => {
                self.about_open = true;
                return;
            }
        };
        self.engine.update_settings(s);
        self.toast = Some((msg, now + 2.4));
    }

    fn open_upgrade(&mut self) {
        self.upgrade_open = true;
        self.upgrade_sel = 0;
    }

    /// Free: yearly / lifetime / restore / close. Pro: (manage subscription) / close.
    fn upgrade_buttons() -> Vec<UpBtn> {
        let b = crate::billing::snapshot();
        if !b.pro {
            vec![UpBtn::Yearly, UpBtn::Lifetime, UpBtn::Restore, UpBtn::Close]
        } else if b.yearly_owned && !b.lifetime_owned {
            vec![UpBtn::Manage, UpBtn::Close]
        } else {
            vec![UpBtn::Close]
        }
    }

    fn handle_upgrade_key(&mut self, key: Key, now: f64) {
        use crate::billing::Action;
        let btns = Self::upgrade_buttons();
        let n = btns.len();
        if self.upgrade_sel >= n {
            self.upgrade_sel = 0;
        }
        match key {
            Key::Up | Key::Left => self.upgrade_sel = (self.upgrade_sel + n - 1) % n,
            Key::Down | Key::Right => self.upgrade_sel = (self.upgrade_sel + 1) % n,
            Key::Ok => {
                let toast = match btns[self.upgrade_sel] {
                    UpBtn::Yearly => {
                        crate::billing::request(Action::BuyYearly);
                        Some(K::ToastBuyYearly)
                    }
                    UpBtn::Lifetime => {
                        crate::billing::request(Action::BuyLifetime);
                        Some(K::ToastBuyLifetime)
                    }
                    UpBtn::Restore => {
                        crate::billing::request(Action::Restore);
                        Some(K::ToastRestore)
                    }
                    UpBtn::Manage => {
                        crate::billing::request(Action::ManageSubs);
                        None
                    }
                    UpBtn::Close => None,
                };
                if let Some(k) = toast {
                    self.toast = Some((t(k).into(), now + 2.4));
                }
                self.upgrade_open = false;
            }
            Key::Back => self.upgrade_open = false,
            _ => {}
        }
    }

    /// Neutral, one-time "rate on Google Play" prompt. Play policy: no opinion question first,
    /// no star picker, no routing by rating (review gating) and no reward, so every user gets
    /// the same two choices.
    fn handle_review_key(&mut self, key: Key, now: f64) {
        match key {
            Key::Left => self.review_focus = 0,
            Key::Right => self.review_focus = 1,
            Key::Ok => {
                if self.review_focus == 0 {
                    crate::billing::request(crate::billing::Action::Review);
                    self.toast = Some((t(K::ToastRateThanks).into(), now + 3.0));
                }
                self.review_open = false;
            }
            Key::Back => self.review_open = false,
            _ => {}
        }
    }

    fn draw_dashboard(&mut self, p: &Painter, snap: &LiveSnapshot, now: f64) {
        let full = Rect::from_min_size(pos2(0.0, 0.0), vec2(W, H));
        if self.dashboard_over_video && snap.mode == StreamMode::Mirroring {
            d::rrect(p, full, 0.0, black(0.78));
        } else {
            d::v_gradient(p, full, rgb(0x06, 0x09, 0x11), rgb(0x0B, 0x10, 0x1E), 16);
            // Ambient glow orbs in top-left and bottom-right
            d::soft(
                p,
                Rect::from_center_size(pos2(160.0, 90.0), vec2(260.0, 180.0)),
                90.0,
                95.0,
                alpha(col::A(), 0.11),
            );
            d::soft(
                p,
                Rect::from_center_size(pos2(W - 180.0, H - 100.0), vec2(280.0, 200.0)),
                90.0,
                110.0,
                alpha(col::B(), 0.11),
            );
        }

        let settings = self.engine.settings();

        // Top Header Bar
        self.draw_header(p, &settings, snap, now);

        // Left Column (x = 40..448, w = 408)
        let left_r = Rect::from_min_max(pos2(40.0, 76.0), pos2(448.0, H - 46.0));
        if snap.mode == StreamMode::AudioOnly {
            self.draw_audio_player_card(p, left_r, &settings, snap, now);
        } else {
            self.draw_receiver_hero_card(p, left_r, &settings, snap, now);
        }

        // Right Column: Settings Panel (x = 468..920, w = 452)
        let right_r = Rect::from_min_max(pos2(468.0, 76.0), pos2(920.0, H - 46.0));
        self.draw_settings_panel(p, right_r, &settings);

        // Bottom Remote Hints
        let hints: Vec<(&str, &str)> = if self.dashboard_over_video {
            vec![
                ("OK", t(K::HintChange)),
                ("\u{e5cb}\u{e5cc}", t(K::HintOption)),
                (t(K::KeyBack), t(K::HintBackToVideo)),
            ]
        } else if snap.mode == StreamMode::AudioOnly {
            vec![
                ("OK", t(K::HintChange)),
                ("\u{e5cb}\u{e5cc}", t(K::HintSetting)),
                (t(K::KeyBack), t(K::Disconnect)),
            ]
        } else {
            vec![
                ("OK", t(K::HintChange)),
                ("\u{e5cb}\u{e5cc}", t(K::HintOption)),
                ("\u{e5ce}\u{e5cf}", t(K::HintNavigate)),
                (t(K::KeyBack), t(K::HintExit)),
            ]
        };
        d::hints(p, pos2(W - 40.0, H - 22.0), &hints, 0.9);

        // Footer version & hardware acceleration note
        d::text_fit(
            p,
            pos2(40.0, H - 22.0),
            Align2::LEFT_CENTER,
            &format!("Ayna v{}  •  {}", env!("CARGO_PKG_VERSION"), t(K::FooterHw)),
            ty::caption(),
            col::TEXT3(),
            W * 0.5,
        );
    }

    fn draw_header(&mut self, p: &Painter, settings: &Settings, snap: &LiveSnapshot, now: f64) {
        // Custom Ayna AirPlay emblem (36x36 rounded gradient badge with screen + broadcast wave)
        let logo_r = Rect::from_min_size(pos2(40.0, 22.0), vec2(38.0, 38.0));
        d::accent_fill(p, logo_r, 11.0, 1.0);
        d::rrect_stroke(p, logo_r, 11.0, 1.0, white(0.25));
        draw_yansit_emblem(p, logo_r.center(), 18.0, Color32::WHITE);

        d::text(
            p,
            pos2(88.0, 32.0),
            Align2::LEFT_CENTER,
            "Ayna",
            ty::h1(),
            col::TEXT(),
        );

        // Pro or Free badge right next to "Ayna"
        let is_pro = crate::billing::is_pro();
        let ayna_w = d::measure(p, "Ayna", ty::h1()).x;
        let badge_x = 88.0 + ayna_w + 12.0;

        if is_pro {
            let badge_w = d::measure(p, "PRO", ty::kicker()).x + 28.0;
            let badge_r = Rect::from_min_size(pos2(badge_x, 22.0), vec2(badge_w, 20.0));
            d::rrect(p, badge_r, 10.0, alpha(rgb(0xF5, 0x9E, 0x0B), 0.22));
            d::rrect_stroke(p, badge_r, 10.0, 1.2, rgb(0xFB, 0xBF, 0x24));
            d::icon(p, pos2(badge_r.left() + 10.0, badge_r.center().y), icons::STAR, 12.0, rgb(0xFB, 0xBF, 0x24));
            d::kicker(p, pos2(badge_r.left() + 18.0, badge_r.center().y), Align2::LEFT_CENTER, "PRO", rgb(0xFB, 0xBF, 0x24));
        } else {
            let free_lbl = t(K::HeaderFree);
            let badge_w = d::measure(p, free_lbl, ty::kicker()).x + 16.0;
            let badge_r = Rect::from_min_size(pos2(badge_x, 22.0), vec2(badge_w, 20.0));
            d::rrect(p, badge_r, 10.0, white(0.08));
            d::rrect_stroke(p, badge_r, 10.0, 1.0, white(0.18));
            d::kicker(p, badge_r.center(), Align2::CENTER_CENTER, free_lbl, col::TEXT2());
        }
        // Right-aligned live status pill
        let (status_txt, status_col) = match snap.mode {
            StreamMode::Idle => (t(K::StatusIdle), col::OK()),
            StreamMode::Connecting => (t(K::StatusConnecting), col::GOLD()),
            StreamMode::Mirroring => (t(K::StatusMirroring), col::A()),
            StreamMode::AudioOnly => (t(K::StatusAudio), col::B()),
        };
        let pill_w = d::kicker_width(p, status_txt) + 38.0;
        let pill_r = Rect::from_min_size(pos2(W - 40.0 - pill_w, 26.0), vec2(pill_w, 28.0));
        d::rrect(p, pill_r, 14.0, alpha(status_col, 0.14));
        d::rrect_stroke(p, pill_r, 14.0, 1.0, alpha(status_col, 0.40));

        let pulse = 0.55 + 0.45 * ((now * 3.2).sin() as f32 * 0.5 + 0.5);
        let dot_c = pos2(pill_r.left() + 14.0, pill_r.center().y);
        p.circle_filled(dot_c, 6.5, alpha(status_col, 0.25 * pulse));
        p.circle_filled(dot_c, 3.8, status_col);
        d::kicker(
            p,
            pos2(pill_r.left() + 24.0, pill_r.center().y),
            Align2::LEFT_CENTER,
            status_txt,
            status_col,
        );

        // Network IP badge next to status pill
        let ip_label = format!("{}:7000", self.local_ip);
        let ip_w = d::measure(p, &ip_label, ty::num(12.0)).x + 32.0;
        let ip_r = Rect::from_min_size(pos2(pill_r.left() - 10.0 - ip_w, 26.0), vec2(ip_w, 28.0));
        d::rrect(p, ip_r, 14.0, white(0.06));
        d::rrect_stroke(p, ip_r, 14.0, 1.0, white(0.10));
        d::icon(
            p,
            pos2(ip_r.left() + 13.0, ip_r.center().y),
            icons::WIFI,
            14.0,
            col::A(),
        );
        d::text(
            p,
            pos2(ip_r.left() + 24.0, ip_r.center().y),
            Align2::LEFT_CENTER,
            &ip_label,
            ty::num(12.0),
            col::TEXT2(),
        );
        // Protocol subtitle: whatever room is left before the IP badge (long translations ellipsize).
        d::text_fit(
            p,
            pos2(88.0, 51.0),
            Align2::LEFT_CENTER,
            t(K::HeaderSubtitle),
            ty::caption(),
            col::TEXT2(),
            ip_r.left() - 16.0 - 88.0,
        );
        let _ = settings;
    }

    fn draw_receiver_hero_card(
        &mut self,
        p: &Painter,
        r: Rect,
        settings: &Settings,
        snap: &LiveSnapshot,
        now: f64,
    ) {
        // Top card: Receiver Identity & Active Capabilities (height 216)
        let top_r = Rect::from_min_max(r.min, pos2(r.max.x, r.min.y + 214.0));
        d::glass(p, top_r, 18.0, 1.0);

        // Decorative subtle gradient header inside card
        let band = Rect::from_min_max(top_r.min, pos2(top_r.max.x, top_r.min.y + 78.0));
        d::accent_fill(p, band, 18.0, 0.16);

        // Receiver icon & name
        let tv_badge = Rect::from_min_size(pos2(top_r.left() + 20.0, top_r.top() + 18.0), vec2(44.0, 44.0));
        d::rrect(p, tv_badge, 12.0, alpha(col::A(), 0.18));
        d::rrect_stroke(p, tv_badge, 12.0, 1.0, alpha(col::A(), 0.45));
        d::icon(p, tv_badge.center(), icons::TV, 24.0, col::A());

        d::kicker(
            p,
            pos2(tv_badge.right() + 14.0, top_r.top() + 25.0),
            Align2::LEFT_CENTER,
            t(K::KickerMirrorName),
            col::A(),
        );
        d::text_fit(
            p,
            pos2(tv_badge.right() + 14.0, top_r.top() + 47.0),
            Align2::LEFT_CENTER,
            &settings.device_name,
            ty::h1(),
            col::TEXT(),
            top_r.width() - 90.0,
        );

        // Connection / Sender Info row
        let info_y = top_r.top() + 96.0;
        if snap.mode != StreamMode::Idle && !snap.client.name.is_empty() {
            let sender_str = if snap.client.model.is_empty() {
                tf(K::ConnectedDevice, &[&snap.client.name])
            } else {
                tf(K::ConnectedWithModel, &[&snap.client.name, &snap.client.model])
            };
            d::icon_fill(
                p,
                pos2(top_r.left() + 26.0, info_y),
                icons::DEVICES,
                16.0,
                col::OK(),
            );
            d::text_fit(
                p,
                pos2(top_r.left() + 42.0, info_y),
                Align2::LEFT_CENTER,
                &sender_str,
                ty::title_s(),
                col::OK(),
                top_r.width() - 56.0,
            );
        } else {
            d::icon(
                p,
                pos2(top_r.left() + 26.0, info_y),
                icons::DEVICES,
                16.0,
                col::TEXT2(),
            );
            d::text_fit(
                p,
                pos2(top_r.left() + 42.0, info_y),
                Align2::LEFT_CENTER,
                t(K::HeroTagline),
                ty::small(),
                col::TEXT2(),
                top_r.width() - 42.0 - 18.0,
            );
        }

        // Capability Chips Grid (2x2)
        let chips = [
            (
                icons::N4K,
                t(K::StatResolution),
                format!("{} • {} FPS", settings.effective_res().short_label(), settings.max_fps),
                col::A(),
            ),
            (
                icons::MEMORY,
                t(K::StatVideoEngine),
                if settings.h265_enabled {
                    t(K::EngineHwBoth).to_string()
                } else {
                    t(K::EngineHwH264).to_string()
                },
                col::B(),
            ),
            (
                icons::BOLT,
                t(K::StatLatency),
                if settings.low_latency {
                    t(K::LatencyUltraShort).to_string()
                } else {
                    t(K::LatencyStandard).to_string()
                },
                col::OK(),
            ),
            (
                icons::VOLUME_UP,
                t(K::StatAudio),
                format!("ALAC & AAC • {} ms", settings.audio_buffer_ms),
                col::GOLD(),
            ),
        ];

        let cw = (top_r.width() - 48.0) * 0.5;
        let ch = 40.0;
        for (i, (ic, k_lbl, val, c_col)) in chips.iter().enumerate() {
            let col_idx = i % 2;
            let row_idx = i / 2;
            let x = top_r.left() + 20.0 + (col_idx as f32) * (cw + 8.0);
            let y = top_r.top() + 118.0 + (row_idx as f32) * (ch + 6.0);
            let cr = Rect::from_min_size(pos2(x, y), vec2(cw, ch));
            d::rrect(p, cr, 10.0, white(0.045));
            d::rrect_stroke(p, cr, 10.0, 1.0, white(0.07));
            d::icon(
                p,
                pos2(cr.left() + 16.0, cr.center().y),
                ic,
                16.0,
                *c_col,
            );
            d::text_fit(
                p,
                pos2(cr.left() + 30.0, cr.top() + 13.0),
                Align2::LEFT_CENTER,
                k_lbl,
                ty::micro(),
                col::TEXT3(),
                cw - 36.0,
            );
            d::text_fit(
                p,
                pos2(cr.left() + 30.0, cr.top() + 27.0),
                Align2::LEFT_CENTER,
                val,
                ty::small(),
                col::TEXT(),
                cw - 36.0,
            );
        }

        // Bottom card: Step-by-Step Connection Guide (height = remaining)
        let bot_r = Rect::from_min_max(pos2(r.min.x, top_r.max.y + 12.0), r.max);
        d::glass(p, bot_r, 18.0, 1.0);

        d::kicker(
            p,
            pos2(bot_r.left() + 20.0, bot_r.top() + 22.0),
            Align2::LEFT_CENTER,
            t(K::GuideKicker),
            col::TEXT2(),
        );

        let apple_desc = t(K::GuideAppleDesc);
        let steps = [
            (
                "1",
                "Apple (iPhone / iPad / Mac) — AirPlay 2",
                apple_desc,
            ),
            (
                "2",
                t(K::GuideAndroidTitle),
                t(K::GuideAndroidDesc),
            ),
            (
                "3",
                t(K::GuideWindowsTitle),
                t(K::GuideWindowsDesc),
            ),
        ];

        for (i, (num, title, desc)) in steps.iter().enumerate() {
            let sy = bot_r.top() + 44.0 + (i as f32) * 52.0;
            let badge_c = pos2(bot_r.left() + 32.0, sy + 16.0);
            p.circle_filled(badge_c, 12.0, alpha(col::A(), 0.16));
            p.circle_stroke(badge_c, 12.0, Stroke::new(1.0, alpha(col::A(), 0.45)));
            d::text(
                p,
                badge_c,
                Align2::CENTER_CENTER,
                num,
                ty::num_b(12.0),
                col::A(),
            );
            d::text_fit(
                p,
                pos2(bot_r.left() + 54.0, sy + 8.0),
                Align2::LEFT_CENTER,
                title,
                ty::title_s(),
                col::TEXT(),
                bot_r.width() - 68.0,
            );
            d::text_fit(
                p,
                pos2(bot_r.left() + 54.0, sy + 26.0),
                Align2::LEFT_CENTER,
                desc,
                ty::caption(),
                col::TEXT2(),
                bot_r.width() - 68.0,
            );
        }
        let _ = now;
    }

    fn draw_audio_player_card(
        &mut self,
        p: &Painter,
        r: Rect,
        settings: &Settings,
        snap: &LiveSnapshot,
        now: f64,
    ) {
        d::glass(p, r, 18.0, 1.0);
        let top_band = Rect::from_min_max(r.min, pos2(r.max.x, r.min.y + 140.0));
        d::accent_fill(p, top_band, 18.0, 0.15);

        // Cover Art or Musical Emblem (116x116)
        let art_r = Rect::from_min_size(pos2(r.left() + 22.0, r.top() + 22.0), vec2(116.0, 116.0));
        d::shadow(p, art_r, 14.0, 0.7);
        if let Some(ref cov) = snap.cover {
            let need_upload = self.cover_tex.as_ref().map_or(true, |(g, _)| *g != cov.gen);
            if need_upload && cov.width > 0 && cov.height > 0 {
                let img = ColorImage::from_rgba_unmultiplied(
                    [cov.width as usize, cov.height as usize],
                    &cov.rgba,
                );
                let tex = self
                    .ctx
                    .load_texture("cover_art", img, TextureOptions::LINEAR);
                self.cover_tex = Some((cov.gen, tex));
            }
        }
        if let Some((_, ref tex)) = self.cover_tex {
            let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            d::image(p, art_r, tex.id(), uv, 14.0, Color32::WHITE);
        } else {
            d::accent_fill(p, art_r, 14.0, 0.85);
            d::icon_fill(p, art_r.center(), icons::VOLUME_UP, 46.0, Color32::WHITE);
        }
        d::rrect_stroke(p, art_r, 14.0, 1.0, white(0.18));

        // Sender & Codec Pill
        let tx = art_r.right() + 18.0;
        let max_tw = r.right() - tx - 18.0;
        let sender_lbl = if snap.client.name.is_empty() {
            t(K::AudioKicker).to_string()
        } else {
            format!(
                "{} • {}",
                i18n::upper(&snap.client.name),
                if snap.audio_codec_label.is_empty() {
                    "ALAC"
                } else {
                    &snap.audio_codec_label
                }
            )
        };
        d::kicker(
            p,
            pos2(tx, art_r.top() + 14.0),
            Align2::LEFT_CENTER,
            &sender_lbl,
            col::A(),
        );

        let title = if snap.meta.title.is_empty() {
            &settings.device_name
        } else {
            &snap.meta.title
        };
        let artist = if snap.meta.artist.is_empty() {
            t(K::LiveAudio)
        } else {
            &snap.meta.artist
        };
        let album = &snap.meta.album;

        d::text_fit(
            p,
            pos2(tx, art_r.top() + 42.0),
            Align2::LEFT_CENTER,
            title,
            ty::h2(),
            col::TEXT(),
            max_tw,
        );
        d::text_fit(
            p,
            pos2(tx, art_r.top() + 68.0),
            Align2::LEFT_CENTER,
            artist,
            ty::title_s(),
            col::TEXT2(),
            max_tw,
        );
        if !album.is_empty() {
            d::text_fit(
                p,
                pos2(tx, art_r.top() + 90.0),
                Align2::LEFT_CENTER,
                album,
                ty::small_r(),
                col::TEXT3(),
                max_tw,
            );
        }

        // Progress Bar
        let bar_y = art_r.bottom() + 34.0;
        let bar_r = Rect::from_min_max(pos2(r.left() + 22.0, bar_y), pos2(r.right() - 22.0, bar_y + 6.0));
        d::rrect(p, bar_r, 3.0, white(0.10));
        let frac = if snap.progress_duration_s > 0.5 {
            (snap.progress_elapsed_s / snap.progress_duration_s).clamp(0.0, 1.0) as f32
        } else {
            ((now * 0.15).fract()) as f32
        };
        let fill_r = Rect::from_min_max(
            bar_r.min,
            pos2(bar_r.left() + bar_r.width() * frac, bar_r.max.y),
        );
        d::accent_fill(p, fill_r, 3.0, 1.0);

        if snap.progress_duration_s > 0.5 {
            d::text(
                p,
                pos2(bar_r.left(), bar_r.bottom() + 14.0),
                Align2::LEFT_CENTER,
                &fmt_mm_ss(snap.progress_elapsed_s),
                ty::num(11.5),
                col::TEXT2(),
            );
            d::text(
                p,
                pos2(bar_r.right(), bar_r.bottom() + 14.0),
                Align2::RIGHT_CENTER,
                &fmt_mm_ss(snap.progress_duration_s),
                ty::num(11.5),
                col::TEXT2(),
            );
        }

        // Audio telemetry cards at bottom of audio player card
        let stat_y = bar_r.bottom() + 42.0;
        let stats = [
            (
                t(K::StatAudioCodec),
                if snap.audio_codec_label.is_empty() {
                    "ALAC 44.1 kHz / 16-bit".into()
                } else {
                    format!("{} • 44.1 kHz Stereo", snap.audio_codec_label)
                },
            ),
            (
                t(K::StatVolume),
                if snap.volume_db <= -30.0 {
                    t(K::Muted).into()
                } else {
                    let pct = (((snap.volume_db + 30.0) / 30.0).clamp(0.0, 1.0) * 100.0).round() as i32;
                    format!("{} ({:.1} dB)", tf(K::Percent, &[&pct.to_string()]), snap.volume_db)
                },
            ),
            (
                t(K::StatPackets),
                tf(K::RtpPackets, &[&snap.audio_packets_total.to_string()]),
            ),
            (
                t(K::StatNtp),
                if snap.ntp_rtt_ms > 0.01 {
                    format!("{:.1} ms RTT", snap.ntp_rtt_ms)
                } else {
                    t(K::Locked).into()
                },
            ),
        ];

        let sw = (r.width() - 52.0) * 0.5;
        for (i, (k_lbl, v_lbl)) in stats.iter().enumerate() {
            let cx = r.left() + 22.0 + ((i % 2) as f32) * (sw + 8.0);
            let cy = stat_y + ((i / 2) as f32) * 52.0;
            let sr = Rect::from_min_size(pos2(cx, cy), vec2(sw, 44.0));
            d::rrect(p, sr, 10.0, white(0.05));
            d::text_fit(
                p,
                pos2(sr.left() + 14.0, sr.top() + 14.0),
                Align2::LEFT_CENTER,
                k_lbl,
                ty::micro(),
                col::TEXT3(),
                sw - 24.0,
            );
            d::text_fit(
                p,
                pos2(sr.left() + 14.0, sr.top() + 30.0),
                Align2::LEFT_CENTER,
                v_lbl,
                ty::small(),
                col::TEXT(),
                sw - 24.0,
            );
        }
    }

    fn draw_settings_panel(&mut self, p: &Painter, r: Rect, s: &Settings) {
        let b = crate::billing::snapshot();
        d::glass(p, r, 18.0, 1.0);

        d::icon(
            p,
            pos2(r.left() + 24.0, r.top() + 24.0),
            icons::TUNE,
            18.0,
            col::A(),
        );
        // Title left, hint right; long translations shrink to their share instead of overlapping.
        let hint = t(K::SettingsHint);
        let head_w = r.width() - 60.0;
        let hint_w = d::measure(p, hint, ty::caption()).x.min(head_w * 0.42);
        d::text_fit(
            p,
            pos2(r.left() + 40.0, r.top() + 24.0),
            Align2::LEFT_CENTER,
            t(K::SettingsTitle),
            ty::title(),
            col::TEXT(),
            head_w - hint_w - 12.0,
        );
        d::text_fit(
            p,
            pos2(r.right() - 20.0, r.top() + 24.0),
            Align2::RIGHT_CENTER,
            hint,
            ty::caption(),
            col::TEXT3(),
            hint_w,
        );

        let eff_res = s.effective_res();
        let on_off = |on: bool, on_key: K| -> String { t(if on { on_key } else { K::Off }).into() };
        let rows: Vec<(&str, String, String, bool)> = ROWS
            .iter()
            .map(|row| match row {
                Row::Language => (
                    icons::LANGUAGE,
                    // Always readable for someone stuck in a language they do not know.
                    if i18n::current() == i18n::Lang::En {
                        t(K::RowLanguage).to_string()
                    } else {
                        format!("{} · Language", t(K::RowLanguage))
                    },
                    match s.language {
                        i18n::LangPref::Auto => tf(K::LangDevice, &[i18n::device_lang().native_name()]),
                        i18n::LangPref::Fixed(l) => l.native_name().to_string(),
                    },
                    true,
                ),
                Row::DeviceName => (icons::TV, t(K::RowDeviceName).into(), s.device_name.clone(), true),
                Row::Resolution => (
                    icons::N4K,
                    t(K::RowResolution).into(),
                    eff_res.label().to_string(),
                    eff_res == ResPreset::Uhd4K,
                ),
                Row::Fps => (icons::SPEED, t(K::RowFps).into(), format!("{} FPS", s.max_fps), s.max_fps >= 60),
                Row::Codec => (
                    icons::HIGH_QUALITY,
                    t(K::RowCodec).into(),
                    if s.h265_enabled { "HEVC (H.265) + H.264".to_string() } else { t(K::CodecH264Only).into() },
                    s.h265_enabled,
                ),
                Row::Latency => (
                    icons::BOLT,
                    t(K::RowLatency).into(),
                    t(if s.low_latency { K::LatencyUltra } else { K::LatencyStandard }).into(),
                    s.low_latency,
                ),
                Row::Color => (
                    icons::HDR_ON,
                    t(K::RowColor).into(),
                    t(if s.enforce_sdr { K::ColorForceSdr } else { K::Auto }).into(),
                    s.enforce_sdr,
                ),
                Row::Scaling => (
                    icons::ASPECT_RATIO,
                    t(K::RowScaling).into(),
                    t(if s.stretch_fill { K::ScaleFill } else { K::ScaleFit }).into(),
                    !s.stretch_fill,
                ),
                Row::AudioBuffer => (
                    icons::VOLUME_UP,
                    t(K::RowAudioBuffer).into(),
                    format!("{} ms", s.audio_buffer_ms),
                    s.audio_buffer_ms <= 40,
                ),
                Row::Pin => (icons::LOCK, t(K::RowPin).into(), on_off(s.require_pin, K::PinOn), s.require_pin),
                Row::AutoWake => (icons::AUTOPLAY, t(K::RowAutoWake).into(), on_off(s.auto_wake, K::AutoWakeOn), s.auto_wake),
                Row::Pro => (
                    icons::STAR,
                    t(K::RowProHighlight).to_string(),
                    if b.pro {
                        t(K::ProActive).to_string()
                    } else if !b.yearly_price.is_empty() {
                        tf(K::ProPerYear, &[&b.yearly_price])
                    } else {
                        t(K::ProFree).to_string()
                    },
                    b.pro,
                ),
                Row::About => (
                    icons::INFO,
                    t(K::RowAbout).into(),
                    format!("v{}  ·  GPL-3.0", env!("CARGO_PKG_VERSION")),
                    false,
                ),
            })
            .collect();

        let top_y = r.top() + 46.0;
        // 12 rows share the panel height (capped at the original 35.5 px).
        let row_h = ((r.bottom() - 8.0 - top_y) / NUM_SETTINGS as f32).min(35.5);
        for (i, (ic, label, val, active_accent)) in rows.iter().enumerate() {
            let focused = self.sel_row == i;
            let f = self.an.toggle(k(("set_row", i)), focused, motion::FOCUS);
            let ry = top_y + (i as f32) * row_h;
            let row_r = Rect::from_min_size(
                pos2(r.left() + 14.0, ry),
                vec2(r.width() - 28.0, row_h - 3.5),
            );

            let is_pro_row = ROWS[i] == Row::Pro;
            if is_pro_row {
                // Distinct prominent VIP styling for Ayna Pro row at top
                if f > 0.01 {
                    d::soft(
                        p,
                        row_r.expand(3.0),
                        10.0,
                        16.0,
                        alpha(rgb(0xF5, 0x9E, 0x0B), 0.45 * f),
                    );
                }
                let bg = if f > 0.5 {
                    Color32::WHITE
                } else if b.pro {
                    alpha(rgb(0x10, 0xB9, 0x81), 0.20)
                } else {
                    alpha(rgb(0xF5, 0x9E, 0x0B), 0.18)
                };
                d::rrect(p, row_r, 8.0, bg);
                if f <= 0.5 {
                    let stroke_col = if b.pro {
                        alpha(rgb(0x10, 0xB9, 0x81), 0.70)
                    } else {
                        alpha(rgb(0xF5, 0x9E, 0x0B), 0.75)
                    };
                    d::rrect_stroke(p, row_r, 8.0, 1.5, stroke_col);
                }
            } else {
                if f > 0.01 {
                    d::soft(
                        p,
                        row_r.expand(2.0),
                        9.0,
                        12.0,
                        alpha(col::A(), 0.25 * f),
                    );
                }
                let bg = mix(
                    if i % 2 == 0 {
                        white(0.035)
                    } else {
                        white(0.015)
                    },
                    Color32::WHITE,
                    f,
                );
                d::rrect(p, row_r, 8.0, bg);
            }

            let (fg, ic_col, val_col) = if is_pro_row && f <= 0.5 {
                let gold = if b.pro { rgb(0x10, 0xB9, 0x81) } else { rgb(0xFB, 0xBF, 0x24) };
                (col::TEXT(), gold, gold)
            } else {
                let fg = mix(col::TEXT(), col::BG(), f);
                let ic_col = if f > 0.5 {
                    col::BG()
                } else if *active_accent {
                    col::A()
                } else {
                    col::TEXT2()
                };
                let val_col = if f > 0.5 {
                    col::BG()
                } else if *active_accent {
                    col::A()
                } else {
                    col::TEXT2()
                };
                (fg, ic_col, val_col)
            };

            d::icon(
                p,
                pos2(row_r.left() + 18.0, row_r.center().y),
                ic,
                16.0,
                ic_col,
            );
            let val_str = if focused {
                format!("‹  {val}  ›")
            } else {
                val.clone()
            };
            // Value keeps up to 58 % of the row; the label gets the rest (both ellipsize).
            let avail = row_r.width() - 36.0 - 12.0;
            let val_w = d::measure(p, &val_str, ty::small()).x.min(avail * 0.58);
            d::text_fit(
                p,
                pos2(row_r.left() + 36.0, row_r.center().y),
                Align2::LEFT_CENTER,
                label,
                ty::body_m(),
                fg,
                (avail - val_w - 10.0).max(60.0),
            );
            d::text_fit(
                p,
                pos2(row_r.right() - 12.0, row_r.center().y),
                Align2::RIGHT_CENTER,
                &val_str,
                ty::small(),
                val_col,
                val_w,
            );
        }
    }

    fn draw_mirror_hud(&mut self, p: &Painter, snap: &LiveSnapshot, _now: f64) {
        let hud_r = Rect::from_min_max(pos2(80.0, H - 118.0), pos2(W - 80.0, H - 24.0));
        d::glass(p, hud_r, 18.0, 0.96);

        // Top row of HUD: Sender info + Live codec/FPS/bitrate/latency pills
        let sender = if snap.client.name.is_empty() {
            t(K::HudMirroring).to_string()
        } else if snap.client.model.is_empty() {
            snap.client.name.clone()
        } else {
            format!("{} ({})", snap.client.name, snap.client.model)
        };

        let dot_c = pos2(hud_r.left() + 24.0, hud_r.top() + 24.0);
        let dot_col = if snap.paused { col::WARN() } else { col::OK() };
        p.circle_filled(dot_c, 5.0, dot_col);
        d::text(
            p,
            pos2(hud_r.left() + 36.0, hud_r.top() + 24.0),
            Align2::LEFT_CENTER,
            &sender,
            ty::title(),
            col::TEXT(),
        );

        let codec_lbl = snap.video_codec.map(|c| c.label()).unwrap_or("HEVC");
        // Only measured values: a part is left out until it is known (no made-up numbers).
        let mut parts: Vec<String> = Vec::with_capacity(5);
        if snap.video_width > 0 && snap.video_height > 0 {
            parts.push(format!("{}×{}", snap.video_width, snap.video_height));
        }
        parts.push(codec_lbl.to_string());
        if snap.fps > 1.0 {
            parts.push(format!("{:.0} FPS", snap.fps));
        }
        if snap.bitrate_mbps > 0.05 {
            parts.push(format!("{:.1} Mbps", snap.bitrate_mbps));
        }
        if snap.decode_ms > 0.1 {
            parts.push(format!("{} {:.1} ms", t(K::Decoder), snap.decode_ms));
        }
        let stats_str = parts.join("  •  ");
        d::text(
            p,
            pos2(hud_r.right() - 22.0, hud_r.top() + 24.0),
            Align2::RIGHT_CENTER,
            &stats_str,
            ty::num(12.5),
            col::A(),
        );

        // Bottom row of HUD: 4 interactive action pills
        let stretch = self.engine.settings().stretch_fill;
        // The aspect-ratio icon already says what the button is about; the label is the mode.
        let btns = [
            (icons::ASPECT_RATIO, t(if stretch { K::ScaleFill } else { K::ScaleFitShort })),
            (icons::CLOSE, t(K::Disconnect)),
            (icons::TUNE, t(K::HudSettings)),
            (icons::VISIBILITY_OFF, t(K::HudHide)),
        ];

        let bw = (hud_r.width() - 44.0 - 30.0) / 4.0;
        for (i, (ic, lbl)) in btns.iter().enumerate() {
            let focused = self.hud_btn == i;
            let f = self.an.toggle(k(("hud_btn", i)), focused, motion::FOCUS);
            let bx = hud_r.left() + 22.0 + (i as f32) * (bw + 10.0);
            let br = Rect::from_min_size(pos2(bx, hud_r.top() + 46.0), vec2(bw, 34.0));
            d::button(p, br, Some(ic), lbl, f, d::Btn::Normal);
        }
    }

    /// Ayna Pro sheet: benefits, yearly ($10/yr) + lifetime ($20) with the localized prices
    /// Google Play returns, restore, and the subscription terms Play policy asks for.
    fn draw_upgrade_modal(&mut self, p: &Painter, _now: f64) {
        let full = Rect::from_min_size(pos2(0.0, 0.0), vec2(W, H));
        d::rrect(p, full, 0.0, black(0.78));

        let b = crate::billing::snapshot();
        let btns = Self::upgrade_buttons();
        if self.upgrade_sel >= btns.len() {
            self.upgrade_sel = 0;
        }
        let (btn_h, gap, head) = (40.0, 8.0, 116.0);
        let foot = if b.pro { 20.0 } else { 56.0 };
        let mh = head + btns.len() as f32 * (btn_h + gap) - gap + foot;
        let mr = Rect::from_center_size(full.center(), vec2(560.0, mh));
        d::rrect(p, mr, 22.0, alpha(col::BG(), 0.97)); // opaque: nothing shows through the sheet
        d::glass(p, mr, 22.0, 1.0);
        d::rrect_stroke(p, mr, 22.0, 1.5, alpha(col::A(), 0.55));

        let cx = mr.center().x;
        let inner_w = mr.width() - 48.0;
        d::icon_fill(p, pos2(cx, mr.top() + 26.0), icons::STAR, 22.0, col::GOLD());
        d::kicker(p, pos2(cx, mr.top() + 50.0), Align2::CENTER_CENTER, "AYNA PRO", col::A());
        let (title, sub) = if b.pro {
            (t(K::UpAlreadyPro), "")
        } else {
            (t(K::UpTitle), t(K::UpBenefits))
        };
        d::text_fit(p, pos2(cx, mr.top() + 76.0), Align2::CENTER_CENTER, title, ty::title(), col::TEXT(), inner_w);
        if !sub.is_empty() {
            d::text_fit(p, pos2(cx, mr.top() + 98.0), Align2::CENTER_CENTER, sub, ty::small(), col::TEXT2(), inner_w);
        }

        let mut y = mr.top() + head;
        for (i, btn) in btns.iter().enumerate() {
            let focused = self.upgrade_sel == i;
            let f = self.an.toggle(k(("up_btn", i)), focused, motion::FOCUS);
            let br = Rect::from_min_size(pos2(mr.left() + 24.0, y), vec2(inner_w, btn_h));
            if f > 0.01 {
                d::soft(p, br.expand(2.0), 12.0, 14.0, alpha(col::A(), 0.3 * f));
            }
            d::rrect(p, br, 12.0, mix(white(0.06), Color32::WHITE, f));
            let fg = mix(col::TEXT(), col::BG(), f);
            let price_or_loading = |price: &str, per_year: bool| -> String {
                if price.is_empty() {
                    t(K::UpPriceLoading).into()
                } else if per_year {
                    tf(K::ProPerYear, &[price])
                } else {
                    price.to_string()
                }
            };
            let (label, price) = match btn {
                UpBtn::Yearly => (t(K::UpYearly), price_or_loading(&b.yearly_price, true)),
                UpBtn::Lifetime => (t(K::UpLifetime), price_or_loading(&b.lifetime_price, false)),
                UpBtn::Restore => (t(K::UpRestore), String::new()),
                UpBtn::Manage => (t(K::UpManage), String::new()),
                UpBtn::Close => (t(K::UpClose), String::new()),
            };
            let price_w = if price.is_empty() {
                0.0
            } else {
                d::measure(p, &price, ty::body_m()).x.min(inner_w * 0.45)
            };
            d::text_fit(
                p,
                pos2(br.left() + 18.0, br.center().y),
                Align2::LEFT_CENTER,
                label,
                ty::body_m(),
                fg,
                inner_w - 36.0 - price_w - 12.0,
            );
            if !price.is_empty() {
                let pc = if f > 0.5 { col::BG() } else { col::A() };
                d::text_fit(
                    p,
                    pos2(br.right() - 18.0, br.center().y),
                    Align2::RIGHT_CENTER,
                    &price,
                    ty::body_m(),
                    pc,
                    price_w,
                );
            }
            y += btn_h + gap;
        }
        if !b.pro {
            d::paragraph(p, pos2(mr.left() + 24.0, y + 4.0), t(K::UpTerms), ty::caption(), col::TEXT3(), inner_w, 2);
        }
    }

    /// About: version, GPL-3.0 notice + source link (written offer), credits, trademarks.
    fn draw_about_modal(&mut self, p: &Painter) {
        let full = Rect::from_min_size(pos2(0.0, 0.0), vec2(W, H));
        d::rrect(p, full, 0.0, black(0.78));
        let mr = Rect::from_center_size(full.center(), vec2(620.0, 360.0));
        d::rrect(p, mr, 22.0, alpha(col::BG(), 0.97));
        d::glass(p, mr, 22.0, 1.0);
        d::rrect_stroke(p, mr, 22.0, 1.5, alpha(col::A(), 0.55));

        let logo_r = Rect::from_min_size(pos2(mr.left() + 28.0, mr.top() + 26.0), vec2(44.0, 44.0));
        d::accent_fill(p, logo_r, 12.0, 1.0);
        draw_yansit_emblem(p, logo_r.center(), 22.0, Color32::WHITE);
        d::text(p, pos2(logo_r.right() + 14.0, logo_r.center().y - 9.0), Align2::LEFT_CENTER, "Ayna", ty::h1(), col::TEXT());
        d::text(
            p,
            pos2(logo_r.right() + 14.0, logo_r.center().y + 14.0),
            Align2::LEFT_CENTER,
            &format!("v{}  ·  GPL-3.0-or-later", env!("CARGO_PKG_VERSION")),
            ty::caption(),
            col::TEXT2(),
        );

        let source = match SOURCE_URL {
            Some(url) => tf(K::AboutSource, &[url]),
            None => t(K::AboutSourceListing).to_string(),
        };
        let inner_w = mr.width() - 56.0;
        let mut y = mr.top() + 92.0;
        for (text, color) in [
            (t(K::AboutLicense).to_string(), col::TEXT()),
            (source, col::A()),
            (t(K::AboutCredits).to_string(), col::TEXT2()),
            (t(K::AboutTrademark).to_string(), col::TEXT3()),
        ] {
            let r = d::paragraph(p, pos2(mr.left() + 28.0, y), &text, ty::small(), color, inner_w, 2);
            y = r.bottom() + 12.0;
        }

        let br = Rect::from_min_size(pos2(mr.center().x - 90.0, mr.bottom() - 58.0), vec2(180.0, 38.0));
        d::button(p, br, Some(icons::CHECK), t(K::UpClose), 1.0, d::Btn::Normal);
    }

    /// One-time neutral rating prompt (see `handle_review_key`).
    fn draw_review_modal(&mut self, p: &Painter, _now: f64) {
        let full = Rect::from_min_size(pos2(0.0, 0.0), vec2(W, H));
        d::rrect(p, full, 0.0, black(0.78));

        let mr = Rect::from_center_size(full.center(), vec2(580.0, 300.0));
        d::rrect(p, mr, 22.0, alpha(col::BG(), 0.97));
        d::glass(p, mr, 22.0, 1.0);
        d::rrect_stroke(p, mr, 22.0, 1.5, alpha(col::A(), 0.55));

        let badge = Rect::from_center_size(pos2(mr.center().x, mr.top() + 48.0), vec2(50.0, 50.0));
        d::rrect(p, badge, 16.0, alpha(col::A(), 0.16));
        d::rrect_stroke(p, badge, 16.0, 1.5, alpha(col::A(), 0.7));
        d::text(p, badge.center(), Align2::CENTER_CENTER, icons::STAR, ty::icon(28.0), col::A());

        d::text_fit(
            p,
            pos2(mr.center().x, mr.top() + 100.0),
            Align2::CENTER_CENTER,
            t(K::ReviewTitle),
            ty::h1(),
            col::TEXT(),
            mr.width() - 48.0,
        );
        d::paragraph(
            p,
            pos2(mr.left() + 36.0, mr.top() + 126.0),
            t(K::ReviewDesc),
            ty::small(),
            col::TEXT2(),
            mr.width() - 72.0,
            3,
        );

        let (btn_w, btn_h, gap) = (230.0, 42.0, 20.0);
        let btn_y = mr.bottom() - 64.0;
        let r0 = Rect::from_min_size(pos2(mr.center().x - btn_w - gap * 0.5, btn_y), vec2(btn_w, btn_h));
        let r1 = Rect::from_min_size(pos2(mr.center().x + gap * 0.5, btn_y), vec2(btn_w, btn_h));
        let f0 = self.an.toggle(k("rev_btn_0"), self.review_focus == 0, motion::FOCUS);
        let f1 = self.an.toggle(k("rev_btn_1"), self.review_focus == 1, motion::FOCUS);
        d::button(p, r0, Some(icons::STAR), t(K::ReviewRate), f0, d::Btn::Primary);
        d::button(p, r1, None, t(K::ReviewLater), f1, d::Btn::Normal);
    }

    fn draw_pin_modal(&mut self, p: &Painter, pin: &str, _now: f64) {
        let full = Rect::from_min_size(pos2(0.0, 0.0), vec2(W, H));
        d::rrect(p, full, 0.0, black(0.75));

        let mr = Rect::from_center_size(full.center(), vec2(420.0, 230.0));
        d::rrect(p, mr, 22.0, alpha(col::BG(), 0.97));
        d::glass(p, mr, 22.0, 1.0);
        d::rrect_stroke(p, mr, 22.0, 1.5, alpha(col::A(), 0.55));

        d::kicker(
            p,
            pos2(mr.center().x, mr.top() + 32.0),
            Align2::CENTER_CENTER,
            t(K::PinKicker),
            col::A(),
        );
        d::text_fit(
            p,
            pos2(mr.center().x, mr.top() + 58.0),
            Align2::CENTER_CENTER,
            t(K::PinDesc),
            ty::small(),
            col::TEXT2(),
            mr.width() - 40.0,
        );

        // 4 digit boxes
        let digits: Vec<char> = pin.chars().take(4).collect();
        let box_w = 64.0;
        let gap = 14.0;
        let total_w = 4.0 * box_w + 3.0 * gap;
        let x0 = mr.center().x - total_w * 0.5;
        for i in 0..4 {
            let ch = digits.get(i).copied().unwrap_or('•');
            let br = Rect::from_min_size(pos2(x0 + (i as f32) * (box_w + gap), mr.top() + 88.0), vec2(box_w, 82.0));
            d::rrect(p, br, 14.0, alpha(col::A(), 0.14));
            d::rrect_stroke(p, br, 14.0, 1.5, alpha(col::A(), 0.5));
            d::text(
                p,
                br.center(),
                Align2::CENTER_CENTER,
                &ch.to_string(),
                ty::num_b(44.0),
                col::TEXT(),
            );
        }
    }

    fn draw_toast(&self, p: &Painter, msg: &str, a: f32) {
        let tw = d::measure(p, msg, ty::body_m()).x + 44.0;
        let tr = Rect::from_center_size(pos2(W * 0.5, H - 56.0), vec2(tw, 36.0));
        d::shadow(p, tr, 18.0, 0.7 * a);
        d::rrect(p, tr, 18.0, alpha(col::SURFACE_HI(), 0.96 * a));
        d::rrect_stroke(p, tr, 18.0, 1.0, alpha(col::A(), 0.5 * a));
        d::icon_fill(
            p,
            pos2(tr.left() + 18.0, tr.center().y),
            icons::CHECK_CIRCLE,
            16.0,
            alpha(col::A(), a),
        );
        d::text(
            p,
            pos2(tr.left() + 32.0, tr.center().y),
            Align2::LEFT_CENTER,
            msg,
            ty::body_m(),
            alpha(col::TEXT(), a),
        );
    }
}

/// Custom vector AirPlay receiver emblem (rounded screen frame + upward triangular cast beam).
/// Ayna mark: a TV on a stand with a bold round "a" inside (same as the launcher icon, see
/// tools/make_icons.py). Original artwork: no AirPlay-style triangle, no Cast waves.
pub fn draw_yansit_emblem(p: &Painter, c: Pos2, size: f32, color: Color32) {
    let s = size * 1.15;
    let lw = (s * 0.085).max(1.4);
    let screen = Rect::from_center_size(pos2(c.x, c.y - s * 0.12), vec2(s, s * 0.66));
    d::rrect_stroke(p, screen, s * 0.12, lw, color);
    let stroke = Stroke::new(lw, color);
    let foot_y = screen.bottom() + s * 0.13;
    p.line_segment([pos2(c.x, screen.bottom()), pos2(c.x, foot_y)], stroke);
    p.line_segment([pos2(c.x - s * 0.2, foot_y), pos2(c.x + s * 0.2, foot_y)], stroke);
    // Single-storey "a": ring + stem flush with its right edge, exactly the ring's height
    // (same geometry as tools/make_icons.py).
    let w = lw * 0.95;
    let aw = Stroke::new(w, color);
    let big_r = screen.height() * 0.27;
    let ac = pos2(c.x, screen.top() + screen.height() * 0.5);
    p.circle_stroke(ac, big_r - w * 0.5, aw);
    let sx = ac.x + big_r - w * 0.5;
    p.line_segment([pos2(sx, ac.y - big_r), pos2(sx, ac.y + big_r)], aw);
}

fn fmt_mm_ss(secs: f64) -> String {
    let s = secs.max(0.0).round() as u64;
    format!("{:02}:{:02}", s / 60, s % 60)
}

fn detect_local_ipv4() -> String {
    // A *connected* UDP socket sends no packets; it only makes the kernel pick the egress
    // interface, which reveals this device's own LAN address. 192.0.2.1 is TEST-NET-1
    // (RFC 5737, reserved for documentation), so no real host, gateway or network belonging
    // to the user is assumed or contacted, and nothing device-specific is baked into the app.
    if let Ok(sock) = std::net::UdpSocket::bind("0.0.0.0:0") {
        if sock.connect("192.0.2.1:80").is_ok() {
            if let Ok(addr) = sock.local_addr() {
                let ip = addr.ip().to_string();
                if ip != "0.0.0.0" {
                    return ip;
                }
            }
        }
    }
    "—".into()
}
