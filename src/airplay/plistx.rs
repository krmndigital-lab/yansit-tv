//! Binary plist helpers and the `GET /info` response builder.

use plist::{Dictionary, Integer, Value};

use super::txt::{
    encode_txt_wire, features_u64, txt_airplay, txt_raop, AIRPLAY_PI, DISPLAY_UUID,
};
use super::{device_id_colon, hex, Config, MODEL, SOURCE_VERSION};

pub fn from_bytes(data: &[u8]) -> Option<Dictionary> {
    if data.is_empty() {
        return Some(Dictionary::new());
    }
    let val = Value::from_reader(std::io::Cursor::new(data)).ok()?;
    val.into_dictionary()
}

pub fn to_bytes(dict: Dictionary) -> Vec<u8> {
    let mut out = Vec::with_capacity(512);
    let _ = Value::Dictionary(dict).to_writer_binary(&mut out);
    out
}

pub fn get_str<'a>(d: &'a Dictionary, k: &str) -> Option<&'a str> {
    d.get(k)?.as_string()
}

pub fn get_u64(d: &Dictionary, k: &str) -> Option<u64> {
    let v = d.get(k)?;
    v.as_unsigned_integer()
        .or_else(|| v.as_signed_integer().map(|x| x as u64))
}

pub fn get_bool(d: &Dictionary, k: &str) -> Option<bool> {
    match d.get(k)? {
        Value::Boolean(b) => Some(*b),
        Value::Integer(i) => Some(i.as_unsigned().unwrap_or(0) != 0),
        _ => None,
    }
}

pub fn get_data<'a>(d: &'a Dictionary, k: &str) -> Option<&'a [u8]> {
    d.get(k)?.as_data()
}

