//! AirPlay RTSP/HTTP server (`Server`).
//!
//! Listens on `cfg.port` (default 7000, with automatic fallback to an ephemeral port if busy),
//! accepts sender connections, and drives the AirPlay 2 classic mirroring & RAOP audio state
//! machine.

use plist::{Dictionary, Integer, Value};
use std::io::{self, BufReader};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use super::audio::AudioStream;
use super::dmap;
use super::fairplay::FairPlay;
use super::http::{self, Request, Response};
use super::mirror::MirrorStream;
use super::ntp::NtpSync;
use super::pairing::{derive_session_aeskey, Identity, PairingSession};
use super::pin::{generate_pin, PinPairing};
use super::plistx;
use super::txt;
use super::{AudioCodec, AudioFormat, Client, Config, Sink};

pub struct Server {
    cfg: Arc<RwLock<Config>>,
    identity: Identity,
    port: u16,
    stop: Arc<AtomicBool>,
    session_gen: Arc<AtomicU64>,
    active_conn: Arc<Mutex<Option<TcpStream>>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Server {
    pub fn start(cfg: Config, sink: Arc<dyn Sink>) -> io::Result<Self> {
        let identity = Identity::load_or_create(&cfg.state_dir, &cfg.device_id);
        let listener = TcpListener::bind(("0.0.0.0", cfg.port))
            .or_else(|_| TcpListener::bind(("0.0.0.0", 0)))?;
        let port = listener.local_addr()?.port();
        let _ = listener.set_nonblocking(true);

        let mut cfg_actual = cfg;
        cfg_actual.port = port;
        let cfg_lock = Arc::new(RwLock::new(cfg_actual));
        let stop = Arc::new(AtomicBool::new(false));
        let session_gen = Arc::new(AtomicU64::new(0));
        let active_conn = Arc::new(Mutex::new(None));

        let cfg_bg = cfg_lock.clone();
        let id_bg = identity.clone();
        let stop_bg = stop.clone();
        let gen_bg = session_gen.clone();
        let conn_bg = active_conn.clone();

        let thread = std::thread::Builder::new()
            .name("yansit-rtsp".into())
            .spawn(move || {
                accept_loop(listener, cfg_bg, id_bg, sink, stop_bg, gen_bg, conn_bg);
            })?;

        Ok(Self {
            cfg: cfg_lock,
            identity,
            port,
            stop,
            session_gen,
            active_conn,
            thread: Some(thread),
        })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn public_key_hex(&self) -> String {
        self.identity.public_hex()
    }

    pub fn txt_airplay(&self) -> Vec<(String, String)> {
        let cfg = self.cfg.read().unwrap();
        txt::txt_airplay(&cfg, &self.identity.public_hex())
    }

    pub fn txt_raop(&self) -> Vec<(String, String)> {
        let cfg = self.cfg.read().unwrap();
        txt::txt_raop(&cfg, &self.identity.public_hex())
    }

    pub fn raop_service_name(&self) -> String {
        let cfg = self.cfg.read().unwrap();
        txt::raop_service_name(&cfg)
    }

    pub fn server_name(&self) -> String {
        self.cfg.read().unwrap().name.clone()
    }

    /// Update configuration live (resolution, FPS, H.265, device name, PIN requirement, etc.).
    pub fn update_config(&self, mut new_cfg: Config) {
        new_cfg.port = self.port;
        *self.cfg.write().unwrap() = new_cfg;
    }

    /// Disconnect the currently connected AirPlay sender, if any.
    pub fn disconnect_client(&self) {
        self.session_gen.fetch_add(1, Ordering::SeqCst);
        if let Some(s) = self.active_conn.lock().unwrap().take() {
            let _ = s.shutdown(std::net::Shutdown::Both);
        }
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
        self.disconnect_client();
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop();
        if let Some(h) = self.thread.take() {
            let _ = h.join();
        }
    }
}

fn accept_loop(
    listener: TcpListener,
    cfg: Arc<RwLock<Config>>,
    identity: Identity,
    sink: Arc<dyn Sink>,
    stop: Arc<AtomicBool>,
    session_gen: Arc<AtomicU64>,
    active_conn: Arc<Mutex<Option<TcpStream>>>,
) {
    while !stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, peer)) => {
                let _ = stream.set_nonblocking(false);
                let _ = stream.set_nodelay(true);
                let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));

                let cfg_c = cfg.clone();
                let id_c = identity.clone();
                let sink_c = sink.clone();
                let stop_c = stop.clone();
                let gen_c = session_gen.clone();
                let conn_c = active_conn.clone();

                let _ = std::thread::Builder::new()
                    .name("yansit-conn".into())
                    .spawn(move || {
                        handle_connection(
                            stream, peer, cfg_c, id_c, sink_c, stop_c, gen_c, conn_c,
                        );
                    });
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(_) => {
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
}

