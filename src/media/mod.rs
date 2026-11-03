//! Media engine (`MediaEngine`), persistent user settings (`Settings`), and live telemetry
//! snapshot (`LiveSnapshot`) shared between the AirPlay protocol threads, hardware decoders,
//! and the TV UI.

pub mod audio;
pub mod video;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use crate::airplay::clock::monotonic_ns;
use crate::airplay::{
    AudioFormat, AudioPacket, Client, Codec, Config, Metadata, NetStats, Sink, VideoFrame,
};

use self::audio::AudioPlayer;
use self::video::VideoPlayer;

/// Resolution preset advertised to iOS/iPadOS/macOS senders.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResPreset {
    Uhd4K,  // 3840 x 2160
    Fhd1080, // 1920 x 1080
    Hd720,   // 1280 x 720
}

impl ResPreset {
    pub fn dims(self) -> (u32, u32) {
        match self {
            ResPreset::Uhd4K => (3840, 2160),
            ResPreset::Fhd1080 => (1920, 1080),
            ResPreset::Hd720 => (1280, 720),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ResPreset::Uhd4K => "4K UHD (3840×2160)",
            ResPreset::Fhd1080 => "1080p Full HD (1920×1080)",
            ResPreset::Hd720 => "720p HD (1280×720)",
        }
    }

    pub fn short_label(self) -> &'static str {
        match self {
            ResPreset::Uhd4K => "4K UHD",
            ResPreset::Fhd1080 => "1080p",
            ResPreset::Hd720 => "720p",
        }
    }

    pub fn next(self) -> Self {
        match self {
            ResPreset::Uhd4K => ResPreset::Fhd1080,
            ResPreset::Fhd1080 => ResPreset::Hd720,
            ResPreset::Hd720 => ResPreset::Uhd4K,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            ResPreset::Uhd4K => ResPreset::Hd720,
            ResPreset::Fhd1080 => ResPreset::Uhd4K,
            ResPreset::Hd720 => ResPreset::Fhd1080,
        }
    }
}

/// Persistent receiver settings controllable from the TV remote UI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub device_name: String,
    pub res_preset: ResPreset,
    pub max_fps: u32,
    pub h265_enabled: bool,
    pub low_latency: bool,
    pub enforce_sdr: bool,
    pub stretch_fill: bool,
    pub audio_buffer_ms: u32,
    pub require_pin: bool,
    pub auto_wake: bool,
    pub boot_start: bool,
    /// UI language: follow the device locale (default) or a fixed language.
    pub language: crate::i18n::LangPref,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            // First-run AirPlay name in the device language ("Living Room TV", "Salon TV", …).
            device_name: crate::i18n::default_device_name().into(),
            res_preset: ResPreset::Uhd4K,
            max_fps: 60,
            h265_enabled: true,
            low_latency: true,
            enforce_sdr: true,
            stretch_fill: false,
            audio_buffer_ms: 25,
            require_pin: false,
            auto_wake: true,
            boot_start: true,
            language: crate::i18n::LangPref::Auto,
        }
    }
}

