//! UPnP / DLNA HTTP Device Description, SCPD XMLs, Event Subscription, and SOAP Control Server.

use crate::media::MediaEngine;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

pub fn run_http_soap_server(
    port: u16,
    device_uuid: String,
    engine: Arc<MediaEngine>,
    stop: Arc<AtomicBool>,
) {
    let listener = match TcpListener::bind(("0.0.0.0", port)) {
        Ok(l) => l,
        Err(e) => {
            log::error!("dlna: failed to bind HTTP/SOAP port {}: {}", port, e);
            return;
        }
    };
    let _ = listener.set_nonblocking(true);
    log::info!("dlna: HTTP/SOAP MediaRenderer listening on 0.0.0.0:{}", port);

    while !stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, peer)) => {
                let uuid = device_uuid.clone();
                let eng = engine.clone();
                std::thread::spawn(move || {
                    let _ = handle_http_client(stream, peer.ip().to_string(), &uuid, &eng);
                });
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                log::warn!("dlna: accept error: {}", e);
                std::thread::sleep(Duration::from_millis(200));
            }
        }
    }
}

fn handle_http_client(
    stream: TcpStream,
    peer_ip: String,
    device_uuid: &str,
    engine: &Arc<MediaEngine>,
) -> std::io::Result<()> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
    let mut writer = stream.try_clone()?;
    let mut reader = BufReader::new(stream);

    let mut req_line = String::new();
    if reader.read_line(&mut req_line)? == 0 {
        return Ok(());
    }
    let parts: Vec<&str> = req_line.trim().split_whitespace().collect();
    if parts.len() < 2 {
        return Ok(());
    }
    let method = parts[0].to_ascii_uppercase();
    let path = parts[1];

    let mut content_length = 0usize;
    let mut soap_action = String::new();
    let mut host_header = String::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            break;
        }
        if let Some((k, v)) = trimmed.split_once(':') {
            let key = k.trim();
            let val = v.trim();
            if key.eq_ignore_ascii_case("content-length") {
                content_length = val.parse().unwrap_or(0);
            } else if key.eq_ignore_ascii_case("soapaction") {
                soap_action = val.trim_matches('"').to_string();
            } else if key.eq_ignore_ascii_case("host") {
                host_header = val.to_string();
            }
        }
    }

    let mut body = String::new();
    if content_length > 0 && content_length <= 256 * 1024 {
        let mut buf = vec![0u8; content_length];
        reader.read_exact(&mut buf)?;
        body = String::from_utf8_lossy(&buf).into_owned();
    }

    match (method.as_str(), path) {
        ("GET", "/description.xml") => {
            let friendly_name = engine.settings().device_name;
            let xml = build_description_xml(&friendly_name, device_uuid);
            send_http_response(&mut writer, "200 OK", "text/xml; charset=\"utf-8\"", &xml, &[])
        }
        ("GET", "/AVTransport.xml") => {
            send_http_response(&mut writer, "200 OK", "text/xml; charset=\"utf-8\"", AVTRANSPORT_SCPD, &[])
        }
        ("GET", "/RenderingControl.xml") => {
            send_http_response(&mut writer, "200 OK", "text/xml; charset=\"utf-8\"", RENDERING_CONTROL_SCPD, &[])
        }
        ("GET", "/ConnectionManager.xml") => {
            send_http_response(&mut writer, "200 OK", "text/xml; charset=\"utf-8\"", CONNECTION_MANAGER_SCPD, &[])
        }
        ("SUBSCRIBE", _) => {
            let sid = format!("uuid:{}-sub", device_uuid);
            send_http_response(
                &mut writer,
                "200 OK",
                "text/html",
                "",
                &[("SID", &sid), ("TIMEOUT", "Second-1800")],
            )
        }
        ("UNSUBSCRIBE", _) => send_http_response(&mut writer, "200 OK", "text/html", "", &[]),
        ("POST", p) if p.contains("AVTransport") || p.contains("RenderingControl") || p.contains("ConnectionManager") => {
            let action = soap_action
                .split('#')
                .nth(1)
                .unwrap_or(&soap_action)
                .trim();
            let resp_xml = handle_soap_action(action, &body, &peer_ip, engine);
            let _ = host_header;
            send_http_response(
                &mut writer,
                "200 OK",
                "text/xml; charset=\"utf-8\"",
                &resp_xml,
                &[("EXT", "")],
            )
        }
        _ => send_http_response(&mut writer, "404 Not Found", "text/plain", "Not Found", &[]),
    }
}

