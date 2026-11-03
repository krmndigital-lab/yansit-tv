#!/bin/bash
# Gradle-free Android App Bundle: android/build/Yansit.aab (armeabi-v7a + arm64-v8a), via
# tools/bundletool.jar. Like build_apk.sh it has NO Play Billing and NO ads (it cannot link the
# AARs; Monetization.java falls back to a no-op). The Google Play artifact with Billing + Ads is
# Gradle's: cd android && ./gradlew bundleRelease (see docs/BUILD.md).
#
# Signing (upload key for Play App Signing) comes from android/keystore.properties (gitignored;
# template keystore.properties.example) or env vars YANSIT_KEYSTORE, YANSIT_STORE_PASSWORD,
# YANSIT_KEY_ALIAS, YANSIT_KEY_PASSWORD. Nothing secret lives in this (published) script.
# Optional: VERSION_CODE (default 10000) and VERSION_NAME (default 1.0.0); Play has seen 2.
# Never pass the Rust `dev-pro` feature here: this output is meant for Google Play.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
AND="$ROOT/android"
OUT="$AND/build"
AAB="$OUT/Yansit.aab"
LIB=libyansit.so
# ABI dir -> Rust target triple
ABIS=("armeabi-v7a:armv7-linux-androideabi" "arm64-v8a:aarch64-linux-android")
VERSION_CODE="${VERSION_CODE:-10000}"
VERSION_NAME="${VERSION_NAME:-1.0.0}"

export JAVA_HOME="${JAVA_HOME:-/usr/lib/jvm/java-17-openjdk-amd64}"
. "$AND/ndk-env.sh"
BUILD_TOOLS="$ANDROID_HOME/build-tools/34.0.0"
PLATFORM="$ANDROID_HOME/platforms/android-34/android.jar"
BUNDLETOOL="$ROOT/tools/bundletool.jar"
export PATH="$TOOLCHAIN/bin:$HOME/.cargo/bin:$PATH"

