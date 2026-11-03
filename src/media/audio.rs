//! Low-latency audio decoder and AAudio output pipeline (`AudioPlayer`).
//!
//! Supports AirPlay RAOP audio formats:
//! - `AudioCodec::Alac` (`audio/alac` with 36-byte `alac` atom magic cookie)
//! - `AudioCodec::AacEld` (`audio/mp4a-latm` with 4-byte ELD AudioSpecificConfig)
//! - `AudioCodec::AacLc` (`audio/mp4a-latm` with 2-byte LC AudioSpecificConfig)
//! - `AudioCodec::Pcm` (raw 16-bit stereo PCM)
//!
//! Outputs stereo 16-bit PCM via Android NDK `AAudio` (`AudioPerformanceMode::LowLatency`),
//! applying AirPlay dB volume attenuation smoothly.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

#[cfg(target_os = "android")]
use std::sync::Mutex;

use crate::airplay::{AudioCodec, AudioFormat, AudioPacket};

#[cfg(target_os = "android")]
use ndk::audio::{
    AudioContentType, AudioDirection, AudioFormat as NdkAudioFormat, AudioPerformanceMode,
    AudioStream, AudioStreamBuilder, AudioUsage,
};
#[cfg(target_os = "android")]
use ndk::media::media_codec::{
    DequeuedInputBufferResult as In, DequeuedOutputBufferInfoResult as Out, MediaCodec,
    MediaCodecDirection,
};
#[cfg(target_os = "android")]
use ndk::media::media_format::MediaFormat;
#[cfg(target_os = "android")]
use std::collections::VecDeque;
#[cfg(target_os = "android")]
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
#[cfg(target_os = "android")]
use std::time::Duration;

#[cfg(target_os = "android")]
enum ACmd {
    Start(AudioFormat),
    Packet(AudioPacket),
    Flush,
    Stop,
}

pub struct AudioPlayer {
    buffer_ms: Arc<AtomicU32>,
    /// Linear gain multiplier stored as `f32::to_bits()`.
    gain_bits: Arc<AtomicU32>,
    #[cfg(target_os = "android")]
    tx: Mutex<Sender<ACmd>>,
}

impl AudioPlayer {
    pub fn new(buffer_ms: u32) -> Self {
        let buffer_ms = Arc::new(AtomicU32::new(buffer_ms));
        let gain_bits = Arc::new(AtomicU32::new(1.0f32.to_bits()));

        #[cfg(target_os = "android")]
        let tx = {
            let (tx, rx) = channel();
            let buf_ms = buffer_ms.clone();
            let gain = gain_bits.clone();
            std::thread::Builder::new()
                .name("yansit-adec".into())
                .spawn(move || {
                    android_audio_worker(rx, buf_ms, gain);
                })
                .expect("spawn audio worker");
            Mutex::new(tx)
        };

        Self {
            buffer_ms,
            gain_bits,
            #[cfg(target_os = "android")]
            tx,
        }
    }

    pub fn set_buffer_ms(&self, ms: u32) {
        self.buffer_ms.store(ms.clamp(10, 200), Ordering::Relaxed);
    }

    /// Convert AirPlay volume (`0.0` = max, `-30.0` = min, `-144.0` = mute) to linear gain.
    pub fn set_volume_db(&self, db: f32) {
        let gain = if db <= -30.0 {
            0.0f32
        } else if db >= 0.0 {
            1.0f32
        } else {
            10.0f32.powf(db / 20.0)
        };
        self.gain_bits.store(gain.to_bits(), Ordering::Relaxed);
    }

    pub fn start(&self, format: AudioFormat) {
        #[cfg(target_os = "android")]
        if let Ok(tx) = self.tx.lock() {
            let _ = tx.send(ACmd::Start(format));
        }
        #[cfg(not(target_os = "android"))]
        let _ = format;
    }

    pub fn push_packet(&self, pkt: AudioPacket) {
        #[cfg(target_os = "android")]
        if let Ok(tx) = self.tx.lock() {
            let _ = tx.send(ACmd::Packet(pkt));
        }
        #[cfg(not(target_os = "android"))]
        let _ = pkt;
    }

    pub fn flush(&self) {
        #[cfg(target_os = "android")]
        if let Ok(tx) = self.tx.lock() {
            let _ = tx.send(ACmd::Flush);
        }
    }

    pub fn stop(&self) {
        #[cfg(target_os = "android")]
        if let Ok(tx) = self.tx.lock() {
            let _ = tx.send(ACmd::Stop);
        }
    }
}