fn send_http_response(
    writer: &mut TcpStream,
    status: &str,
    content_type: &str,
    body: &str,
    extra_headers: &[(&str, &str)],
) -> std::io::Result<()> {
    let mut hdr = format!(
        "HTTP/1.1 {}\r\n\
         Content-Type: {}\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         Server: Linux/4.9 UPnP/1.0 Yansit/2.0\r\n",
        status,
        content_type,
        body.len()
    );
    for (k, v) in extra_headers {
        hdr.push_str(&format!("{}: {}\r\n", k, v));
    }
    hdr.push_str("\r\n");
    writer.write_all(hdr.as_bytes())?;
    if !body.is_empty() {
        writer.write_all(body.as_bytes())?;
    }
    writer.flush()
}

pub fn handle_soap_action(
    action: &str,
    body: &str,
    peer_ip: &str,
    engine: &Arc<MediaEngine>,
) -> String {
    match action {
        "SetAVTransportURI" => {
            let raw_uri = extract_xml_tag(body, "CurrentURI").unwrap_or_default();
            let uri = unescape_xml(&raw_uri).trim().to_string();
            let raw_meta = extract_xml_tag(body, "CurrentURIMetaData").unwrap_or_default();
            let meta_unescaped = unescape_xml(&raw_meta);

            let title = extract_xml_tag(&meta_unescaped, "dc:title")
                .or_else(|| extract_xml_tag(&meta_unescaped, "title"))
                .map(|s| unescape_xml(&s))
                .unwrap_or_else(|| guess_title_from_url(&uri));
            let artist = extract_xml_tag(&meta_unescaped, "upnp:artist")
                .or_else(|| extract_xml_tag(&meta_unescaped, "dc:creator"))
                .map(|s| unescape_xml(&s))
                .unwrap_or_default();

            let lower_uri = uri.to_ascii_lowercase();
            let lower_meta = meta_unescaped.to_ascii_lowercase();
            let is_audio_only = lower_meta.contains("object.item.audioitem")
                || lower_uri.ends_with(".mp3")
                || lower_uri.ends_with(".flac")
                || lower_uri.ends_with(".wav")
                || lower_uri.ends_with(".m4a")
                || lower_uri.ends_with(".aac")
                || lower_uri.ends_with(".ogg");
            let is_video = !is_audio_only;

            if !uri.is_empty() {
                engine.dlna_set_uri(
                    uri.clone(),
                    raw_meta,
                    title,
                    artist,
                    is_video,
                    peer_ip.to_string(),
                );
                crate::dlna::player::start_url(uri, is_video, engine.clone());
            }
            soap_envelope(
                "urn:schemas-upnp-org:service:AVTransport:1",
                "SetAVTransportURIResponse",
                "",
            )
        }
        "SetNextAVTransportURI" => soap_envelope(
            "urn:schemas-upnp-org:service:AVTransport:1",
            "SetNextAVTransportURIResponse",
            "",
        ),
        "Play" => {
            engine.dlna_play();
            soap_envelope("urn:schemas-upnp-org:service:AVTransport:1", "PlayResponse", "")
        }
        "Pause" => {
            engine.dlna_pause();
            soap_envelope("urn:schemas-upnp-org:service:AVTransport:1", "PauseResponse", "")
        }
        "Stop" => {
            engine.dlna_stop();
            soap_envelope("urn:schemas-upnp-org:service:AVTransport:1", "StopResponse", "")
        }
        "Seek" => {
            if let Some(target_str) = extract_xml_tag(body, "Target") {
                if let Some(ms) = parse_hms_to_ms(&unescape_xml(&target_str)) {
                    engine.dlna_seek(ms);
                }
            }
            soap_envelope("urn:schemas-upnp-org:service:AVTransport:1", "SeekResponse", "")
        }
        "GetTransportInfo" => {
            let st = engine.dlna_state();
            let inner = format!(
                "<CurrentTransportState>{}</CurrentTransportState>\
                 <CurrentTransportStatus>OK</CurrentTransportStatus>\
                 <CurrentSpeed>1</CurrentSpeed>",
                st.transport_state
            );
            soap_envelope(
                "urn:schemas-upnp-org:service:AVTransport:1",
                "GetTransportInfoResponse",
                &inner,
            )
        }
        "GetPositionInfo" => {
            let st = engine.dlna_state();
            let dur_str = format_ms_to_hms(st.duration_ms);
            let pos_str = format_ms_to_hms(st.position_ms);
            let uri_escaped = escape_xml(&st.current_uri);
            let inner = format!(
                "<Track>{}</Track>\
                 <TrackDuration>{}</TrackDuration>\
                 <TrackMetaData>{}</TrackMetaData>\
                 <TrackURI>{}</TrackURI>\
                 <RelTime>{}</RelTime>\
                 <AbsTime>{}</AbsTime>\
                 <RelCount>2147483647</RelCount>\
                 <AbsCount>2147483647</AbsCount>",
                if st.current_uri.is_empty() { 0 } else { 1 },
                dur_str,
                st.current_meta_xml,
                uri_escaped,
                pos_str,
                pos_str
            );
            soap_envelope(
                "urn:schemas-upnp-org:service:AVTransport:1",
                "GetPositionInfoResponse",
                &inner,
            )
        }
        "GetMediaInfo" => {
            let st = engine.dlna_state();
            let dur_str = format_ms_to_hms(st.duration_ms);
            let uri_escaped = escape_xml(&st.current_uri);
            let inner = format!(
                "<NrTracks>{}</NrTracks>\
                 <MediaDuration>{}</MediaDuration>\
                 <CurrentURI>{}</CurrentURI>\
                 <CurrentURIMetaData>{}</CurrentURIMetaData>\
                 <NextURI></NextURI>\
                 <NextURIMetaData></NextURIMetaData>\
                 <PlayMedium>NETWORK</PlayMedium>\
                 <RecordMedium>NOT_IMPLEMENTED</RecordMedium>\
                 <WriteStatus>NOT_IMPLEMENTED</WriteStatus>",
                if st.current_uri.is_empty() { 0 } else { 1 },
                dur_str,
                uri_escaped,
                st.current_meta_xml
            );
            soap_envelope(
                "urn:schemas-upnp-org:service:AVTransport:1",
                "GetMediaInfoResponse",
                &inner,
            )
        }
        "GetDeviceCapabilities" => soap_envelope(
            "urn:schemas-upnp-org:service:AVTransport:1",
            "GetDeviceCapabilitiesResponse",
            "<PlayMedia>NETWORK,NONE</PlayMedia><RecMedia>NOT_IMPLEMENTED</RecMedia><RecQualityModes>NOT_IMPLEMENTED</RecQualityModes>",
        ),
        "GetTransportSettings" => soap_envelope(
            "urn:schemas-upnp-org:service:AVTransport:1",
            "GetTransportSettingsResponse",
            "<PlayMode>NORMAL</PlayMode><RecQualityMode>NOT_IMPLEMENTED</RecQualityMode>",
        ),
        "GetCurrentTransportActions" => soap_envelope(
            "urn:schemas-upnp-org:service:AVTransport:1",
            "GetCurrentTransportActionsResponse",
            "<Actions>Play,Pause,Stop,Seek</Actions>",
        ),
        "GetVolume" => {
            let st = engine.dlna_state();
            soap_envelope(
                "urn:schemas-upnp-org:service:RenderingControl:1",
                "GetVolumeResponse",
                &format!("<CurrentVolume>{}</CurrentVolume>", st.volume_percent),
            )
        }
        "SetVolume" => {
            if let Some(v_str) = extract_xml_tag(body, "DesiredVolume") {
                if let Ok(v) = v_str.trim().parse::<u32>() {
                    engine.dlna_set_volume(v);
                }
            }
            soap_envelope(
                "urn:schemas-upnp-org:service:RenderingControl:1",
                "SetVolumeResponse",
                "",
            )
        }
        "GetMute" => {
            let st = engine.dlna_state();
            soap_envelope(
                "urn:schemas-upnp-org:service:RenderingControl:1",
                "GetMuteResponse",
                &format!("<CurrentMute>{}</CurrentMute>", if st.muted { 1 } else { 0 }),
            )
        }
        "SetMute" => {
            if let Some(m_str) = extract_xml_tag(body, "DesiredMute") {
                let m = m_str.trim() == "1" || m_str.trim().eq_ignore_ascii_case("true");
                engine.dlna_set_mute(m);
            }
            soap_envelope(
                "urn:schemas-upnp-org:service:RenderingControl:1",
                "SetMuteResponse",
                "",
            )
        }
        "GetProtocolInfo" => soap_envelope(
            "urn:schemas-upnp-org:service:ConnectionManager:1",
            "GetProtocolInfoResponse",
            "<Source></Source>\
             <Sink>http-get:*:video/mp4:*,http-get:*:video/x-matroska:*,http-get:*:video/webm:*,http-get:*:video/mpeg:*,http-get:*:video/mp2t:*,http-get:*:video/avi:*,http-get:*:application/vnd.apple.mpegurl:*,http-get:*:application/x-mpegURL:*,http-get:*:audio/mpeg:*,http-get:*:audio/mp4:*,http-get:*:audio/flac:*,http-get:*:audio/wav:*,http-get:*:audio/aac:*,http-get:*:audio/ogg:*,http-get:*:image/jpeg:*,http-get:*:image/png:*,http-get:*:*:*</Sink>",
        ),
        "GetCurrentConnectionIDs" => soap_envelope(
            "urn:schemas-upnp-org:service:ConnectionManager:1",
            "GetCurrentConnectionIDsResponse",
            "<ConnectionIDs>0</ConnectionIDs>",
        ),
        "GetCurrentConnectionInfo" => soap_envelope(
            "urn:schemas-upnp-org:service:ConnectionManager:1",
            "GetCurrentConnectionInfoResponse",
            "<RcsID>0</RcsID><AVTransportID>0</AVTransportID><ProtocolInfo></ProtocolInfo><PeerConnectionManager></PeerConnectionManager><PeerConnectionID>-1</PeerConnectionID><Direction>Input</Direction><Status>OK</Status>",
        ),
        other => {
            log::debug!("dlna: unhandled SOAP action '{}'", other);
            soap_envelope(
                "urn:schemas-upnp-org:service:AVTransport:1",
                &format!("{}Response", other),
                "",
            )
        }
    }
}

