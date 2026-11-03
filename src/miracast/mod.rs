//! Miracast / Wi-Fi Display (WFD) Sink Receiver (100% Pure Rust).
//!
//! Initializes Android's `WifiP2pManager` WFD Sink via Rust JNI and keeps P2P peer discovery
//! active in the background. When a Wi-Fi Direct group comes up (`p2p*` interface, found with
//! `getifaddrs`), the group subnet is scanned for the source's RTSP server on TCP 7236 and the
//! WFD RTSP 1.0 client (`rtsp.rs`) + RTP/MPEG-TS demuxer (`ts.rs`) are started on that connection.

pub mod rtsp;
pub mod ts;

use crate::airplay::Sink;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
#[cfg(target_os = "android")]
use std::time::Instant;
use std::time::Duration;

struct ActiveWfdSession {
    source_ip: String,
    source_port: u16,
    stop: Arc<AtomicBool>,
}

static ACTIVE_SESSION: OnceLock<Mutex<Option<ActiveWfdSession>>> = OnceLock::new();
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
static DAEMON_STARTED: AtomicBool = AtomicBool::new(false);

fn session_lock() -> &'static Mutex<Option<ActiveWfdSession>> {
    ACTIVE_SESSION.get_or_init(|| Mutex::new(None))
}

/// Start the pure-Rust Miracast P2P WFD Sink daemon.
#[cfg(target_os = "android")]
pub fn start_daemon(vm: jni::JavaVM, sink: Arc<dyn Sink>) {
    if DAEMON_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    let _ = std::thread::Builder::new()
        .name("miracast-p2p".into())
        .spawn(move || {
            run_p2p_daemon_android(vm, sink);
        });
}

