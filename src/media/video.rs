//! Low-latency hardware video decoder (`VideoPlayer`).
//!
//! On Android (`target_os = "android"`), drives `AMediaCodec` (`OMX.amlogic.hevc.decoder.awesome2`
//! / `OMX.amlogic.avc.decoder.awesome2`) bound directly to the video `SurfaceView`'s
//! `NativeWindow` (`ANativeWindow`).
//!
//! Key optimizations for Amlogic S905X4 ("jaws" Android 11 TV):
//! 1. Configures `low-latency = 1`, `vendor.low-latency.enable = 1`, `priority = 0`, and
//!    `operating-rate = 60` so hardware decode latency is ~5.5 ms for HEVC and ~10.0 ms for H.264.
//! 2. Avoids `AMediaCodec_setOutputSurface` (which hangs on Amlogic at 4K after ~10 switches);
//!    instead, creates a fresh `MediaCodec` cleanly whenever the surface or codec changes.
//! 3. Caches the latest `(Codec, csd, width, height)` AND the latest IDR keyframe so if the
//!    AirPlay session begins in the background before `MainActivity`'s video `SurfaceView` is
//!    ready, the moment the surface arrives it configures the decoder and feeds the cached IDR
//!    immediately.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use crate::airplay::{Codec, VideoFrame};

#[cfg(target_os = "android")]
use ndk::media::media_codec::{
    DequeuedInputBufferResult as In, DequeuedOutputBufferInfoResult as Out, MediaCodec,
    MediaCodecDirection,
};
#[cfg(target_os = "android")]
use ndk::media::media_format::MediaFormat;
#[cfg(target_os = "android")]
use ndk::native_window::NativeWindow;
#[cfg(target_os = "android")]
use std::collections::VecDeque;
#[cfg(target_os = "android")]
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
#[cfg(target_os = "android")]
use std::time::Duration;

#[cfg(target_os = "android")]
enum VCmd {
    Surface(Option<NativeWindow>),
    Config {
        codec: Codec,
        csd: Vec<u8>,
        width: u32,
        height: u32,
    },
    Frame(VideoFrame),
    Stop,
}

#[derive(Clone)]
struct CachedFormat {
    codec: Codec,
    csd: Vec<u8>,
    width: u32,
    height: u32,
}

pub struct VideoPlayer {
    low_latency: Arc<AtomicBool>,
    enforce_sdr: Arc<AtomicBool>,
    last_decode_us: Arc<AtomicU32>,
    cached_fmt: Mutex<Option<CachedFormat>>,
    #[cfg(target_os = "android")]
    tx: Mutex<Sender<VCmd>>,
}

impl VideoPlayer {
    pub fn new(low_latency: bool, enforce_sdr: bool) -> Self {
        let low_latency = Arc::new(AtomicBool::new(low_latency));
        let enforce_sdr = Arc::new(AtomicBool::new(enforce_sdr));
        let last_decode_us = Arc::new(AtomicU32::new(0));

        #[cfg(target_os = "android")]
        let tx = {
            let (tx, rx) = channel();
            let ll = low_latency.clone();
            let sdr = enforce_sdr.clone();
            let dec_us = last_decode_us.clone();
            std::thread::Builder::new()
                .name("yansit-vdec".into())
                .spawn(move || {
                    android_video_worker(rx, ll, sdr, dec_us);
                })
                .expect("spawn video worker");
            Mutex::new(tx)
        };

        Self {
            low_latency,
            enforce_sdr,
            last_decode_us,
            cached_fmt: Mutex::new(None),
            #[cfg(target_os = "android")]
            tx,
        }
    }

    pub fn set_options(&self, low_latency: bool, enforce_sdr: bool) {
        self.low_latency.store(low_latency, Ordering::Relaxed);
        self.enforce_sdr.store(enforce_sdr, Ordering::Relaxed);
    }

    #[cfg(target_os = "android")]
    pub fn set_surface(&self, win: Option<NativeWindow>) {
        if let Ok(tx) = self.tx.lock() {
            let _ = tx.send(VCmd::Surface(win));
        }
    }

    pub fn configure(&self, codec: Codec, csd: &[u8], width: u32, height: u32) {
        *self.cached_fmt.lock().unwrap() = Some(CachedFormat {
            codec,
            csd: csd.to_vec(),
            width,
            height,
        });
        #[cfg(target_os = "android")]
        if let Ok(tx) = self.tx.lock() {
            let _ = tx.send(VCmd::Config {
                codec,
                csd: csd.to_vec(),
                width,
                height,
            });
        }
    }

