//! Monetization entitlement state (paid membership + free-tier ads), shared between the egui
//! UI (reads it to gate 4K / show the upgrade row / decide on ads) and the Java billing+ads
//! shell (writes purchase state in, polls user actions out). See `docs/MONETIZATION.md`.
//!
//! Products: `yansit_pro_yearly` (subscription, $10/yr) and `yansit_pro_lifetime` (one-time
//! INAPP, $20). Prices live in Play Console; this layer only displays the strings Billing hands
//! back and never parses amounts. GPL-3.0-or-later.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering::Relaxed};
use std::sync::{Mutex, OnceLock, RwLock};
use std::time::{Duration, Instant};

/// Snapshot of billing/ads state the UI renders from.
#[derive(Clone, Debug)]
pub struct Billing {
    /// True when the yearly subscription is active OR the lifetime product is owned.
    pub pro: bool,
    pub yearly_owned: bool,
    pub lifetime_owned: bool,
    /// Localized display strings from Billing ("" until known). Display only.
    pub yearly_price: String,
    pub lifetime_price: String,
    /// Billing client connected and products queried (so the UI can show a spinner vs prices).
    pub ready: bool,
}

impl Billing {
    pub const fn new() -> Self {
        Billing {
            pro: false,
            yearly_owned: false,
            lifetime_owned: false,
            yearly_price: String::new(),
            lifetime_price: String::new(),
            ready: false,
        }
    }
}

impl Default for Billing {
    fn default() -> Self {
        Billing::new()
    }
}

/// Purchase action the UI requests; Java polls and clears it (0 none, 1 yearly, 2 lifetime,
/// 3 restore, 4 manage subscriptions, 5 open store review).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    None = 0,
    BuyYearly = 1,
    BuyLifetime = 2,
    Restore = 3,
    ManageSubs = 4,
    Review = 5,
}

static STATE: RwLock<Billing> = RwLock::new(Billing::new());
static ACTION: AtomicI32 = AtomicI32::new(0);
/// Set by the free-tier gate; Java polls once to show an interstitial between sessions.
static SHOW_INTERSTITIAL: AtomicBool = AtomicBool::new(false);
/// Java tells us an interstitial finished loading (so we only request when one is ready).
static INTERSTITIAL_READY: AtomicBool = AtomicBool::new(false);
static REVIEW_PROMPTED: AtomicBool = AtomicBool::new(false);
/// Frequency cap: at most one interstitial per `AD_MIN_GAP` (free tier stays pleasant to use).
static LAST_INTERSTITIAL: Mutex<Option<Instant>> = Mutex::new(None);
const AD_MIN_GAP: Duration = Duration::from_secs(10 * 60);
static STATE_DIR: OnceLock<PathBuf> = OnceLock::new();
/// Pro flipped (purchase / expiry / restore): the receiver must re-advertise its display size
/// (free = 1080p, Pro = 4K). Consumed by `N.pollConfigChanged`.
static PRO_CHANGED: AtomicBool = AtomicBool::new(false);

/// Load the cached Pro flag so a paying user never flashes the free UI / an ad on cold start,
/// before the Billing client has reconnected and re-confirmed ownership.
pub fn init(state_dir: &Path) {
    let _ = STATE_DIR.set(state_dir.to_path_buf());
    let cached = std::fs::read_to_string(state_dir.join("pro.cache"))
        .map(|s| s.trim() == "1")
        .unwrap_or(false);
    let reviewed = state_dir.join("reviewed").exists();
    REVIEW_PROMPTED.store(reviewed, Relaxed);
    let mut st = STATE.write().unwrap();
    st.pro = cached;
    st.ready = false;
}

pub fn is_review_prompted() -> bool {
    REVIEW_PROMPTED.load(Relaxed)
}

pub fn mark_review_prompted() {
    REVIEW_PROMPTED.store(true, Relaxed);
    if let Some(dir) = STATE_DIR.get() {
        let _ = std::fs::write(dir.join("reviewed"), "1");
    }
}

/// Current snapshot (cheap clone of two short strings).
pub fn snapshot() -> Billing {
    let mut b = STATE.read().unwrap().clone();
    b.pro |= cfg!(feature = "dev-pro");
    b
}

pub fn is_pro() -> bool {
    cfg!(feature = "dev-pro") || STATE.read().unwrap().pro
}