# --- Signing config (read as data, never `source`d) -------------------------------------------
PROPS="$AND/keystore.properties"
prop() {
  [ -f "$PROPS" ] || return 0
  sed -n "s/^[[:space:]]*$1[[:space:]]*=[[:space:]]*//p" "$PROPS" | tail -n 1 | tr -d '\r'
}
KS_FILE="$(prop storeFile)";       KS_FILE="${KS_FILE:-${YANSIT_KEYSTORE:-}}"
KS_STORE_PASS="$(prop storePassword)"; KS_STORE_PASS="${KS_STORE_PASS:-${YANSIT_STORE_PASSWORD:-}}"
KS_ALIAS="$(prop keyAlias)";       KS_ALIAS="${KS_ALIAS:-${YANSIT_KEY_ALIAS:-}}"
KS_KEY_PASS="$(prop keyPassword)"; KS_KEY_PASS="${KS_KEY_PASS:-${YANSIT_KEY_PASSWORD:-$KS_STORE_PASS}}"
case "$KS_FILE" in /*) ;; "") ;; *) KS_FILE="$AND/$KS_FILE" ;; esac
if [ -z "$KS_FILE" ] || [ ! -f "$KS_FILE" ] || [ -z "$KS_STORE_PASS" ] || [ -z "$KS_ALIAS" ]; then
  echo "Signing config missing: create android/keystore.properties from keystore.properties.example" >&2
  echo "(or set YANSIT_KEYSTORE / YANSIT_STORE_PASSWORD / YANSIT_KEY_ALIAS). See docs/BUILD.md." >&2
  exit 1
fi
export KS_STORE_PASS KS_KEY_PASS   # handed to jarsigner via :env, so they never hit argv / ps

cd "$ROOT"
echo "=== 1. Rust (release, no features) for ${ABIS[*]}"
# Always run cargo (incremental): a dev-pro .so left by `build_apk.sh --features dev-pro` is
# rebuilt here because cargo fingerprints the feature set.
for pair in "${ABIS[@]}"; do
  triple="${pair#*:}"
  CARGO_TARGET_DIR="$ROOT/target" CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-3}" nice -n 10 \
    cargo build --target "$triple" --release --lib
done

echo "=== 2. Resources (aapt2, proto format) + Java (javac + d8)"
rm -rf "$OUT/gen" "$OUT/obj" "$OUT/dex" "$OUT/bundle_staging" "$OUT/proto_unzipped" \
  "$OUT/base.zip" "$OUT/base_proto.apk" "$OUT/compiled_res.zip" "$OUT/unsigned.aab" "$AAB"
mkdir -p "$OUT/gen" "$OUT/obj" "$OUT/dex" "$OUT/bundle_staging"
"$BUILD_TOOLS/aapt2" compile --dir "$AND/res" -o "$OUT/compiled_res.zip"
"$BUILD_TOOLS/aapt2" link -I "$PLATFORM" \
  --manifest "$AND/AndroidManifest.xml" \
  --min-sdk-version 30 --target-sdk-version 34 \
  --version-code "$VERSION_CODE" --version-name "$VERSION_NAME" \
  --proto-format -o "$OUT/base_proto.apk" --java "$OUT/gen" --auto-add-overlay \
  "$OUT/compiled_res.zip"

# Billing/Ads sources need the Play AARs (Gradle only); without them Monetization is a no-op.
mapfile -t JAVA_SRCS < <(find "$AND/java/com/yansit/tv" -name '*.java' \
  ! -name BillingManager.java ! -name AdsManager.java ! -name PlayMonetization.java | sort)
javac -encoding UTF-8 --release 11 -Xlint:-options -d "$OUT/obj" -cp "$PLATFORM" \
  "$OUT/gen/com/yansit/tv/R.java" "${JAVA_SRCS[@]}"
mapfile -t CLASSES < <(find "$OUT/obj" -name '*.class')
"$BUILD_TOOLS/d8" --release --lib "$PLATFORM" --min-api 30 --output "$OUT/dex" "${CLASSES[@]}"

echo "=== 3. Base module for bundletool"
STAGING="$OUT/bundle_staging/base"
mkdir -p "$STAGING/manifest" "$STAGING/dex"
unzip -q "$OUT/base_proto.apk" -d "$OUT/proto_unzipped"
mv "$OUT/proto_unzipped/AndroidManifest.xml" "$STAGING/manifest/"
[ -d "$OUT/proto_unzipped/res" ] && mv "$OUT/proto_unzipped/res" "$STAGING/"
[ -f "$OUT/proto_unzipped/resources.pb" ] && mv "$OUT/proto_unzipped/resources.pb" "$STAGING/"
rm -rf "$OUT/proto_unzipped"
cp "$OUT/dex/classes.dex" "$STAGING/dex/"
for pair in "${ABIS[@]}"; do
  abi="${pair%%:*}"; triple="${pair#*:}"
  SO="$ROOT/target/$triple/release/$LIB"
  [ -f "$SO" ] || { echo "missing $SO" >&2; exit 1; }
  mkdir -p "$STAGING/lib/$abi"
  "$TOOLCHAIN/bin/llvm-strip" --strip-unneeded -o "$STAGING/lib/$abi/$LIB" "$SO"
done
(cd "$STAGING" && zip -q -r "$OUT/base.zip" .)

echo "=== 4. bundletool build-bundle"
java -jar "$BUNDLETOOL" build-bundle --modules="$OUT/base.zip" --output="$OUT/unsigned.aab"

echo "=== 5. Sign (upload key)"
jarsigner -keystore "$KS_FILE" -storepass:env KS_STORE_PASS -keypass:env KS_KEY_PASS \
  "$OUT/unsigned.aab" "$KS_ALIAS" >/dev/null
mv "$OUT/unsigned.aab" "$AAB"
if ! jarsigner -verify "$AAB" | grep "jar verified" >/dev/null; then
  echo "AAB signature verification FAILED" >&2; exit 1
fi
echo "signature: jar verified"
unzip -l "$AAB" | grep -E "base/lib/.*/$LIB" || true
ls -lh "$AAB"
echo "=== AAB BUILD OK (no Billing/Ads, versionCode $VERSION_CODE): $AAB"
