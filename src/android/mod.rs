//! Android JNI glue (`com.yansit.tv.N`).
//!
//! Coordinates both the persistent foreground `AirPlayService` (which runs even when the UI
//! activity is closed and advertises `_airplay._tcp` / `_raop._tcp` via Android `NsdManager`)
//! and `MainActivity` (which hosts the hardware video `SurfaceView` and the translucent GLES
//! UI overlay `SurfaceView`).

pub mod egl;
pub mod keys;
mod render;

use jni::objects::{JClass, JObject, JString};
use jni::sys::{jboolean, jint, jlong, jstring};
use jni::JNIEnv;
use ndk::native_window::NativeWindow;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering::Relaxed};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use crate::airplay::Server;
use crate::media::MediaEngine;
use keys::KeyMap;
use render::Ev;

pub static EXIT: AtomicBool = AtomicBool::new(false);

struct Runtime {
    engine: Arc<MediaEngine>,
    server: Server,
}

static RUNTIME: OnceLock<Runtime> = OnceLock::new();
static TX: OnceLock<Mutex<Sender<Ev>>> = OnceLock::new();
static KEYS: Mutex<Option<KeyMap>> = Mutex::new(None);
static LAYOUT: AtomicI64 = AtomicI64::new(-1);
static AWAKE: AtomicI64 = AtomicI64::new(-1);

fn ensure_runtime(files_dir: &str) -> &'static Runtime {
    RUNTIME.get_or_init(|| {
        android_logger::init_once(
            android_logger::Config::default()
                .with_max_level(log::LevelFilter::Info)
                .with_tag("yansit"),
        );
        std::panic::set_hook(Box::new(|i| log::error!("panic: {i}")));

        let state_dir = if files_dir.is_empty() {
            PathBuf::from("/data/data/com.yansit.tv/files")
        } else {
            PathBuf::from(files_dir)
        };
        crate::billing::init(&state_dir);
        let engine = MediaEngine::new(state_dir.clone());
        // Before the first advertisement: the stored preset name follows the UI language.
        engine.localize_device_name(crate::i18n::apply(engine.settings().language));
        engine.take_reconfig_dirty();
        let cfg = engine
            .settings()
            .to_airplay_config(&state_dir, engine.device_id(), 7000);
        let server = Server::start(cfg, engine.clone()).expect("start AirPlay server");
        log::info!(
            "Ayna AirPlay server listening on port {} as '{}'",
            server.port(),
            server.server_name()
        );
        crate::dlna::start(engine.clone());
        Runtime { engine, server }
    })
}

/// Device UI locale as a BCP-47 tag ("tr-TR", "pt-BR"), from `java.util.Locale.getDefault()`.
fn device_locale_tag(env: &mut JNIEnv) -> Option<String> {
    let loc = env
        .call_static_method("java/util/Locale", "getDefault", "()Ljava/util/Locale;", &[])
        .ok()?
        .l()
        .ok()?;
    let tag = env
        .call_method(&loc, "toLanguageTag", "()Ljava/lang/String;", &[])
        .ok()?
        .l()
        .ok()?;
    env.get_string(&JString::from(tag)).ok().map(Into::into)
}

/// Read the device language before anything localized is created (first-run receiver name)
/// and on every activity start, so a system-language change is picked up without a reinstall.
fn detect_device_language(env: &mut JNIEnv) {
    match device_locale_tag(env) {
        Some(tag) => {
            let l = crate::i18n::set_device_tag(&tag);
            log::info!("device locale {tag} -> UI language {}", l.code());
        }
        None => {
            if env.exception_check().unwrap_or(false) {
                let _ = env.exception_clear();
            }
        }
    }
}

pub fn disconnect_active_client() {
    crate::miracast::on_peer_disconnected();
    if let Some(rt) = RUNTIME.get() {
        rt.server.disconnect_client();
        rt.engine.dlna_stop();
    }
}

fn send(ev: Ev) -> bool {
    TX.get()
        .is_some_and(|tx| tx.lock().unwrap().send(ev).is_ok())
}

fn jstr(env: &mut JNIEnv, s: &JString) -> String {
    if s.is_null() {
        return String::new();
    }
    env.get_string(s).map(Into::into).unwrap_or_default()
}

fn to_jstring(env: &JNIEnv, s: &str) -> jstring {
    env.new_string(s)
        .map(|j| j.into_raw())
        .unwrap_or(std::ptr::null_mut())
}

