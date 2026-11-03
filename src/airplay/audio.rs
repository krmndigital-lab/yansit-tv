//! AirPlay RAOP UDP audio & control stream (`type = 96`), after UxPlay `lib/raop_rtp.c` and
//! `lib/raop_buffer.c`:
//! - two UDP sockets: `dataPort` (RTP audio, payload type 96) and `controlPort` (sync 0x54 and
//!   retransmission replies 0x56, sent with the RTP marker bit set);
//! - AES-128-CBC per packet with a fresh IV; only whole 16-byte blocks are encrypted;
//! - ONE [`Reorder`] buffer shared by both sockets: packets leave strictly in sequence order,
//!   duplicates are dropped (AAC-ELD senders transmit every packet several times for
//!   redundancy), a gap waits briefly for its retransmission (requested with `0x80 0xD5`) and is
//!   then skipped so a lost packet costs a tiny click instead of stalled audio;
//! - placeholders (header-only, the AAC-ELD "no data" marker, ALAC format packets) advance the
//!   sequence but are never handed to the decoder.

use aes::cipher::{BlockDecryptMut, KeyIvInit};
use cbc::Decryptor;
use std::collections::BTreeMap;
use std::io;
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::clock::monotonic_ns;
use super::ntp::NtpSync;
use super::{AudioCodec, AudioFormat, AudioPacket, Sink};

type Aes128CbcDec = Decryptor<aes::Aes128>;

/// Longest wait for a missing packet before it is skipped (a few frames: AAC-ELD frames last
/// ~10.9 ms, ALAC ~8 ms at 44.1 kHz).
const GAP_WAIT: Duration = Duration::from_millis(60);
/// Out-of-order packets held at most; beyond this a gap is skipped at once.
const MAX_PENDING: usize = 96;
/// This many consecutive "older than expected" packets mean the sender restarted numbering.
const RESYNC_AFTER_LATE: u32 = 32;
/// Socket read timeout; also bounds how long a stalled gap can hold audio back.
const READ_TIMEOUT: Duration = Duration::from_millis(40);

pub struct AudioStream {
    data_port: u16,
    control_port: u16,
    stop: Arc<AtomicBool>,
    reorder: Arc<Mutex<Reorder>>,
    data_thread: Option<std::thread::JoinHandle<()>>,
    ctrl_thread: Option<std::thread::JoinHandle<()>>,
}

impl AudioStream {
    pub fn start(
        client_ip: IpAddr,
        remote_control_port: u16,
        format: AudioFormat,
        aeskey: [u8; 16],
        aesiv: [u8; 16],
        ntp: Arc<NtpSync>,
        sink: Arc<dyn Sink>,
    ) -> io::Result<Self> {
        let bind_addr: SocketAddr = match client_ip {
            IpAddr::V4(_) => "0.0.0.0:0".parse().unwrap(),
            IpAddr::V6(_) => "[::]:0".parse().unwrap(),
        };
        let data_sock = UdpSocket::bind(bind_addr)?;
        let ctrl_sock = UdpSocket::bind(bind_addr)?;
        let data_port = data_sock.local_addr()?.port();
        let control_port = ctrl_sock.local_addr()?.port();
        let _ = data_sock.set_read_timeout(Some(READ_TIMEOUT));
        let _ = ctrl_sock.set_read_timeout(Some(READ_TIMEOUT));
        let ctrl_sender = ctrl_sock.try_clone().ok();

        sink.audio_start(&format);

        let stop = Arc::new(AtomicBool::new(false));
        let reorder = Arc::new(Mutex::new(Reorder::default()));
        let remote_ctrl = (remote_control_port > 0).then(|| SocketAddr::new(client_ip, remote_control_port));
        let ctx = Arc::new(Ctx {
            codec: format.codec,
            sample_rate: format.sample_rate,
            key: aeskey,
            iv: aesiv,
            ntp,
            sink,
            reorder: reorder.clone(),
        });

        let (ctx_d, stop_d) = (ctx.clone(), stop.clone());
        let data_thread = std::thread::Builder::new()
            .name("yansit-aud-rtp".into())
            .spawn(move || audio_data_loop(data_sock, ctrl_sender, remote_ctrl, &ctx_d, &stop_d))?;
        let (ctx_c, stop_c) = (ctx, stop.clone());
        let ctrl_thread = std::thread::Builder::new()
            .name("yansit-aud-rtcp".into())
            .spawn(move || audio_ctrl_loop(ctrl_sock, &ctx_c, &stop_c))?;

        Ok(Self {
            data_port,
            control_port,
            stop,
            reorder,
            data_thread: Some(data_thread),
            ctrl_thread: Some(ctrl_thread),
        })
    }

