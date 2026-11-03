//! Pure-Rust DLNA/UPnP HTTP Media Stream Demuxer & Player (`AMediaExtractor` NDK FFI).
//!
//! Demuxes HTTP/HTTPS video & audio streams (`.mp4`, `.mkv`, `.webm`, `.ts`, `.m3u8`,
//! `.mp3`, `.flac`, `.aac`, `.wav`) in Rust via Android NDK's `libmediandk.so`
//! (`AMediaExtractor`) and feeds video access units into `MediaEngine`'s hardware
//! `VideoPlayer` (`AMediaCodec`) and audio frames into `AudioPlayer` (`AAudio`).

#[cfg(target_os = "android")]
use crate::airplay::clock::monotonic_ns;
#[cfg(target_os = "android")]
use crate::airplay::{AudioCodec, AudioFormat, AudioPacket, Codec, Sink, VideoFrame};
use crate::media::MediaEngine;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
#[cfg(target_os = "android")]
use std::time::{Duration, Instant};

struct ActiveDlnaPlayback {
    stop: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    seek_target_us: Arc<AtomicI64>,
}

static ACTIVE_PLAYBACK: OnceLock<Mutex<Option<ActiveDlnaPlayback>>> = OnceLock::new();

fn playback_lock() -> &'static Mutex<Option<ActiveDlnaPlayback>> {
    ACTIVE_PLAYBACK.get_or_init(|| Mutex::new(None))
}

pub fn start_url(url: String, is_video: bool, engine: Arc<MediaEngine>) {
    stop();

    let stop_flag = Arc::new(AtomicBool::new(false));
    let paused_flag = Arc::new(AtomicBool::new(false));
    let seek_us = Arc::new(AtomicI64::new(-1));

    {
        let mut g = playback_lock().lock().unwrap();
        *g = Some(ActiveDlnaPlayback {
            stop: stop_flag.clone(),
            paused: paused_flag.clone(),
            seek_target_us: seek_us.clone(),
        });
    }

    let _ = std::thread::Builder::new()
        .name("dlna-player".into())
        .spawn(move || {
            #[cfg(target_os = "android")]
            android_extractor_loop(url, is_video, engine, stop_flag, paused_flag, seek_us);
            #[cfg(not(target_os = "android"))]
            let _ = (url, is_video, engine, stop_flag, paused_flag, seek_us);
        });
}

pub fn set_paused(paused: bool) {
    if let Some(ref p) = *playback_lock().lock().unwrap() {
        p.paused.store(paused, Ordering::Relaxed);
    }
}

pub fn seek_ms(target_ms: u64) {
    if let Some(ref p) = *playback_lock().lock().unwrap() {
        p.seek_target_us
            .store((target_ms as i64) * 1000, Ordering::Relaxed);
    }
}

pub fn stop() {
    if let Some(existing) = playback_lock().lock().unwrap().take() {
        existing.stop.store(true, Ordering::Relaxed);
    }
}

#[cfg(target_os = "android")]
mod ffi {
    use std::os::raw::{c_char, c_int, c_void};

