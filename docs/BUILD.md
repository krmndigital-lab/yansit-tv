# Ayna — Building (Gradle / Play) and the Gradle-free fallbacks

Contract for IDs, prices, JNI and ads rules: [`MONETIZATION.md`](MONETIZATION.md). This file covers
only how to build, sign and ship.

## Build paths at a glance

| Path | Output | ABIs | Billing + Ads | Use for |
|---|---|---|---|---|
| **Gradle** `cd android && ./gradlew bundleRelease` | `android/build/gradle/outputs/bundle/release/Yansit-release.aab` | armeabi-v7a + arm64-v8a | **yes** | **Google Play upload** |
| Gradle `./gradlew assembleDebug` | `android/build/gradle/outputs/apk/debug/Yansit-debug.apk` | both | yes (TEST ad ids) | on-device testing |
| `android/build_apk.sh [--skip-rust] [--features dev-pro]` | `android/build/Yansit.apk` | armeabi-v7a | no (no-op) | quick dev loop, no network |
| `android/build_bundle.sh` | `android/build/Yansit.aab` | both | no (no-op) | emergency AAB without Gradle |

The two shell scripts cannot link the Billing/Ads AARs, so they leave `BillingManager.java`,
`AdsManager.java` and `PlayMonetization.java` out. `Monetization.java` then falls back to a no-op
and the app runs as the plain free tier with no purchase path and no ads. **Only the Gradle AAB
can be sold on Play.**

## Prerequisites

- JDK 17 (`JAVA_HOME`), Android SDK with `platforms;android-34`, `build-tools;34.0.0` and
  `ndk;27.2.12479018` (licenses accepted: `sdkmanager --licenses`).
- Rust with `rustup target add armv7-linux-androideabi aarch64-linux-android`.
- Network on the first Gradle run (Gradle 8.7 via the wrapper, AGP 8.5.2, Billing 8.x,
  play-services-ads 23.6.0). The wrapper pins the Gradle zip's SHA-256.
- `android/local.properties` (gitignored): `sdk.dir=/path/to/android-sdk`, or set `ANDROID_HOME`.
  The Gradle cargo task (and `android/ndk-env.sh`, sourced by the shell scripts) passes the NDK
  linker/CC/AR to cargo as environment variables computed from the SDK path, so the repo holds
  no machine-specific path. `cargo` is taken from `~/.cargo/bin`, then `PATH`
  (override: `-Pyansit.cargo=/path/to/cargo`).

## Gradle commands (run in `android/`)

```sh
./gradlew assembleDebug                      # debug APK, both ABIs, real Billing + TEST ads
./gradlew -PdevPro=true assembleDebug        # OWNER build: Rust `dev-pro` feature (forced Pro)
./gradlew bundleRelease                      # signed AAB for Play (never dev-pro)
./gradlew cargoBuildRelease                  # just the two libyansit.so (strip included)
adb install -r build/gradle/outputs/apk/debug/Yansit-debug.apk
```

How the native part works: `cargoBuild<Variant>` runs `cargo build --release --lib --target <triple>`
once per ABI in the repo root, strips with the NDK's `llvm-strip` and writes
`build/gradle/generated/jniLibs/cargoBuild<Variant>/<abi>/libyansit.so`. That directory is registered as a
generated jniLibs source, so AGP orders the tasks itself. Rust is always built with the release
profile; the debug variant only changes the Java side (debuggable, debug-signed).

**`dev-pro` rule:** the release variant never receives Rust features. `-PdevPro=true` only
affects debug, uses its own `target/dev-pro` directory (so a forced-Pro `.so` can never be
packaged by a later Play build) and is ignored, with a warning, for release. `build_bundle.sh`
always reruns cargo without features for the same reason.

Gradle writes to `android/build/gradle/`, apart from the shell scripts' `android/build/{gen,obj,dex,pkg}`.
On a low-RAM box add `--no-daemon`. Cargo honours `CARGO_BUILD_JOBS` (e.g. `CARGO_BUILD_JOBS=3 nice -n 10 ./gradlew …`).

## Signing (upload key) and Play App Signing

Secrets never go in committed files (the repo is public under GPL-3.0).

1. `cp android/keystore.properties.example android/keystore.properties` and fill it in
   (`storeFile` is relative to `android/`). Instead of that file, CI can set `YANSIT_KEYSTORE`,
   `YANSIT_STORE_PASSWORD`, `YANSIT_KEY_ALIAS` and `YANSIT_KEY_PASSWORD`. Both Gradle and
   `build_bundle.sh` read the file or the env vars.
2. Creating a new key (prompts for the password and the name fields, so neither ends up in
   shell history or scripts):
   `keytool -genkeypair -v -keystore android/release.keystore -alias yansit -keyalg RSA -keysize 4096 -validity 10000`
3. **Enrol in Play App Signing** (the default for new apps). Google holds the app signing key, and
   `release.keystore` is only the **upload key**. If the upload key is ever lost or leaked, Play
   support can reset it. Without Play App Signing, a lost key means you can never update the app.
4. **Back up `release.keystore` and its password offline**: two copies (e.g. an encrypted USB stick
   plus a password manager entry), never inside the repo.
5. `.gitignore` covers `/android/*.keystore`, `/android/*.jks`, `/android/keystore.properties`
   and `/android/local.properties`. Run `git status --ignored` before the first public push.