/// Called by `AirPlayService.onCreate` to start the Rust AirPlay server in the background.
#[no_mangle]
pub extern "system" fn Java_com_yansit_tv_N_ensureServer(
    mut env: JNIEnv,
    _: JClass,
    files: JString,
) -> jint {
    let f = jstr(&mut env, &files);
    detect_device_language(&mut env);
    let rt = ensure_runtime(&f);
    let lang = crate::i18n::apply(rt.engine.settings().language);
    rt.engine.localize_device_name(lang);
    if let Ok(vm) = env.get_java_vm() {
        crate::miracast::start_daemon(vm, rt.engine.clone());
    }
    rt.server.port() as jint
}

#[no_mangle]
pub extern "system" fn Java_com_yansit_tv_N_port(_: JNIEnv, _: JClass) -> jint {
    RUNTIME.get().map_or(7000, |rt| rt.server.port() as jint)
}

#[no_mangle]
pub extern "system" fn Java_com_yansit_tv_N_serverName(env: JNIEnv, _: JClass) -> jstring {
    let name = RUNTIME
        .get()
        .map_or_else(|| "Salon TV".into(), |rt| rt.server.server_name());
    to_jstring(&env, &name)
}

#[no_mangle]
pub extern "system" fn Java_com_yansit_tv_N_raopName(env: JNIEnv, _: JClass) -> jstring {
    let name = RUNTIME
        .get()
        .map_or_else(|| "0259414E5354@Salon TV".into(), |rt| rt.server.raop_service_name());
    to_jstring(&env, &name)
}

/// Newline-separated `key=value` entries for `_airplay._tcp` mDNS registration.
#[no_mangle]
pub extern "system" fn Java_com_yansit_tv_N_airplayTxt(env: JNIEnv, _: JClass) -> jstring {
    let Some(rt) = RUNTIME.get() else {
        return to_jstring(&env, "");
    };
    let s = rt
        .server
        .txt_airplay()
        .into_iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("\n");
    to_jstring(&env, &s)
}

/// Newline-separated `key=value` entries for `_raop._tcp` mDNS registration.
#[no_mangle]
pub extern "system" fn Java_com_yansit_tv_N_raopTxt(env: JNIEnv, _: JClass) -> jstring {
    let Some(rt) = RUNTIME.get() else {
        return to_jstring(&env, "");
    };
    let s = rt
        .server
        .txt_raop()
        .into_iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("\n");
    to_jstring(&env, &s)
}

/// Polled by `AirPlayService`: if settings changed in the UI, updates the server config and
/// returns `true` so `AirPlayService` re-registers mDNS with the updated name/features.
#[no_mangle]
pub extern "system" fn Java_com_yansit_tv_N_pollConfigChanged(_: JNIEnv, _: JClass) -> jboolean {
    let Some(rt) = RUNTIME.get() else {
        return 0;
    };
    // Settings that senders see changed, or Pro flipped (1080p <-> 4K). Consume both flags on
    // every poll: non-short-circuit `|`.
    if rt.engine.take_reconfig_dirty() | crate::billing::take_pro_changed() {
        let cfg = rt.engine.settings().to_airplay_config(
            rt.engine.state_dir(),
            rt.engine.device_id(),
            rt.server.port(),
        );
        rt.server.update_config(cfg);
        return 1;
    }
    0
}

/// Polled by `AirPlayService`: returns `true` when a new AirPlay session starts and
/// `auto_wake` is enabled, so `AirPlayService` brings `MainActivity` to the front.
#[no_mangle]
pub extern "system" fn Java_com_yansit_tv_N_pollWakeRequest(_: JNIEnv, _: JClass) -> jboolean {
    let Some(rt) = RUNTIME.get() else {
        return 0;
    };
    rt.engine.poll_wake_request() as jboolean
}

#[no_mangle]
pub extern "system" fn Java_com_yansit_tv_N_create(
    mut env: JNIEnv,
    _: JClass,
    files: JString,
    _cache: JString,
) {
    AWAKE.store(-1, Relaxed);
    LAYOUT.store(-1, Relaxed);
    EXIT.store(false, Relaxed);

    let f = jstr(&mut env, &files);
    detect_device_language(&mut env);
    let rt = ensure_runtime(&f);
    let lang = crate::i18n::apply(rt.engine.settings().language);
    rt.engine.localize_device_name(lang);
    if let Ok(vm) = env.get_java_vm() {
        crate::miracast::start_daemon(vm, rt.engine.clone());
    }

    if TX.get().is_some() {
        send(Ev::Wake(Duration::ZERO));
        return;
    }
    let (tx, rx) = channel();
    let _ = TX.set(Mutex::new(tx.clone()));
    render::spawn(tx.clone(), rx);
    let _ = tx.send(Ev::Ready(rt.engine.clone()));
}