impl Settings {
    pub fn load(state_dir: &Path) -> Self {
        let mut s = Self::default();
        let Ok(text) = std::fs::read_to_string(state_dir.join("settings.cfg")) else {
            return s;
        };
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let (k, v) = (k.trim(), v.trim());
            match k {
                "device_name" if !v.is_empty() => s.device_name = v.to_string(),
                "res" => {
                    s.res_preset = match v {
                        "1080p" => ResPreset::Fhd1080,
                        "720p" => ResPreset::Hd720,
                        _ => ResPreset::Uhd4K,
                    }
                }
                "max_fps" => {
                    if let Ok(fps) = v.parse::<u32>() {
                        s.max_fps = if fps <= 30 { 30 } else { 60 };
                    }
                }
                "h265" => s.h265_enabled = v != "0" && v != "false",
                "low_latency" => s.low_latency = v != "0" && v != "false",
                "enforce_sdr" => s.enforce_sdr = v != "0" && v != "false",
                "stretch_fill" => s.stretch_fill = v == "1" || v == "true",
                "audio_buffer_ms" => {
                    if let Ok(ms) = v.parse::<u32>() {
                        s.audio_buffer_ms = ms.clamp(10, 200);
                    }
                }
                "require_pin" => s.require_pin = v == "1" || v == "true",
                "auto_wake" => s.auto_wake = v != "0" && v != "false",
                "boot_start" => s.boot_start = v != "0" && v != "false",
                "lang" => s.language = crate::i18n::LangPref::from_cfg(v),
                _ => {}
            }
        }
        s
    }

    pub fn save(&self, state_dir: &Path) {
        let _ = std::fs::create_dir_all(state_dir);
        let res_str = match self.res_preset {
            ResPreset::Uhd4K => "4k",
            ResPreset::Fhd1080 => "1080p",
            ResPreset::Hd720 => "720p",
        };
        let content = format!(
            "device_name={}\nres={}\nmax_fps={}\nh265={}\nlow_latency={}\nenforce_sdr={}\nstretch_fill={}\naudio_buffer_ms={}\nrequire_pin={}\nauto_wake={}\nboot_start={}\nlang={}\n",
            self.device_name,
            res_str,
            self.max_fps,
            self.h265_enabled,
            self.low_latency,
            self.enforce_sdr,
            self.stretch_fill,
            self.audio_buffer_ms,
            self.require_pin,
            self.auto_wake,
            self.boot_start,
            self.language.to_cfg(),
        );
        let _ = std::fs::write(state_dir.join("settings.cfg"), content);
    }

    /// Resolution the receiver really advertises: the free tier is capped at 1080p, 4K is a
    /// Ayna Pro feature (docs/MONETIZATION.md). The stored preset is kept, so buying Pro
    /// restores 4K without the user re-selecting it.
    pub fn effective_res(&self) -> ResPreset {
        if self.res_preset == ResPreset::Uhd4K && !crate::billing::is_pro() {
            ResPreset::Fhd1080
        } else {
            self.res_preset
        }
    }

    /// True when a change between `self` and `other` alters what senders see over mDNS/RTSP
    /// (name, display size, codecs, PIN). UI-only changes (language, colours, scaling, audio
    /// buffer…) must not re-register the services, or the TV blinks out of iPhone lists.
    pub fn airplay_differs(&self, other: &Settings) -> bool {
        self.device_name != other.device_name
            || self.res_preset != other.res_preset
            || self.max_fps != other.max_fps
            || self.h265_enabled != other.h265_enabled
            || self.require_pin != other.require_pin
    }

    pub fn to_airplay_config(&self, state_dir: &Path, device_id: [u8; 6], port: u16) -> Config {
        let (width, height) = self.effective_res().dims();
        Config {
            name: self.device_name.clone(),
            device_id,
            port,
            width,
            height,
            refresh_hz: 60,
            max_fps: self.max_fps,
            overscanned: false,
            h265: self.h265_enabled,
            alac: true,
            aac: true,
            require_pin: self.require_pin,
            audio_latency_ms: 250,
            state_dir: state_dir.to_path_buf(),
        }
    }
}

// Receiver-name presets are localized: see crate::i18n::device_name_presets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum StreamMode {
    #[default]
    Idle,
    Connecting,
    Mirroring,
    AudioOnly,
}

#[derive(Clone, Default)]
pub struct CoverArt {
    pub gen: u64,
    pub width: u32,
    pub height: u32,
    pub rgba: Arc<Vec<u8>>,
}

#[derive(Clone, Debug)]
pub struct DlnaState {
    pub current_uri: String,
    pub current_meta_xml: String,
    pub title: String,
    pub artist: String,
    pub transport_state: String,
    pub position_ms: u64,
    pub duration_ms: u64,
    pub volume_percent: u32,
    pub muted: bool,
    pub is_video: bool,
}

impl Default for DlnaState {
    fn default() -> Self {
        Self {
            current_uri: String::new(),
            current_meta_xml: String::new(),
            title: String::new(),
            artist: String::new(),
            transport_state: "STOPPED".to_string(),
            position_ms: 0,
            duration_ms: 0,
            volume_percent: 85,
            muted: false,
            is_video: false,
        }
    }
}

#[derive(Clone, Default)]
pub struct LiveSnapshot {
    pub mode: StreamMode,
    pub client: Client,
    pub video_codec: Option<Codec>,
    pub audio_codec_label: String,
    pub video_width: u32,
    pub video_height: u32,
    pub fps: f32,
    pub bitrate_mbps: f32,
    pub decode_ms: f32,
    pub ntp_rtt_ms: f32,
    pub frames_total: u64,
    pub audio_packets_total: u64,
    pub paused: bool,
    pub volume_db: f32,
    pub meta: Metadata,
    pub cover: Option<CoverArt>,
    pub progress_elapsed_s: f64,
    pub progress_duration_s: f64,
    pub active_pin: String,
}