/// Build the 36-byte `alac` atom magic cookie required by Android's `audio/alac` MediaCodec.
pub fn build_alac_csd(sample_rate: u32, channels: u32, spf: u32) -> [u8; 36] {
    let mut out = [0u8; 36];
    // 12-byte atom header: size=36 (0x24), 'alac', version/flags=0
    out[0..12].copy_from_slice(&[0x00, 0x00, 0x00, 0x24, b'a', b'l', b'a', b'c', 0, 0, 0, 0]);
    // 24-byte ALACSpecificConfig (big-endian)
    out[12..16].copy_from_slice(&spf.to_be_bytes());
    out[16] = 0; // compatibleVersion
    out[17] = 16; // bitDepth = 16
    out[18] = 40; // pb
    out[19] = 10; // mb
    out[20] = 14; // kb
    out[21] = channels as u8;
    out[22] = 0x00;
    out[23] = 0xff; // maxRun = 255
    // [24..28] maxFrameBytes = 0, [28..32] avgBitRate = 0
    out[32..36].copy_from_slice(&sample_rate.to_be_bytes());
    out
}

/// Build the AudioSpecificConfig `csd-0` for AAC-LC or AAC-ELD over `audio/mp4a-latm`.
pub fn build_aac_csd(codec: AudioCodec, spf: u32) -> Vec<u8> {
    match codec {
        AudioCodec::AacEld => vec![0xF8, 0xE8, if spf == 512 { 0x40 } else { 0x50 }, 0x00],
        _ => vec![0x12, if spf == 960 { 0x14 } else { 0x10 }],
    }
}

#[cfg(target_os = "android")]
fn android_audio_worker(
    rx: Receiver<ACmd>,
    buffer_ms: Arc<AtomicU32>,
    gain_bits: Arc<AtomicU32>,
) {
    unsafe {
        libc::setpriority(libc::PRIO_PROCESS, 0, -18);
    }

    let mut cur_fmt: Option<AudioFormat> = None;
    let mut decoder: Option<MediaCodec> = None;
    let mut aaudio: Option<AudioStream> = None;
    let mut queue: VecDeque<AudioPacket> = VecDeque::with_capacity(64);
    let mut pts_us_seq: u64 = 0;
    let mut pcm_enc: i32 = 2; // 2 = PCM_16BIT, 4 = PCM_FLOAT

    loop {
        let wait = if cur_fmt.is_some() {
            Duration::from_millis(2)
        } else {
            Duration::from_millis(20)
        };

        let mut cmds = Vec::new();
        match rx.recv_timeout(wait) {
            Ok(c) => cmds.push(c),
            Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {}
        }
        cmds.extend(rx.try_iter());

        for cmd in cmds {
            match cmd {
                ACmd::Start(fmt) => {
                    queue.clear();
                    drop(decoder.take());
                    drop(aaudio.take());
                    pcm_enc = 2;
                    aaudio = open_aaudio_stream(fmt.sample_rate as i32, fmt.channels as i32);
                    if fmt.codec != AudioCodec::Pcm {
                        decoder = create_android_audio_decoder(&fmt);
                    }
                    cur_fmt = Some(fmt);
                }
                ACmd::Packet(pkt) => {
                    let max_pkts = ((buffer_ms.load(Ordering::Relaxed).max(20) * 6) / 8) as usize;
                    while queue.len() > max_pkts.max(24) {
                        queue.pop_front();
                    }
                    queue.push_back(pkt);
                }
                ACmd::Flush => {
                    queue.clear();
                    if let Some(ref c) = decoder {
                        let _ = c.flush();
                    }
                }
                ACmd::Stop => {
                    queue.clear();
                    drop(decoder.take());
                    drop(aaudio.take());
                    cur_fmt = None;
                }
            }
        }

        let Some(ref fmt) = cur_fmt else {
            continue;
        };
        let gain = f32::from_bits(gain_bits.load(Ordering::Relaxed));

        if fmt.codec == AudioCodec::Pcm {
            while let Some(pkt) = queue.pop_front() {
                if let Some(ref st) = aaudio {
                    let mut samples: Vec<i16> = pkt
                        .data
                        .chunks_exact(2)
                        .map(|b| i16::from_le_bytes([b[0], b[1]]))
                        .collect();
                    apply_gain(&mut samples, gain);
                    write_aaudio(st, &samples, fmt.channels.max(1) as usize);
                }
            }
            continue;
        }

        let Some(ref codec) = decoder else {
            queue.clear();
            continue;
        };

        while !queue.is_empty() {
            let Ok(In::Buffer(mut ibuf)) = codec.dequeue_input_buffer(Duration::ZERO) else {
                break;
            };
            let pkt = queue.pop_front().unwrap();
            let dst = ibuf.buffer_mut();
            let n = pkt.data.len().min(dst.len());
            unsafe {
                std::ptr::copy_nonoverlapping(
                    pkt.data.as_ptr(),
                    dst.as_mut_ptr().cast::<u8>(),
                    n,
                );
            }
            pts_us_seq += ((fmt.spf as u64) * 1_000_000) / (fmt.sample_rate.max(1) as u64);
            let _ = codec.queue_input_buffer(ibuf, 0, n, pts_us_seq, 0);
        }

        loop {
            match codec.dequeue_output_buffer(Duration::ZERO) {
                Ok(Out::Buffer(obuf)) => {
                    let (o, s) = (obuf.info().offset() as usize, obuf.info().size() as usize);
                    let raw = obuf
                        .buffer()
                        .get(o..o + s)
                        .map(<[u8]>::to_vec)
                        .unwrap_or_default();
                    let _ = codec.release_output_buffer(obuf, false);
                    if !raw.is_empty() {
                        if let Some(ref st) = aaudio {
                            let mut samples: Vec<i16> = if pcm_enc == 4 {
                                raw.chunks_exact(4)
                                    .map(|b| {
                                        (f32::from_le_bytes([b[0], b[1], b[2], b[3]]) * 32767.0)
                                            .clamp(-32768.0, 32767.0)
                                            as i16
                                    })
                                    .collect()
                            } else {
                                raw.chunks_exact(2)
                                    .map(|b| i16::from_le_bytes([b[0], b[1]]))
                                    .collect()
                            };
                            apply_gain(&mut samples, gain);
                            write_aaudio(st, &samples, fmt.channels.max(1) as usize);
                        }
                    }
                }
                Ok(Out::OutputFormatChanged) => {
                    let f = codec.output_format();
                    pcm_enc = f.i32("pcm-encoding").unwrap_or(2);
                    continue;
                }
                Ok(Out::OutputBuffersChanged) => continue,
                _ => break,
            }
        }
    }
}

