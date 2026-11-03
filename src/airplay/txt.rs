//! DNS-SD TXT records and AirPlay feature bitmasks for `_airplay._tcp` and `_raop._tcp`.

use super::{device_id_colon, Config, MODEL, SOURCE_VERSION};

/// Low 32 bits of AirPlay features (matching UxPlay `FEATURES_1 = 0x5A7FFEE6` with
/// HLS bits 0 & 4 off so iOS/iPadOS always uses low-latency screen mirroring + RAOP audio,
/// and bit 27 `SupportsLegacyPairing` on).
pub const FEATURES_1: u32 = 0x5A7F_FEE6;

/// Bit 42 (`SupportsScreenMultiCodec` -> H.265 / HEVC mirroring, required for 4K).
pub const FEATURE_H265_HI: u32 = 1 << (42 - 32);

pub const AIRPLAY_PI: &str = "2e388006-13ba-4041-9a67-25dd4a43d536";
pub const DISPLAY_UUID: &str = "e0ff8a27-6738-3d56-8a16-cc53aacee925";

pub fn features_pair(h265: bool) -> (u32, u32) {
    let f2 = if h265 { FEATURE_H265_HI } else { 0 };
    (FEATURES_1, f2)
}

pub fn features_u64(h265: bool) -> u64 {
    let (f1, f2) = features_pair(h265);
    ((f2 as u64) << 32) | (f1 as u64)
}

pub fn features_str(h265: bool) -> String {
    let (f1, f2) = features_pair(h265);
    format!("0x{f1:X},0x{f2:X}")
}

pub fn codecs_cn(alac: bool, aac: bool) -> String {
    let mut s = String::from("0");
    if alac {
        s.push_str(",1");
    }
    if aac {
        s.push_str(",2,3");
    }
    s
}

/// Service name for `_raop._tcp`: `<12-hex-uppercase-mac>@<Name>`.
pub fn raop_service_name(cfg: &Config) -> String {
    let mac: String = cfg.device_id.iter().map(|b| format!("{b:02X}")).collect();
    format!("{mac}@{}", cfg.name)
}

pub fn txt_airplay(cfg: &Config, pk_hex: &str) -> Vec<(String, String)> {
    let dev = device_id_colon(&cfg.device_id);
    let ft = features_str(cfg.h265);
    let pw = if cfg.require_pin { "true" } else { "false" };
    vec![
        ("deviceid".into(), dev),
        ("features".into(), ft),
        ("pw".into(), pw.into()),
        ("flags".into(), "0x4".into()),
        ("model".into(), MODEL.into()),
        ("pk".into(), pk_hex.into()),
        ("pi".into(), AIRPLAY_PI.into()),
        ("srcvers".into(), SOURCE_VERSION.into()),
        ("vv".into(), "2".into()),
    ]
}

pub fn txt_raop(cfg: &Config, pk_hex: &str) -> Vec<(String, String)> {
    let ft = features_str(cfg.h265);
    let cn = codecs_cn(cfg.alac, cfg.aac);
    let (pw, sf) = if cfg.require_pin {
        ("true", "0x8c")
    } else {
        ("false", "0x4")
    };
    vec![
        ("ch".into(), "2".into()),
        ("cn".into(), cn),
        ("da".into(), "true".into()),
        ("et".into(), "0,3,5".into()),
        ("vv".into(), "2".into()),
        ("ft".into(), ft),
        ("am".into(), MODEL.into()),
        ("md".into(), "0,1,2".into()),
        ("rhd".into(), "5.6.0.0".into()),
        ("pw".into(), pw.into()),
        ("sf".into(), sf.into()),
        ("sr".into(), "44100".into()),
        ("ss".into(), "16".into()),
        ("sv".into(), "false".into()),
        ("tp".into(), "UDP".into()),
        ("txtvers".into(), "1".into()),
        ("vs".into(), SOURCE_VERSION.into()),
        ("vn".into(), "65537".into()),
        ("pk".into(), pk_hex.into()),
    ]
}

/// DNS-SD length-prefixed wire format used in `GET /info` (`txtAirPlay` / `txtRAOP`).
pub fn encode_txt_wire(entries: &[(String, String)]) -> Vec<u8> {
    let mut out = Vec::new();
    for (k, v) in entries {
        let s = format!("{k}={v}");
        let b = s.as_bytes();
        if b.len() <= 255 {
            out.push(b.len() as u8);
            out.extend_from_slice(b);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn h265_sets_bit_42() {
        assert_eq!(features_str(false), "0x5A7FFEE6,0x0");
        assert_eq!(features_str(true), "0x5A7FFEE6,0x400");
        assert_eq!(features_u64(true) & (1u64 << 42), 1u64 << 42);
    }
}