    pub fn data_port(&self) -> u16 {
        self.data_port
    }

    pub fn control_port(&self) -> u16 {
        self.control_port
    }

    /// RTSP FLUSH (seek / pause / next track): forget the sequence position and buffered
    /// packets so the new numbering starts cleanly.
    pub fn flush(&self) {
        self.reorder.lock().unwrap().reset();
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Drop for AudioStream {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.data_thread.take() {
            let _ = h.join();
        }
        if let Some(h) = self.ctrl_thread.take() {
            let _ = h.join();
        }
    }
}

/// State shared by the data and control threads.
struct Ctx {
    codec: AudioCodec,
    sample_rate: u32,
    key: [u8; 16],
    iv: [u8; 16],
    ntp: Arc<NtpSync>,
    sink: Arc<dyn Sink>,
    reorder: Arc<Mutex<Reorder>>,
}

impl Ctx {
    fn packet(&self, rtp_pkt: &[u8]) -> Pending {
        let seq = u16::from_be_bytes([rtp_pkt[2], rtp_pkt[3]]);
        let rtp = u32::from_be_bytes([rtp_pkt[4], rtp_pkt[5], rtp_pkt[6], rtp_pkt[7]]);
        let data = if is_placeholder(rtp_pkt, self.codec) {
            Vec::new()
        } else {
            let mut d = rtp_pkt[12..].to_vec();
            decrypt_audio_payload(&mut d, &self.key, &self.iv);
            d
        };
        Pending { data, rtp, seq, arrival: monotonic_ns() }
    }

    /// Hand in-order packets to the decoder. Called with the reorder lock held, so the data
    /// and control threads can never interleave their output out of order.
    fn deliver(&self, ready: Vec<Pending>) {
        for p in ready {
            if p.data.is_empty() {
                continue; // placeholder: keeps the sequence moving, nothing to decode
            }
            let pts_local = self.ntp.rtp_to_local_ns(p.rtp, self.sample_rate);
            self.sink.audio_packet(AudioPacket {
                data: p.data,
                rtp: p.rtp,
                seq: p.seq,
                pts_local,
                arrival: p.arrival,
            });
        }
    }

    fn push(&self, p: Pending) -> Gap {
        let mut r = self.reorder.lock().unwrap();
        let (ready, gap) = r.push(p, Instant::now());
        self.deliver(ready);
        gap
    }

