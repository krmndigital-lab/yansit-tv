# Sourced by build_apk.sh / build_bundle.sh (and handy in a shell: `. android/ndk-env.sh`).
# Points cargo at the NDK clang for both Android targets, so the repo's .cargo/config.toml
# needs no machine-specific path. Override ANDROID_HOME / NDK_VERSION / ANDROID_API if needed.
export ANDROID_HOME="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-$HOME/android-sdk}}"
NDK_VERSION="${NDK_VERSION:-27.2.12479018}"
ANDROID_API="${ANDROID_API:-30}"
case "$(uname -s)" in Darwin) _host=darwin-x86_64 ;; *) _host=linux-x86_64 ;; esac
TOOLCHAIN="$ANDROID_HOME/ndk/$NDK_VERSION/toolchains/llvm/prebuilt/$_host"
export CARGO_TARGET_ARMV7_LINUX_ANDROIDEABI_LINKER="$TOOLCHAIN/bin/armv7a-linux-androideabi$ANDROID_API-clang"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$TOOLCHAIN/bin/aarch64-linux-android$ANDROID_API-clang"
export CC_armv7_linux_androideabi="$CARGO_TARGET_ARMV7_LINUX_ANDROIDEABI_LINKER"
export CXX_armv7_linux_androideabi="$CARGO_TARGET_ARMV7_LINUX_ANDROIDEABI_LINKER++"
export AR_armv7_linux_androideabi="$TOOLCHAIN/bin/llvm-ar"
export CC_aarch64_linux_android="$CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER"
export CXX_aarch64_linux_android="$CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER++"
export AR_aarch64_linux_android="$TOOLCHAIN/bin/llvm-ar"
# No build-machine paths (user name, home dir) in panic messages inside the .so; cargo appends
# these to the target rustflags from .cargo/config.toml.
_root="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")/.." && pwd -P)"
_remap="--remap-path-prefix=$_root=/ayna --remap-path-prefix=${CARGO_HOME:-$HOME/.cargo}=/cargo --remap-path-prefix=${RUSTUP_HOME:-$HOME/.rustup}=/rustup --remap-path-prefix=$HOME=/home"
export CARGO_TARGET_ARMV7_LINUX_ANDROIDEABI_RUSTFLAGS="$_remap"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS="$_remap"
export YANSIT_SOURCE_URL="${YANSIT_SOURCE_URL:-https://github.com/krmndigital-lab/yansit-tv}"
unset _host _root _remap