#[cfg(target_os = "android")]
fn run_p2p_daemon_android(vm: jni::JavaVM, sink: Arc<dyn Sink>) {
    use jni::objects::{GlobalRef, JObject, JValue};

    // Wait briefly for Android application context to finish initializing
    std::thread::sleep(Duration::from_millis(400));

    let mut p2p_mgr_global: Option<GlobalRef> = None;
    let mut p2p_chan_global: Option<GlobalRef> = None;

    if let Ok(mut env) = vm.attach_current_thread() {
        let init_res: Result<(GlobalRef, GlobalRef), jni::errors::Error> = (|| {
            let at_cls = env.find_class("android/app/ActivityThread")?;
            let app = env
                .call_static_method(at_cls, "currentApplication", "()Landroid/app/Application;", &[])?
                .l()?;
            if app.is_null() {
                return Err(jni::errors::Error::NullPtr("currentApplication null"));
            }

            let looper = env
                .call_method(&app, "getMainLooper", "()Landroid/os/Looper;", &[])?
                .l()?;
            let svc_name = env.new_string("wifip2p")?;
            let p2p_obj = env
                .call_method(
                    &app,
                    "getSystemService",
                    "(Ljava/lang/String;)Ljava/lang/Object;",
                    &[JValue::Object(&svc_name)],
                )?
                .l()?;
            if p2p_obj.is_null() {
                return Err(jni::errors::Error::NullPtr("wifip2p null"));
            }

            let null_listener = JObject::null();
            let chan_obj = env
                .call_method(
                    &p2p_obj,
                    "initialize",
                    "(Landroid/content/Context;Landroid/os/Looper;Landroid/net/wifi/p2p/WifiP2pManager$ChannelListener;)Landroid/net/wifi/p2p/WifiP2pManager$Channel;",
                    &[
                        JValue::Object(&app),
                        JValue::Object(&looper),
                        JValue::Object(&null_listener),
                    ],
                )?
                .l()?;
            if chan_obj.is_null() {
                return Err(jni::errors::Error::NullPtr("p2p channel null"));
            }

            // Configure WifiP2pWfdInfo (Primary Sink, WFD R2 enabled, port 7236, 300 Mbps throughput)
            let wfd_res: Result<(), jni::errors::Error> = (|| {
                let wfd_cls = env.find_class("android/net/wifi/p2p/WifiP2pWfdInfo")?;
                let wfd_info = env.new_object(wfd_cls, "()V", &[])?;
                if env
                    .call_method(&wfd_info, "setEnabled", "(Z)V", &[JValue::Bool(1)])
                    .is_err()
                {
                    let _ = env.exception_clear();
                    let _ = env.call_method(&wfd_info, "setWfdEnabled", "(Z)V", &[JValue::Bool(1)]);
                }
                // 1 = DEVICE_TYPE_PRIMARY_SINK
                let _ = env.call_method(&wfd_info, "setDeviceType", "(I)Z", &[JValue::Int(1)]);
                if env
                    .call_method(&wfd_info, "setR2DeviceType", "(I)Z", &[JValue::Int(1)])
                    .is_err()
                {
                    let _ = env.exception_clear();
                }
                let _ = env.call_method(
                    &wfd_info,
                    "setSessionAvailable",
                    "(Z)V",
                    &[JValue::Bool(1)],
                );
                let _ =
                    env.call_method(&wfd_info, "setControlPort", "(I)V", &[JValue::Int(7236)]);
                let _ =
                    env.call_method(&wfd_info, "setMaxThroughput", "(I)V", &[JValue::Int(300)]);
                let _ = env.call_method(
                    &p2p_obj,
                    "setWfdInfo",
                    "(Landroid/net/wifi/p2p/WifiP2pManager$Channel;Landroid/net/wifi/p2p/WifiP2pWfdInfo;Landroid/net/wifi/p2p/WifiP2pManager$ActionListener;)V",
                    &[
                        JValue::Object(&chan_obj),
                        JValue::Object(&wfd_info),
                        JValue::Object(&null_listener),
                    ],
                )?;
                Ok(())
            })();
            if wfd_res.is_err() {
                let _ = env.exception_clear();
            }

            let mgr_ref = env.new_global_ref(p2p_obj)?;
            let chan_ref = env.new_global_ref(chan_obj)?;
            Ok((mgr_ref, chan_ref))
        })();

        match init_res {
            Ok((mgr, chan)) => {
                log::info!("miracast: initialized WifiP2pManager WFD Sink from Rust");
                p2p_mgr_global = Some(mgr);
                p2p_chan_global = Some(chan);
            }
            Err(e) => {
                let _ = env.exception_clear();
                log::warn!("miracast: WifiP2pManager JNI init failed: {}", e);
            }
        }
    }

    // Give WifiP2pNative 800ms to finish setting up p2p0 interface
    std::thread::sleep(Duration::from_millis(800));
    log::info!(
        "miracast: getifaddrs IPv4 interfaces visible to app: {:?}",
        list_ipv4_ifaces()
    );

    let mut last_discover = Instant::now() - Duration::from_secs(30);
    // (interface name, local IPv4) of the P2P group we are currently handling.
    let mut current_group: Option<(String, Ipv4Addr)> = None;
    let mut group_seen_at = Instant::now();
    let mut session_started_for_group = false;
    let mut last_scan = Instant::now() - Duration::from_secs(10);

    loop {
        match find_p2p_ipv4() {
            Some((ifname, local_ip, mask)) => {
                let key = (ifname.clone(), local_ip);
                if current_group.as_ref() != Some(&key) {
                    log::info!(
                        "miracast: P2P group up on {} (local {} / {}) — scanning for WFD source on :7236",
                        ifname,
                        local_ip,
                        mask
                    );
                    current_group = Some(key);
                    group_seen_at = Instant::now();
                    session_started_for_group = false;
                    last_scan = Instant::now() - Duration::from_secs(10);
                }

                // Sources usually open their RTSP server 0.5–3 s after DHCP; keep scanning for up
                // to 45 s per group, but only start one session per group (no reconnect loops
                // after the sender tears the session down).
                if !session_started_for_group
                    && !is_session_active()
                    && group_seen_at.elapsed() < Duration::from_secs(45)
                    && last_scan.elapsed() >= Duration::from_millis(900)
                {
                    last_scan = Instant::now();
                    if let Some((peer_ip, stream)) = scan_for_wfd_source(local_ip, mask, 7236) {
                        log::info!(
                            "miracast: found WFD source RTSP server at {}:7236 (local {} on {})",
                            peer_ip,
                            local_ip,
                            ifname
                        );
                        session_started_for_group = true;
                        start_session(
                            peer_ip.to_string(),
                            7236,
                            "Miracast Cihazı".to_string(),
                            Some(stream),
                            sink.clone(),
                        );
                    }
                }
            }
            None => {
                if current_group.take().is_some() {
                    log::info!("miracast: P2P group removed");
                    on_peer_disconnected();
                    last_discover = Instant::now() - Duration::from_secs(30);
                }

                if last_discover.elapsed() >= Duration::from_secs(16) {
                    if let (Some(ref mgr), Some(ref chan)) = (&p2p_mgr_global, &p2p_chan_global) {
                        if let Ok(mut env) = vm.attach_current_thread() {
                            let null_listener = JObject::null();
                            let res = env.call_method(
                                mgr.as_obj(),
                                "discoverPeers",
                                "(Landroid/net/wifi/p2p/WifiP2pManager$Channel;Landroid/net/wifi/p2p/WifiP2pManager$ActionListener;)V",
                                &[JValue::Object(chan.as_obj()), JValue::Object(&null_listener)],
                            );
                            if res.is_err() {
                                let _ = env.exception_clear();
                            }
                        }
                    }
                    last_discover = Instant::now();
                }
            }
        }

        std::thread::sleep(Duration::from_millis(500));
    }
}