    fn tick(&self) {
        let mut r = self.reorder.lock().unwrap();
        let ready = r.drain(Instant::now());
        self.deliver(ready);
    }
}

/// Header-only packets, the AAC-ELD "no data" marker and ALAC's 44-byte format-only packets
/// carry no audio (UxPlay skips the same ones), but they do use sequence numbers.
fn is_placeholder(pkt: &[u8], codec: AudioCodec) -> bool {
    pkt.len() <= 12
        || (pkt.len() == 16 && pkt[12..16] == [0x00, 0x68, 0x34, 0x00])
        || (codec == AudioCodec::Alac && pkt.len() == 44)
}

/// Decrypt an RTP audio payload in place using AES-128-CBC (reset IV per packet).
/// Only full 16-byte blocks are encrypted; any trailing `<16` bytes are left untouched.
pub fn decrypt_audio_payload(payload: &mut [u8], key: &[u8; 16], iv: &[u8; 16]) {
    let block_bytes = payload.len() & !0x0f;
    if block_bytes == 0 {
        return;
    }
    let mut dec = Aes128CbcDec::new(key.into(), iv.into());
    for chunk in payload[..block_bytes].chunks_exact_mut(16) {
        let block: &mut aes::Block = chunk.into();
        dec.decrypt_block_mut(block);
    }
}

fn audio_data_loop(
    sock: UdpSocket,
    ctrl_sender: Option<UdpSocket>,
    remote_ctrl: Option<SocketAddr>,
    ctx: &Ctx,
    stop: &AtomicBool,
) {
    let mut buf = [0u8; 4096];
    let mut resend_seq: u16 = 0;
    while !stop.load(Ordering::Relaxed) {
        match sock.recv_from(&mut buf) {
            Ok((len, _)) if len >= 12 && (buf[1] & 0x7f) == 0x60 => {
                let gap = ctx.push(ctx.packet(&buf[..len]));
                if let (Some((first, count)), Some(cs), Some(rc)) = (gap, ctrl_sender.as_ref(), remote_ctrl) {
                    // 0x80 0xD5 <our seq> <first missing seq> <count> (UxPlay resend request)
                    resend_seq = resend_seq.wrapping_add(1);
                    let mut req = [0u8; 8];
                    req[0] = 0x80;
                    req[1] = 0xd5;
                    req[2..4].copy_from_slice(&resend_seq.to_be_bytes());
                    req[4..6].copy_from_slice(&first.to_be_bytes());
                    req[6..8].copy_from_slice(&count.to_be_bytes());
                    let _ = cs.send_to(&req, rc);
                }
            }
            Ok(_) => {}
            // Read timeout: a gap whose retransmission never came must still be skipped.
            Err(_) => ctx.tick(),
        }
    }
    ctx.sink.audio_stop();
}

fn audio_ctrl_loop(sock: UdpSocket, ctx: &Ctx, stop: &AtomicBool) {
    let mut buf = [0u8; 4096];
    while !stop.load(Ordering::Relaxed) {
        let Ok((len, _addr)) = sock.recv_from(&mut buf) else {
            continue;
        };
        if len < 8 {
            continue;
        }
        match buf[1] & 0x7f {
            0x54 if len >= 20 => {
                // Sync: [4..8] RTP time "now", [8..16] sender NTP time, [16..20] next RTP time.
                let rtp_sync = u32::from_be_bytes(buf[4..8].try_into().unwrap());
                let remote_ntp = u64::from_be_bytes(buf[8..16].try_into().unwrap());
                ctx.ntp.record_audio_sync(rtp_sync, remote_ntp);
            }
            0x56 if len >= 16 => {
                // Retransmission: 4-byte header, then the original RTP packet.
                let rtp_pkt = &buf[4..len];
                if (rtp_pkt[1] & 0x7f) == 0x60 {
                    ctx.push(ctx.packet(rtp_pkt));
                }
            }
            _ => {}
        }
    }
}

/// One received packet waiting for its turn.
struct Pending {
    data: Vec<u8>,
    rtp: u32,
    seq: u16,
    arrival: u64,
}

/// A newly detected gap to request from the sender: (first missing seq, count).
type Gap = Option<(u16, u16)>;

/// Sequence-ordering, de-duplicating jitter guard for RTP audio.
#[derive(Default)]
struct Reorder {
    /// Highest extended (wrap-free) sequence number seen.
    last_ext: Option<u64>,
    /// Extended sequence number that may be delivered next.
    next: Option<u64>,
    pending: BTreeMap<u64, Pending>,
    /// When we started waiting for `next` while later packets were already here.
    gap_since: Option<Instant>,
    late_streak: u32,
}

impl Reorder {
    fn reset(&mut self) {
        *self = Reorder::default();
    }

    /// 16-bit RTP sequence → monotonically growing 64-bit position (handles wrap-around).
    fn extend(&mut self, seq: u16) -> u64 {
        match self.last_ext {
            None => {
                let e = (1u64 << 32) | seq as u64;
                self.last_ext = Some(e);
                e
            }
            Some(last) => {
                let diff = seq.wrapping_sub(last as u16) as i16 as i64;
                let e = (last as i64 + diff) as u64;
                if e > last {
                    self.last_ext = Some(e);
                }
                e
            }
        }
    }

    /// Insert one packet; returns what is now playable in order plus a newly found gap.
    fn push(&mut self, p: Pending, now: Instant) -> (Vec<Pending>, Gap) {
        let ext = self.extend(p.seq);
        let next = *self.next.get_or_insert(ext);
        if ext < next {
            self.late_streak += 1;
            if self.late_streak < RESYNC_AFTER_LATE {
                return (Vec::new(), None); // duplicate copy or too late to matter
            }
            self.reset(); // the sender restarted its numbering: follow it
            return self.push(p, now);
        }
        self.late_streak = 0;
        if self.pending.contains_key(&ext) {
            return (Vec::new(), None); // duplicate copy
        }
        self.pending.insert(ext, p);
        let had_gap = self.gap_since.is_some();
        let out = self.drain(now);
        let gap = match (had_gap, self.gap_since, self.next, self.pending.keys().next()) {
            (false, Some(_), Some(missing), Some(&have)) => {
                Some((missing as u16, (have - missing).min(u16::MAX as u64) as u16))
            }
            _ => None,
        };
        (out, gap)
    }

