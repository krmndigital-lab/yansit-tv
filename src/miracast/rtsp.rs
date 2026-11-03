//! Wi-Fi Display (WFD / Miracast) RTSP 1.0 Client State Machine (M1..M16).
//!
//! Connects to the WFD Source RTSP server (`<p2p peer>:7236`),
//! negotiates video (H.264 CBP/CHP up to 1080p60) and audio (LPCM/AAC) formats,
//! triggers `SETUP` and `PLAY`, and maintains the session with keepalives and IDR requests.

use crate::airplay::{Client, Sink};
use crate::miracast::ts::run_rtp_receiver;
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const DEFAULT_RTP_PORT_START: u16 = 19028;

/// Run a complete WFD Sink session against `source_ip:source_port`.
pub fn run_wfd_session(
    source_ip: String,
    source_port: u16,
    peer_name: String,
    pre_connected: Option<TcpStream>,
    stop: Arc<AtomicBool>,
    sink: Arc<dyn Sink>,
) {
    log::info!(
        "miracast: starting WFD session with '{}' at {}:{}",
        peer_name,
        source_ip,
        source_port
    );

    // 1. Bind local UDP RTP socket on an even port
    let (rtp_socket, rtp_port) = match bind_even_udp_port(DEFAULT_RTP_PORT_START) {
        Some(v) => v,
        None => {
            log::error!("miracast: failed to bind UDP RTP socket");
            return;
        }
    };
    log::info!("miracast: bound local UDP RTP port {}", rtp_port);

    // 2. Connect TCP RTSP to source_ip:source_port with retries (source may take 0.5-3s after DHCP)
    let target_addr: SocketAddr = match format!("{}:{}", source_ip, source_port).parse() {
        Ok(a) => a,
        Err(e) => {
            log::error!(
                "miracast: invalid source address {}:{}: {}",
                source_ip,
                source_port,
                e
            );
            return;
        }
    };

    let mut tcp_stream = pre_connected;
    for attempt in 1..=30 {
        if tcp_stream.is_some() {
            break;
        }
        if stop.load(Ordering::Relaxed) {
            return;
        }
        match TcpStream::connect_timeout(&target_addr, Duration::from_millis(600)) {
            Ok(s) => {
                log::info!(
                    "miracast: connected to WFD RTSP server {} on attempt {}",
                    target_addr,
                    attempt
                );
                tcp_stream = Some(s);
                break;
            }
            Err(e) => {
                if attempt % 5 == 1 {
                    log::info!(
                        "miracast: waiting for source RTSP server {} (attempt {}/30: {})",
                        target_addr,
                        attempt,
                        e
                    );
                }
                std::thread::sleep(Duration::from_millis(250));
            }
        }
    }

    let stream = match tcp_stream {
        Some(s) => s,
        None => {
            log::error!("miracast: timed out connecting to RTSP {}", target_addr);
            return;
        }
    };

    let _ = stream.set_nodelay(true);
    let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
    let writer_stream = match stream.try_clone() {
        Ok(s) => s,
        Err(e) => {
            log::error!("miracast: TcpStream clone failed: {}", e);
            return;
        }
    };
    let writer = Arc::new(Mutex::new(writer_stream));

    // Notify UI / MediaEngine that a Miracast session is active
    let display_name = if peer_name.trim().is_empty() {
        format!("Miracast ({})", source_ip)
    } else {
        peer_name.trim().to_string()
    };
    sink.session_start(&Client {
        name: display_name,
        model: "Miracast / WFD".to_string(),
        device_id: String::new(),
        addr: source_ip.clone(),
        dacp_id: String::new(),
        active_remote: String::new(),
    });

    // 3. Spawn UDP RTP + MPEG-TS receiver thread
    let rtp_stop = stop.clone();
    let rtp_sink = sink.clone();
    let idr_writer = writer.clone();
    let idr_url = Arc::new(Mutex::new(format!("rtsp://{}/wfd1.0/streamid=0", source_ip)));
    let idr_session = Arc::new(Mutex::new(String::new()));
    let cseq_counter = Arc::new( std::sync::atomic::AtomicU32::new(1));

    let idr_url_clone = idr_url.clone();
    let idr_session_clone = idr_session.clone();
    let cseq_clone = cseq_counter.clone();
    let last_idr_sent = Arc::new(Mutex::new(Instant::now() - Duration::from_secs(5)));

    let rtp_handle = std::thread::Builder::new()
        .name("wfd-rtp".into())
        .spawn(move || {
            run_rtp_receiver(
                rtp_socket,
                1920,
                1080,
                rtp_stop,
                rtp_sink,
                move || {
                    if let Ok(mut last) = last_idr_sent.lock() {
                        if last.elapsed() < Duration::from_millis(1200) {
                            return;
                        }
                        *last = Instant::now();
                    }
                    let url = idr_url_clone.lock().unwrap().clone();
                    let sess = idr_session_clone.lock().unwrap().clone();
                    if !sess.is_empty() {
                        let cseq = cseq_clone.fetch_add(1, Ordering::Relaxed);
                        let body = "wfd_idr_request\r\n";
                        let req = format!(
                            "SET_PARAMETER {} RTSP/1.0\r\n\
                             CSeq: {}\r\n\
                             Session: {}\r\n\
                             Content-Type: text/parameters\r\n\
                             Content-Length: {}\r\n\
                             \r\n\
                             {}",
                            url,
                            cseq,
                            sess,
                            body.len(),
                            body
                        );
                        if let Ok(mut w) = idr_writer.lock() {
                            let _ = w.write_all(req.as_bytes());
                            let _ = w.flush();
                            log::info!("miracast: sent M13 wfd_idr_request (CSeq={})", cseq);
                        }
                    }
                },
            );
        })
        .ok();

    // 4. Run RTSP M1..M16 loop
    let mut reader = BufReader::new(stream);
    let mut awaiting_setup_response = false;
    let mut last_keepalive = Instant::now();

    while !stop.load(Ordering::Relaxed) {
        match read_rtsp_message(&mut reader) {
            Ok(Some(msg)) => {
                if msg.start_line.starts_with("RTSP/1.0") {
                    // Response from WFD Source to a request we sent (M2, M6, M7, etc.)
                    if let Some(sess_hdr) = msg.header("session") {
                        let clean_sess = sess_hdr
                            .split(';')
                            .next()
                            .unwrap_or(sess_hdr)
                            .trim()
                            .to_string();
                        if !clean_sess.is_empty() {
                            *idr_session.lock().unwrap() = clean_sess.clone();
                        }
                    }

                    if awaiting_setup_response && msg.start_line.contains("200") {
                        awaiting_setup_response = false;
                        // Send M7 PLAY immediately!
                        let cseq = cseq_counter.fetch_add(1, Ordering::Relaxed);
                        let url = idr_url.lock().unwrap().clone();
                        let sess = idr_session.lock().unwrap().clone();
                        let play_req = build_m7_play_request(&url, cseq, &sess);
                        log::info!("miracast: sending M7 PLAY to {} (session={})", url, sess);
                        if send_raw(&writer, &play_req).is_err() {
                            break;
                        }
                    }
                } else {
                    // Request from WFD Source (M1, M3, M4, M5, M16, etc.)
                    let method = msg
                        .start_line
                        .split_whitespace()
                        .next()
                        .unwrap_or("")
                        .to_ascii_uppercase();
                    let req_cseq = msg.header("cseq").unwrap_or("0").to_string();

                    match method.as_str() {
                        "OPTIONS" => {
                            // M1 received -> reply 200 OK and send M2 OPTIONS
                            let resp = format!(
                                "RTSP/1.0 200 OK\r\n\
                                 CSeq: {}\r\n\
                                 Public: org.wfa.wfd1.0, SETUP, TEARDOWN, PLAY, PAUSE, GET_PARAMETER, SET_PARAMETER\r\n\
                                 \r\n",
                                req_cseq
                            );
                            if send_raw(&writer, &resp).is_err() {
                                break;
                            }
                            let m2_cseq = cseq_counter.fetch_add(1, Ordering::Relaxed);
                            let m2_req = format!(
                                "OPTIONS * RTSP/1.0\r\n\
                                 CSeq: {}\r\n\
                                 Require: org.wfa.wfd1.0\r\n\
                                 User-Agent: Yansit-WFD/1.0\r\n\
                                 \r\n",
                                m2_cseq
                            );
                            log::info!("miracast: M1 OPTIONS answered, sending M2 OPTIONS");
                            if send_raw(&writer, &m2_req).is_err() {
                                break;
                            }
                        }
                        "GET_PARAMETER" => {
                            // M3 capability query or M16 keepalive
                            if !msg.body.trim().is_empty() {
                                log::info!(
                                    "miracast: M3 GET_PARAMETER request params:\n{}",
                                    msg.body.trim()
                                );
                            }
                            let body = build_m3_response_body(&msg.body, rtp_port);
                            let sess = idr_session.lock().unwrap().clone();
                            let sess_hdr = if sess.is_empty() {
                                String::new()
                            } else {
                                format!("Session: {}\r\n", sess)
                            };
                            let resp = if body.is_empty() {
                                format!(
                                    "RTSP/1.0 200 OK\r\n\
                                     CSeq: {}\r\n\
                                     {}\r\n",
                                    req_cseq, sess_hdr
                                )
                            } else {
                                format!(
                                    "RTSP/1.0 200 OK\r\n\
                                     CSeq: {}\r\n\
                                     {}\
                                     Content-Type: text/parameters\r\n\
                                     Content-Length: {}\r\n\
                                     \r\n\
                                     {}",
                                    req_cseq,
                                    sess_hdr,
                                    body.len(),
                                    body
                                )
                            };
                            log::info!(
                                "miracast: answering GET_PARAMETER (CSeq={}, body={} bytes)",
                                req_cseq,
                                body.len()
                            );
                            if send_raw(&writer, &resp).is_err() {
                                break;
                            }
                        }
                        "SET_PARAMETER" => {
                            // M4 capability set or M5 trigger method
                            if !msg.body.trim().is_empty() {
                                log::info!(
                                    "miracast: SET_PARAMETER request body:\n{}",
                                    msg.body.trim()
                                );
                            }
                            if let Some(pres_url) = parse_wfd_presentation_url(&msg.body) {
                                log::info!("miracast: source presentation URL = {}", pres_url);
                                *idr_url.lock().unwrap() = pres_url;
                            }
                            let trigger = parse_wfd_trigger_method(&msg.body);
                            let sess = idr_session.lock().unwrap().clone();
                            let sess_hdr = if sess.is_empty() {
                                String::new()
                            } else {
                                format!("Session: {}\r\n", sess)
                            };
                            let resp = format!(
                                "RTSP/1.0 200 OK\r\n\
                                 CSeq: {}\r\n\
                                 {}\r\n",
                                req_cseq, sess_hdr
                            );
                            if send_raw(&writer, &resp).is_err() {
                                break;
                            }

                            if let Some(trig) = trigger {
                                match trig.as_str() {
                                    "SETUP" => {
                                        let cseq = cseq_counter.fetch_add(1, Ordering::Relaxed);
                                        let url = idr_url.lock().unwrap().clone();
                                        let setup_req = build_m6_setup_request(&url, cseq, rtp_port);
                                        awaiting_setup_response = true;
                                        log::info!(
                                            "miracast: M5 trigger=SETUP -> sending M6 SETUP to {}",
                                            url
                                        );
                                        if send_raw(&writer, &setup_req).is_err() {
                                            break;
                                        }
                                    }
                                    "PLAY" => {
                                        let cseq = cseq_counter.fetch_add(1, Ordering::Relaxed);
                                        let url = idr_url.lock().unwrap().clone();
                                        let play_req = build_m7_play_request(&url, cseq, &sess);
                                        log::info!(
                                            "miracast: M5 trigger=PLAY -> sending M7 PLAY to {}",
                                            url
                                        );
                                        if send_raw(&writer, &play_req).is_err() {
                                            break;
                                        }
                                    }
                                    "PAUSE" => {
                                        let cseq = cseq_counter.fetch_add(1, Ordering::Relaxed);
                                        let url = idr_url.lock().unwrap().clone();
                                        let pause_req = format!(
                                            "PAUSE {} RTSP/1.0\r\n\
                                             CSeq: {}\r\n\
                                             Session: {}\r\n\
                                             \r\n",
                                            url, cseq, sess
                                        );
                                        let _ = send_raw(&writer, &pause_req);
                                    }
                                    "TEARDOWN" => {
                                        log::info!("miracast: source triggered TEARDOWN");
                                        let cseq = cseq_counter.fetch_add(1, Ordering::Relaxed);
                                        let url = idr_url.lock().unwrap().clone();
                                        let td_req = format!(
                                            "TEARDOWN {} RTSP/1.0\r\n\
                                             CSeq: {}\r\n\
                                             Session: {}\r\n\
                                             \r\n",
                                            url, cseq, sess
                                        );
                                        let _ = send_raw(&writer, &td_req);
                                        break;
                                    }
                                    _ => {}
                                }
                            }
                        }
                        "TEARDOWN" => {
                            let resp = format!("RTSP/1.0 200 OK\r\nCSeq: {}\r\n\r\n", req_cseq);
                            let _ = send_raw(&writer, &resp);
                            log::info!("miracast: received TEARDOWN from source");
                            break;
                        }
                        _ => {
                            let resp = format!("RTSP/1.0 200 OK\r\nCSeq: {}\r\n\r\n", req_cseq);
                            let _ = send_raw(&writer, &resp);
                        }
                    }
                }
            }
            Ok(None) => {
                // Read timeout — check if we should send a periodic keepalive
                if last_keepalive.elapsed() >= Duration::from_secs(20) {
                    let sess = idr_session.lock().unwrap().clone();
                    if !sess.is_empty() {
                        let cseq = cseq_counter.fetch_add(1, Ordering::Relaxed);
                        let url = idr_url.lock().unwrap().clone();
                        let ka = format!(
                            "GET_PARAMETER {} RTSP/1.0\r\n\
                             CSeq: {}\r\n\
                             Session: {}\r\n\
                             \r\n",
                            url, cseq, sess
                        );
                        let _ = send_raw(&writer, &ka);
                    }
                    last_keepalive = Instant::now();
                }
            }
            Err(e) => {
                log::info!("miracast: RTSP connection closed ({})", e);
                break;
            }
        }
    }

    stop.store(true, Ordering::Relaxed);
    if let Some(h) = rtp_handle {
        let _ = h.join();
    }
    sink.video_stop();
    sink.audio_stop();
    sink.session_end();
    log::info!("miracast: WFD session ended for {}", source_ip);
}

