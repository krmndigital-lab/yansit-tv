//! AirPlay receiver core (platform independent, std threads + blocking sockets).
//!
//! Protocol = classic "AirPlay 2 mirroring" as implemented by UxPlay (which iOS 9 … 26 and
//! macOS accept): RTSP on one TCP port, legacy pairing (pair-setup / pair-verify with
//! Ed25519 + X25519), FairPlay SAP key exchange (fp-setup), SETUP for the NTP timing
//! channel + mirror stream (type 110, TCP, AES-CTR) + audio stream (type 96, RTP/UDP,
//! AES-CBC), SET_PARAMETER for volume / metadata / cover art / progress.
//!
//! Media is handed to a [`Sink`] (Android: MediaCodec + AAudio; host tests: recorder).
//! Discovery (mDNS) is done by the platform with [`Server::txt_airplay`] / [`Server::txt_raop`].

pub mod audio;
pub mod clock;
pub mod dmap;
pub mod fairplay;
pub mod http;
pub mod mirror;
pub mod ntp;
pub mod pairing;
pub mod pin;
pub mod plistx;
pub mod server;
pub mod txt;

pub use server::Server;

/// Video codec of the mirror stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Codec {
    H264,
    H265,
}

impl Codec {
    pub fn mime(self) -> &'static str {
        match self {
            Codec::H264 => "video/avc",
            Codec::H265 => "video/hevc",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Codec::H264 => "H.264",
            Codec::H265 => "HEVC",
        }
    }
}

/// RTP audio payload format ("ct" in SETUP).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioCodec {
    Pcm,
    Alac,
    AacLc,
    AacEld,
}

impl AudioCodec {
    pub fn from_ct(ct: u64) -> Option<Self> {
        Some(match ct {
            1 => AudioCodec::Pcm,
            2 => AudioCodec::Alac,
            4 => AudioCodec::AacLc,
            8 => AudioCodec::AacEld,
            _ => return None,
        })
    }
    pub fn label(self) -> &'static str {
        match self {
            AudioCodec::Pcm => "PCM",
            AudioCodec::Alac => "ALAC",
            AudioCodec::AacLc => "AAC",
            AudioCodec::AacEld => "AAC-ELD",
        }
    }
}

#[derive(Clone, Debug)]
pub struct AudioFormat {
    pub codec: AudioCodec,
    /// Samples per frame (ALAC 352, AAC-LC 1024, AAC-ELD 480).
    pub spf: u32,
    pub sample_rate: u32,
    pub channels: u32,
    /// Sent together with screen mirroring (low latency) vs. music / video playback.
    pub using_screen: bool,
    pub is_media: bool,
}

/// One decrypted access unit of the mirror stream, Annex-B (00 00 00 01 start codes).
pub struct VideoFrame {
    pub data: Vec<u8>,
    /// Includes VPS/SPS/PPS + IDR: the decoder can (re)start from here.
    pub key: bool,
    /// Sender timestamp mapped to local CLOCK_MONOTONIC ns (0 = clock not synced yet).
    pub pts_local: u64,
    /// Local CLOCK_MONOTONIC ns when the packet was complete.
    pub arrival: u64,
}

/// One decrypted audio packet (still encoded: ALAC / AAC / PCM).
pub struct AudioPacket {
    pub data: Vec<u8>,
    pub rtp: u32,
    pub seq: u16,
    pub pts_local: u64,
    pub arrival: u64,
}

#[derive(Clone, Debug, Default)]
pub struct Client {
    pub name: String,
    pub model: String,
    pub device_id: String,
    pub addr: String,
    /// DACP remote control (music: play/pause/next) — DACP-ID + Active-Remote headers.
    pub dacp_id: String,
    pub active_remote: String,
}

#[derive(Clone, Debug, Default)]
pub struct Metadata {
    pub title: String,
    pub artist: String,
    pub album: String,
}

/// Everything the receiver tells the platform. Calls come from protocol threads.
pub trait Sink: Send + Sync {
    /// A sender started a session (SETUP 1): show "connecting", wake the screen.
    fn session_start(&self, client: &Client);
    /// The session ended (TEARDOWN, disconnect or takeover by another sender).
    fn session_end(&self);
    /// Mirror stream format (first SPS/PPS and on every resolution / codec change).
    /// `csd` = Annex-B VPS/SPS/PPS.
    fn video_format(&self, codec: Codec, csd: &[u8], width: u32, height: u32);
    fn video_frame(&self, frame: VideoFrame);
    /// Sender paused (screen locked / app backgrounded) or resumed the mirror stream.
    fn video_paused(&self, paused: bool);
    fn video_stop(&self);
    fn audio_start(&self, format: &AudioFormat);
    fn audio_packet(&self, packet: AudioPacket);
    /// FLUSH (seek / pause in music apps): drop buffered audio.
    fn audio_flush(&self);
    fn audio_stop(&self);
    /// Sender volume in dB: 0 = max, -30 = min, -144 = mute.
    fn volume(&self, db: f32);
    fn metadata(&self, meta: &Metadata);
    fn cover_art(&self, data: &[u8]);
    /// Track progress in seconds (start, current, end) relative to the RTP clock.
    fn progress(&self, elapsed_s: f64, duration_s: f64);
    /// On-screen PIN to display ("" = hide).
    fn pin(&self, pin: &str);
    /// Statistics from the protocol threads (bytes, NTP delay, …), for the debug overlay.
    fn net_stats(&self, _stats: &NetStats) {}
}

#[derive(Clone, Debug, Default)]
pub struct NetStats {
    pub ntp_delay_ms: f64,
    pub ntp_offset_valid: bool,
}

/// Receiver configuration (from the app settings).
#[derive(Clone, Debug)]
pub struct Config {
    /// Name shown in "Screen Mirroring" on the iPad / iPhone.
    pub name: String,
    /// Stable 6-byte device id (MAC-like), also the mDNS RAOP prefix.
    pub device_id: [u8; 6],
    /// RTSP port (7000 like an Apple TV).
    pub port: u16,
    /// Advertised display (the sender scales its stream to fit).
    pub width: u32,
    pub height: u32,
    pub refresh_hz: u32,
    pub max_fps: u32,
    pub overscanned: bool,
    /// Advertise HEVC (features bit 42): iOS then sends H.265, needed for 4K.
    pub h265: bool,
    pub alac: bool,
    pub aac: bool,
    /// Require a 4-digit on-screen PIN for new devices.
    pub require_pin: bool,
    /// Audio-Latency reported to the sender for music streams (ms).
    pub audio_latency_ms: u32,
    /// Directory for the persistent pairing identity + PIN-paired devices.
    pub state_dir: std::path::PathBuf,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            name: "Salon TV".into(),
            device_id: [0x02, 0x59, 0x41, 0x4e, 0x53, 0x54],
            port: 7000,
            width: 1920,
            height: 1080,
            refresh_hz: 60,
            max_fps: 60,
            overscanned: false,
            h265: true,
            alac: true,
            aac: true,
            require_pin: false,
            audio_latency_ms: 300,
            state_dir: std::env::temp_dir(),
        }
    }
}

/// Model / version we present: an Apple TV 3 class receiver, like UxPlay.
pub const MODEL: &str = "AppleTV3,2";
pub const SOURCE_VERSION: &str = "220.68";

pub fn hex(d: &[u8]) -> String {
    d.iter().map(|b| format!("{b:02x}")).collect()
}

/// "aa:bb:cc:dd:ee:ff" (AirPlay deviceid) form of the device id.
pub fn device_id_colon(id: &[u8; 6]) -> String {
    id.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(":")
}
