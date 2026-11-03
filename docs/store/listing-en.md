# Ayna — Google Play listing: English (`en-US`)

> Copy into Play Console → *Grow → Store presence → Main store listing* (default language
> `en-US`). Limits checked: title ≤ 30, short description ≤ 80, full description ≤ 4000
> characters. Every claim below is backed by the code (see "Claim sources" at the end).

## Title

```text
Ayna: AirPlay & Miracast TV
```

Brand first, then a plain descriptive phrase. Never use AirPlay, Miracast, Chromecast, Apple or
Samsung as the app's own name, and keep "free", "best", "#1" and prices out of the title.

## Short description

```text
Fast, low-latency mirroring from iPhone, iPad, Mac, Android & Windows to your TV
```

## Full description

```text
Turn your Android TV or Google TV into a fast wireless screen-mirroring receiver for your iPhone, iPad, Mac, Android phone or Windows PC — no extra hardware or cables.

NATIVE RUST ENGINE
Ayna is written from scratch in native Rust:
• No Java virtual machine or garbage-collector pauses in the video path.
• Light on your TV: small download, low CPU and memory use.
• Starts quickly and keeps the receiver ready in the background while it is switched on.

LOW LATENCY & 60 FPS
• Hardware HEVC (H.265) and H.264 decoding on your TV's own video chip, with low-latency decoder settings.
• Up to 60 fps for smooth presentations, videos and photos.
• Picture and sound stay in sync; latency mode and audio buffer are adjustable.

THREE PROTOCOLS, ONE APP
• AirPlay: mirror your iPhone, iPad or Mac screen, or stream music (Control Center → Screen Mirroring).
• Miracast: Android phones and tablets with a built-in wireless display / screen cast option connect directly over Wi-Fi Direct.
• Windows 10 / 11: press Win + K to duplicate or extend your desktop to the TV.
• DLNA / UPnP: send videos and music from DLNA apps on your network (MP4, MKV, WebM, TS, MP3, FLAC, AAC).

CLEAR AUDIO
• Lossless ALAC (44.1 kHz / 16-bit) music over AirPlay, plus AAC and PCM.
• AAC-ELD audio for AirPlay screen mirroring.
• Now Playing card with album art, title, artist and progress.

PRIVATE BY DESIGN
• No account, no cloud, no external servers.
• Your screen and media go straight from your device to your TV over your local network or Wi-Fi Direct.
• Optional on-screen PIN code for AirPlay connections.

MADE FOR THE REMOTE
• Clean, remote-friendly interface with a live status indicator.
• Step-by-step connection guide for Apple, Android and Windows.
• 10 languages: English, Turkish, German, Spanish, French, Italian, Portuguese, Russian, Polish, Indonesian.
• Settings for resolution (1080p / 4K), frame rate, codec, latency mode and audio buffer.

FREE AND PRO
All three protocols work in the free version:
• Free: AirPlay, Miracast and DLNA with hardware HEVC/H.264 and 1080p mirroring. Ads appear only between sessions, never over your content.
• Ayna Pro: 4K (2160p) mirroring and no ads — as a yearly subscription or a one-time lifetime purchase.

OPEN SOURCE
Ayna is free software under the GNU GPL v3:
https://github.com/krmndigital-lab/yansit-tv

GOOD TO KNOW
• Your TV and the sending device must be on the same Wi-Fi network (Miracast connects directly over Wi-Fi Direct).
• Google Cast (Chromecast) is not supported; on Android use Miracast or DLNA.
• 4K needs a 4K TV, a sender that can send 4K and Ayna Pro.

TRADEMARKS
AirPlay, iPhone, iPad and Mac are trademarks of Apple Inc. Miracast is a trademark of Wi-Fi Alliance. DLNA is a trademark of Digital Living Network Alliance. Android, Android TV, Google TV and Chromecast are trademarks of Google LLC. Windows is a trademark of Microsoft Corporation. Ayna is an independent app and is not affiliated with these companies.
```

---

## Store settings (global, set once)