fn bind_even_udp_port(start_port: u16) -> Option<(UdpSocket, u16)> {
    let mut port = if start_port % 2 == 0 {
        start_port
    } else {
        start_port + 1
    };
    while port < 19200 {
        if let Ok(sock) = UdpSocket::bind(("0.0.0.0", port)) {
            return Some((sock, port));
        }
        port += 2;
    }
    UdpSocket::bind(("0.0.0.0", 0)).ok().and_then(|s| {
        let p = s.local_addr().ok()?.port();
        Some((s, p))
    })
}

fn send_raw(writer: &Arc<Mutex<TcpStream>>, data: &str) -> std::io::Result<()> {
    let mut w = writer.lock().unwrap();
    w.write_all(data.as_bytes())?;
    w.flush()
}

#[derive(Debug, Clone)]
pub struct RtspMessage {
    pub start_line: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl RtspMessage {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

fn is_timeout(e: &std::io::Error) -> bool {
    matches!(
        e.kind(),
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
    )
}

/// `read_line` that keeps partial data across socket read timeouts (up to ~10 s).
fn read_line_retry<R: BufRead>(reader: &mut R, line: &mut String) -> std::io::Result<usize> {
    let mut tries = 0;
    loop {
        match reader.read_line(line) {
            Ok(n) => return Ok(n),
            Err(ref e) if is_timeout(e) && tries < 20 => tries += 1,
            Err(e) => return Err(e),
        }
    }
}

/// Read one RTSP message from `reader`. Returns `Ok(None)` when no data arrived before the
/// socket read timeout (so the caller can run keepalives).
pub fn read_rtsp_message<R: BufRead>(reader: &mut R) -> std::io::Result<Option<RtspMessage>> {
    let mut start_line = String::new();
    loop {
        match reader.read_line(&mut start_line) {
            Ok(0) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "EOF",
                ))
            }
            Ok(_) => {
                if start_line.ends_with('\n') {
                    let trimmed = start_line.trim();
                    if !trimmed.is_empty() {
                        start_line = trimmed.to_string();
                        break;
                    }
                    start_line.clear();
                }
            }
            Err(ref e) if is_timeout(e) => {
                if start_line.trim().is_empty() {
                    return Ok(None);
                }
                // partial start line: keep reading
            }
            Err(e) => return Err(e),
        }
    }

    let mut headers = Vec::new();
    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        let n = read_line_retry(reader, &mut line)?;
        if n == 0 {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            break;
        }
        if let Some((k, v)) = trimmed.split_once(':') {
            let key = k.trim().to_string();
            let val = v.trim().to_string();
            if key.eq_ignore_ascii_case("content-length") {
                content_length = val.parse().unwrap_or(0);
            }
            headers.push((key, val));
        }
    }

    let mut body = String::new();
    if content_length > 0 && content_length <= 65536 {
        let mut buf = vec![0u8; content_length];
        let mut got = 0;
        let mut tries = 0;
        while got < content_length {
            match reader.read(&mut buf[got..]) {
                Ok(0) => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        "EOF in body",
                    ))
                }
                Ok(n) => got += n,
                Err(ref e) if is_timeout(e) && tries < 20 => tries += 1,
                Err(e) => return Err(e),
            }
        }
        body = String::from_utf8_lossy(&buf).into_owned();
    }

    Ok(Some(RtspMessage {
        start_line,
        headers,
        body,
    }))
}

