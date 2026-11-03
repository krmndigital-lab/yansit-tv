//! DLNA / UPnP `MediaRenderer:1` + SSDP (`239.255.255.250:1900`) + HTTP/SOAP (`7001`) Receiver.

pub mod player;
pub mod soap;
pub mod ssdp;

use crate::media::MediaEngine;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

pub const DLNA_HTTP_PORT: u16 = 7001;

static DLNA_STOP: OnceLock<Arc<AtomicBool>> = OnceLock::new();

/// Start the DLNA SSDP discovery responder and HTTP/SOAP `MediaRenderer:1` control server.
pub fn start(engine: Arc<MediaEngine>) {
    if let Some(prev) = DLNA_STOP.get() {
        if !prev.load(Ordering::Relaxed) {
            return;
        }
    }
    let stop = Arc::new(AtomicBool::new(false));
    let _ = DLNA_STOP.set(stop.clone());

    let id = engine.device_id();
    let device_uuid = format!(
        "59414e53-4954-444c-4e41-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        id[0], id[1], id[2], id[3], id[4], id[5]
    );

    let ssdp_uuid = device_uuid.clone();
    let ssdp_stop = stop.clone();
    let _ = std::thread::Builder::new()
        .name("dlna-ssdp".into())
        .spawn(move || {
            ssdp::run_ssdp_responder(ssdp_uuid, DLNA_HTTP_PORT, ssdp_stop);
        });

    let soap_uuid = device_uuid;
    let soap_stop = stop;
    let _ = std::thread::Builder::new()
        .name("dlna-soap".into())
        .spawn(move || {
            soap::run_http_soap_server(DLNA_HTTP_PORT, soap_uuid, engine, soap_stop);
        });
}