struct RollingWindow {
    window_start_ns: u64,
    frames_in_win: u32,
    bytes_in_win: u64,
    fps: f32,
    bitrate_mbps: f32,
}

impl Default for RollingWindow {
    fn default() -> Self {
        Self {
            window_start_ns: monotonic_ns(),
            frames_in_win: 0,
            bytes_in_win: 0,
            fps: 0.0,
            bitrate_mbps: 0.0,
        }
    }
}

pub struct MediaEngine {
    state_dir: PathBuf,
    device_id: [u8; 6],
    settings: RwLock<Settings>,
    snap: Mutex<LiveSnapshot>,
    rolling: Mutex<RollingWindow>,
    dlna: Mutex<DlnaState>,
    video: VideoPlayer,
    audio: AudioPlayer,
    cover_gen: AtomicU64,
    wake_seq: AtomicU64,
    wake_seen: AtomicU64,
    ui_visible: AtomicBool,
    reconfig_dirty: AtomicBool,
    video_dims_packed: AtomicU64,
    decode_us: AtomicU32,
}

impl MediaEngine {
    pub fn new(state_dir: PathBuf) -> Arc<Self> {
        let device_id = load_or_create_device_id(&state_dir);
        let settings = Settings::load(&state_dir);
        let low_lat = settings.low_latency;
        let sdr = settings.enforce_sdr;
        let aud_buf = settings.audio_buffer_ms;

        Arc::new(Self {
            state_dir,
            device_id,
            settings: RwLock::new(settings),
            snap: Mutex::new(LiveSnapshot::default()),
            rolling: Mutex::new(RollingWindow::default()),
            dlna: Mutex::new(DlnaState::default()),
            video: VideoPlayer::new(low_lat, sdr),
            audio: AudioPlayer::new(aud_buf),
            cover_gen: AtomicU64::new(0),
            wake_seq: AtomicU64::new(0),
            wake_seen: AtomicU64::new(0),
            ui_visible: AtomicBool::new(false),
            reconfig_dirty: AtomicBool::new(false),
            video_dims_packed: AtomicU64::new(0),
            decode_us: AtomicU32::new(0),
        })
    }

    pub fn state_dir(&self) -> &Path {
        &self.state_dir
    }

    pub fn device_id(&self) -> [u8; 6] {
        self.device_id
    }

    pub fn settings(&self) -> Settings {
        self.settings.read().unwrap().clone()
    }

    /// Keep a built-in receiver name in the UI language ("Salon TV" -> "Living Room TV" after a
    /// switch to English). Custom names are never touched. Re-advertises AirPlay when it changes.
    pub fn localize_device_name(&self, lang: crate::i18n::Lang) {
        let mut s = self.settings();
        if let Some(name) = crate::i18n::localize_preset(&s.device_name, lang) {
            log::info!("receiver name '{}' -> '{}' ({})", s.device_name, name, lang.code());
            s.device_name = name.to_string();
            self.update_settings(s);
        }
    }

    pub fn update_settings(&self, new_settings: Settings) {
        new_settings.save(&self.state_dir);
        self.video
            .set_options(new_settings.low_latency, new_settings.enforce_sdr);
        self.audio.set_buffer_ms(new_settings.audio_buffer_ms);
        let airplay_changed = self.settings.read().unwrap().airplay_differs(&new_settings);
        *self.settings.write().unwrap() = new_settings;
        if airplay_changed {
            self.reconfig_dirty.store(true, Ordering::SeqCst);
        }
    }

    pub fn take_reconfig_dirty(&self) -> bool {
        self.reconfig_dirty.swap(false, Ordering::SeqCst)
    }

    pub fn snapshot(&self) -> LiveSnapshot {
        let mut s = self.snap.lock().unwrap().clone();
        let dec_us = self.decode_us.load(Ordering::Relaxed);
        if dec_us > 0 {
            s.decode_ms = (dec_us as f32) / 1000.0;
        }
        // Decay FPS & bitrate to 0 if no frames arrived in the last 1.8s
        let now = monotonic_ns();
        let mut rw = self.rolling.lock().unwrap();
        if now.saturating_sub(rw.window_start_ns) > 1_800_000_000 && rw.frames_in_win == 0 {
            rw.fps = 0.0;
            rw.bitrate_mbps = 0.0;
        }
        s.fps = rw.fps;
        s.bitrate_mbps = rw.bitrate_mbps;
        s
    }