/// Build the WFD M3 `GET_PARAMETER` response body matching the parameters requested by the source.
pub fn build_m3_response_body(request_body: &str, rtp_port: u16) -> String {
    if request_body.trim().is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for raw_line in request_body.lines() {
        let param = raw_line.trim().trim_end_matches(':');
        if param.is_empty() {
            continue;
        }
        match param {
            "wfd_video_formats" => {
                // Native = 40 (CEA index 8 = 1920x1080p60).
                // Both H.264 CHP (02) and CBP (01) advertise Level 4.2 (10).
                // CEA mask 00202963 forces 60fps at 1080p by advertising bit 8 (1080p60),
                // bit 6 (720p60), bit 1 (480p60), bit 0 (480p60), bit 5 (720p30 mandatory),
                // bit 13 (1080p50), and bit 21 (4K60), while explicitly DISABLING bit 7
                // (1080p30 = 0x80) so Samsung/Android sources cannot negotiate 30fps at 1080p.
                // VESA mask bfffffff includes 2K60 (bit 31 = 2560x1440p60) and disables 2K30 (bit 30).
                // Frame-rate control support = 11 (dynamic 60fps).
                out.push_str(
                    "wfd_video_formats: 40 00 02 10 00202963 bfffffff 00000fff 00 0000 0000 11 none none, 01 10 00202963 bfffffff 00000fff 00 0000 0000 11 none none\r\n",
                );
            }
            "wfd2_video_formats" => {
                // WFD R2 (v2.1 Spec Section 6.1.22):
                // Entry 1: H.265 / HEVC (02) Main Profile (01) Level 5.1 (0010), CEA 0000003fffff (up to 4K60),
                //          VESA 0003ffffffff (up to 2560x1440p60 / 2560x1600p60), HH 000000000fff, frc=11.
                // Entry 2: H.264 (01) High Profile (02) Level 5.2 (0080), up to 2K60/4K60, non-transcoding=01.
                out.push_str(
                    "wfd2_video_formats: 40 02 01 0010 0000003fffff 0003ffffffff 000000000fff 00 0000 0000 11, 01 02 0080 0000003fffff 0003ffffffff 000000000fff 00 0000 0000 11 01\r\n",
                );
            }
            "wfd_audio_codecs" => {
                out.push_str("wfd_audio_codecs: LPCM 00000003 00, AAC 00000001 00\r\n");
            }
            "wfd2_audio_codecs" => {
                out.push_str("wfd2_audio_codecs: LPCM 00000003 00, AAC 000000ff 00\r\n");
            }
            "wfd_client_rtp_ports" => {
                out.push_str(&format!(
                    "wfd_client_rtp_ports: RTP/AVP/UDP;unicast {} 0 mode=play\r\n",
                    rtp_port
                ));
            }
            "wfd_3d_video_formats" => {
                out.push_str("wfd_3d_video_formats: none\r\n");
            }
            "wfd_content_protection" => {
                out.push_str("wfd_content_protection: none\r\n");
            }
            "wfd_display_edid" => {
                // 2-block (256-byte) HDMI EDID ("Mi TV") advertising 1920x1080p60 native preferred
                // timing + CEA-861 VIC 16 (1080p60) & HDMI 4K modes.
                out.push_str(
                    "wfd_display_edid: 0002 00FFFFFFFFFFFF0061A40001010000000117010380683B780A298DA5555099270E474A21080001010101010101010101010101010101023A801871382D40582C450010494200001F011D007251D01E206E28550010494200001E000000FC004D692054560A20202020202020000000FD03324B1E4B0F000A202020202020018002032F714B901F140413021103060712492122053C3D3E15162023090707830100006C030C001000B02D20A0020141023A80D072382D40102C458010494200001E011D00BC52D01E20B828554010494200001E011D8018711C1620582C250010494200009E011D80D0721C1620102C258010494200009E0000000000000000D8\r\n",
                );
            }
            "wfd_coupled_sink" => {
                out.push_str("wfd_coupled_sink: 00 none\r\n");
            }
            "wfd_uibc_capability" => {
                out.push_str("wfd_uibc_capability: none\r\n");
            }
            "wfd_connector_type" => {
                out.push_str("wfd_connector_type: 05\r\n");
            }
            "wfd_standby_resume_capability" => {
                out.push_str("wfd_standby_resume_capability: supported\r\n");
            }
            "wfd_idr_request_capability" => {
                out.push_str("wfd_idr_request_capability: 1\r\n");
            }
            "wfd_sec_framerate" => {
                out.push_str("wfd_sec_framerate: 60\r\n");
            }
            "wfd_sec_screensharing" => {
                out.push_str("wfd_sec_screensharing: 1\r\n");
            }
            "wfd_sec_portrait_display" => {
                out.push_str("wfd_sec_portrait_display: supported\r\n");
            }
            "wfd_sec_rotation" => {
                out.push_str("wfd_sec_rotation: 1\r\n");
            }
            "wfd_sec_hw_rotation" => {
                out.push_str("wfd_sec_hw_rotation: 1\r\n");
            }
            other => {
                // Respond `none` to vendor-specific extensions (e.g. wfd_sec_*, wfd_intel_*, wfd_microsoft_*)
                out.push_str(&format!("{}: none\r\n", other));
            }
        }
    }
    out
}