    /// Release in-order packets; skip a gap that waited too long or overflowed the buffer.
    fn drain(&mut self, now: Instant) -> Vec<Pending> {
        let mut out = Vec::new();
        while let Some(next) = self.next {
            if let Some(p) = self.pending.remove(&next) {
                self.next = Some(next + 1);
                self.gap_since = None;
                out.push(p);
                continue;
            }
            let Some(&first) = self.pending.keys().next() else {
                self.gap_since = None;
                break;
            };
            let since = *self.gap_since.get_or_insert(now);
            if now.duration_since(since) >= GAP_WAIT || self.pending.len() > MAX_PENDING {
                self.next = Some(first); // give up on the missing packet(s)
                self.gap_since = None;
                continue;
            }
            break;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pkt(seq: u16) -> Pending {
        Pending { data: vec![seq as u8, 1], rtp: seq as u32 * 480, seq, arrival: 0 }
    }

    fn seqs(v: &[Pending]) -> Vec<u16> {
        v.iter().map(|p| p.seq).collect()
    }

    #[test]
    fn in_order_and_duplicates() {
        let mut r = Reorder::default();
        let t = Instant::now();
        let mut got = Vec::new();
        // AAC-ELD style redundancy: every packet arrives several times.
        for s in [10u16, 10, 11, 10, 11, 12, 11, 12, 13] {
            got.extend(seqs(&r.push(pkt(s), t).0));
        }
        assert_eq!(got, vec![10, 11, 12, 13]);
    }

    #[test]
    fn reorders_and_reports_gap() {
        let mut r = Reorder::default();
        let t = Instant::now();
        assert_eq!(seqs(&r.push(pkt(1), t).0), vec![1]);
        let (out, gap) = r.push(pkt(4), t);
        assert!(out.is_empty());
        assert_eq!(gap, Some((2, 2))); // 2 and 3 missing
        let (out, gap) = r.push(pkt(3), t);
        assert!(out.is_empty() && gap.is_none()); // still waiting for 2, gap already reported
        assert_eq!(seqs(&r.push(pkt(2), t).0), vec![2, 3, 4]);
    }

    #[test]
    fn skips_a_gap_after_waiting_then_drops_the_late_packet() {
        let mut r = Reorder::default();
        let t = Instant::now();
        r.push(pkt(1), t);
        assert!(r.push(pkt(3), t).0.is_empty());
        assert!(r.drain(t + Duration::from_millis(20)).is_empty());
        assert_eq!(seqs(&r.drain(t + GAP_WAIT + Duration::from_millis(1))), vec![3]);
        assert!(r.push(pkt(2), t).0.is_empty()); // too late now
        assert_eq!(seqs(&r.push(pkt(4), t).0), vec![4]);
    }

    #[test]
    fn handles_sequence_wraparound() {
        let mut r = Reorder::default();
        let t = Instant::now();
        let mut got = Vec::new();
        for s in [65534u16, 0, 65535, 1] {
            got.extend(seqs(&r.push(pkt(s), t).0));
        }
        assert_eq!(got, vec![65534, 65535, 0, 1]);
    }

    #[test]
    fn follows_a_restarted_sequence() {
        let mut r = Reorder::default();
        let t = Instant::now();
        for s in 30000u16..30010 {
            r.push(pkt(s), t);
        }
        let mut got = Vec::new();
        for s in 100u16..140 {
            got.extend(seqs(&r.push(pkt(s), t).0));
        }
        assert!(!got.is_empty(), "never resynchronised");
        assert_eq!(*got.last().unwrap(), 139);
    }

    #[test]
    fn placeholders_are_recognised() {
        let mut p = vec![0x80u8, 0x60, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0];
        assert!(is_placeholder(&p, AudioCodec::AacEld)); // header only
        p.extend_from_slice(&[0x00, 0x68, 0x34, 0x00]);
        assert!(is_placeholder(&p, AudioCodec::AacEld)); // "no data" marker
        let alac_fmt = vec![0u8; 44];
        assert!(is_placeholder(&alac_fmt, AudioCodec::Alac));
        assert!(!is_placeholder(&alac_fmt, AudioCodec::AacEld));
        assert!(!is_placeholder(&vec![0u8; 300], AudioCodec::AacEld));
    }
}
