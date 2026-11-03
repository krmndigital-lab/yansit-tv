# Ayna — Monetization & Build Contract (single source of truth)

Chosen path: **open-source (GPL-3.0-or-later), all 3 protocols kept, sold on Google Play with
ads (free tier) + paid membership.** Selling GPL software and bundling ads/IAP is allowed; we
publish the corresponding source (see "GPL compliance"). No code "encryption"/closed-sourcing.

Everyone (coordinator + subagents) must follow the IDs, semantics and JNI signatures here
verbatim so the Rust core and the Java shell agree.

## Products (prices are set in Play Console, NOT in the APK)
- `yansit_pro_yearly` — **subscription**, base plan id `yearly`, price **$10.00 / year**
  (localized per country in Console).
- `yansit_pro_lifetime` — **one-time in-app product (INAPP)**, price **$20.00** one-time.

A device is **Pro** if the yearly subscription is active **or** the lifetime product is owned.

## Free vs Pro split (keeps the demo fully usable; upsell = quality + no ads)
| Capability | Free | Pro |
|---|---|---|
| AirPlay + Miracast + DLNA receive | ✅ | ✅ |
| Max mirror resolution | 1080p | 2160p (4K) |
| Max fps | 60 | 60 |
| HEVC / H.265 decode | ✅ (kept — it is the fast path) | ✅ |
| Ads (house/AdMob interstitial between sessions) | shown | none |
| Interstitial frequency cap | ≤ 1 per 10 min, only after a session ≥ 20 s | — |

Rationale: we do NOT cripple the free experience (that tanks ratings); Pro sells on 4K + ad-free.
The marketing claim "all three protocols, ultra-fast" stays true on the free tier.

## JNI contract (class `com.yansit.tv.N`)

Java → Rust (push billing/ads state; call whenever it changes):
```
public static native void billingState(boolean pro, boolean yearlyOwned, boolean lifetimeOwned,
                                        String yearlyPrice, String lifetimePrice, boolean billingReady);
public static native void adLoaded(boolean interstitialReady);
```
- `yearlyPrice` / `lifetimePrice`: localized display strings from Billing (e.g. "₺349,99",
  "$9.99") or "" if not yet known. Rust only displays them; never parses amounts.

Rust → Java (polled every vsync by MainActivity, same pattern as `awake()` / `vsync()`):
```
public static native int  pollBillingAction(); // 0 none, 1 buy yearly, 2 buy lifetime, 3 restore, 4 manage subs, 5 open Play listing (rate)
public static native boolean pollShowInterstitial(); // true once when an interstitial should show
```
Rust sets these from the UI (user pressed "Yükselt → Yıllık / Ömür boyu / Satın alımları geri yükle")
and from the session-gate logic (free tier, after a mirror session ends).

## Ads
- Library: Google Mobile Ads SDK (AdMob) 24.x, behind the `ADS_ENABLED` switch in AdsManager.
  Production ids (supplied by the owner):
  - App id (manifest meta `com.google.android.gms.ads.APPLICATION_ID`):
    `ca-app-pub-9385203501275935~7266524492`
  - Interstitial unit: `ca-app-pub-9385203501275935/3742212452`
- Test devices are never hard-coded: debug builds read `admob.testDevices` from the gitignored
  `android/local.properties`; for Play builds register the owner's TV in AdMob → Settings →
  Test devices.
- Only interstitial, only on the free tier, only *between* mirror sessions (never over live
  video — Android TV ad policy: full-screen, dismissible, no overlay on content), at most one
  per 10 minutes and only after a session of at least 20 s.

## Rating prompt (Play "User Ratings, Reviews and Installs" policy)
Shown once, after the first mirror session of at least 30 s, instead of that session's ad.
Neutral wording and two buttons ("Open Google Play" / "Not now"): no "do you like it?" question,
no star picker, no routing by rating (review gating), no reward.

## Build / ABI
- Native lib `libyansit.so` built for **both** `armeabi-v7a` (armv7-linux-androideabi, 32-bit)
  and `arm64-v8a` (aarch64-linux-android, 64-bit). 64-bit is mandatory for Play.
- `minSdk = 30` (Android 11), `targetSdk = 34` (Android 14 — the Play minimum for Android TV
  apps); `android.software.leanback` required → distributed to Android TV / Google TV only.
- Version: `Cargo.toml` `version` is the single source; versionCode = MAJOR·10000 + MINOR·100 +
  PATCH (1.0.0 → 10000). Play has already seen versionCode 2, so never go below 3.
- Play Billing Library 8.x (Play requires ≥ 8 for new apps/updates since 2026-08-31).
- Billing + Ads are AAR libraries with resources + manifest + transitive deps → the hand-rolled
  `aapt2`/`d8` build cannot link them reliably. The Android/Java track migrates the build to
  Gradle (AGP) so manifest-merge, resource-merge and dependency resolution are handled. If this
  sandbox cannot fetch Gradle/Maven deps, set the project up correctly and report that the final
  assembling must run on a machine with network — do not hand-wire the AARs.

## GPL compliance (required because we distribute)
- Keep all upstream license headers (UxPlay LGPL/GPL, shairplay-rust LGPL) intact.
- Ship a `NOTICE`/about screen crediting UxPlay + shairplay-rust + Rust crates, and a
  **written offer of source**: a public repo URL (owner provides) shown in the Play listing and
  in the app's About row. The published source must match the shipped binary.

## No personal data in the APK
No home IPs, gateways, SSIDs, hostnames, account names or device ids may be compiled in.
Device name presets are generic ("Salon TV" etc.). The only runtime network address is the
device's own LAN IP, discovered at runtime (see `detect_local_ipv4`, RFC 5737 TEST-NET probe).