#[cfg(target_os = "android")]
fn apply_gain(samples: &mut [i16], gain: f32) {
    if (gain - 1.0).abs() < 0.001 {
        return;
    }
    if gain <= 0.0001 {
        samples.fill(0);
        return;
    }
    for s in samples.iter_mut() {
        *s = ((*s as f32) * gain).clamp(-32768.0, 32767.0) as i16;
    }
}

#[cfg(target_os = "android")]
fn open_aaudio_stream(sample_rate: i32, channels: i32) -> Option<AudioStream> {
    let stream = AudioStreamBuilder::new()
        .ok()?
        .direction(AudioDirection::Output)
        .format(NdkAudioFormat::PCM_I16)
        .channel_count(channels.clamp(1, 2))
        .sample_rate(sample_rate.max(8000))
        .performance_mode(AudioPerformanceMode::LowLatency)
        .usage(AudioUsage::Media)
        .content_type(AudioContentType::Music)
        .open_stream()
        .ok()?;
    stream.request_start().ok()?;
    Some(stream)
}

#[cfg(target_os = "android")]
fn write_aaudio(st: &AudioStream, pcm: &[i16], ch: usize) {
    let mut off = 0;
    while off < pcm.len() {
        let frames = ((pcm.len() - off) / ch) as i32;
        if frames <= 0 {
            break;
        }
        match unsafe { st.write(pcm[off..].as_ptr().cast(), frames, 50_000_000) } {
            Ok(0) | Err(_) => break,
            Ok(n) => off += (n as usize) * ch,
        }
    }
}

#[cfg(target_os = "android")]
fn create_android_audio_decoder(fmt: &AudioFormat) -> Option<MediaCodec> {
    let (mime, csd) = match fmt.codec {
        AudioCodec::Alac => (
            "audio/alac",
            build_alac_csd(fmt.sample_rate, fmt.channels, fmt.spf).to_vec(),
        ),
        AudioCodec::AacLc | AudioCodec::AacEld => {
            ("audio/mp4a-latm", build_aac_csd(fmt.codec, fmt.spf))
        }
        AudioCodec::Pcm => return None,
    };
    let codec = MediaCodec::from_decoder_type(mime)?;
    let mut mf = MediaFormat::new();
    mf.set_str("mime", mime);
    mf.set_i32("sample-rate", fmt.sample_rate as i32);
    mf.set_i32("channel-count", fmt.channels as i32);
    mf.set_buffer("csd-0", &csd);
    mf.set_i32("priority", 0);

    if let Err(e) = codec.configure(&mf, None, MediaCodecDirection::Decoder) {
        log::warn!("Audio decoder configure failed ({mime}): {e:?}");
        return None;
    }
    if let Err(e) = codec.start() {
        log::warn!("Audio decoder start failed ({mime}): {e:?}");
        return None;
    }
    log::info!("Audio decoder started: {mime}");
    Some(codec)
}
