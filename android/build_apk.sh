#!/bin/bash
# Builds android/build/Yansit.apk without Gradle:
#   cargo (armv7-linux-androideabi, release) -> llvm-strip -> aapt2 compile/link ->
#   javac --release 11 + d8 -> zip lib/armeabi-v7a/libyansit.so -> zipalign -> apksigner.
#
# FALLBACK ONLY (no Play Billing, no ads, armeabi-v7a only): handy for quick on-device tests.
# The Google Play build is Gradle: cd android && ./gradlew bundleRelease (see docs/BUILD.md).
# Billing/Ads need AARs that this script cannot link, so their sources are left out and
# Monetization.java falls back to a no-op.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
AND="$ROOT/android"
OUT="$AND/build"
APK="$OUT/Yansit.apk"
TARGET=armv7-linux-androideabi
LIB=libyansit.so

export JAVA_HOME="${JAVA_HOME:-/usr/lib/jvm/java-17-openjdk-amd64}"
. "$AND/ndk-env.sh"
BUILD_TOOLS="$ANDROID_HOME/build-tools/34.0.0"
PLATFORM="$ANDROID_HOME/platforms/android-34/android.jar"
export PATH="$TOOLCHAIN/bin:$HOME/.cargo/bin:$PATH"

SKIP_RUST=0
FEATURES=""
while [ $# -gt 0 ]; do
  case "$1" in
    --skip-rust) SKIP_RUST=1; shift ;;
    --features) FEATURES="$2"; shift 2 ;;  # e.g. --features dev-pro (owner builds only, never Play)
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

KEYSTORE="$AND/debug.keystore"
if [ ! -f "$KEYSTORE" ]; then
  keytool -genkeypair -v -keystore "$KEYSTORE" -storepass android -alias androiddebugkey \
    -keypass android -keyalg RSA -keysize 2048 -validity 10000 \
    -dname "CN=Yansit TV,O=Yansit,C=TR" >/dev/null 2>&1
fi

cd "$ROOT"
SO="$ROOT/target/$TARGET/release/$LIB"
if [ "$SKIP_RUST" = 0 ]; then
  echo "=== 1. Rust ($TARGET, release${FEATURES:+, features: $FEATURES})"
  CARGO_TARGET_DIR="$ROOT/target" CARGO_BUILD_JOBS=3 nice -n 10 \
    cargo build --target "$TARGET" --release ${FEATURES:+--features "$FEATURES"}
fi
[ -f "$SO" ] || { echo "missing $SO" >&2; exit 1; }

echo "=== 2. Staging"
rm -rf "$OUT/gen" "$OUT/obj" "$OUT/dex" "$OUT/pkg" "$OUT/compiled_res.zip" "$OUT"/*.apk "$OUT"/*.idsig
mkdir -p "$OUT/gen" "$OUT/obj" "$OUT/dex" "$OUT/pkg/lib/armeabi-v7a"
"$TOOLCHAIN/bin/llvm-strip" --strip-unneeded -o "$OUT/pkg/lib/armeabi-v7a/$LIB" "$SO"

echo "=== 3. Resources (aapt2)"
"$BUILD_TOOLS/aapt2" compile --dir "$AND/res" -o "$OUT/compiled_res.zip"
"$BUILD_TOOLS/aapt2" link -I "$PLATFORM" \
  --manifest "$AND/AndroidManifest.xml" \
  --min-sdk-version 30 --target-sdk-version 34 \
  --version-code 1 --version-name 1.0.0 \
  -o "$OUT/unaligned.apk" --java "$OUT/gen" --auto-add-overlay \
  "$OUT/compiled_res.zip"

echo "=== 4. Java shell (javac + d8)"
# Billing/Ads sources need the Play AARs (Gradle only); without them Monetization is a no-op.
mapfile -t JAVA_SRCS < <(find "$AND/java/com/yansit/tv" -name '*.java' \
  ! -name BillingManager.java ! -name AdsManager.java ! -name PlayMonetization.java | sort)
javac -encoding UTF-8 --release 11 -Xlint:-options -d "$OUT/obj" -cp "$PLATFORM" \
  "$OUT/gen/com/yansit/tv/R.java" "${JAVA_SRCS[@]}"
# shellcheck disable=SC2046
"$BUILD_TOOLS/d8" --release --lib "$PLATFORM" --min-api 30 --output "$OUT/dex" $(find "$OUT/obj" -name '*.class')

echo "=== 5. Package"
cp "$OUT/dex/classes.dex" "$OUT/pkg/"
(cd "$OUT/pkg" && zip -q -u -0 ../unaligned.apk classes.dex "lib/armeabi-v7a/$LIB")

echo "=== 6. Align + sign"
"$BUILD_TOOLS/zipalign" -f -p 4 "$OUT/unaligned.apk" "$OUT/aligned.apk"
"$BUILD_TOOLS/apksigner" sign --ks "$KEYSTORE" --ks-key-alias androiddebugkey \
  --ks-pass "pass:android" --key-pass "pass:android" --out "$APK" "$OUT/aligned.apk"
"$BUILD_TOOLS/apksigner" verify --verbose "$APK" | grep -E "Verifies|v2 scheme|v3 scheme"
rm -f "$OUT/unaligned.apk" "$OUT/aligned.apk"
ls -lh "$APK" "$OUT/pkg/lib/armeabi-v7a/$LIB"
echo "=== BUILD OK: $APK"