    pub fn is_active(&self) -> bool {
        let s = self.snap.lock().unwrap();
        !matches!(s.mode, StreamMode::Idle) || !s.active_pin.is_empty()
    }

    pub fn set_ui_visible(&self, visible: bool) {
        self.ui_visible.store(visible, Ordering::SeqCst);
        if visible {
            let cur = self.wake_seq.load(Ordering::SeqCst);
            self.wake_seen.store(cur, Ordering::SeqCst);
        }
    }

    /// Returns `true` once per new incoming connection when `auto_wake` is enabled and
    /// `MainActivity` is not already visible in the foreground.
    pub fn poll_wake_request(&self) -> bool {
        if !self.settings.read().unwrap().auto_wake {
            return false;
        }
        let cur = self.wake_seq.load(Ordering::SeqCst);
        let prev = self.wake_seen.swap(cur, Ordering::SeqCst);
        cur > prev && !self.ui_visible.load(Ordering::SeqCst)
    }

    /// Current video layout dimensions `(width << 32) | height` (or `0` when stretch-to-fill is on
    /// or no video is active).
    pub fn video_layout_dims(&self) -> u64 {
        if self.settings.read().unwrap().stretch_fill {
            return 0;
        }
        self.video_dims_packed.load(Ordering::Relaxed)
    }

    pub fn video_player(&self) -> &VideoPlayer {
        &self.video
    }

    pub fn dlna_state(&self) -> DlnaState {
        self.dlna.lock().unwrap().clone()
    }

    pub fn dlna_set_uri(
        &self,
        uri: String,
        meta_xml: String,
        title: String,
        artist: String,
        is_video: bool,
        sender_addr: String,
    ) {
        log::info!(
            "DLNA SetAVTransportURI: '{}' (title='{}', video={}) from {}",
            uri,
            title,
            is_video,
            sender_addr
        );
        self.video.stop();
        self.audio.stop();
        {
            let mut d = self.dlna.lock().unwrap();
            d.current_uri = uri;
            d.current_meta_xml = meta_xml;
            d.title = title.clone();
            d.artist = artist.clone();
            d.is_video = is_video;
            d.position_ms = 0;
            d.duration_ms = 0;
            d.transport_state = "PLAYING".to_string();
        }
        {
            let mut s = self.snap.lock().unwrap();
            s.mode = if is_video {
                StreamMode::Mirroring
            } else {
                StreamMode::AudioOnly
            };
            s.client = Client {
                name: if title.is_empty() {
                    "DLNA Medya".to_string()
                } else {
                    title.clone()
                },
                model: "DLNA / UPnP".to_string(),
                device_id: String::new(),
                addr: sender_addr,
                dacp_id: String::new(),
                active_remote: String::new(),
            };
            s.video_codec = if is_video { Some(Codec::H264) } else { None };
            s.audio_codec_label = "DLNA".to_string();
            s.paused = false;
            s.meta = Metadata {
                title: if title.is_empty() {
                    crate::i18n::t(crate::i18n::K::DlnaStream).to_string()
                } else {
                    title
                },
                artist: if artist.is_empty() {
                    "UPnP / DLNA".to_string()
                } else {
                    artist
                },
                album: String::new(),
            };
            s.progress_elapsed_s = 0.0;
            s.progress_duration_s = 0.0;
        }
        self.wake_seq.fetch_add(1, Ordering::SeqCst);
    }

    pub fn dlna_play(&self) {
        self.dlna.lock().unwrap().transport_state = "PLAYING".to_string();
        crate::dlna::player::set_paused(false);
        self.snap.lock().unwrap().paused = false;
    }

    pub fn dlna_pause(&self) {
        self.dlna.lock().unwrap().transport_state = "PAUSED_PLAYBACK".to_string();
        crate::dlna::player::set_paused(true);
        self.snap.lock().unwrap().paused = true;
    }

    pub fn dlna_stop(&self) {
        crate::dlna::player::stop();
        {
            let mut d = self.dlna.lock().unwrap();
            d.transport_state = "STOPPED".to_string();
            d.position_ms = 0;
        }
        self.video.stop();
        self.audio.stop();
        self.video_dims_packed.store(0, Ordering::Relaxed);
        let mut s = self.snap.lock().unwrap();
        if s.client.model == "DLNA / UPnP" {
            *s = LiveSnapshot::default();
        }
    }