/// Build the binary plist returned by `GET /info`.
/// Matches UxPlay `raop_handler_info` in `lib/raop_handlers.h`.
pub fn info_response(cfg: &Config, pk_bytes: &[u8; 32], req_body: &[u8]) -> Vec<u8> {
    let pk_hex = hex(pk_bytes);
    let req = from_bytes(req_body).unwrap_or_default();

    // Check if client requested specific TXT qualifiers (`txtAirPlay`, `txtRAOP`).
    let mut has_txt_airplay = false;
    let mut has_txt_raop = false;
    if let Some(Value::Array(quals)) = req.get("qualifier") {
        for q in quals {
            if let Some(s) = q.as_string() {
                if s == "txtAirPlay" {
                    has_txt_airplay = true;
                } else if s == "txtRAOP" {
                    has_txt_raop = true;
                }
            }
        }
    }

    let mut resp = Dictionary::new();
    if has_txt_airplay || has_txt_raop {
        if has_txt_airplay {
            let wire = encode_txt_wire(&txt_airplay(cfg, &pk_hex));
            resp.insert("txtAirPlay".into(), Value::Data(wire));
        }
        if has_txt_raop {
            let wire = encode_txt_wire(&txt_raop(cfg, &pk_hex));
            resp.insert("txtRAOP".into(), Value::Data(wire));
        }
        return to_bytes(resp);
    }

    // Audio latency descriptors (type 100 = general, type 101 = screen mirroring).
    let mut al0 = Dictionary::new();
    al0.insert("outputLatencyMicros".into(), Value::Boolean(false));
    al0.insert("type".into(), Value::Integer(Integer::from(100u64)));
    al0.insert("audioType".into(), Value::String("default".into()));
    al0.insert("inputLatencyMicros".into(), Value::Boolean(false));

    let mut al1 = Dictionary::new();
    al1.insert("outputLatencyMicros".into(), Value::Boolean(false));
    al1.insert("type".into(), Value::Integer(Integer::from(101u64)));
    al1.insert("audioType".into(), Value::String("default".into()));
    al1.insert("inputLatencyMicros".into(), Value::Boolean(false));

    resp.insert(
        "audioLatencies".into(),
        Value::Array(vec![Value::Dictionary(al0), Value::Dictionary(al1)]),
    );

    // Display descriptor — reports configured resolution (e.g. 3840x2160 or 1920x1080) & maxFPS.
    let mut disp = Dictionary::new();
    disp.insert("uuid".into(), Value::String(DISPLAY_UUID.into()));
    disp.insert("widthPhysical".into(), Value::Boolean(false));
    disp.insert("heightPhysical".into(), Value::Boolean(false));
    disp.insert(
        "width".into(),
        Value::Integer(Integer::from(cfg.width as u64)),
    );
    disp.insert(
        "height".into(),
        Value::Integer(Integer::from(cfg.height as u64)),
    );
    disp.insert(
        "widthPixels".into(),
        Value::Integer(Integer::from(cfg.width as u64)),
    );
    disp.insert(
        "heightPixels".into(),
        Value::Integer(Integer::from(cfg.height as u64)),
    );
    let refresh_rate = 1.0f64 / (cfg.refresh_hz.max(1) as f64);
    disp.insert("refreshRate".into(), Value::Real(refresh_rate));
    disp.insert(
        "maxFPS".into(),
        Value::Integer(Integer::from(cfg.max_fps as u64)),
    );
    disp.insert("features".into(), Value::Integer(Integer::from(14u64)));
    disp.insert("rotation".into(), Value::Boolean(true));
    disp.insert("overscanned".into(), Value::Boolean(cfg.overscanned));

    resp.insert(
        "displays".into(),
        Value::Array(vec![Value::Dictionary(disp)]),
    );

    // Audio formats (100 = general, 101 = mirroring).
    let mut af0 = Dictionary::new();
    af0.insert("type".into(), Value::Integer(Integer::from(100u64)));
    af0.insert(
        "audioInputFormats".into(),
        Value::Integer(Integer::from(67108860u64)),
    );
    af0.insert(
        "audioOutputFormats".into(),
        Value::Integer(Integer::from(67108860u64)),
    );

    let mut af1 = Dictionary::new();
    af1.insert("type".into(), Value::Integer(Integer::from(101u64)));
    af1.insert(
        "audioInputFormats".into(),
        Value::Integer(Integer::from(67108860u64)),
    );
    af1.insert(
        "audioOutputFormats".into(),
        Value::Integer(Integer::from(67108860u64)),
    );

    resp.insert(
        "audioFormats".into(),
        Value::Array(vec![Value::Dictionary(af0), Value::Dictionary(af1)]),
    );

    let dev_id = device_id_colon(&cfg.device_id);
    let status_flags: u64 = if cfg.require_pin { 0x8c } else { 0x04 };
    resp.insert(
        "features".into(),
        Value::Integer(Integer::from(features_u64(cfg.h265) as i64)),
    );
    resp.insert(
        "statusFlags".into(),
        Value::Integer(Integer::from(status_flags)),
    );
    resp.insert("keepAliveLowPower".into(), Value::Integer(Integer::from(1u64)));
    resp.insert(
        "keepAliveSendStatsAsBody".into(),
        Value::Integer(Integer::from(1u64)),
    );
    resp.insert("pi".into(), Value::String(AIRPLAY_PI.into()));
    resp.insert("pk".into(), Value::Data(pk_bytes.to_vec()));
    resp.insert("vv".into(), Value::Integer(Integer::from(2u64)));
    resp.insert("deviceID".into(), Value::String(dev_id.clone()));
    resp.insert("macAddress".into(), Value::String(dev_id));
    resp.insert("model".into(), Value::String(MODEL.into()));
    resp.insert("name".into(), Value::String(cfg.name.clone()));
    resp.insert(
        "sourceVersion".into(),
        Value::String(SOURCE_VERSION.into()),
    );
    resp.insert("initialVolume".into(), Value::Real(0.0));

    to_bytes(resp)
}