/// Find the IPv4 address + netmask of the active Wi-Fi Direct group interface (`p2p-p2p0-N`,
/// `p2p0`, …) via `getifaddrs` (works for untrusted apps; the `ip` binary is blocked by SELinux
/// because it binds a netlink socket). The subnet is chosen by the ROM (e.g. `192.168.105.0/24`
/// on Xiaomi, `192.168.49.0/24` on AOSP), so nothing is hard-coded.
pub fn find_p2p_ipv4() -> Option<(String, Ipv4Addr, Ipv4Addr)> {
    #[cfg(unix)]
    unsafe {
        let mut ifap: *mut libc::ifaddrs = std::ptr::null_mut();
        if libc::getifaddrs(&mut ifap) != 0 || ifap.is_null() {
            return None;
        }
        let mut found = None;
        let mut cur = ifap;
        while !cur.is_null() {
            let ifa = &*cur;
            cur = ifa.ifa_next;
            if ifa.ifa_addr.is_null() || ifa.ifa_name.is_null() {
                continue;
            }
            if (*ifa.ifa_addr).sa_family as i32 != libc::AF_INET {
                continue;
            }
            let name = std::ffi::CStr::from_ptr(ifa.ifa_name)
                .to_string_lossy()
                .into_owned();
            if !name.starts_with("p2p") {
                continue;
            }
            let sin = &*(ifa.ifa_addr as *const libc::sockaddr_in);
            let ip = Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr));
            let mask = if ifa.ifa_netmask.is_null() {
                Ipv4Addr::new(255, 255, 255, 0)
            } else {
                let m = &*(ifa.ifa_netmask as *const libc::sockaddr_in);
                Ipv4Addr::from(u32::from_be(m.sin_addr.s_addr))
            };
            if ip.is_unspecified() || ip.is_loopback() {
                continue;
            }
            found = Some((name, ip, mask));
            break;
        }
        libc::freeifaddrs(ifap);
        found
    }
    #[cfg(not(unix))]
    {
        None
    }
}

/// All IPv4 interfaces visible via `getifaddrs` (diagnostics).
pub fn list_ipv4_ifaces() -> Vec<(String, Ipv4Addr)> {
    let mut out = Vec::new();
    #[cfg(unix)]
    unsafe {
        let mut ifap: *mut libc::ifaddrs = std::ptr::null_mut();
        if libc::getifaddrs(&mut ifap) != 0 || ifap.is_null() {
            return out;
        }
        let mut cur = ifap;
        while !cur.is_null() {
            let ifa = &*cur;
            cur = ifa.ifa_next;
            if ifa.ifa_addr.is_null() || ifa.ifa_name.is_null() {
                continue;
            }
            if (*ifa.ifa_addr).sa_family as i32 != libc::AF_INET {
                continue;
            }
            let name = std::ffi::CStr::from_ptr(ifa.ifa_name).to_string_lossy().into_owned();
            let sin = &*(ifa.ifa_addr as *const libc::sockaddr_in);
            out.push((name, Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr))));
        }
        libc::freeifaddrs(ifap);
    }
    out
}

/// Candidate peer addresses inside the P2P subnet (capped to a /24 around `local`), most likely
/// first: `.1` (typical GO), then the rest. `local`, network and broadcast are skipped.
pub fn p2p_scan_candidates(local: Ipv4Addr, mask: Ipv4Addr) -> Vec<Ipv4Addr> {
    let l = u32::from(local);
    let mut m = u32::from(mask);
    if m < 0xffff_ff00 {
        m = 0xffff_ff00; // never scan more than 254 hosts
    }
    let net = l & m;
    let bcast = net | !m;
    let mut out = Vec::new();
    let first = net + 1;
    if first != l && first < bcast {
        out.push(Ipv4Addr::from(first));
    }
    for h in (net + 2)..bcast {
        if h != l {
            out.push(Ipv4Addr::from(h));
        }
    }
    out
}

