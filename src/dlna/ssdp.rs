//! SSDP (Simple Service Discovery Protocol) Multicast Responder & Notifier (`239.255.255.250:1900`).
//!
//! Advertises Ayna as a UPnP `MediaRenderer:1` (`AVTransport:1`, `RenderingControl:1`,
//! `ConnectionManager:1`) and DIAL (`urn:dial-multiscreen-org:service:dial:1`) receiver
//! for Windows ("Cast to Device" / Media Player / Edge) and Android (VLC / BubbleUPnP /
//! Web Video Caster / Gallery) clients.

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const SSDP_MULTICAST_ADDR: Ipv4Addr = Ipv4Addr::new(239, 255, 255, 250);
const SSDP_PORT: u16 = 1900;

const ADVERTISED_NTS: &[&str] = &[
    "upnp:rootdevice",
    "urn:schemas-upnp-org:device:MediaRenderer:1",
    "urn:schemas-upnp-org:service:AVTransport:1",
    "urn:schemas-upnp-org:service:RenderingControl:1",
    "urn:schemas-upnp-org:service:ConnectionManager:1",
    "urn:dial-multiscreen-org:service:dial:1",
];

pub fn run_ssdp_responder(device_uuid: String, http_port: u16, stop: Arc<AtomicBool>) {
    let sock = match bind_ssdp_socket() {
        Ok(s) => s,
        Err(e) => {
            log::error!("dlna: failed to bind SSDP UDP 1900: {}", e);
            return;
        }
    };
    let _ = sock.set_read_timeout(Some(Duration::from_millis(500)));

    log::info!(
        "dlna: SSDP responder active on 239.255.255.250:1900 (uuid={})",
        device_uuid
    );

    let mut buf = [0u8; 4096];
    let mut last_notify = Instant::now() - Duration::from_secs(60);

    while !stop.load(Ordering::Relaxed) {
        if last_notify.elapsed() >= Duration::from_secs(25) {
            if let Some(ip) = primary_lan_ipv4() {
                send_alive_notifications(&sock, ip, http_port, &device_uuid);
            }
            last_notify = Instant::now();
        }

        match sock.recv_from(&mut buf) {
            Ok((n, peer)) if n > 0 => {
                let req = String::from_utf8_lossy(&buf[..n]);
                if req.starts_with("M-SEARCH ") {
                    if let Some(st) = extract_header(&req, "ST") {
                        let local_ip = local_ip_for_peer(peer)
                            .or_else(primary_lan_ipv4)
                            .unwrap_or(Ipv4Addr::new(127, 0, 0, 1));
                        let responses =
                            build_msearch_responses(&st, local_ip, http_port, &device_uuid);
                        for resp in responses {
                            let _ = sock.send_to(resp.as_bytes(), peer);
                        }
                    }
                }
            }
            Ok(_) => {}
            Err(ref e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(e) => {
                log::warn!("dlna: SSDP recv_from error: {}", e);
                std::thread::sleep(Duration::from_millis(200));
            }
        }
    }
}

fn bind_ssdp_socket() -> std::io::Result<UdpSocket> {
    unsafe {
        let fd = libc::socket(libc::AF_INET, libc::SOCK_DGRAM, 0);
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let yes: libc::c_int = 1;
        libc::setsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_REUSEADDR,
            &yes as *const _ as *const libc::c_void,
            std::mem::size_of_val(&yes) as libc::socklen_t,
        );
        #[cfg(any(target_os = "linux", target_os = "android"))]
        libc::setsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_REUSEPORT,
            &yes as *const _ as *const libc::c_void,
            std::mem::size_of_val(&yes) as libc::socklen_t,
        );

        let mut addr: libc::sockaddr_in = std::mem::zeroed();
        addr.sin_family = libc::AF_INET as libc::sa_family_t;
        addr.sin_port = SSDP_PORT.to_be();
        addr.sin_addr.s_addr = 0;

        if libc::bind(
            fd,
            &addr as *const _ as *const libc::sockaddr,
            std::mem::size_of_val(&addr) as libc::socklen_t,
        ) < 0
        {
            let err = std::io::Error::last_os_error();
            libc::close(fd);
            return Err(err);
        }

        use std::os::unix::io::FromRawFd;
        let sock = UdpSocket::from_raw_fd(fd);
        let _ = sock.join_multicast_v4(&SSDP_MULTICAST_ADDR, &Ipv4Addr::UNSPECIFIED);
        if let Some(ip) = primary_lan_ipv4() {
            let _ = sock.join_multicast_v4(&SSDP_MULTICAST_ADDR, &ip);
        }
        Ok(sock)
    }
}

pub fn extract_header<'a>(req: &'a str, name: &str) -> Option<&'a str> {
    for line in req.lines() {
        if let Some((k, v)) = line.split_once(':') {
            if k.trim().eq_ignore_ascii_case(name) {
                return Some(v.trim());
            }
        }
    }
    None
}