fn soap_envelope(service_urn: &str, response_tag: &str, inner_xml: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
         <s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\">\
         <s:Body>\
         <u:{tag} xmlns:u=\"{urn}\">{inner}</u:{tag}>\
         </s:Body>\
         </s:Envelope>",
        tag = response_tag,
        urn = service_urn,
        inner = inner_xml
    )
}

pub fn extract_xml_tag(xml: &str, tag: &str) -> Option<String> {
    let open1 = format!("<{}>", tag);
    let open2 = format!("<{} ", tag);
    let close = format!("</{}>", tag);

    let start_idx = if let Some(p) = xml.find(&open1) {
        p + open1.len()
    } else if let Some(p) = xml.find(&open2) {
        let rest = &xml[p..];
        p + rest.find('>')? + 1
    } else {
        return None;
    };

    let end_idx = xml[start_idx..].find(&close)? + start_idx;
    Some(xml[start_idx..end_idx].to_string())
}

pub fn unescape_xml(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

pub fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn guess_title_from_url(url: &str) -> String {
    url.split('?')
        .next()
        .unwrap_or(url)
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("DLNA Medya")
        .replace("%20", " ")
}

pub fn parse_hms_to_ms(hms: &str) -> Option<u64> {
    let parts: Vec<&str> = hms.trim().split(':').collect();
    match parts.len() {
        3 => {
            let h: u64 = parts[0].parse().ok()?;
            let m: u64 = parts[1].parse().ok()?;
            let s: f64 = parts[2].parse().ok()?;
            Some((h * 3600 + m * 60) * 1000 + (s * 1000.0) as u64)
        }
        2 => {
            let m: u64 = parts[0].parse().ok()?;
            let s: f64 = parts[1].parse().ok()?;
            Some(m * 60 * 1000 + (s * 1000.0) as u64)
        }
        1 => {
            let s: f64 = parts[0].parse().ok()?;
            Some((s * 1000.0) as u64)
        }
        _ => None,
    }
}

pub fn format_ms_to_hms(ms: u64) -> String {
    let total_secs = ms / 1000;
    let h = total_secs / 3600;
    let m = (total_secs % 3600) / 60;
    let s = total_secs % 60;
    format!("{:02}:{:02}:{:02}", h, m, s)
}

fn build_description_xml(friendly_name: &str, device_uuid: &str) -> String {
    let safe_name = escape_xml(friendly_name);
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
<root xmlns=\"urn:schemas-upnp-org:device-1-0\" xmlns:dlna=\"urn:schemas-dlna-org:device-1-0\" xmlns:df=\"http://www.microsoft.com/windows/2008/09/devicefoundation\">\
  <specVersion><major>1</major><minor>0</minor></specVersion>\
  <device>\
    <deviceType>urn:schemas-upnp-org:device:MediaRenderer:1</deviceType>\
    <friendlyName>{name}</friendlyName>\
    <manufacturer>Yansit</manufacturer>\
    <manufacturerURL>https://yansit.tv</manufacturerURL>\
    <modelDescription>Yansit 4K Multi-Protocol Receiver (AirPlay 2 + Miracast + DLNA)</modelDescription>\
    <modelName>Yansit TV Receiver</modelName>\
    <modelNumber>2.0</modelNumber>\
    <UDN>uuid:{uuid}</UDN>\
    <dlna:X_DLNADOC xmlns:dlna=\"urn:schemas-dlna-org:device-1-0\">DMR-1.50</dlna:X_DLNADOC>\
    <df:X_deviceCategory>Multimedia.DMR</df:X_deviceCategory>\
    <serviceList>\
      <service>\
        <serviceType>urn:schemas-upnp-org:service:AVTransport:1</serviceType>\
        <serviceId>urn:upnp-org:serviceId:AVTransport</serviceId>\
        <SCPDURL>/AVTransport.xml</SCPDURL>\
        <controlURL>/AVTransport/control</controlURL>\
        <eventSubURL>/AVTransport/event</eventSubURL>\
      </service>\
      <service>\
        <serviceType>urn:schemas-upnp-org:service:RenderingControl:1</serviceType>\
        <serviceId>urn:upnp-org:serviceId:RenderingControl</serviceId>\
        <SCPDURL>/RenderingControl.xml</SCPDURL>\
        <controlURL>/RenderingControl/control</controlURL>\
        <eventSubURL>/RenderingControl/event</eventSubURL>\
      </service>\
      <service>\
        <serviceType>urn:schemas-upnp-org:service:ConnectionManager:1</serviceType>\
        <serviceId>urn:upnp-org:serviceId:ConnectionManager</serviceId>\
        <SCPDURL>/ConnectionManager.xml</SCPDURL>\
        <controlURL>/ConnectionManager/control</controlURL>\
        <eventSubURL>/ConnectionManager/event</eventSubURL>\
      </service>\
    </serviceList>\
  </device>\
</root>",
        name = safe_name,
        uuid = device_uuid
    )
}