    #[link(name = "mediandk")]
    extern "C" {
        pub fn AMediaExtractor_new() -> *mut c_void;
        pub fn AMediaExtractor_delete(ex: *mut c_void) -> c_int;
        pub fn AMediaExtractor_setDataSource(ex: *mut c_void, location: *const c_char) -> c_int;
        pub fn AMediaExtractor_getTrackCount(ex: *mut c_void) -> usize;
        pub fn AMediaExtractor_getTrackFormat(ex: *mut c_void, idx: usize) -> *mut c_void;
        pub fn AMediaExtractor_selectTrack(ex: *mut c_void, idx: usize) -> c_int;
        pub fn AMediaExtractor_readSampleData(
            ex: *mut c_void,
            buffer: *mut u8,
            capacity: usize,
        ) -> isize;
        pub fn AMediaExtractor_getSampleTime(ex: *mut c_void) -> i64;
        pub fn AMediaExtractor_getSampleTrackIndex(ex: *mut c_void) -> c_int;
        pub fn AMediaExtractor_getSampleFlags(ex: *mut c_void) -> u32;
        pub fn AMediaExtractor_advance(ex: *mut c_void) -> bool;
        pub fn AMediaExtractor_seekTo(ex: *mut c_void, seekPosUs: i64, mode: c_int) -> c_int;

        pub fn AMediaFormat_delete(fmt: *mut c_void) -> c_int;
        pub fn AMediaFormat_getString(
            fmt: *mut c_void,
            name: *const c_char,
            out: *mut *const c_char,
        ) -> bool;
        pub fn AMediaFormat_getInt32(
            fmt: *mut c_void,
            name: *const c_char,
            out: *mut i32,
        ) -> bool;
        pub fn AMediaFormat_getInt64(
            fmt: *mut c_void,
            name: *const c_char,
            out: *mut i64,
        ) -> bool;
        pub fn AMediaFormat_getBuffer(
            fmt: *mut c_void,
            name: *const c_char,
            data: *mut *mut c_void,
            size: *mut usize,
        ) -> bool;
    }
}

#[cfg(target_os = "android")]
fn get_fmt_string(fmt: *mut std::os::raw::c_void, key: &std::ffi::CStr) -> Option<String> {
    unsafe {
        let mut ptr: *const std::os::raw::c_char = std::ptr::null();
        if ffi::AMediaFormat_getString(fmt, key.as_ptr(), &mut ptr) && !ptr.is_null() {
            Some(
                std::ffi::CStr::from_ptr(ptr)
                    .to_string_lossy()
                    .into_owned(),
            )
        } else {
            None
        }
    }
}

#[cfg(target_os = "android")]
fn get_fmt_i32(fmt: *mut std::os::raw::c_void, key: &std::ffi::CStr) -> Option<i32> {
    unsafe {
        let mut val: i32 = 0;
        if ffi::AMediaFormat_getInt32(fmt, key.as_ptr(), &mut val) {
            Some(val)
        } else {
            None
        }
    }
}

#[cfg(target_os = "android")]
fn get_fmt_i64(fmt: *mut std::os::raw::c_void, key: &std::ffi::CStr) -> Option<i64> {
    unsafe {
        let mut val: i64 = 0;
        if ffi::AMediaFormat_getInt64(fmt, key.as_ptr(), &mut val) {
            Some(val)
        } else {
            None
        }
    }
}

#[cfg(target_os = "android")]
fn get_fmt_buffer(fmt: *mut std::os::raw::c_void, key: &std::ffi::CStr) -> Option<Vec<u8>> {
    unsafe {
        let mut data: *mut std::os::raw::c_void = std::ptr::null_mut();
        let mut size: usize = 0;
        if ffi::AMediaFormat_getBuffer(fmt, key.as_ptr(), &mut data, &mut size)
            && !data.is_null()
            && size > 0
        {
            let slice = std::slice::from_raw_parts(data as *const u8, size);
            Some(slice.to_vec())
        } else {
            None
        }
    }
}

