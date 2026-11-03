//! AirPlay UDP NTP timing channel (`raop_ntp.c`) and RTP <-> local `CLOCK_MONOTONIC` synchronization.
//!
//! When a session starts (`SETUP` 1 with `timingPort`), we open a local UDP socket, report our
//! local port back as `timingPort`, and spawn a thread that sends 32-byte NTP timing requests
//! (`80 d2 00 07 ...`) to the sender's `timingPort`.
//!
//! Optimization: we send an initial rapid burst of 5 probes (50ms apart) so the clock offset is
//! locked before the first video/audio packets arrive, then settle into 2.5s periodic probes.

use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use super::clock::{monotonic_ns, ns_to_ntp, ntp_to_ns};
use super::{NetStats, Sink};

pub struct NtpSync {
    local_port: u16,
    stop: Arc<AtomicBool>,
    /// `local_ns - remote_ns` in nanoseconds (`i64::MIN` until first reply arrives).
    offset_ns: Arc<AtomicI64>,
    rtt_ns: Arc<AtomicU64>,
    /// Anchor from RTCP 0xD4 sync packet for mapping 44.1kHz RTP audio timestamps.
    sync_rtp: AtomicU32,
    sync_local_ns: AtomicU64,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl NtpSync {
    pub fn start(
        client_ip: IpAddr,
        remote_timing_port: u16,
        sink: Arc<dyn Sink>,
    ) -> std::io::Result<Arc<Self>> {
        let bind_addr: SocketAddr = match client_ip {
            IpAddr::V4(_) => "0.0.0.0:0".parse().unwrap(),
            IpAddr::V6(_) => "[::]:0".parse().unwrap(),
        };
        let sock = UdpSocket::bind(bind_addr)?;
        let local_port = sock.local_addr()?.port();
        let _ = sock.set_read_timeout(Some(Duration::from_millis(80)));

        let stop = Arc::new(AtomicBool::new(false));
        let offset_ns = Arc::new(AtomicI64::new(i64::MIN));
        let rtt_ns = Arc::new(AtomicU64::new(0));

        let stop_bg = stop.clone();
        let offset_bg = offset_ns.clone();
        let rtt_bg = rtt_ns.clone();

        let thread = if remote_timing_port > 0 {
            let remote = SocketAddr::new(client_ip, remote_timing_port);
            Some(
                std::thread::Builder::new()
                    .name("yansit-ntp".into())
                    .spawn(move || {
                        ntp_loop(sock, remote, stop_bg, offset_bg, rtt_bg, sink);
                    })?,
            )
        } else {
            None
        };

        Ok(Arc::new(Self {
            local_port,
            stop,
            offset_ns,
            rtt_ns,
            sync_rtp: AtomicU32::new(0),
            sync_local_ns: AtomicU64::new(0),
            thread,
        }))
    }

    pub fn local_port(&self) -> u16 {
        self.local_port
    }

    pub fn is_synced(&self) -> bool {
        self.offset_ns.load(Ordering::Relaxed) != i64::MIN
    }

    /// Map a remote monotonic / NTP timestamp (in nanoseconds) to local `CLOCK_MONOTONIC` ns.
    pub fn remote_ns_to_local(&self, remote_ns: u64) -> u64 {
        if remote_ns == 0 {
            return 0;
        }
        let off = self.offset_ns.load(Ordering::Relaxed);
        if off == i64::MIN {
            return 0;
        }
        (remote_ns as i128 + off as i128).max(1) as u64
    }

    /// Record an RTCP 0xD4 audio sync anchor (`rtp_now` at `remote_ntp_raw`).
    pub fn record_audio_sync(&self, rtp_now: u32, remote_ntp_raw: u64) {
        let remote_ns = ntp_to_ns(remote_ntp_raw);
        let local_ns = self.remote_ns_to_local(remote_ns);
        let anchor_ns = if local_ns > 0 {
            local_ns
        } else {
            monotonic_ns()
        };
        self.sync_rtp.store(rtp_now, Ordering::Relaxed);
        self.sync_local_ns.store(anchor_ns, Ordering::Relaxed);
    }

    /// Map an audio packet's 32-bit RTP timestamp (at `sample_rate` Hz) to local `CLOCK_MONOTONIC` ns.
    pub fn rtp_to_local_ns(&self, rtp: u32, sample_rate: u32) -> u64 {
        let anchor_ns = self.sync_local_ns.load(Ordering::Relaxed);
        if anchor_ns == 0 || sample_rate == 0 {
            return 0;
        }
        let anchor_rtp = self.sync_rtp.load(Ordering::Relaxed);
        let delta_samples = rtp.wrapping_sub(anchor_rtp) as i32 as i64;
        let delta_ns = (delta_samples * 1_000_000_000i64) / (sample_rate as i64);
        (anchor_ns as i128 + delta_ns as i128).max(1) as u64
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Drop for NtpSync {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.thread.take() {
            let _ = h.join();
        }
    }
}

fn ntp_loop(
    sock: UdpSocket,
    remote: SocketAddr,
    stop: Arc<AtomicBool>,
    offset_ns: Arc<AtomicI64>,
    rtt_ns: Arc<AtomicU64>,
    sink: Arc<dyn Sink>,
) {
    let mut probe_count = 0u32;
    let mut last_send_ns = 0u64;
    let mut min_rtt_ns = u64::MAX;
    let mut buf = [0u8; 64];

    while !stop.load(Ordering::Relaxed) {
        let now = monotonic_ns();
        // Fast initial burst (5 probes at 60ms intervals), then every 2.5 seconds.
        let interval_ns = if probe_count < 5 {
            60_000_000
        } else {
            2_500_000_000
        };

        if now.saturating_sub(last_send_ns) >= interval_ns {
            let mut req = [0u8; 32];
            req[0] = 0x80;
            req[1] = 0xd2;
            req[3] = 0x07;
            let t0_ntp = ns_to_ntp(monotonic_ns());
            req[24..32].copy_from_slice(&t0_ntp.to_be_bytes());
            let _ = sock.send_to(&req, remote);
            last_send_ns = now;
            probe_count = probe_count.saturating_add(1);
        }

        if let Ok((len, _addr)) = sock.recv_from(&mut buf) {
            let t3_ns = monotonic_ns();
            if len >= 32 && buf[0] == 0x80 && buf[1] == 0xd3 {
                let t0_ntp = u64::from_be_bytes(buf[8..16].try_into().unwrap());
                let t1_ntp = u64::from_be_bytes(buf[16..24].try_into().unwrap());
                let t2_ntp = u64::from_be_bytes(buf[24..32].try_into().unwrap());

                let t0_ns = ntp_to_ns(t0_ntp);
                let t1_ns = ntp_to_ns(t1_ntp);
                let t2_ns = ntp_to_ns(t2_ntp);

                let rtt = t3_ns
                    .saturating_sub(t0_ns)
                    .saturating_sub(t2_ns.saturating_sub(t1_ns));
                let off =
                    ((t0_ns as i128 - t1_ns as i128) + (t3_ns as i128 - t2_ns as i128)) / 2;

                // Prefer samples with lower RTT (less Wi-Fi queuing jitter) or gently EWMA-smooth.
                let prev = offset_ns.load(Ordering::Relaxed);
                let new_off = if prev == i64::MIN || rtt <= min_rtt_ns.saturating_add(3_000_000) {
                    min_rtt_ns = min_rtt_ns.min(rtt);
                    if prev == i64::MIN {
                        off as i64
                    } else {
                        ((prev as i128 * 3 + off) / 4) as i64
                    }
                } else {
                    ((prev as i128 * 7 + off) / 8) as i64
                };

                offset_ns.store(new_off, Ordering::Relaxed);
                rtt_ns.store(rtt, Ordering::Relaxed);
                sink.net_stats(&NetStats {
                    ntp_delay_ms: (rtt as f64) / 1_000_000.0,
                    ntp_offset_valid: true,
                });
            }
        }
    }
}