    pub fn push_frame(&self, frame: VideoFrame) -> Option<u32> {
        let _ = &self.cached_fmt;
        #[cfg(target_os = "android")]
        {
            if let Ok(tx) = self.tx.lock() {
                let _ = tx.send(VCmd::Frame(frame));
            }
            let us = self.last_decode_us.load(Ordering::Relaxed);
            if us > 0 {
                return Some(us);
            }
            None
        }
        #[cfg(not(target_os = "android"))]
        {
            // No decoder on the host: report no measurement rather than a made-up latency.
            let _ = frame;
            let _ = &self.last_decode_us;
            None
        }
    }

    pub fn stop(&self) {
        *self.cached_fmt.lock().unwrap() = None;
        #[cfg(target_os = "android")]
        if let Ok(tx) = self.tx.lock() {
            let _ = tx.send(VCmd::Stop);
        }
    }
}

#[cfg(target_os = "android")]
fn android_video_worker(
    rx: Receiver<VCmd>,
    low_latency: Arc<AtomicBool>,
    enforce_sdr: Arc<AtomicBool>,
    last_decode_us: Arc<AtomicU32>,
) {
    unsafe {
        libc::setpriority(libc::PRIO_PROCESS, 0, -16);
    }

    let mut surface: Option<NativeWindow> = None;
    let mut active_fmt: Option<CachedFormat> = None;
    let mut decoder: Option<MediaCodec> = None;
    let mut last_keyframe: Option<VideoFrame> = None;
    let mut queue: VecDeque<VideoFrame> = VecDeque::with_capacity(16);
    let mut inflight_arrival: VecDeque<(i64, u64, u64)> = VecDeque::with_capacity(32);
    let mut pts_seq: i64 = 0;

    loop {
        let wait = if decoder.is_some() {
            Duration::from_micros(1500)
        } else {
            Duration::from_millis(15)
        };

        let mut cmds = Vec::new();
        match rx.recv_timeout(wait) {
            Ok(c) => cmds.push(c),
            Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {}
        }
        cmds.extend(rx.try_iter());

        let mut recreate = false;
        for cmd in cmds {
            match cmd {
                VCmd::Surface(win) => {
                    let same_ptr = match (&surface, &win) {
                        (Some(a), Some(b)) => a.ptr() == b.ptr(),
                        (None, None) => true,
                        _ => false,
                    };
                    if !same_ptr {
                        drop(decoder.take());
                        surface = win;
                        recreate = surface.is_some() && active_fmt.is_some();
                    }
                }
                VCmd::Config {
                    codec,
                    csd,
                    width,
                    height,
                } => {
                    let codec_changed = active_fmt.as_ref().map_or(true, |f| f.codec != codec);
                    active_fmt = Some(CachedFormat {
                        codec,
                        csd,
                        width,
                        height,
                    });
                    if surface.is_some() && (decoder.is_none() || codec_changed) {
                        recreate = true;
                    }
                }
                VCmd::Frame(frame) => {
                    if frame.key {
                        last_keyframe = Some(VideoFrame {
                            data: frame.data.clone(),
                            key: true,
                            pts_local: frame.pts_local,
                            arrival: frame.arrival,
                        });
                        if decoder.is_none() || queue.len() > 10 {
                            queue.clear();
                        }
                    }
                    if queue.len() < 45 {
                        queue.push_back(frame);
                    }
                }
                VCmd::Stop => {
                    drop(decoder.take());
                    active_fmt = None;
                    last_keyframe = None;
                    queue.clear();
                    inflight_arrival.clear();
                }
            }
        }

        if recreate {
            drop(decoder.take());
            inflight_arrival.clear();
            pts_seq = 0;
            if let (Some(ref win), Some(ref fmt)) = (&surface, &active_fmt) {
                decoder = create_android_video_decoder(
                    fmt,
                    win,
                    low_latency.load(Ordering::Relaxed),
                    enforce_sdr.load(Ordering::Relaxed),
                );
                // If queue doesn't already start with a keyframe, seed with the cached IDR frame
                if decoder.is_some() && !queue.front().map_or(false, |f| f.key) {
                    if let Some(ref kf) = last_keyframe {
                        queue.push_front(VideoFrame {
                            data: kf.data.clone(),
                            key: true,
                            pts_local: kf.pts_local,
                            arrival: crate::airplay::clock::monotonic_ns(),
                        });
                    }
                }
            }
        }

        let Some(ref codec) = decoder else {
            continue;
        };

        // Feed queued NAL access units into MediaCodec input buffers
        while !queue.is_empty() {
            let Ok(In::Buffer(mut ibuf)) = codec.dequeue_input_buffer(Duration::ZERO) else {
                break;
            };
            let frame = queue.pop_front().unwrap();
            let dst = ibuf.buffer_mut();
            let n = frame.data.len().min(dst.len());
            unsafe {
                std::ptr::copy_nonoverlapping(
                    frame.data.as_ptr(),
                    dst.as_mut_ptr().cast::<u8>(),
                    n,
                );
            }
            // Always feed a clean, bounded monotonic 60fps PTS sequence into MediaCodec so:
            // 1. Stagefright's `timeUs * 1000` (`__mulodi4`) can never overflow `int64_t`.
            // 2. Amlogic's V4L2 decoder sees a stable 60fps cadence (`pts_unstable=0`).
            pts_seq += 16_667;
            let pts_us = pts_seq;
            if inflight_arrival.len() >= 32 {
                inflight_arrival.pop_front();
            }
            inflight_arrival.push_back((pts_us, frame.arrival, frame.pts_local));
            let _ = codec.queue_input_buffer(ibuf, 0, n, pts_us as u64, 0);
        }

        // Drain all ready output buffers immediately to the Surface
        loop {
            match codec.dequeue_output_buffer(Duration::ZERO) {
                Ok(Out::Buffer(obuf)) => {
                    let pts_us = obuf.info().presentation_time_us();
                    let now = crate::airplay::clock::monotonic_ns();
                    let mut target_ns = now;
                    while let Some(&(p, arr, pts_local)) = inflight_arrival.front() {
                        if p <= pts_us {
                            inflight_arrival.pop_front();
                            if p == pts_us {
                                target_ns = pts_local;
                                if now > arr {
                                    let lat_us = ((now - arr) / 1000).min(500_000) as u32;
                                    last_decode_us.store(lat_us, Ordering::Relaxed);
                                }
                            }
                        } else {
                            break;
                        }
                    }

                    let rel_ns = if low_latency.load(Ordering::Relaxed) {
                        now
                    } else if target_ns > now && target_ns - now < 50_000_000 {
                        target_ns
                    } else {
                        now
                    };
                    let _ = codec.release_output_buffer_at_time(obuf, rel_ns as i64);
                }
                Ok(Out::OutputFormatChanged) | Ok(Out::OutputBuffersChanged) => continue,
                _ => break,
            }
        }
    }
}