const AVTRANSPORT_SCPD: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<scpd xmlns="urn:schemas-upnp-org:service-1-0">
  <specVersion><major>1</major><minor>0</minor></specVersion>
  <actionList>
    <action><name>SetAVTransportURI</name><argumentList>
      <argument><name>InstanceID</name><direction>in</direction><relatedStateVariable>A_ARG_TYPE_InstanceID</relatedStateVariable></argument>
      <argument><name>CurrentURI</name><direction>in</direction><relatedStateVariable>AVTransportURI</relatedStateVariable></argument>
      <argument><name>CurrentURIMetaData</name><direction>in</direction><relatedStateVariable>AVTransportURIMetaData</relatedStateVariable></argument>
    </argumentList></action>
    <action><name>SetNextAVTransportURI</name><argumentList>
      <argument><name>InstanceID</name><direction>in</direction><relatedStateVariable>A_ARG_TYPE_InstanceID</relatedStateVariable></argument>
      <argument><name>NextURI</name><direction>in</direction><relatedStateVariable>NextAVTransportURI</relatedStateVariable></argument>
      <argument><name>NextURIMetaData</name><direction>in</direction><relatedStateVariable>NextAVTransportURIMetaData</relatedStateVariable></argument>
    </argumentList></action>
    <action><name>Play</name><argumentList>
      <argument><name>InstanceID</name><direction>in</direction><relatedStateVariable>A_ARG_TYPE_InstanceID</relatedStateVariable></argument>
      <argument><name>Speed</name><direction>in</direction><relatedStateVariable>TransportPlaySpeed</relatedStateVariable></argument>
    </argumentList></action>
    <action><name>Pause</name><argumentList>
      <argument><name>InstanceID</name><direction>in</direction><relatedStateVariable>A_ARG_TYPE_InstanceID</relatedStateVariable></argument>
    </argumentList></action>
    <action><name>Stop</name><argumentList>
      <argument><name>InstanceID</name><direction>in</direction><relatedStateVariable>A_ARG_TYPE_InstanceID</relatedStateVariable></argument>
    </argumentList></action>
    <action><name>Seek</name><argumentList>
      <argument><name>InstanceID</name><direction>in</direction><relatedStateVariable>A_ARG_TYPE_InstanceID</relatedStateVariable></argument>
      <argument><name>Unit</name><direction>in</direction><relatedStateVariable>A_ARG_TYPE_SeekMode</relatedStateVariable></argument>
      <argument><name>Target</name><direction>in</direction><relatedStateVariable>A_ARG_TYPE_SeekTarget</relatedStateVariable></argument>
    </argumentList></action>
    <action><name>GetPositionInfo</name><argumentList>
      <argument><name>InstanceID</name><direction>in</direction><relatedStateVariable>A_ARG_TYPE_InstanceID</relatedStateVariable></argument>
      <argument><name>Track</name><direction>out</direction><relatedStateVariable>CurrentTrack</relatedStateVariable></argument>
      <argument><name>TrackDuration</name><direction>out</direction><relatedStateVariable>CurrentTrackDuration</relatedStateVariable></argument>
      <argument><name>TrackMetaData</name><direction>out</direction><relatedStateVariable>CurrentTrackMetaData</relatedStateVariable></argument>
      <argument><name>TrackURI</name><direction>out</direction><relatedStateVariable>CurrentTrackURI</relatedStateVariable></argument>
      <argument><name>RelTime</name><direction>out</direction><relatedStateVariable>RelativeTimePosition</relatedStateVariable></argument>
      <argument><name>AbsTime</name><direction>out</direction><relatedStateVariable>AbsoluteTimePosition</relatedStateVariable></argument>
      <argument><name>RelCount</name><direction>out</direction><relatedStateVariable>RelativeCounterPosition</relatedStateVariable></argument>
      <argument><name>AbsCount</name><direction>out</direction><relatedStateVariable>AbsoluteCounterPosition</relatedStateVariable></argument>
    </argumentList></action>
    <action><name>GetTransportInfo</name><argumentList>
      <argument><name>InstanceID</name><direction>in</direction><relatedStateVariable>A_ARG_TYPE_InstanceID</relatedStateVariable></argument>
      <argument><name>CurrentTransportState</name><direction>out</direction><relatedStateVariable>TransportState</relatedStateVariable></argument>
      <argument><name>CurrentTransportStatus</name><direction>out</direction><relatedStateVariable>TransportStatus</relatedStateVariable></argument>
      <argument><name>CurrentSpeed</name><direction>out</direction><relatedStateVariable>TransportPlaySpeed</relatedStateVariable></argument>
    </argumentList></action>
    <action><name>GetMediaInfo</name><argumentList>
      <argument><name>InstanceID</name><direction>in</direction><relatedStateVariable>A_ARG_TYPE_InstanceID</relatedStateVariable></argument>
      <argument><name>NrTracks</name><direction>out</direction><relatedStateVariable>NumberOfTracks</relatedStateVariable></argument>
      <argument><name>MediaDuration</name><direction>out</direction><relatedStateVariable>CurrentMediaDuration</relatedStateVariable></argument>
      <argument><name>CurrentURI</name><direction>out</direction><relatedStateVariable>AVTransportURI</relatedStateVariable></argument>
      <argument><name>CurrentURIMetaData</name><direction>out</direction><relatedStateVariable>AVTransportURIMetaData</relatedStateVariable></argument>
      <argument><name>NextURI</name><direction>out</direction><relatedStateVariable>NextAVTransportURI</relatedStateVariable></argument>
      <argument><name>NextURIMetaData</name><direction>out</direction><relatedStateVariable>NextAVTransportURIMetaData</relatedStateVariable></argument>
      <argument><name>PlayMedium</name><direction>out</direction><relatedStateVariable>PlaybackStorageMedium</relatedStateVariable></argument>
      <argument><name>RecordMedium</name><direction>out</direction><relatedStateVariable>RecordStorageMedium</relatedStateVariable></argument>
      <argument><name>WriteStatus</name><direction>out</direction><relatedStateVariable>RecordMediumWriteStatus</relatedStateVariable></argument>
    </argumentList></action>
  </actionList>
  <serviceStateTable>
    <stateVariable sendEvents="no"><name>A_ARG_TYPE_InstanceID</name><dataType>ui4</dataType></stateVariable>
    <stateVariable sendEvents="no"><name>A_ARG_TYPE_SeekMode</name><dataType>string</dataType></stateVariable>
    <stateVariable sendEvents="no"><name>A_ARG_TYPE_SeekTarget</name><dataType>string</dataType></stateVariable>
    <stateVariable sendEvents="no"><name>TransportState</name><dataType>string</dataType></stateVariable>
    <stateVariable sendEvents="no"><name>TransportStatus</name><dataType>string</dataType></stateVariable>
    <stateVariable sendEvents="no"><name>TransportPlaySpeed</name><dataType>string</dataType></stateVariable>
    <stateVariable sendEvents="no"><name>NumberOfTracks</name><dataType>ui4</dataType></stateVariable>
    <stateVariable sendEvents="no"><name>CurrentTrack</name><dataType>ui4</dataType></stateVariable>
    <stateVariable sendEvents="no"><name>CurrentTrackDuration</name><dataType>string</dataType></stateVariable>
    <stateVariable sendEvents="no"><name>CurrentMediaDuration</name><dataType>string</dataType></stateVariable>
    <stateVariable sendEvents="no"><name>CurrentTrackMetaData</name><dataType>string</dataType></stateVariable>
    <stateVariable sendEvents="no"><name>CurrentTrackURI</name><dataType>string</dataType></stateVariable>
    <stateVariable sendEvents="no"><name>AVTransportURI</name><dataType>string</dataType></stateVariable>
    <stateVariable sendEvents="no"><name>AVTransportURIMetaData</name><dataType>string</dataType></stateVariable>
    <stateVariable sendEvents="no"><name>NextAVTransportURI</name><dataType>string</dataType></stateVariable>
    <stateVariable sendEvents="no"><name>NextAVTransportURIMetaData</name><dataType>string</dataType></stateVariable>
    <stateVariable sendEvents="no"><name>RelativeTimePosition</name><dataType>string</dataType></stateVariable>
    <stateVariable sendEvents="no"><name>AbsoluteTimePosition</name><dataType>string</dataType></stateVariable>
    <stateVariable sendEvents="no"><name>RelativeCounterPosition</name><dataType>i4</dataType></stateVariable>
    <stateVariable sendEvents="no"><name>AbsoluteCounterPosition</name><dataType>i4</dataType></stateVariable>
    <stateVariable sendEvents="no"><name>PlaybackStorageMedium</name><dataType>string</dataType></stateVariable>
    <stateVariable sendEvents="no"><name>RecordStorageMedium</name><dataType>string</dataType></stateVariable>
    <stateVariable sendEvents="no"><name>RecordMediumWriteStatus</name><dataType>string</dataType></stateVariable>
  </serviceStateTable>