/// Update from the Billing client (called via JNI on Android).
pub fn set_state(
    pro: bool,
    yearly_owned: bool,
    lifetime_owned: bool,
    yearly_price: String,
    lifetime_price: String,
    ready: bool,
) {
    {
        let mut st = STATE.write().unwrap();
        if st.pro != pro {
            PRO_CHANGED.store(true, Relaxed);
        }
        st.pro = pro;
        st.yearly_owned = yearly_owned;
        st.lifetime_owned = lifetime_owned;
        if !yearly_price.is_empty() {
            st.yearly_price = yearly_price;
        }
        if !lifetime_price.is_empty() {
            st.lifetime_price = lifetime_price;
        }
        st.ready = ready;
    }
    // Persist Pro so cold starts don't show ads to a paying user.
    if let Some(dir) = STATE_DIR.get() {
        let _ = std::fs::write(dir.join("pro.cache"), if pro { "1" } else { "0" });
    }
}

/// True once after the Pro state flipped (see `PRO_CHANGED`).
pub fn take_pro_changed() -> bool {
    PRO_CHANGED.swap(false, Relaxed)
}

/// UI → request a purchase / restore / manage flow.
pub fn request(action: Action) {
    ACTION.store(action as i32, Relaxed);
}

/// Java polls this and launches the matching Play flow; returns then clears.
pub fn take_action() -> i32 {
    ACTION.swap(0, Relaxed)
}

/// Free-tier gate asks for an interstitial (no-op for Pro, when none is loaded, or within
/// `AD_MIN_GAP` of the previous one).
pub fn request_interstitial() {
    if is_pro() || !INTERSTITIAL_READY.load(Relaxed) {
        return;
    }
    let now = Instant::now();
    let mut last = LAST_INTERSTITIAL.lock().unwrap();
    if ad_gap_elapsed(*last, now) {
        *last = Some(now);
        SHOW_INTERSTITIAL.store(true, Relaxed);
    }
}

fn ad_gap_elapsed(last: Option<Instant>, now: Instant) -> bool {
    last.map_or(true, |t| now.saturating_duration_since(t) >= AD_MIN_GAP)
}

/// Java polls once; returns true a single time when an interstitial should be shown.
pub fn take_show_interstitial() -> bool {
    SHOW_INTERSTITIAL.swap(false, Relaxed)
}

pub fn set_interstitial_ready(ready: bool) {
    INTERSTITIAL_READY.store(ready, Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interstitial_frequency_cap() {
        let t0 = Instant::now();
        assert!(ad_gap_elapsed(None, t0));
        assert!(!ad_gap_elapsed(Some(t0), t0 + Duration::from_secs(60)));
        assert!(!ad_gap_elapsed(Some(t0), t0 + AD_MIN_GAP - Duration::from_secs(1)));
        assert!(ad_gap_elapsed(Some(t0), t0 + AD_MIN_GAP));
    }
}

#[cfg(target_os = "android")]
mod jni_api {
    use jni::objects::{JClass, JString};
    use jni::sys::{jboolean, jint};
    use jni::JNIEnv;

    fn jstr(env: &mut JNIEnv, s: &JString) -> String {
        env.get_string(s).map(Into::into).unwrap_or_default()
    }

    /// Java → Rust: full billing state (see docs/MONETIZATION.md).
    #[no_mangle]
    pub extern "system" fn Java_com_yansit_tv_N_billingState(
        mut env: JNIEnv,
        _: JClass,
        pro: jboolean,
        yearly_owned: jboolean,
        lifetime_owned: jboolean,
        yearly_price: JString,
        lifetime_price: JString,
        ready: jboolean,
    ) {
        let yp = jstr(&mut env, &yearly_price);
        let lp = jstr(&mut env, &lifetime_price);
        super::set_state(pro != 0, yearly_owned != 0, lifetime_owned != 0, yp, lp, ready != 0);
    }

    #[no_mangle]
    pub extern "system" fn Java_com_yansit_tv_N_adLoaded(
        _: JNIEnv,
        _: JClass,
        interstitial_ready: jboolean,
    ) {
        super::set_interstitial_ready(interstitial_ready != 0);
    }

    #[no_mangle]
    pub extern "system" fn Java_com_yansit_tv_N_pollBillingAction(_: JNIEnv, _: JClass) -> jint {
        super::take_action()
    }

    #[no_mangle]
    pub extern "system" fn Java_com_yansit_tv_N_pollShowInterstitial(
        _: JNIEnv,
        _: JClass,
    ) -> jboolean {
        super::take_show_interstitial() as jboolean
    }
}