    pub fn dlna_seek(&self, target_ms: u64) {
        self.dlna.lock().unwrap().position_ms = target_ms;
        crate::dlna::player::seek_ms(target_ms);
    }

    pub fn dlna_set_volume(&self, vol_percent: u32) {
        let v = vol_percent.min(100);
        self.dlna.lock().unwrap().volume_percent = v;
        let db = if v == 0 {
            -144.0
        } else {
            ((v as f32 / 100.0) - 1.0) * 30.0
        };
        self.audio.set_volume_db(db);
    }

    pub fn dlna_set_mute(&self, muted: bool) {
        let v = {
            let mut d = self.dlna.lock().unwrap();
            d.muted = muted;
            if muted { 0 } else { d.volume_percent }
        };
        let db = if v == 0 {
            -144.0
        } else {
            ((v as f32 / 100.0) - 1.0) * 30.0
        };
        self.audio.set_volume_db(db);
    }

    pub fn update_dlna_status(
        &self,
        pos_ms: u64,
        dur_ms: u64,
        playing: bool,
        video_w: u32,
        video_h: u32,
    ) {
        {
            let mut d = self.dlna.lock().unwrap();
            d.position_ms = pos_ms;
            if dur_ms > 0 {
                d.duration_ms = dur_ms;
            }
            if playing {
                d.transport_state = "PLAYING".to_string();
            } else if d.transport_state == "PLAYING" {
                d.transport_state = "PAUSED_PLAYBACK".to_string();
            }
        }
        let mut s = self.snap.lock().unwrap();
        if s.client.model == "DLNA / UPnP" {
            s.progress_elapsed_s = (pos_ms as f64) / 1000.0;
            if dur_ms > 0 {
                s.progress_duration_s = (dur_ms as f64) / 1000.0;
            }
            s.paused = !playing;
            if video_w > 0 && video_h > 0 {
                s.video_width = video_w;
                s.video_height = video_h;
                self.video_dims_packed.store(
                    ((video_w as u64) << 32) | (video_h as u64),
                    Ordering::Relaxed,
                );
            }
        }
    }
}

impl Sink for MediaEngine {
    fn session_start(&self, client: &Client) {
        log::info!(
            "Session started: '{}' ({}) @ {}",
            client.name,
            client.model,
            client.addr
        );
        crate::dlna::player::stop();
        {
            let mut d = self.dlna.lock().unwrap();
            if d.transport_state != "STOPPED" {
                d.transport_state = "STOPPED".to_string();
            }
        }
        {
            let mut s = self.snap.lock().unwrap();
            s.mode = StreamMode::Connecting;
            s.client = client.clone();
            s.paused = false;
            s.frames_total = 0;
            s.audio_packets_total = 0;
            s.meta = Metadata::default();
            s.cover = None;
            s.progress_elapsed_s = 0.0;
            s.progress_duration_s = 0.0;
        }
        self.wake_seq.fetch_add(1, Ordering::SeqCst);
    }

    fn session_end(&self) {
        log::info!("AirPlay session ended");
        self.video.stop();
        self.audio.stop();
        self.video_dims_packed.store(0, Ordering::Relaxed);
        self.decode_us.store(0, Ordering::Relaxed);
        let mut s = self.snap.lock().unwrap();
        *s = LiveSnapshot::default();
    }

    fn video_format(&self, codec: Codec, csd: &[u8], width: u32, height: u32) {
        log::info!(
            "Mirror video format: {} {}x{} (csd={}B)",
            codec.label(),
            width,
            height,
            csd.len()
        );
        {
            let mut s = self.snap.lock().unwrap();
            s.mode = StreamMode::Mirroring;
            s.video_codec = Some(codec);
            s.video_width = width;
            s.video_height = height;
            s.paused = false;
        }
        self.video_dims_packed
            .store(((width as u64) << 32) | (height as u64), Ordering::Relaxed);
        self.video.configure(codec, csd, width, height);
        self.wake_seq.fetch_add(1, Ordering::SeqCst);
    }