</scpd>"#;

const RENDERING_CONTROL_SCPD: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<scpd xmlns="urn:schemas-upnp-org:service-1-0">
  <specVersion><major>1</major><minor>0</minor></specVersion>
  <actionList>
    <action><name>GetVolume</name><argumentList>
      <argument><name>InstanceID</name><direction>in</direction><relatedStateVariable>A_ARG_TYPE_InstanceID</relatedStateVariable></argument>
      <argument><name>Channel</name><direction>in</direction><relatedStateVariable>A_ARG_TYPE_Channel</relatedStateVariable></argument>
      <argument><name>CurrentVolume</name><direction>out</direction><relatedStateVariable>Volume</relatedStateVariable></argument>
    </argumentList></action>
    <action><name>SetVolume</name><argumentList>
      <argument><name>InstanceID</name><direction>in</direction><relatedStateVariable>A_ARG_TYPE_InstanceID</relatedStateVariable></argument>
      <argument><name>Channel</name><direction>in</direction><relatedStateVariable>A_ARG_TYPE_Channel</relatedStateVariable></argument>
      <argument><name>DesiredVolume</name><direction>in</direction><relatedStateVariable>Volume</relatedStateVariable></argument>
    </argumentList></action>
    <action><name>GetMute</name><argumentList>
      <argument><name>InstanceID</name><direction>in</direction><relatedStateVariable>A_ARG_TYPE_InstanceID</relatedStateVariable></argument>
      <argument><name>Channel</name><direction>in</direction><relatedStateVariable>A_ARG_TYPE_Channel</relatedStateVariable></argument>
      <argument><name>CurrentMute</name><direction>out</direction><relatedStateVariable>Mute</relatedStateVariable></argument>
    </argumentList></action>
    <action><name>SetMute</name><argumentList>
      <argument><name>InstanceID</name><direction>in</direction><relatedStateVariable>A_ARG_TYPE_InstanceID</relatedStateVariable></argument>
      <argument><name>Channel</name><direction>in</direction><relatedStateVariable>A_ARG_TYPE_Channel</relatedStateVariable></argument>
      <argument><name>DesiredMute</name><direction>in</direction><relatedStateVariable>Mute</relatedStateVariable></argument>
    </argumentList></action>
  </actionList>
  <serviceStateTable>
    <stateVariable sendEvents="no"><name>A_ARG_TYPE_InstanceID</name><dataType>ui4</dataType></stateVariable>
    <stateVariable sendEvents="no"><name>A_ARG_TYPE_Channel</name><dataType>string</dataType></stateVariable>
    <stateVariable sendEvents="yes"><name>Volume</name><dataType>ui2</dataType><allowedValueRange><minimum>0</minimum><maximum>100</maximum><step>1</step></allowedValueRange></stateVariable>
    <stateVariable sendEvents="yes"><name>Mute</name><dataType>boolean</dataType></stateVariable>
  </serviceStateTable>