The debug variant signs with `android/debug.keystore` (Android's public debug credentials, same
as `build_apk.sh`), so a Gradle debug APK installs over a script-built one without uninstalling.
Without `keystore.properties`, `bundleRelease` still builds an **unsigned** AAB (Play rejects it).

## Every Play upload

- Bump `versionCode` (and `versionName`) in `android/build.gradle`. Play rejects a reused code.
  For `build_bundle.sh`: `VERSION_CODE=2 VERSION_NAME=1.0.1 android/build_bundle.sh`.
- Publish the matching source (GPL-3.0 written offer, see MONETIZATION.md "GPL compliance").
- Test the AAB locally: `java -jar tools/bundletool.jar build-apks --bundle=<aab> --output=/tmp/y.apks --connected-device --ks=android/release.keystore --ks-key-alias=yansit`
  then `java -jar tools/bundletool.jar install-apks --apks=/tmp/y.apks`.

## Play Console setup (Billing)

- Products: `yansit_pro_yearly` = subscription with base plan **`yearly`** (auto-renewing, 1 year,
  $10 base price); `yansit_pro_lifetime` = one-time product ($20). Both must be **active**.
- Billing only works once a build with the `BILLING` permission has been uploaded to a track
  (internal testing is enough) and the tester account is in **License testing**. Test installs may
  be debug-signed sideloads with the same `applicationId`.
- What the app does: on start and on every resume it queries SUBS + INAPP purchases, acknowledges
  any unacknowledged ones (both products are entitlements; neither is ever consumed), and pushes
  `N.billingState(...)`. It never pushes `pro=false` before both queries have answered, so the
  Pro flag Rust caches survives a slow or offline Play Store. "Restore" re-queries; "Manage"
  opens `play.google.com/store/account/subscriptions` (the yearly sub's page when owned).
- Optional hardening: verify purchases server-side via the Play Developer API (out of scope for
  an open-source, server-less app; any client-side check can be patched out of a GPL build anyway).

## Ads (AdMob)

- Currently Google's **TEST** ids: app id in `AndroidManifest.xml`
  (`com.google.android.gms.ads.APPLICATION_ID`), interstitial unit in
  `AdsManager.INTERSTITIAL_UNIT_ID`. Replace both only when the owner supplies real ids and accepts
  the TV-ads risk (MONETIZATION.md).
- `AdsManager.ADS_ENABLED = false` turns ads off completely: the SDK is never initialised, loaded
  or shown, and Rust always sees `adLoaded(false)`.
- Ads load only for a confirmed free user (both ownership queries answered, not Pro) or where Play
  Billing is unavailable. They are dropped as soon as Pro is detected. Shown only when Rust polls
  `pollShowInterstitial()` (between mirror sessions, never over live video). Full-screen,
  dismissible, own activity, reloaded after each show.
- Before release: verify on a real TV that **Back dismisses the interstitial** with the D-pad
  (Android TV ad quality rule). Interstitials are capped at one per 10 minutes.
- Play Console: declare **"Contains ads"**, the **Advertising ID** usage (the manifest declares
  `AD_ID`), and AdMob's data collection in **Data safety**.

## Permission review (current manifest)

| Permission / attribute | Status |
|---|---|
| `ACCESS_BACKGROUND_LOCATION` | Not requested (removed: Play declaration + rejection risk). |
| Foreground service | `AirPlayService` has `foregroundServiceType="connectedDevice"`, declares `FOREGROUND_SERVICE_CONNECTED_DEVICE` and passes the type to `startForeground()` on API 29+ (no Android 14 `SecurityException`). Play Console needs the matching foreground-service declaration (type *Connected device*: "keeps the AirPlay/Miracast/DLNA receiver reachable while the TV shows other apps"). |
| `ACCESS_FINE/COARSE_LOCATION` | `maxSdkVersion="32"`: only Android 11–12 ask (Wi-Fi P2P needs it there). Android 13+ uses `NEARBY_WIFI_DEVICES` with `neverForLocation`. A rationale dialog is shown before the system prompt. |
| `SYSTEM_ALERT_WINDOW` | Lets the service bring the UI forward when a sender connects (auto-wake). Special permission; reviewers may ask. |
| `AD_ID`, `ACCESS_ADSERVICES_*` (AdMob) | Advertising ID + Data safety declarations; incompatible with a children's audience (Families policy). |
| `BILLING` | Play Billing 8.x. |
| `AirPlayService` `exported="false"`; `BootReceiver` exported (boot / package-replaced / `com.yansit.tv.START`) | Low risk: the custom action only starts the receiver. |
| `usesCleartextTraffic="true"` | Needed for local AirPlay/DLNA HTTP on the LAN. |
| INTERNET, network/Wi-Fi state, multicast, WAKE_LOCK, BOOT_COMPLETED | Needed. |

**Target API:** `targetSdk 34` meets Play's requirement for Android TV apps (API 34+, checked
2026-11; phones need 36). The app is TV-only (`leanback` required), so it is never offered to
phones. Re-check every August.

## Troubleshooting

- `NDK toolchain not found`: install `ndk;27.2.12479018`, or fix `sdk.dir`.
- `cargo: command not found` inside Gradle: `-Pyansit.cargo=$HOME/.cargo/bin/cargo`.
- AGP warns that `package=` / `extractNativeLibs` are set in the manifest: expected. Both stay
  because the Gradle-free scripts need them; Gradle uses `namespace` and `useLegacyPackaging`.