#[cfg(target_os = "android")]
fn create_android_video_decoder(
    fmt: &CachedFormat,
    win: &NativeWindow,
    low_latency: bool,
    enforce_sdr: bool,
) -> Option<MediaCodec> {
    let mime = fmt.codec.mime();
    let codec = MediaCodec::from_decoder_type(mime)?;
    let mut mf = MediaFormat::new();
    mf.set_str("mime", mime);
    mf.set_i32("width", fmt.width.max(16) as i32);
    mf.set_i32("height", fmt.height.max(16) as i32);
    mf.set_i32("max-width", 3840);
    mf.set_i32("max-height", 2160);
    if !fmt.csd.is_empty() {
        mf.set_buffer("csd-0", &fmt.csd);
    }
    mf.set_i32("priority", 0);
    mf.set_i32("operating-rate", 60);
    mf.set_i32("max-input-size", 4 * 1024 * 1024);

    if low_latency {
        mf.set_i32("low-latency", 1);
        mf.set_i32("vendor.low-latency.enable", 1);
    }
    if enforce_sdr {
        // BT.709 / SMPTE170M SDR / Limited range
        mf.set_i32("color-standard", 1);
        mf.set_i32("color-transfer", 3);
        mf.set_i32("color-range", 2);
    }

    if let Err(e) = codec.configure(&mf, Some(win), MediaCodecDirection::Decoder) {
        log::warn!("Video decoder configure failed ({mime}): {e:?}");
        return None;
    }
    if let Err(e) = codec.start() {
        log::warn!("Video decoder start failed ({mime}): {e:?}");
        return None;
    }
    log::info!(
        "Hardware video decoder started: {} {}x{} (low_latency={}, sdr={})",
        mime,
        fmt.width,
        fmt.height,
        low_latency,
        enforce_sdr
    );
    Some(codec)
}