/// Parallel TCP scan of the P2P subnet for a host listening on the WFD RTSP control port.
/// Returns the first connected stream (kept open and handed to the RTSP client, so the source
/// sees exactly one control connection).
pub fn scan_for_wfd_source(
    local: Ipv4Addr,
    mask: Ipv4Addr,
    port: u16,
) -> Option<(Ipv4Addr, TcpStream)> {
    // Fast path: check `/proc/net/arp` for any already-resolved peer on this P2P subnet.
    if let Ok(arp) = std::fs::read_to_string("/proc/net/arp") {
        let l = u32::from(local);
        let m = u32::from(mask).max(0xffff_ff00);
        let net = l & m;
        for line in arp.lines().skip(1) {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 6 {
                let mac = parts[3];
                let dev = parts[5];
                if mac == "00:00:00:00:00:00" {
                    continue;
                }
                if let Ok(ip) = parts[0].parse::<Ipv4Addr>() {
                    if ip != local && (dev.starts_with("p2p") || (u32::from(ip) & m) == net) {
                        let addr = SocketAddr::V4(SocketAddrV4::new(ip, port));
                        if let Ok(s) =
                            TcpStream::connect_timeout(&addr, Duration::from_millis(450))
                        {
                            return Some((ip, s));
                        }
                    }
                }
            }
        }
    }

    let candidates = Arc::new(p2p_scan_candidates(local, mask));
    if candidates.is_empty() {
        return None;
    }
    let next = Arc::new(AtomicUsize::new(0));
    let found: Arc<Mutex<Option<(Ipv4Addr, TcpStream)>>> = Arc::new(Mutex::new(None));
    let done = Arc::new(AtomicBool::new(false));
    let workers = candidates.len().min(48);

    let handles: Vec<_> = (0..workers)
        .filter_map(|_| {
            let candidates = candidates.clone();
            let next = next.clone();
            let found = found.clone();
            let done = done.clone();
            std::thread::Builder::new()
                .name("wfd-scan".into())
                .stack_size(64 * 1024)
                .spawn(move || loop {
                    if done.load(Ordering::Relaxed) {
                        break;
                    }
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(&ip) = candidates.get(i) else { break };
                    let addr = SocketAddr::V4(SocketAddrV4::new(ip, port));
                    if let Ok(s) = TcpStream::connect_timeout(&addr, Duration::from_millis(350)) {
                        let mut g = found.lock().unwrap();
                        if g.is_none() {
                            *g = Some((ip, s));
                            done.store(true, Ordering::Relaxed);
                        }
                        break;
                    }
                })
                .ok()
        })
        .collect();
    for h in handles {
        let _ = h.join();
    }
    let result = found.lock().unwrap().take();
    result
}

/// Start (or restart) a Miracast WFD Sink session with a connected P2P source (`source_ip:port`).
pub fn on_peer_connected(
    source_ip: String,
    source_port: u16,
    peer_name: String,
    sink: Arc<dyn Sink>,
) {
    start_session(source_ip, source_port, peer_name, None, sink);
}

/// Start a WFD session; `pre_connected` is an already-open RTSP control connection (from the
/// subnet scan) that is reused instead of reconnecting.
pub fn start_session(
    source_ip: String,
    source_port: u16,
    peer_name: String,
    pre_connected: Option<TcpStream>,
    sink: Arc<dyn Sink>,
) {
    let port = if source_port == 0 { 7236 } else { source_port };
    let stop = Arc::new(AtomicBool::new(false));

    {
        let mut guard = session_lock().lock().unwrap();
        if let Some(ref existing) = *guard {
            if existing.source_ip == source_ip
                && existing.source_port == port
                && !existing.stop.load(Ordering::Relaxed)
            {
                return;
            }
            existing.stop.store(true, Ordering::Relaxed);
        }
        *guard = Some(ActiveWfdSession {
            source_ip: source_ip.clone(),
            source_port: port,
            stop: stop.clone(),
        });
    }

    let stop_clone = stop.clone();
    let _ = std::thread::Builder::new()
        .name("miracast-session".into())
        .spawn(move || {
            rtsp::run_wfd_session(
                source_ip,
                port,
                peer_name,
                pre_connected,
                stop_clone.clone(),
                sink,
            );
            stop_clone.store(true, Ordering::Relaxed);
        });
}