pub fn build_msearch_responses(
    st: &str,
    local_ip: Ipv4Addr,
    http_port: u16,
    device_uuid: &str,
) -> Vec<String> {
    let st_trimmed = st.trim();
    let uuid_usn = format!("uuid:{}", device_uuid);

    let targets: Vec<String> = if st_trimmed.eq_ignore_ascii_case("ssdp:all") {
        let mut v: Vec<String> = ADVERTISED_NTS.iter().map(|s| (*s).to_string()).collect();
        v.push(uuid_usn.clone());
        v
    } else if st_trimmed.eq_ignore_ascii_case(&uuid_usn) {
        vec![uuid_usn.clone()]
    } else if st_trimmed.starts_with("urn:schemas-upnp-org:device:MediaRenderer:")
        || st_trimmed.starts_with("urn:schemas-upnp-org:service:AVTransport:")
        || st_trimmed.starts_with("urn:schemas-upnp-org:service:RenderingControl:")
        || st_trimmed.starts_with("urn:schemas-upnp-org:service:ConnectionManager:")
        || st_trimmed.eq_ignore_ascii_case("upnp:rootdevice")
        || st_trimmed.eq_ignore_ascii_case("urn:dial-multiscreen-org:service:dial:1")
    {
        vec![st_trimmed.to_string()]
    } else {
        return Vec::new();
    };

    let location = format!("http://{}:{}/description.xml", local_ip, http_port);
    let app_url = format!("http://{}:{}/apps/", local_ip, http_port);

    targets
        .into_iter()
        .map(|target| {
            let usn = if target == uuid_usn {
                uuid_usn.clone()
            } else {
                format!("{}::{}", uuid_usn, target)
            };
            format!(
                "HTTP/1.1 200 OK\r\n\
                 CACHE-CONTROL: max-age=1800\r\n\
                 EXT:\r\n\
                 LOCATION: {}\r\n\
                 Application-URL: {}\r\n\
                 SERVER: Linux/4.9 UPnP/1.0 Yansit/2.0\r\n\
                 ST: {}\r\n\
                 USN: {}\r\n\
                 \r\n",
                location, app_url, target, usn
            )
        })
        .collect()
}

fn send_alive_notifications(
    sock: &UdpSocket,
    local_ip: Ipv4Addr,
    http_port: u16,
    device_uuid: &str,
) {
    let dest = SocketAddr::V4(SocketAddrV4::new(SSDP_MULTICAST_ADDR, SSDP_PORT));
    let location = format!("http://{}:{}/description.xml", local_ip, http_port);
    let uuid_usn = format!("uuid:{}", device_uuid);

    for &nt in ADVERTISED_NTS {
        let usn = format!("{}::{}", uuid_usn, nt);
        let msg = format!(
            "NOTIFY * HTTP/1.1\r\n\
             HOST: 239.255.255.250:1900\r\n\
             CACHE-CONTROL: max-age=1800\r\n\
             LOCATION: {}\r\n\
             NT: {}\r\n\
             NTS: ssdp:alive\r\n\
             SERVER: Linux/4.9 UPnP/1.0 Yansit/2.0\r\n\
             USN: {}\r\n\
             \r\n",
            location, nt, usn
        );
        let _ = sock.send_to(msg.as_bytes(), dest);
    }
}

fn local_ip_for_peer(peer: SocketAddr) -> Option<Ipv4Addr> {
    let tmp = UdpSocket::bind("0.0.0.0:0").ok()?;
    tmp.connect(peer).ok()?;
    match tmp.local_addr().ok()? {
        SocketAddr::V4(v4) if !v4.ip().is_loopback() && !v4.ip().is_unspecified() => Some(*v4.ip()),
        _ => None,
    }
}

pub fn primary_lan_ipv4() -> Option<Ipv4Addr> {
    let tmp = UdpSocket::bind("0.0.0.0:0").ok()?;
    tmp.connect("8.8.8.8:80").ok()?;
    match tmp.local_addr().ok()? {
        SocketAddr::V4(v4) if !v4.ip().is_loopback() && !v4.ip().is_unspecified() => Some(*v4.ip()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_msearch_media_renderer_response() {
        let req = "M-SEARCH * HTTP/1.1\r\n\
                   HOST: 239.255.255.250:1900\r\n\
                   MAN: \"ssdp:discover\"\r\n\
                   MX: 2\r\n\
                   ST: urn:schemas-upnp-org:device:MediaRenderer:1\r\n\r\n";
        let st = extract_header(req, "ST").unwrap();
        let resps = build_msearch_responses(
            st,
            Ipv4Addr::new(192, 0, 2, 10),
            7001,
            "12345678-1234-1234-1234-123456789abc",
        );
        assert_eq!(resps.len(), 1);
        assert!(resps[0].contains("LOCATION: http://192.0.2.10:7001/description.xml"));
        assert!(resps[0].contains("ST: urn:schemas-upnp-org:device:MediaRenderer:1"));
    }
}