/// `kind == 0`: hardware video `SurfaceView`; `kind == 1`: translucent GLES overlay `SurfaceView`.
#[no_mangle]
pub extern "system" fn Java_com_yansit_tv_N_surface(
    env: JNIEnv,
    _: JClass,
    kind: jint,
    surface: JObject,
) {
    let win = if surface.is_null() {
        None
    } else {
        unsafe { NativeWindow::from_surface(env.get_raw(), surface.as_raw()) }
    };
    if kind == 0 {
        if let Some(rt) = RUNTIME.get() {
            rt.engine.video_player().set_surface(win);
        }
        return;
    }
    let (ack, done) = channel();
    let destroyed = win.is_none();
    if send(Ev::Overlay(win, ack))
        && destroyed
        && done.recv_timeout(Duration::from_secs(2)).is_err()
    {
        log::warn!("overlay surface ack timeout");
    }
}

#[no_mangle]
pub extern "system" fn Java_com_yansit_tv_N_resized(_: JNIEnv, _: JClass) {
    send(Ev::Wake(Duration::ZERO));
}

#[no_mangle]
pub extern "system" fn Java_com_yansit_tv_N_key(
    _: JNIEnv,
    _: JClass,
    code: jint,
    action: jint,
    repeat: jint,
    long: jboolean,
    uni: jint,
) -> jboolean {
    if !render::UI_READY.load(Relaxed) {
        return 0;
    }
    let (consumed, key) = KEYS
        .lock()
        .unwrap()
        .get_or_insert_with(KeyMap::default)
        .on(code, action, repeat, long != 0, uni.max(0) as u32);
    if let Some(k) = key {
        send(Ev::Key(k));
    }
    consumed as jboolean
}

#[no_mangle]
pub extern "system" fn Java_com_yansit_tv_N_life(_: JNIEnv, _: JClass, ev: jint) {
    if let Some(rt) = RUNTIME.get() {
        match ev {
            0 | 1 => rt.engine.set_ui_visible(true),
            2 | 3 | 4 => rt.engine.set_ui_visible(false),
            _ => {}
        }
    }
    if ev == 0 || ev == 1 {
        send(Ev::Wake(Duration::ZERO));
    }
}

#[no_mangle]
pub extern "system" fn Java_com_yansit_tv_N_vsync(
    _: JNIEnv,
    _: JClass,
    _t: jlong,
    _period: jlong,
) -> jboolean {
    EXIT.swap(false, Relaxed) as jboolean
}

/// Keep screen on (1 / 0 / -1 unchanged).
#[no_mangle]
pub extern "system" fn Java_com_yansit_tv_N_awake(_: JNIEnv, _: JClass) -> jint {
    let on = RUNTIME.get().is_some_and(|rt| rt.engine.is_active()) as i64;
    if AWAKE.swap(on, Relaxed) == on {
        -1
    } else {
        on as jint
    }
}

/// Polled every vsync: returns `(w << 32) | h` when the aspect-fitted video view dimensions change, else `-1`.
#[no_mangle]
pub extern "system" fn Java_com_yansit_tv_N_videoLayout(
    _: JNIEnv,
    _: JClass,
    sw: jint,
    sh: jint,
) -> jlong {
    if sw <= 0 || sh <= 0 {
        return -1;
    }
    let d = RUNTIME
        .get()
        .map_or(0, |rt| rt.engine.video_layout_dims());
    let (vw, vh) = ((d >> 32) as i32, d as u32 as i32);
    let (w, h) = if vw <= 0 || vh <= 0 {
        (sw, sh)
    } else {
        let a = vw as f64 / vh as f64;
        if a > sw as f64 / sh as f64 {
            (sw, (sw as f64 / a).round() as i32)
        } else {
            ((sh as f64 * a).round() as i32, sh)
        }
    };
    let v = (w as i64) << 32 | (h as u32 as i64);
    if LAYOUT.swap(v, Relaxed) == v {
        -1
    } else {
        v
    }
}