#[cfg(target_os = "android")]
fn android_extractor_loop(
    url: String,
    _is_video_hint: bool,
    engine: Arc<MediaEngine>,
    stop: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    seek_target_us: Arc<AtomicI64>,
) {
    use std::ffi::CString;

    log::info!("dlna-player: opening URL '{}' via NDK AMediaExtractor", url);
    let c_url = match CString::new(url.as_str()) {
        Ok(c) => c,
        Err(_) => return,
    };

    let ex = unsafe { ffi::AMediaExtractor_new() };
    if ex.is_null() {
        log::error!("dlna-player: AMediaExtractor_new returned null");
        return;
    }

    let status = unsafe { ffi::AMediaExtractor_setDataSource(ex, c_url.as_ptr()) };
    if status != 0 {
        log::warn!(
            "dlna-player: AMediaExtractor_setDataSource failed with status {}",
            status
        );
        unsafe {
            ffi::AMediaExtractor_delete(ex);
        }
        return;
    }

    let track_count = unsafe { ffi::AMediaExtractor_getTrackCount(ex) };
    let k_mime = CString::new("mime").unwrap();
    let k_width = CString::new("width").unwrap();
    let k_height = CString::new("height").unwrap();
    let k_dur = CString::new("durationUs").unwrap();
    let k_sr = CString::new("sample-rate").unwrap();
    let k_ch = CString::new("channel-count").unwrap();
    let k_csd0 = CString::new("csd-0").unwrap();
    let k_csd1 = CString::new("csd-1").unwrap();

    let mut video_track: Option<i32> = None;
    let mut video_codec;
    let mut video_w = 1920u32;
    let mut video_h = 1080u32;
    let mut duration_ms = 0u64;

    let mut audio_track: Option<i32> = None;
    let mut audio_seq = 0u16;
    let mut audio_rtp = 0u32;

    for i in 0..track_count {
        let fmt = unsafe { ffi::AMediaExtractor_getTrackFormat(ex, i) };
        if fmt.is_null() {
            continue;
        }
        let mime = get_fmt_string(fmt, &k_mime).unwrap_or_default();
        if let Some(d_us) = get_fmt_i64(fmt, &k_dur) {
            if d_us > 0 {
                duration_ms = duration_ms.max((d_us as u64) / 1000);
            }
        }

        if video_track.is_none() && (mime.starts_with("video/avc") || mime.starts_with("video/hevc"))
        {
            video_codec = if mime.starts_with("video/hevc") {
                Codec::H265
            } else {
                Codec::H264
            };
            video_w = get_fmt_i32(fmt, &k_width).unwrap_or(1920).max(64) as u32;
            video_h = get_fmt_i32(fmt, &k_height).unwrap_or(1080).max(64) as u32;
            let mut csd = Vec::new();
            if let Some(b0) = get_fmt_buffer(fmt, &k_csd0) {
                csd.extend_from_slice(&ensure_annexb(&b0));
            }
            if let Some(b1) = get_fmt_buffer(fmt, &k_csd1) {
                csd.extend_from_slice(&ensure_annexb(&b1));
            }
            unsafe {
                ffi::AMediaExtractor_selectTrack(ex, i);
            }
            engine.video_format(video_codec, &csd, video_w, video_h);
            video_track = Some(i as i32);
            log::info!(
                "dlna-player: selected video track {} ({} {}x{}, csd={}B)",
                i,
                mime,
                video_w,
                video_h,
                csd.len()
            );
        } else if audio_track.is_none() && mime.starts_with("audio/mp4a-latm") {
            let sr = get_fmt_i32(fmt, &k_sr).unwrap_or(48_000).max(8_000) as u32;
            let ch = get_fmt_i32(fmt, &k_ch).unwrap_or(2).max(1) as u32;
            unsafe {
                ffi::AMediaExtractor_selectTrack(ex, i);
            }
            engine.audio_start(&AudioFormat {
                codec: AudioCodec::AacLc,
                spf: 1024,
                sample_rate: sr,
                channels: ch,
                using_screen: video_track.is_some(),
                is_media: true,
            });
            audio_track = Some(i as i32);
            log::info!(
                "dlna-player: selected AAC audio track {} ({}Hz {}ch)",
                i,
                sr,
                ch
            );
        }

        unsafe {
            ffi::AMediaFormat_delete(fmt);
        }
    }

    let mut sample_buf = vec![0u8; 2 * 1024 * 1024];
    let mut start_wall = Instant::now();
    let mut base_pts_us: Option<i64> = None;

    while !stop.load(Ordering::Relaxed) {
        let seek_us = seek_target_us.swap(-1, Ordering::Relaxed);
        if seek_us >= 0 {
            unsafe {
                ffi::AMediaExtractor_seekTo(ex, seek_us, 0);
            }
            start_wall = Instant::now();
            base_pts_us = Some(seek_us);
            engine.audio_flush();
        }

        if paused.load(Ordering::Relaxed) {
            engine.update_dlna_status(
                base_pts_us.unwrap_or(0).max(0) as u64 / 1000,
                duration_ms,
                false,
                video_w,
                video_h,
            );
            std::thread::sleep(Duration::from_millis(50));
            start_wall = Instant::now();
            continue;
        }

        let track_idx = unsafe { ffi::AMediaExtractor_getSampleTrackIndex(ex) };
        if track_idx < 0 {
            log::info!("dlna-player: reached end of stream");
            break;
        }

        let n = unsafe {
            ffi::AMediaExtractor_readSampleData(ex, sample_buf.as_mut_ptr(), sample_buf.len())
        };
        if n <= 0 {
            if !unsafe { ffi::AMediaExtractor_advance(ex) } {
                break;
            }
            continue;
        }
        let sample_len = n as usize;
        let pts_us = unsafe { ffi::AMediaExtractor_getSampleTime(ex) }.max(0);
        let flags = unsafe { ffi::AMediaExtractor_getSampleFlags(ex) };

        let first_pts = *base_pts_us.get_or_insert(pts_us);
        let rel_us = (pts_us - first_pts).max(0) as u64;
        let elapsed_us = start_wall.elapsed().as_micros() as u64;
        if rel_us > elapsed_us + 5_000 {
            let sleep_us = (rel_us - elapsed_us).min(200_000);
            std::thread::sleep(Duration::from_micros(sleep_us));
        } else if elapsed_us > rel_us + 250_000 {
            // Resync wall clock if stream jumped ahead
            base_pts_us = Some(pts_us);
            start_wall = Instant::now();
        }

        let now_ns = monotonic_ns();
        if Some(track_idx) == video_track {
            let annexb = ensure_annexb(&sample_buf[..sample_len]);
            let is_key = (flags & 1) != 0;
            engine.video_frame(VideoFrame {
                data: annexb,
                key: is_key,
                pts_local: now_ns,
                arrival: now_ns,
            });
        } else if Some(track_idx) == audio_track {
            audio_seq = audio_seq.wrapping_add(1);
            audio_rtp = audio_rtp.wrapping_add(1024);
            engine.audio_packet(AudioPacket {
                data: sample_buf[..sample_len].to_vec(),
                rtp: audio_rtp,
                seq: audio_seq,
                pts_local: now_ns,
                arrival: now_ns,
            });
        }

        engine.update_dlna_status(
            (pts_us as u64) / 1000,
            duration_ms,
            true,
            video_w,
            video_h,
        );

        if !unsafe { ffi::AMediaExtractor_advance(ex) } {
            break;
        }
    }

    unsafe {
        ffi::AMediaExtractor_delete(ex);
    }
    engine.update_dlna_status(0, duration_ms, false, 0, 0);
    engine.video_stop();
    engine.audio_stop();
}

#[cfg(target_os = "android")]
fn ensure_annexb(data: &[u8]) -> Vec<u8> {
    if data.len() >= 4
        && ((data[0] == 0 && data[1] == 0 && data[2] == 0 && data[3] == 1)
            || (data[0] == 0 && data[1] == 0 && data[2] == 1))
    {
        return data.to_vec();
    }
    // Convert 4-byte big-endian length-prefixed AVCC/HVCC NALs to Annex-B
    let mut out = Vec::with_capacity(data.len() + 16);
    let mut pos = 0;
    while pos + 4 <= data.len() {
        let nal_len =
            u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
        pos += 4;
        if nal_len == 0 || pos + nal_len > data.len() {
            return data.to_vec();
        }
        out.extend_from_slice(&[0, 0, 0, 1]);
        out.extend_from_slice(&data[pos..pos + nal_len]);
        pos += nal_len;
    }
    if out.is_empty() {
        data.to_vec()
    } else {
        out
    }
}
