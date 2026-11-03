//! AirPlay TCP screen mirroring stream (`type = 110`).
//!
//! Matches UxPlay `lib/mirror_buffer.c` and `lib/raop_rtp_mirror.c`:
//! - Binds an ephemeral TCP listener and reports its `dataPort` in `SETUP` 2.
//! - Accepts the sender's TCP connection (sets `TCP_NODELAY` and large `SO_RCVBUF`).
//! - Reads 128-byte packet headers (`payload_size: u32_le`, `payload_type: u8`, `option: u8`,
//!   `timestamp_ntp: u64_le`, `width: f32_le @ 56`, `height: f32_le @ 60`).
//! - `payload_type == 0x01`: unencrypted AVCC (H.264) or HVCC (H.265) codec configuration.
//!   Converts SPS/PPS (and VPS for HEVC) to Annex-B (`00 00 00 01`), calls `Sink::video_format`,
//!   and prepends CSD to the next video access unit.
//! - `payload_type == 0x00`: AES-128-CTR encrypted video NAL units (continuous stream cipher
//!   with key = `SHA512("AirPlayStreamKey{streamConnectionID}" || aeskey)[0..16]` and
//!   IV = `SHA512("AirPlayStreamIV{streamConnectionID}" || aeskey)[0..16]`).
//!   Replaces 4-byte big-endian NAL lengths with `00 00 00 01` in-place and passes `VideoFrame`
//!   to `Sink::video_frame`.

use aes::cipher::{KeyIvInit, StreamCipher};
use aes::Aes128;
use ctr::Ctr128BE;
use sha2::{Digest, Sha512};
use std::io::{self, Read};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use super::clock::{monotonic_ns, ntp_to_ns};
use super::ntp::NtpSync;
use super::{Codec, Sink, VideoFrame};

type Aes128Ctr = Ctr128BE<Aes128>;