struct SessionState {
    my_gen: u64,
    client: Client,
    pairing: PairingSession,
    pin_pairing: PinPairing,
    fairplay: FairPlay,
    session_aeskey: [u8; 16],
    session_aesiv: [u8; 16],
    ntp: Option<Arc<NtpSync>>,
    event_listener: Option<TcpListener>,
    mirror: Option<MirrorStream>,
    audio: Option<AudioStream>,
    started: bool,
}

impl SessionState {
    fn stop_all(&mut self, sink: &dyn Sink, session_gen: &AtomicU64) {
        if let Some(m) = self.mirror.take() {
            m.stop();
        }
        if let Some(a) = self.audio.take() {
            a.stop();
        }
        if let Some(n) = self.ntp.take() {
            n.stop();
        }
        self.event_listener = None;
        if self.started {
            self.started = false;
            // Only notify session_end if a newer connection hasn't already taken over
            if session_gen.load(Ordering::SeqCst) == self.my_gen {
                sink.pin("");
                sink.session_end();
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_connection(
    stream: TcpStream,
    peer: SocketAddr,
    cfg: Arc<RwLock<Config>>,
    identity: Identity,
    sink: Arc<dyn Sink>,
    stop: Arc<AtomicBool>,
    session_gen: Arc<AtomicU64>,
    active_conn: Arc<Mutex<Option<TcpStream>>>,
) {
    let Ok(mut writer) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::with_capacity(16 * 1024, stream);

    let mut state = SessionState {
        my_gen: session_gen.load(Ordering::SeqCst),
        client: Client {
            addr: peer.ip().to_string(),
            ..Default::default()
        },
        pairing: PairingSession::new(identity.clone()),
        pin_pairing: PinPairing::new(identity.clone(), String::from("1234")),
        fairplay: FairPlay::new(),
        session_aeskey: [0u8; 16],
        session_aesiv: [0u8; 16],
        ntp: None,
        event_listener: None,
        mirror: None,
        audio: None,
        started: false,
    };

    while !stop.load(Ordering::Relaxed) {
        let req = match http::read_request(&mut reader) {
            Ok(Some(r)) => r,
            Ok(None) => break,
            Err(ref e)
                if e.kind() == io::ErrorKind::WouldBlock
                    || e.kind() == io::ErrorKind::TimedOut =>
            {
                if state.started && session_gen.load(Ordering::SeqCst) != state.my_gen {
                    break;
                }
                continue;
            }
            Err(_) => break,
        };

        if let Some(dacp) = req.header("DACP-ID") {
            state.client.dacp_id = dacp.to_string();
        }
        if let Some(ar) = req.header("Active-Remote") {
            state.client.active_remote = ar.to_string();
        }

        let (resp, should_close) = dispatch_request(
            &req,
            peer.ip(),
            &cfg,
            &identity,
            &sink,
            &session_gen,
            &active_conn,
            &writer,
            &mut state,
        );

        if http::write_response(&mut writer, &req, &resp).is_err() {
            break;
        }
        if should_close {
            break;
        }
    }

    state.stop_all(sink.as_ref(), &session_gen);
}

#[allow(clippy::too_many_arguments)]
fn dispatch_request(
    req: &Request,
    client_ip: IpAddr,
    cfg_lock: &Arc<RwLock<Config>>,
    identity: &Identity,
    sink: &Arc<dyn Sink>,
    session_gen: &AtomicU64,
    active_conn: &Mutex<Option<TcpStream>>,
    writer_stream: &TcpStream,
    state: &mut SessionState,
) -> (Response, bool) {
    let method = req.method.as_str();
    let path = req.url.as_str();

    match (method, path) {
        ("OPTIONS", _) => {
            let resp = Response::ok().header(
                "Public",
                "ANNOUNCE, SETUP, RECORD, PAUSE, FLUSH, TEARDOWN, OPTIONS, GET_PARAMETER, SET_PARAMETER, POST, GET",
            );
            (resp, false)
        }
        ("GET", "/info") => {
            let cfg = cfg_lock.read().unwrap().clone();
            let body = plistx::info_response(&cfg, &identity.public_bytes(), &req.body);
            (
                Response::ok().with_body("application/x-apple-binary-plist", body),
                false,
            )
        }
        ("POST", "/pair-pin-start") => {
            let pin = generate_pin();
            state.pin_pairing.set_pin(pin.clone());
            sink.pin(&pin);
            (Response::ok(), false)
        }
        ("POST", "/pair-setup-pin") => match state.pin_pairing.handle_setup_pin(&req.body) {
            Ok((body, finished)) => {
                if finished {
                    sink.pin("");
                }
                (
                    Response::ok().with_body("application/x-apple-binary-plist", body),
                    false,
                )
            }
            Err(_) => {
                sink.pin("");
                (Response::status(403, "Forbidden"), false)
            }
        },
        ("POST", "/pair-setup") => {
            let pk = state.pairing.pair_setup(&req.body);
            (
                Response::ok().with_body("application/octet-stream", pk.to_vec()),
                false,
            )
        }
        ("POST", "/pair-verify") => match state.pairing.pair_verify(&req.body) {
            Ok(out) => (
                Response::ok().with_body("application/octet-stream", out),
                false,
            ),
            Err(_) => (Response::status(403, "Forbidden"), false),
        },
        ("POST", "/fp-setup") => {
            let reply = if let Ok(arr) = <&[u8; 16]>::try_from(req.body.as_slice()) {
                state.fairplay.setup(arr).ok().map(|a| a.to_vec())
            } else if let Ok(arr) = <&[u8; 164]>::try_from(req.body.as_slice()) {
                state.fairplay.handshake(arr).ok().map(|a| a.to_vec())
            } else {
                None
            };
            match reply {
                Some(bytes) => (
                    Response::ok().with_body("application/octet-stream", bytes),
                    false,
                ),
                None => (Response::status(400, "Bad Request"), false),
            }
        }
        ("SETUP", _) => handle_setup(
            req,
            client_ip,
            sink,
            session_gen,
            active_conn,
            writer_stream,
            state,
        ),
        ("RECORD", _) => {
            let latency_ms = cfg_lock.read().unwrap().audio_latency_ms;
            let samples = (latency_ms * 44100) / 1000;
            let resp = Response::ok().header("Audio-Latency", samples.to_string());
            (resp, false)
        }
        ("GET_PARAMETER", _) => {
            let body_str = String::from_utf8_lossy(&req.body);
            if body_str.contains("volume") {
                (
                    Response::ok().with_body("text/parameters", b"volume: 0.000000\r\n".to_vec()),
                    false,
                )
            } else {
                (Response::ok(), false)
            }
        }
        ("SET_PARAMETER", _) => {
            let ct = req.header("Content-Type").unwrap_or("");
            if ct.starts_with("text/parameters") {
                let text = String::from_utf8_lossy(&req.body);
                for line in text.lines() {
                    if let Some(rest) = line.strip_prefix("volume:") {
                        if let Ok(db) = rest.trim().parse::<f32>() {
                            sink.volume(db);
                        }
                    } else if let Some(rest) = line.strip_prefix("progress:") {
                        let parts: Vec<&str> = rest.trim().split('/').collect();
                        if parts.len() == 3 {
                            if let (Ok(start), Ok(curr), Ok(end)) = (
                                parts[0].parse::<f64>(),
                                parts[1].parse::<f64>(),
                                parts[2].parse::<f64>(),
                            ) {
                                let dur = (end - start).max(0.0) / 44100.0;
                                let el = (curr - start).clamp(0.0, (end - start).max(0.0)) / 44100.0;
                                sink.progress(el, dur);
                            }
                        }
                    }
                }
            } else if ct.starts_with("application/x-dmap-tagged") {
                let meta = dmap::parse(&req.body);
                sink.metadata(&meta);
            } else if ct.starts_with("image/") {
                if !req.body.is_empty() {
                    sink.cover_art(&req.body);
                }
            }
            (Response::ok(), false)
        }
        ("FLUSH", _) => {
            // Seek / pause / next track: restart sequence tracking, then drop decoded audio.
            if let Some(a) = state.audio.as_ref() {
                a.flush();
            }
            sink.audio_flush();
            (Response::ok(), false)
        }
        ("TEARDOWN", _) => {
            let dict = plistx::from_bytes(&req.body).unwrap_or_default();
            if let Some(Value::Array(streams)) = dict.get("streams") {
                for s in streams {
                    if let Some(sd) = s.as_dictionary() {
                        match plistx::get_u64(sd, "type") {
                            Some(110) => {
                                if let Some(m) = state.mirror.take() {
                                    m.stop();
                                }
                            }
                            Some(96) => {
                                if let Some(a) = state.audio.take() {
                                    a.stop();
                                }
                            }
                            _ => {}
                        }
                    }
                }
                if state.mirror.is_none() && state.audio.is_none() {
                    state.stop_all(sink.as_ref(), session_gen);
                    return (Response::ok(), true);
                }
                (Response::ok(), false)
            } else {
                state.stop_all(sink.as_ref(), session_gen);
                (Response::ok(), true)
            }
        }
        ("POST", "/reverse") => {
            let resp = Response::status(101, "Switching Protocols")
                .header("Upgrade", "PTTH/1.0")
                .header("Connection", "Upgrade");
            (resp, false)
        }
        ("POST", "/feedback") | ("POST", "/audioMode") | ("POST", "/stop") | ("PAUSE", _) => {
            (Response::ok(), false)
        }
        _ => (Response::ok(), false),
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_setup(
    req: &Request,
    client_ip: IpAddr,
    sink: &Arc<dyn Sink>,
    session_gen: &AtomicU64,
    active_conn: &Mutex<Option<TcpStream>>,
    writer_stream: &TcpStream,
    state: &mut SessionState,
) -> (Response, bool) {
    let Some(dict) = plistx::from_bytes(&req.body) else {
        return (Response::status(400, "Bad Request"), false);
    };

    let mut resp_dict = Dictionary::new();

    // Phase 1: Session initialization (contains `ekey`, `eiv`, `timingPort`, sender info).
    if dict.get("ekey").is_some() || dict.get("timingPort").is_some() {
        if let Some(name) = plistx::get_str(&dict, "name") {
            state.client.name = name.to_string();
        }
        if let Some(model) = plistx::get_str(&dict, "model") {
            state.client.model = model.to_string();
        }
        if let Some(dev_id) = plistx::get_str(&dict, "deviceID")
            .or_else(|| plistx::get_str(&dict, "macAddress"))
        {
            state.client.device_id = dev_id.to_string();
        }

        if let Some(ekey) = plistx::get_data(&dict, "ekey") {
            if let Ok(arr) = <&[u8; 72]>::try_from(ekey) {
                if let Ok(raw_aeskey) = state.fairplay.decrypt(arr) {
                    state.session_aeskey =
                        derive_session_aeskey(&raw_aeskey, state.pairing.ecdh_secret());
                }
            }
        }
        if let Some(eiv) = plistx::get_data(&dict, "eiv") {
            if eiv.len() >= 16 {
                state.session_aesiv.copy_from_slice(&eiv[..16]);
            }
        }

        // Take over active session slot (disconnecting any stale previous sender)
        let new_gen = session_gen.fetch_add(1, Ordering::SeqCst) + 1;
        state.my_gen = new_gen;
        if let Ok(mut guard) = active_conn.lock() {
            if let Some(old) = guard.take() {
                let _ = old.shutdown(std::net::Shutdown::Both);
            }
            if let Ok(cloned) = writer_stream.try_clone() {
                *guard = Some(cloned);
            }
        }

        let remote_timing_port = plistx::get_u64(&dict, "timingPort").unwrap_or(0) as u16;
        let ntp = NtpSync::start(client_ip, remote_timing_port, sink.clone()).unwrap_or_else(
            |_| {
                NtpSync::start(IpAddr::V4(Ipv4Addr::LOCALHOST), 0, sink.clone())
                    .expect("fallback ntp")
            },
        );
        let local_timing_port = ntp.local_port();
        state.ntp = Some(ntp);

        let event_port = match TcpListener::bind("0.0.0.0:0") {
            Ok(l) => {
                let p = l.local_addr().map(|a| a.port()).unwrap_or(0);
                state.event_listener = Some(l);
                p
            }
            Err(_) => 0,
        };

        if !state.started {
            state.started = true;
            sink.session_start(&state.client);
        }

        resp_dict.insert(
            "timingPort".into(),
            Value::Integer(Integer::from(local_timing_port as u64)),
        );
        resp_dict.insert(
            "eventPort".into(),
            Value::Integer(Integer::from(event_port as u64)),
        );
    }

    // Phase 2: Stream setup (`streams` array containing type 110 mirroring and/or type 96 audio).
    if let Some(Value::Array(streams)) = dict.get("streams") {
        if !state.started {
            state.started = true;
            sink.session_start(&state.client);
        }
        let ntp = state.ntp.clone().unwrap_or_else(|| {
            let n = NtpSync::start(client_ip, 0, sink.clone()).expect("ntp");
            state.ntp = Some(n.clone());
            n
        });

        let mut out_streams = Vec::new();
        for s in streams {
            let Some(sd) = s.as_dictionary() else {
                continue;
            };
            let stream_type = plistx::get_u64(sd, "type").unwrap_or(0);
            match stream_type {
                110 => {
                    // Screen mirroring stream (TCP)
                    let conn_id = plistx::get_u64(sd, "streamConnectionID").unwrap_or(0);
                    if let Some(old) = state.mirror.take() {
                        old.stop();
                    }
                    if let Ok(mirror) = MirrorStream::start(
                        conn_id,
                        state.session_aeskey,
                        ntp.clone(),
                        sink.clone(),
                    ) {
                        let port = mirror.port();
                        state.mirror = Some(mirror);
                        let mut rs = Dictionary::new();
                        rs.insert("type".into(), Value::Integer(Integer::from(110u64)));
                        rs.insert("dataPort".into(), Value::Integer(Integer::from(port as u64)));
                        out_streams.push(Value::Dictionary(rs));
                    }
                }
                96 => {
                    // Realtime RAOP audio stream (UDP)
                    let ct = plistx::get_u64(sd, "ct").unwrap_or(2);
                    let codec = AudioCodec::from_ct(ct).unwrap_or(AudioCodec::Alac);
                    let spf = plistx::get_u64(sd, "spf").unwrap_or(match codec {
                        AudioCodec::Alac => 352,
                        AudioCodec::AacLc => 1024,
                        AudioCodec::AacEld => 480,
                        AudioCodec::Pcm => 352,
                    }) as u32;
                    let sr = plistx::get_u64(sd, "sr").unwrap_or(44100) as u32;
                    let ch = plistx::get_u64(sd, "ch").unwrap_or(2) as u32;
                    let remote_ctrl = plistx::get_u64(sd, "controlPort").unwrap_or(0) as u16;
                    let using_screen = plistx::get_bool(sd, "usingScreen").unwrap_or(false);
                    let is_media = plistx::get_bool(sd, "isMedia").unwrap_or(false);

                    let fmt = AudioFormat {
                        codec,
                        spf,
                        sample_rate: sr,
                        channels: ch,
                        using_screen,
                        is_media,
                    };

                    if let Some(old) = state.audio.take() {
                        old.stop();
                    }
                    if let Ok(aud) = AudioStream::start(
                        client_ip,
                        remote_ctrl,
                        fmt,
                        state.session_aeskey,
                        state.session_aesiv,
                        ntp.clone(),
                        sink.clone(),
                    ) {
                        let dp = aud.data_port();
                        let cp = aud.control_port();
                        state.audio = Some(aud);
                        let mut rs = Dictionary::new();
                        rs.insert("type".into(), Value::Integer(Integer::from(96u64)));
                        rs.insert("dataPort".into(), Value::Integer(Integer::from(dp as u64)));
                        rs.insert(
                            "controlPort".into(),
                            Value::Integer(Integer::from(cp as u64)),
                        );
                        out_streams.push(Value::Dictionary(rs));
                    }
                }
                _ => {
                    // Unknown stream type — allocate dummy UDP port so sender doesn't fail
                    let dp = UdpSocket::bind("0.0.0.0:0")
                        .and_then(|u| u.local_addr())
                        .map(|a| a.port())
                        .unwrap_or(0);
                    let mut rs = Dictionary::new();
                    rs.insert("type".into(), Value::Integer(Integer::from(stream_type)));
                    rs.insert("dataPort".into(), Value::Integer(Integer::from(dp as u64)));
                    out_streams.push(Value::Dictionary(rs));
                }
            }
        }
        resp_dict.insert("streams".into(), Value::Array(out_streams));
    }

    let body = plistx::to_bytes(resp_dict);
    (
        Response::ok().with_body("application/x-apple-binary-plist", body),
        false,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::airplay::{AudioPacket, Codec, Metadata, VideoFrame};
    use std::io::Write;

    struct DummySink {
        started: AtomicBool,
        ended: AtomicBool,
        vol_db: Mutex<f32>,
        meta: Mutex<Metadata>,
    }

    impl Sink for DummySink {
        fn session_start(&self, _client: &Client) {
            self.started.store(true, Ordering::SeqCst);
        }
        fn session_end(&self) {
            self.ended.store(true, Ordering::SeqCst);
        }
        fn video_format(&self, _codec: Codec, _csd: &[u8], _width: u32, _height: u32) {}
        fn video_frame(&self, _frame: VideoFrame) {}
        fn video_paused(&self, _paused: bool) {}
        fn video_stop(&self) {}
        fn audio_start(&self, _format: &AudioFormat) {}
        fn audio_packet(&self, _packet: AudioPacket) {}
        fn audio_flush(&self) {}
        fn audio_stop(&self) {}
        fn volume(&self, db: f32) {
            *self.vol_db.lock().unwrap() = db;
        }
        fn metadata(&self, meta: &Metadata) {
            *self.meta.lock().unwrap() = meta.clone();
        }
        fn cover_art(&self, _data: &[u8]) {}
        fn progress(&self, _elapsed_s: f64, _duration_s: f64) {}
        fn pin(&self, _pin: &str) {}
    }

    #[test]
    fn full_rtsp_airplay_handshake_and_setup() {
        let sink = Arc::new(DummySink {
            started: AtomicBool::new(false),
            ended: AtomicBool::new(false),
            vol_db: Mutex::new(0.0),
            meta: Mutex::new(Metadata::default()),
        });
        let cfg = Config {
            port: 0,
            width: 3840,
            height: 2160,
            h265: true,
            ..Config::default()
        };
        let srv = Server::start(cfg, sink.clone()).expect("start server");
        let port = srv.port();
        assert!(port > 0);

        let mut sock = TcpStream::connect(("127.0.0.1", port)).expect("connect");
        sock.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let mut reader = BufReader::new(sock.try_clone().unwrap());

        // 1. GET /info
        sock.write_all(b"GET /info RTSP/1.0\r\nCSeq: 1\r\n\r\n")
            .unwrap();
        let info_resp = http::read_request_or_response_for_test(&mut reader);
        assert!(info_resp.starts_with(b"bplist00"));
        let info_dict = plistx::from_bytes(&info_resp).expect("parse /info plist");
        assert_eq!(plistx::get_str(&info_dict, "model"), Some("AppleTV3,2"));
        assert_eq!(plistx::get_str(&info_dict, "name"), Some("Salon TV"));

        // 2. POST /pair-setup
        let client_pk = [0x42u8; 32];
        sock.write_all(b"POST /pair-setup RTSP/1.0\r\nCSeq: 2\r\nContent-Length: 32\r\n\r\n")
            .unwrap();
        sock.write_all(&client_pk).unwrap();
        let ps_resp = http::read_request_or_response_for_test(&mut reader);
        assert_eq!(ps_resp.len(), 32);

        // 3. SETUP phase 1 & phase 2
        let mut setup1 = Dictionary::new();
        setup1.insert("name".into(), Value::String("iPad Pro".into()));
        setup1.insert("model".into(), Value::String("iPad13,4".into()));
        setup1.insert("timingPort".into(), Value::Integer(Integer::from(55555u64)));
        let setup1_bytes = plistx::to_bytes(setup1);
        write!(
            sock,
            "SETUP rtsp://127.0.0.1/123 RTSP/1.0\r\nCSeq: 3\r\nContent-Length: {}\r\n\r\n",
            setup1_bytes.len()
        )
        .unwrap();
        sock.write_all(&setup1_bytes).unwrap();
        let s1_resp = plistx::from_bytes(&http::read_request_or_response_for_test(&mut reader))
            .expect("setup1 resp");
        assert!(plistx::get_u64(&s1_resp, "timingPort").unwrap_or(0) > 0);
        assert!(sink.started.load(Ordering::SeqCst));

        // 4. SETUP phase 2 (mirroring stream type 110 + audio stream type 96)
        let mut s_mirror = Dictionary::new();
        s_mirror.insert("type".into(), Value::Integer(Integer::from(110u64)));
        s_mirror.insert(
            "streamConnectionID".into(),
            Value::Integer(Integer::from(987654321u64)),
        );
        let mut s_audio = Dictionary::new();
        s_audio.insert("type".into(), Value::Integer(Integer::from(96u64)));
        s_audio.insert("ct".into(), Value::Integer(Integer::from(2u64)));
        s_audio.insert("spf".into(), Value::Integer(Integer::from(352u64)));
        let mut setup2 = Dictionary::new();
        setup2.insert(
            "streams".into(),
            Value::Array(vec![Value::Dictionary(s_mirror), Value::Dictionary(s_audio)]),
        );
        let setup2_bytes = plistx::to_bytes(setup2);
        write!(
            sock,
            "SETUP rtsp://127.0.0.1/123 RTSP/1.0\r\nCSeq: 4\r\nContent-Length: {}\r\n\r\n",
            setup2_bytes.len()
        )
        .unwrap();
        sock.write_all(&setup2_bytes).unwrap();
        let s2_resp = plistx::from_bytes(&http::read_request_or_response_for_test(&mut reader))
            .expect("setup2 resp");
        let streams = s2_resp.get("streams").and_then(|v| v.as_array()).unwrap();
        assert_eq!(streams.len(), 2);

        // 5. SET_PARAMETER volume
        let vol_body = b"volume: -12.500000\r\n";
        write!(
            sock,
            "SET_PARAMETER rtsp://127.0.0.1/123 RTSP/1.0\r\nCSeq: 5\r\nContent-Type: text/parameters\r\nContent-Length: {}\r\n\r\n",
            vol_body.len()
        )
        .unwrap();
        sock.write_all(vol_body).unwrap();
        let _ = http::read_request_or_response_for_test(&mut reader);
        assert!((*sink.vol_db.lock().unwrap() - (-12.5)).abs() < 0.01);

        // 6. TEARDOWN
        sock.write_all(b"TEARDOWN rtsp://127.0.0.1/123 RTSP/1.0\r\nCSeq: 6\r\n\r\n")
            .unwrap();
        let _ = http::read_request_or_response_for_test(&mut reader);
        assert!(sink.ended.load(Ordering::SeqCst));
    }
}