    fn video_frame(&self, frame: VideoFrame) {
        let now = monotonic_ns();
        let bytes = frame.data.len() as u64;
        {
            let mut rw = self.rolling.lock().unwrap();
            rw.frames_in_win += 1;
            rw.bytes_in_win += bytes;
            let elapsed_ns = now.saturating_sub(rw.window_start_ns);
            if elapsed_ns >= 500_000_000 {
                let secs = (elapsed_ns as f32) / 1_000_000_000.0;
                rw.fps = (rw.frames_in_win as f32) / secs;
                rw.bitrate_mbps = ((rw.bytes_in_win as f32) * 8.0) / (secs * 1_000_000.0);
                rw.window_start_ns = now;
                rw.frames_in_win = 0;
                rw.bytes_in_win = 0;
            }
        }
        {
            let mut s = self.snap.lock().unwrap();
            s.mode = StreamMode::Mirroring;
            s.frames_total += 1;
        }
        if let Some(dec_us) = self.video.push_frame(frame) {
            self.decode_us.store(dec_us, Ordering::Relaxed);
        }
    }

    fn video_paused(&self, paused: bool) {
        self.snap.lock().unwrap().paused = paused;
    }

    fn video_stop(&self) {
        self.video.stop();
        self.video_dims_packed.store(0, Ordering::Relaxed);
        let mut s = self.snap.lock().unwrap();
        if s.mode == StreamMode::Mirroring {
            s.mode = if s.audio_packets_total > 0 {
                StreamMode::AudioOnly
            } else {
                StreamMode::Idle
            };
        }
    }

    fn audio_start(&self, format: &AudioFormat) {
        log::info!(
            "Audio stream start: {} {}Hz {}ch spf={} screen={}",
            format.codec.label(),
            format.sample_rate,
            format.channels,
            format.spf,
            format.using_screen
        );
        {
            let mut s = self.snap.lock().unwrap();
            s.audio_codec_label = format.codec.label().to_string();
            if s.mode == StreamMode::Idle || s.mode == StreamMode::Connecting {
                if !format.using_screen {
                    s.mode = StreamMode::AudioOnly;
                }
            }
        }
        self.audio.start(format.clone());
    }

    fn audio_packet(&self, packet: AudioPacket) {
        {
            let mut s = self.snap.lock().unwrap();
            s.audio_packets_total += 1;
            if s.mode == StreamMode::Connecting {
                s.mode = StreamMode::AudioOnly;
            }
        }
        self.audio.push_packet(packet);
    }

    fn audio_flush(&self) {
        self.audio.flush();
    }

    fn audio_stop(&self) {
        self.audio.stop();
    }

    fn volume(&self, db: f32) {
        self.snap.lock().unwrap().volume_db = db;
        self.audio.set_volume_db(db);
    }

    fn metadata(&self, meta: &Metadata) {
        let mut s = self.snap.lock().unwrap();
        s.meta = meta.clone();
    }

    fn cover_art(&self, data: &[u8]) {
        if let Ok(img) = image::load_from_memory(data) {
            let thumb = img.thumbnail(360, 360).to_rgba8();
            let (w, h) = thumb.dimensions();
            let gen = self.cover_gen.fetch_add(1, Ordering::Relaxed) + 1;
            let mut s = self.snap.lock().unwrap();
            s.cover = Some(CoverArt {
                gen,
                width: w,
                height: h,
                rgba: Arc::new(thumb.into_raw()),
            });
        }
    }

    fn progress(&self, elapsed_s: f64, duration_s: f64) {
        let mut s = self.snap.lock().unwrap();
        s.progress_elapsed_s = elapsed_s;
        s.progress_duration_s = duration_s;
    }

    fn pin(&self, pin: &str) {
        let mut s = self.snap.lock().unwrap();
        s.active_pin = pin.to_string();
        if !pin.is_empty() {
            self.wake_seq.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn net_stats(&self, stats: &NetStats) {
        let mut s = self.snap.lock().unwrap();
        s.ntp_rtt_ms = stats.ntp_delay_ms as f32;
    }
}

fn load_or_create_device_id(state_dir: &Path) -> [u8; 6] {
    let p = state_dir.join("device_id.bin");
    if let Ok(b) = std::fs::read(&p) {
        if b.len() == 6 {
            let mut id = [0u8; 6];
            id.copy_from_slice(&b);
            return id;
        }
    }
    let mut id = [0x02, 0x59, 0x41, 0x4e, 0x53, 0x54];
    let mut rnd = [0u8; 4];
    crate::airplay::pairing::fill_random(&mut rnd);
    id[2..6].copy_from_slice(&rnd);
    let _ = std::fs::create_dir_all(state_dir);
    let _ = std::fs::write(&p, &id);
    id
}
