# Ayna — AirPlay, Miracast & DLNA receiver for Android TV

Ayna turns an Android TV / Google TV into a wireless display for iPhone, iPad, Mac (AirPlay),
Android phones (Miracast), Windows 10/11 (Win + K, Miracast) and DLNA apps. The receiver,
decoder pipeline and TV interface are written in Rust; Java is only a thin Android shell
(activity, foreground service, Play Billing, AdMob).

- Package: `com.yansit.tv` · min Android 11 (API 30) · target API 34 · armeabi-v7a + arm64-v8a
- Hardware HEVC/H.264 via `MediaCodec`, AAudio output, egui UI on GLES
- 10 languages (auto from the TV's language, selectable in Settings)
- Free: all three protocols, 1080p · Ayna Pro: 4K and no ads (yearly or lifetime)

Google Play: <https://play.google.com/store/apps/details?id=com.yansit.tv> ·
Privacy policy: <https://krmndigital-lab.github.io/yansit-tv/>

## Build

```sh
rustup target add armv7-linux-androideabi aarch64-linux-android
cd android && ./gradlew bundleRelease      # signed AAB (needs android/keystore.properties)
cargo test --release                       # host tests
```

Details: [docs/BUILD.md](docs/BUILD.md). Monetization rules: [docs/MONETIZATION.md](docs/MONETIZATION.md).
Publishing (Play Developer API + GitHub Actions): [docs/PLAY_PUBLISHING.md](docs/PLAY_PUBLISHING.md).

## License

GPL-3.0-or-later. The AirPlay implementation follows UxPlay (GPL-3.0) and vendors the FairPlay
module of shairplay-rust (LGPL-3.0-or-later); see [docs/store/NOTICE.md](docs/store/NOTICE.md)
for all credits and [docs/store/SOURCE-OFFER.md](docs/store/SOURCE-OFFER.md) for the source offer.
Every Play release is built by GitHub Actions from the matching `vX.Y.Z` tag of this repository.

AirPlay, iPhone, iPad and Mac are trademarks of Apple Inc.; Miracast of Wi-Fi Alliance; DLNA of
the Digital Living Network Alliance; Android, Android TV and Google TV of Google LLC; Windows of
Microsoft Corporation. Ayna is independent and not affiliated with them.