</scpd>"#;

const CONNECTION_MANAGER_SCPD: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<scpd xmlns="urn:schemas-upnp-org:service-1-0">
  <specVersion><major>1</major><minor>0</minor></specVersion>
  <actionList>
    <action><name>GetProtocolInfo</name><argumentList>
      <argument><name>Source</name><direction>out</direction><relatedStateVariable>SourceProtocolInfo</relatedStateVariable></argument>
      <argument><name>Sink</name><direction>out</direction><relatedStateVariable>SinkProtocolInfo</relatedStateVariable></argument>
    </argumentList></action>
    <action><name>GetCurrentConnectionIDs</name><argumentList>
      <argument><name>ConnectionIDs</name><direction>out</direction><relatedStateVariable>CurrentConnectionIDs</relatedStateVariable></argument>
    </argumentList></action>
    <action><name>GetCurrentConnectionInfo</name><argumentList>
      <argument><name>ConnectionID</name><direction>in</direction><relatedStateVariable>A_ARG_TYPE_ConnectionID</relatedStateVariable></argument>
      <argument><name>RcsID</name><direction>out</direction><relatedStateVariable>A_ARG_TYPE_RcsID</relatedStateVariable></argument>
      <argument><name>AVTransportID</name><direction>out</direction><relatedStateVariable>A_ARG_TYPE_AVTransportID</relatedStateVariable></argument>
      <argument><name>ProtocolInfo</name><direction>out</direction><relatedStateVariable>A_ARG_TYPE_ProtocolInfo</relatedStateVariable></argument>
      <argument><name>PeerConnectionManager</name><direction>out</direction><relatedStateVariable>A_ARG_TYPE_ConnectionManager</relatedStateVariable></argument>
      <argument><name>PeerConnectionID</name><direction>out</direction><relatedStateVariable>A_ARG_TYPE_ConnectionID</relatedStateVariable></argument>
      <argument><name>Direction</name><direction>out</direction><relatedStateVariable>A_ARG_TYPE_Direction</relatedStateVariable></argument>
      <argument><name>Status</name><direction>out</direction><relatedStateVariable>A_ARG_TYPE_ConnectionStatus</relatedStateVariable></argument>
    </argumentList></action>
  </actionList>
  <serviceStateTable>
    <stateVariable sendEvents="yes"><name>SourceProtocolInfo</name><dataType>string</dataType></stateVariable>
    <stateVariable sendEvents="yes"><name>SinkProtocolInfo</name><dataType>string</dataType></stateVariable>
    <stateVariable sendEvents="yes"><name>CurrentConnectionIDs</name><dataType>string</dataType></stateVariable>
    <stateVariable sendEvents="no"><name>A_ARG_TYPE_ConnectionStatus</name><dataType>string</dataType></stateVariable>
    <stateVariable sendEvents="no"><name>A_ARG_TYPE_ConnectionManager</name><dataType>string</dataType></stateVariable>
    <stateVariable sendEvents="no"><name>A_ARG_TYPE_Direction</name><dataType>string</dataType></stateVariable>
    <stateVariable sendEvents="no"><name>A_ARG_TYPE_ProtocolInfo</name><dataType>string</dataType></stateVariable>
    <stateVariable sendEvents="no"><name>A_ARG_TYPE_ConnectionID</name><dataType>i4</dataType></stateVariable>
    <stateVariable sendEvents="no"><name>A_ARG_TYPE_AVTransportID</name><dataType>i4</dataType></stateVariable>
    <stateVariable sendEvents="no"><name>A_ARG_TYPE_RcsID</name><dataType>i4</dataType></stateVariable>
  </serviceStateTable>
</scpd>"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hms_parsing_and_formatting() {
        assert_eq!(parse_hms_to_ms("01:02:03"), Some(3_723_000));
        assert_eq!(format_ms_to_hms(3_723_000), "01:02:03");
    }

    #[test]
    fn test_xml_extraction() {
        let body = "<u:SetAVTransportURI><CurrentURI>http://192.0.2.50:8080/video.mp4</CurrentURI><CurrentURIMetaData>&lt;DIDL-Lite&gt;&lt;item&gt;&lt;dc:title&gt;Test Film&lt;/dc:title&gt;&lt;/item&gt;&lt;/DIDL-Lite&gt;</CurrentURIMetaData></u:SetAVTransportURI>";
        let uri = extract_xml_tag(body, "CurrentURI").unwrap();
        assert_eq!(uri, "http://192.0.2.50:8080/video.mp4");
        let meta = unescape_xml(&extract_xml_tag(body, "CurrentURIMetaData").unwrap());
        assert_eq!(
            extract_xml_tag(&meta, "dc:title"),
            Some("Test Film".to_string())
        );
    }
}