pub fn parse_wfd_trigger_method(body: &str) -> Option<String> {
    for line in body.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("wfd_trigger_method:") {
            let method = rest.trim().to_ascii_uppercase();
            if !method.is_empty() {
                return Some(method);
            }
        }
    }
    None
}

pub fn parse_wfd_presentation_url(body: &str) -> Option<String> {
    for line in body.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("wfd_presentation_URL:") {
            if let Some(url) = rest.split_whitespace().next() {
                if url.starts_with("rtsp://") {
                    return Some(url.to_string());
                }
            }
        }
    }
    None
}

pub fn build_m6_setup_request(url: &str, cseq: u32, rtp_port: u16) -> String {
    format!(
        "SETUP {} RTSP/1.0\r\n\
         CSeq: {}\r\n\
         Transport: RTP/AVP/UDP;unicast;client_port={}-{}\r\n\
         User-Agent: Yansit-WFD/1.0\r\n\
         \r\n",
        url,
        cseq,
        rtp_port,
        rtp_port + 1
    )
}

pub fn build_m7_play_request(url: &str, cseq: u32, session: &str) -> String {
    format!(
        "PLAY {} RTSP/1.0\r\n\
         CSeq: {}\r\n\
         Session: {}\r\n\
         User-Agent: Yansit-WFD/1.0\r\n\
         \r\n",
        url, cseq, session
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_m3_response_generation() {
        let req = "wfd_video_formats\r\nwfd2_video_formats\r\nwfd_audio_codecs\r\nwfd_client_rtp_ports\r\nwfd_content_protection\r\nwfd_uibc_capability\r\nwfd_sec_framerate\r\n";
        let resp = build_m3_response_body(req, 19028);
        assert!(resp.contains("wfd_video_formats: 40 00 02 10 00202963 bfffffff 00000fff 00 0000 0000 11 none none, 01 10"));
        assert!(resp.contains("wfd2_video_formats: 40 02 01 0010 0000003fffff 0003ffffffff"));
        assert!(resp.contains("wfd_audio_codecs: LPCM 00000003 00, AAC 00000001 00"));
        assert!(resp.contains("wfd_client_rtp_ports: RTP/AVP/UDP;unicast 19028 0 mode=play"));
        assert!(resp.contains("wfd_content_protection: none"));
        assert!(resp.contains("wfd_uibc_capability: none"));
        assert!(resp.contains("wfd_sec_framerate: 60"));
    }

    #[test]
    fn test_parse_trigger_and_presentation_url() {
        let m4_body = "wfd_video_formats: 00 00 02 04 00000080 00000000 00000000 00 0000 0000 00 none none\r\n\
                       wfd_audio_codecs: LPCM 00000002 00\r\n\
                       wfd_presentation_URL: rtsp://192.168.49.1/wfd1.0/streamid=0 none\r\n";
        assert_eq!(
            parse_wfd_presentation_url(m4_body),
            Some("rtsp://192.168.49.1/wfd1.0/streamid=0".to_string())
        );

        let m5_body = "wfd_trigger_method: SETUP\r\n";
        assert_eq!(parse_wfd_trigger_method(m5_body), Some("SETUP".to_string()));
    }
}