/// Stop any active Miracast WFD Sink session when Wi-Fi P2P group disconnects.
pub fn on_peer_disconnected() {
    let mut guard = session_lock().lock().unwrap();
    if let Some(existing) = guard.take() {
        log::info!(
            "miracast: P2P peer {}:{} disconnected, stopping WFD session",
            existing.source_ip,
            existing.source_port
        );
        existing.stop.store(true, Ordering::Relaxed);
    }
}

/// Returns `true` if a Miracast session is currently running.
pub fn is_session_active() -> bool {
    let guard = session_lock().lock().unwrap();
    guard
        .as_ref()
        .map(|s| !s.stop.load(Ordering::Relaxed))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::airplay::{AudioFormat, AudioPacket, Client, Codec, Metadata, VideoFrame};
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;

    #[test]
    fn scan_candidates_xiaomi_subnet() {
        let c = p2p_scan_candidates(
            Ipv4Addr::new(192, 168, 105, 61),
            Ipv4Addr::new(255, 255, 255, 0),
        );
        assert_eq!(c.len(), 253);
        assert_eq!(c[0], Ipv4Addr::new(192, 168, 105, 1));
        assert!(c.contains(&Ipv4Addr::new(192, 168, 105, 192)));
        assert!(!c.contains(&Ipv4Addr::new(192, 168, 105, 61)));
        assert!(!c.contains(&Ipv4Addr::new(192, 168, 105, 255)));
        // A /16 mask is capped to the local /24.
        let c16 = p2p_scan_candidates(
            Ipv4Addr::new(192, 168, 49, 1),
            Ipv4Addr::new(255, 255, 0, 0),
        );
        assert_eq!(c16.len(), 253);
    }

    #[derive(Default)]
    struct NullSink {
        started: std::sync::atomic::AtomicBool,
    }
    impl Sink for NullSink {
        fn session_start(&self, _c: &Client) {
            self.started.store(true, Ordering::SeqCst);
        }
        fn session_end(&self) {}
        fn video_format(&self, _: Codec, _: &[u8], _: u32, _: u32) {}
        fn video_frame(&self, _: VideoFrame) {}
        fn video_paused(&self, _: bool) {}
        fn video_stop(&self) {}
        fn audio_start(&self, _: &AudioFormat) {}
        fn audio_packet(&self, _: AudioPacket) {}
        fn audio_flush(&self) {}
        fn audio_stop(&self) {}
        fn volume(&self, _: f32) {}
        fn metadata(&self, _: &Metadata) {}
        fn cover_art(&self, _: &[u8]) {}
        fn progress(&self, _: f64, _: f64) {}
        fn pin(&self, _: &str) {}
    }

    /// Minimal WFD source: reads one RTSP message (start line, headers, body).
    fn read_msg(r: &mut BufReader<TcpStream>) -> (String, String) {
        let mut start = String::new();
        r.read_line(&mut start).unwrap();
        let mut len = 0;
        loop {
            let mut l = String::new();
            r.read_line(&mut l).unwrap();
            if l.trim().is_empty() {
                break;
            }
            if let Some(v) = l.to_ascii_lowercase().strip_prefix("content-length:") {
                len = v.trim().parse().unwrap();
            }
        }
        let mut body = vec![0u8; len];
        r.read_exact(&mut body).unwrap();
        (start.trim().to_string(), String::from_utf8(body).unwrap())
    }

    #[test]
    fn wfd_handshake_m1_to_m7_against_fake_source() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let source = std::thread::spawn(move || {
            let (s, _) = listener.accept().unwrap();
            s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let mut w = s.try_clone().unwrap();
            let mut r = BufReader::new(s);
            // M1
            w.write_all(b"OPTIONS * RTSP/1.0\r\nCSeq: 1\r\nRequire: org.wfa.wfd1.0\r\n\r\n").unwrap();
            let (m1_resp, _) = read_msg(&mut r);
            assert!(m1_resp.starts_with("RTSP/1.0 200"));
            // M2 from sink
            let (m2, _) = read_msg(&mut r);
            assert!(m2.starts_with("OPTIONS * RTSP/1.0"));
            w.write_all(b"RTSP/1.0 200 OK\r\nCSeq: 1\r\nPublic: org.wfa.wfd1.0, SETUP, TEARDOWN, PLAY, PAUSE, GET_PARAMETER, SET_PARAMETER\r\n\r\n").unwrap();
            // M3
            let body = "wfd_video_formats\r\nwfd_audio_codecs\r\nwfd_client_rtp_ports\r\n";
            w.write_all(format!("GET_PARAMETER rtsp://localhost/wfd1.0 RTSP/1.0\r\nCSeq: 2\r\nContent-Type: text/parameters\r\nContent-Length: {}\r\n\r\n{}", body.len(), body).as_bytes()).unwrap();
            let (m3_resp, m3_body) = read_msg(&mut r);
            assert!(m3_resp.starts_with("RTSP/1.0 200"));
            assert!(m3_body.contains("wfd_client_rtp_ports: RTP/AVP/UDP;unicast"));
            // M4
            let body = "wfd_video_formats: 00 00 02 04 00000080 00000000 00000000 00 0000 0000 00 none none\r\nwfd_audio_codecs: LPCM 00000002 00\r\nwfd_presentation_URL: rtsp://127.0.0.1/wfd1.0/streamid=0 none\r\nwfd_client_rtp_ports: RTP/AVP/UDP;unicast 19028 0 mode=play\r\n";
            w.write_all(format!("SET_PARAMETER rtsp://localhost/wfd1.0 RTSP/1.0\r\nCSeq: 3\r\nContent-Type: text/parameters\r\nContent-Length: {}\r\n\r\n{}", body.len(), body).as_bytes()).unwrap();
            assert!(read_msg(&mut r).0.starts_with("RTSP/1.0 200"));
            // M5 trigger SETUP
            let body = "wfd_trigger_method: SETUP\r\n";
            w.write_all(format!("SET_PARAMETER rtsp://localhost/wfd1.0 RTSP/1.0\r\nCSeq: 4\r\nContent-Type: text/parameters\r\nContent-Length: {}\r\n\r\n{}", body.len(), body).as_bytes()).unwrap();
            assert!(read_msg(&mut r).0.starts_with("RTSP/1.0 200"));
            // M6 SETUP from sink
            let (m6, _) = read_msg(&mut r);
            assert!(m6.starts_with("SETUP rtsp://127.0.0.1/wfd1.0/streamid=0"), "{m6}");
            w.write_all(b"RTSP/1.0 200 OK\r\nCSeq: 2\r\nSession: 6B8B4567;timeout=30\r\nTransport: RTP/AVP/UDP;unicast;client_port=19028;server_port=5000-5001\r\n\r\n").unwrap();
            // M7 PLAY from sink
            let (m7, _) = read_msg(&mut r);
            assert!(m7.starts_with("PLAY rtsp://127.0.0.1/wfd1.0/streamid=0"), "{m7}");
            w.write_all(b"RTSP/1.0 200 OK\r\nCSeq: 3\r\nSession: 6B8B4567\r\n\r\n").unwrap();
            // Teardown from source ends the session.
            let body = "wfd_trigger_method: TEARDOWN\r\n";
            w.write_all(format!("SET_PARAMETER rtsp://localhost/wfd1.0 RTSP/1.0\r\nCSeq: 5\r\nContent-Type: text/parameters\r\nContent-Length: {}\r\n\r\n{}", body.len(), body).as_bytes()).unwrap();
            let _ = read_msg(&mut r);
            true
        });

        // Like the daemon: the scan finds the server and hands the open stream to the session.
        let found = scan_for_wfd_source(
            Ipv4Addr::new(127, 0, 0, 2),
            Ipv4Addr::new(255, 255, 255, 0),
            port,
        )
        .expect("scan should find the fake source");
        assert_eq!(found.0, Ipv4Addr::new(127, 0, 0, 1));

        let sink = Arc::new(NullSink::default());
        let stop = Arc::new(AtomicBool::new(false));
        let s2 = sink.clone();
        let st2 = stop.clone();
        let sess = std::thread::spawn(move || {
            rtsp::run_wfd_session("127.0.0.1".into(), port, "Test".into(), Some(found.1), st2, s2);
        });
        assert!(source.join().unwrap());
        sess.join().unwrap();
        assert!(sink.started.load(Ordering::SeqCst));
    }
}