pub struct MirrorStream {
    port: u16,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl MirrorStream {
    pub fn start(
        stream_connection_id: u64,
        session_aeskey: [u8; 16],
        ntp: Arc<NtpSync>,
        sink: Arc<dyn Sink>,
    ) -> io::Result<Self> {
        let listener = TcpListener::bind("0.0.0.0:0")?;
        let port = listener.local_addr()?.port();
        let _ = listener.set_nonblocking(true);

        let (key, iv) = derive_mirror_key_iv(stream_connection_id, &session_aeskey);
        let stop = Arc::new(AtomicBool::new(false));
        let stop_bg = stop.clone();

        let thread = std::thread::Builder::new()
            .name("yansit-mirror".into())
            .spawn(move || {
                mirror_accept_loop(listener, key, iv, ntp, sink, stop_bg);
            })?;

        Ok(Self {
            port,
            stop,
            thread: Some(thread),
        })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Drop for MirrorStream {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.thread.take() {
            let _ = h.join();
        }
    }
}

pub fn derive_mirror_key_iv(stream_connection_id: u64, aeskey: &[u8; 16]) -> ([u8; 16], [u8; 16]) {
    let key_str = format!("AirPlayStreamKey{stream_connection_id}");
    let iv_str = format!("AirPlayStreamIV{stream_connection_id}");

    let mut hk = Sha512::new();
    hk.update(key_str.as_bytes());
    hk.update(aeskey);
    let dk = hk.finalize();

    let mut hiv = Sha512::new();
    hiv.update(iv_str.as_bytes());
    hiv.update(aeskey);
    let div = hiv.finalize();

    let mut key = [0u8; 16];
    let mut iv = [0u8; 16];
    key.copy_from_slice(&dk[..16]);
    iv.copy_from_slice(&div[..16]);
    (key, iv)
}

fn mirror_accept_loop(
    listener: TcpListener,
    key: [u8; 16],
    iv: [u8; 16],
    ntp: Arc<NtpSync>,
    sink: Arc<dyn Sink>,
    stop: Arc<AtomicBool>,
) {
    // Wait up to 8 seconds for the sender to connect to our mirror dataPort.
    let deadline = monotonic_ns() + 8_000_000_000;
    let stream = loop {
        if stop.load(Ordering::Relaxed) || monotonic_ns() > deadline {
            return;
        }
        match listener.accept() {
            Ok((s, _addr)) => break s,
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(_) => return,
        }
    };

    let _ = stream.set_nonblocking(false);
    let _ = stream.set_nodelay(true);
    let _ = stream.set_read_timeout(Some(Duration::from_millis(250)));

    let mut cipher = Aes128Ctr::new((&key).into(), (&iv).into());
    let _ = process_mirror_stream(stream, &mut cipher, &ntp, sink.as_ref(), &stop);
    sink.video_stop();
}

pub fn process_mirror_stream<R: Read>(
    mut reader: R,
    cipher: &mut Aes128Ctr,
    ntp: &NtpSync,
    sink: &dyn Sink,
    stop: &AtomicBool,
) -> io::Result<()> {
    let mut hdr = [0u8; 128];
    let mut codec = Codec::H264;
    let mut pending_csd: Option<Vec<u8>> = None;
    let mut paused = false;

    while !stop.load(Ordering::Relaxed) {
        match read_exact_interruptible(&mut reader, &mut hdr, stop) {
            Ok(true) => {}
            Ok(false) => break,
            Err(_) => break,
        }

        let payload_size = u32::from_le_bytes(hdr[0..4].try_into().unwrap()) as usize;
        if payload_size > 16 * 1024 * 1024 {
            // Guard against corrupt packet headers
            break;
        }
        let payload_type = hdr[4] & 0x0f;
        let option = hdr[6];
        let ntp_raw = u64::from_le_bytes(hdr[8..16].try_into().unwrap());
        let remote_ns = ntp_to_ns(ntp_raw);
        let pts_local = ntp.remote_ns_to_local(remote_ns);

        if payload_type == 0x00 && (option == 0x56 || option == 0x5e) {
            if !paused {
                paused = true;
                sink.video_paused(true);
            }
        } else if payload_type == 0x00 && (option == 0x16 || option == 0x1e) && paused {
            paused = false;
            sink.video_paused(false);
        }

        let mut payload = vec![0u8; payload_size];
        if payload_size > 0 {
            if !read_exact_interruptible(&mut reader, &mut payload, stop)? {
                break;
            }
        }
        let arrival = monotonic_ns();

        match payload_type {
            0x01 => {
                // Codec configuration packet (AVCC or HVCC)
                let w = f32::from_le_bytes(hdr[56..60].try_into().unwrap()).round() as u32;
                let h = f32::from_le_bytes(hdr[60..64].try_into().unwrap()).round() as u32;
                let width = if w > 0 { w } else { 1920 };
                let height = if h > 0 { h } else { 1080 };

                if let Some((detected_codec, csd)) = parse_codec_config(&payload) {
                    codec = detected_codec;
                    sink.video_format(codec, &csd, width, height);
                    pending_csd = Some(csd);
                }
            }
            0x00 => {
                if payload.is_empty() {
                    continue;
                }
                // Decrypt video payload in-place with continuous AES-128-CTR stream
                cipher.apply_keystream(&mut payload);

                // Convert 4-byte big-endian NAL lengths to Annex-B 00 00 00 01 start codes
                let is_key = convert_avcc_to_annexb(&mut payload, codec);

                let frame_data = if let Some(csd) = pending_csd.take() {
                    let mut combined = Vec::with_capacity(csd.len() + payload.len());
                    combined.extend_from_slice(&csd);
                    combined.extend_from_slice(&payload);
                    combined
                } else {
                    payload
                };

                sink.video_frame(VideoFrame {
                    data: frame_data,
                    key: is_key,
                    pts_local,
                    arrival,
                });
            }
            _ => {
                // Heartbeat (type 5) or other control packet — ignore body
            }
        }
    }
    Ok(())
}

fn read_exact_interruptible<R: Read>(
    reader: &mut R,
    mut buf: &mut [u8],
    stop: &AtomicBool,
) -> io::Result<bool> {
    while !buf.is_empty() {
        if stop.load(Ordering::Relaxed) {
            return Ok(false);
        }
        match reader.read(buf) {
            Ok(0) => return Ok(false),
            Ok(n) => buf = &mut buf[n..],
            Err(ref e)
                if e.kind() == io::ErrorKind::WouldBlock
                    || e.kind() == io::ErrorKind::TimedOut
                    || e.kind() == io::ErrorKind::Interrupted =>
            {
                continue;
            }
            Err(e) => return Err(e),
        }
    }
    Ok(true)
}

/// Extract Annex-B CSD (`00 00 00 01` + VPS/SPS/PPS) from AVCC or HVCC payload.
pub fn parse_codec_config(payload: &[u8]) -> Option<(Codec, Vec<u8>)> {
    // Check if H.265 `hvc1` box (UxPlay `raop_rtp_mirror.c:218`) or raw HVCC record
    if payload.len() > 0x76 && &payload[4..8] == b"hvc1" {
        if let Some(csd) = parse_hvcc_arrays(&payload[0x74..]) {
            return Some((Codec::H265, csd));
        }
    }
    // Also support direct HVCC configuration record (`configurationVersion == 1` and numOfArrays at byte 22)
    if payload.len() > 23 && payload[0] == 1 && (payload[22] >= 3 && payload[22] <= 8) && payload[5] != 0xe1 {
        if let Some(csd) = parse_hvcc_arrays(&payload[22..]) {
            return Some((Codec::H265, csd));
        }
    }

    // Standard H.264 AVCDecoderConfigurationRecord
    if payload.len() >= 11 {
        let num_sps = (payload[5] & 0x1f) as usize;
        let mut idx = 6;
        let mut out = Vec::with_capacity(128);
        for _ in 0..num_sps {
            if idx + 2 > payload.len() {
                return None;
            }
            let sps_len = u16::from_be_bytes([payload[idx], payload[idx + 1]]) as usize;
            idx += 2;
            if idx + sps_len > payload.len() {
                return None;
            }
            out.extend_from_slice(&[0, 0, 0, 1]);
            out.extend_from_slice(&payload[idx..idx + sps_len]);
            idx += sps_len;
        }
        if idx < payload.len() {
            let num_pps = payload[idx] as usize;
            idx += 1;
            for _ in 0..num_pps {
                if idx + 2 > payload.len() {
                    return None;
                }
                let pps_len = u16::from_be_bytes([payload[idx], payload[idx + 1]]) as usize;
                idx += 2;
                if idx + pps_len > payload.len() {
                    return None;
                }
                out.extend_from_slice(&[0, 0, 0, 1]);
                out.extend_from_slice(&payload[idx..idx + pps_len]);
                idx += pps_len;
            }
        }
        if !out.is_empty() {
            return Some((Codec::H264, out));
        }
    }
    None
}

fn parse_hvcc_arrays(data: &[u8]) -> Option<Vec<u8>> {
    if data.is_empty() {
        return None;
    }
    let num_arrays = data[0] as usize;
    let mut idx = 1;
    let mut out = Vec::with_capacity(256);
    for _ in 0..num_arrays {
        if idx + 3 > data.len() {
            return None;
        }
        let _nal_type = data[idx] & 0x3f;
        let num_nalus = u16::from_be_bytes([data[idx + 1], data[idx + 2]]) as usize;
        idx += 3;
        for _ in 0..num_nalus {
            if idx + 2 > data.len() {
                return None;
            }
            let nal_len = u16::from_be_bytes([data[idx], data[idx + 1]]) as usize;
            idx += 2;
            if idx + nal_len > data.len() {
                return None;
            }
            out.extend_from_slice(&[0, 0, 0, 1]);
            out.extend_from_slice(&data[idx..idx + nal_len]);
            idx += nal_len;
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// Replace 4-byte big-endian NAL unit lengths with `00 00 00 01` in place and detect IDR keyframes.
pub fn convert_avcc_to_annexb(buf: &mut [u8], codec: Codec) -> bool {
    let mut idx = 0usize;
    let mut is_key = false;
    while idx + 4 <= buf.len() {
        let nal_len = u32::from_be_bytes(buf[idx..idx + 4].try_into().unwrap()) as usize;
        buf[idx] = 0;
        buf[idx + 1] = 0;
        buf[idx + 2] = 0;
        buf[idx + 3] = 1;
        if idx + 4 < buf.len() {
            let hdr_byte = buf[idx + 4];
            match codec {
                Codec::H264 => {
                    let nal_type = hdr_byte & 0x1f;
                    if nal_type == 5 || nal_type == 7 {
                        is_key = true;
                    }
                }
                Codec::H265 => {
                    let nal_type = (hdr_byte >> 1) & 0x3f;
                    // IDR_W_RADL (19), IDR_N_LP (20), CRA_NUT (21), VPS (32), SPS (33)
                    if (19..=21).contains(&nal_type) || nal_type == 32 || nal_type == 33 {
                        is_key = true;
                    }
                }
            }
        }
        if nal_len == 0 || idx + 4 + nal_len > buf.len() {
            break;
        }
        idx += 4 + nal_len;
    }
    is_key
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn avcc_to_annexb_and_keyframe_detection() {
        // Fake H.264 IDR NAL (type 5 -> 0x65) of length 4 + non-IDR (type 1 -> 0x41) of length 3
        let mut data = vec![0, 0, 0, 4, 0x65, 0xaa, 0xbb, 0xcc, 0, 0, 0, 3, 0x41, 0x11, 0x22];
        let is_key = convert_avcc_to_annexb(&mut data, Codec::H264);
        assert!(is_key);
        assert_eq!(&data[0..4], &[0, 0, 0, 1]);
        assert_eq!(&data[8..12], &[0, 0, 0, 1]);
    }
}