| Setting | Suggestion |
|---|---|
| App category | **Video Players & Editors** (alternative: Tools) |
| Tags | Pick the closest ones from Play's predefined list (video player, streaming/casting-related). Play only accepts tags from its own list. |
| Content rating (IARC) | Expected **Everyone / PEGI 3 / USK 0**. The app makes no content of its own: no violence, sexual content, profanity, gambling or user-to-user chat. It only shows what the user sends from their own device. It cannot browse the web; DLNA only plays links that the user's own sender app pushes, so answer **No** to "unrestricted internet access". IARC assigns the final rating. |
| Contains ads | **Yes** (AdMob interstitials in the free tier only, between sessions). This must match the build you upload. |
| In-app purchases | Yes: `yansit_pro_yearly` (subscription, base plan `yearly`, US$10.00) and `yansit_pro_lifetime` (one-time, US$20.00). Set prices per country in Play Console. |
| Target audience | Select **13+ age groups only**. Selecting any under-13 group brings in the Families policy (certified ad networks, stricter ad rules). |
| Data safety | See the appendix of `privacy-policy.md` (developer collects nothing; the AdMob SDK collects ad data in the free tier). |
| App access (for reviewers) | Everything works without a login. Reviewer note: *"Testing needs a second device on the same Wi-Fi: an iPhone/iPad/Mac (Control Center → Screen Mirroring), a Windows 10/11 PC or Android phone with Miracast, or any DLNA app."* |
| Form factors | Opt in to **Android TV** (Release → Advanced settings → Form factors). This needs the TV banner, at least one TV screenshot and Google's TV quality review. |

## Additional information

- Privacy policy URL: host `docs/store/privacy-policy.md` as a public static page (e.g. GitHub
  Pages) and paste the URL into Play Console → *App content → Privacy policy*.
- Source code (GPL-3.0-or-later): `https://github.com/krmndigital-lab/yansit-tv`. The listing paragraph "OPEN SOURCE"
  above is the "clear directions next to the object code" that GPLv3 §6(d) requires; see
  `SOURCE-OFFER.md`.
- Support e-mail (Play Console contact details): `krmndigital@gmail.com`.

## Claim sources (for reviewers of this listing)

| Claim | Where it comes from |
|---|---|
| AirPlay 2 mirroring + music, iOS/iPadOS/macOS senders | `src/airplay/mod.rs` header (UxPlay-compatible mirroring, RAOP audio) |
| ALAC 44.1 kHz/16-bit, AAC-LC/ELD, PCM | `src/media/audio.rs` header and UI label "ALAC 44.1 kHz / 16-bit" |
| Miracast sink over Wi-Fi Direct (RTSP 7236, MPEG-TS) | `src/miracast/mod.rs` header. Verified with an Android phone (1080p60) during development; Windows Win + K is the standard Miracast sender path but not yet tested on a real PC. No phone brand is named. |
| DLNA/UPnP MediaRenderer, MP4/MKV/WebM/TS/MP3/FLAC/AAC/WAV | `src/dlna/mod.rs` and `src/dlna/player.rs` headers. HLS (`.m3u8`) is deliberately **not** claimed: there is no playlist handling and `AMediaExtractor` cannot open HLS. |
| Hardware HEVC/H.264, low-latency decoder flags | `src/media/video.rs`. The decoder is chosen by MIME type (`MediaCodec::from_decoder_type`), not by vendor, so no chipset is named. |
| "Designed for millisecond-class decode times" | Design goal only. The ~5.5 ms figure in `video.rs` was measured on one reference box and is **not** used in the listing. |
| Native Rust engine, no GC pauses in the video path | Whole receiver is Rust (`src/`); Java is a thin shell (activity, service, Billing/Ads). `src/ui/mod.rs` header: no GL swap over the video plane while the HUD is hidden |
| Free = 1080p + ads, Pro = 4K + no ads | `docs/MONETIZATION.md`; 4K gate at `src/media/mod.rs` (`is_pro()` check) and `src/ui/mod.rs` |
| 10 languages, auto-detect + picker | `src/i18n.rs` (`Lang`, `LangPref::Auto`), Settings row `Language` |
| No account / no servers / local only | No backend in the repository; MONETIZATION.md "No personal data in the APK" |
