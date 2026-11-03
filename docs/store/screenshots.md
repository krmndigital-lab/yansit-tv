# Ayna — store graphics: screenshots, TV banner, feature graphic, icon

> **Fix before submitting (store-policy risk).** The current artwork from `tools/make_icons.py`
> (the in-APK `banner.png` and `icon.png`, and anything rendered from that script) has two problems:
> 1. The banner subtitle reads **"AirPlay 4K Alıcı"**. That puts Apple's trademark in the
>    app-name position, which the Play trademark/impersonation rules forbid. It also promises 4K,
>    which is Pro-only (free = 1080p).
> 2. The emblem (a screen with a solid triangle at the bottom centre) closely resembles
>    **Apple's AirPlay glyph**. Using it as the app icon or banner invites an IP complaint or
>    rejection.
>
> Use a distinct emblem (e.g. a TV with converging waves, or a "Y" monogram) and no protocol names
> in the banner or icon. This is a code/asset change outside this docs package.

---

## 1. Screenshots

### Why there is no `adb` capture script

On the reference TV box, `adb shell screencap` returns a **black frame for both layers**:

- the hardware video plane (the lower `SurfaceView`, bound directly to `MediaCodec`), and
- the egui overlay (the upper, translucent GL `SurfaceView` where Rust draws the UI/HUD; see
  `android/java/com/yansit/tv/MainActivity.java`).

Device capture can't be used, so the store screenshots are rendered **offscreen on the host from
the real egui UI code**, for all 10 languages. Also never use a photo or recording of a live
mirroring session showing third-party content (copyright, and Play's misrepresentation rules).

### Play requirements (Android TV)

| Item | Requirement |
|---|---|
| Size / ratio | **1920 × 1080**, 16:9 landscape |
| Format | PNG **24-bit, no alpha**, or JPEG; ≤ 8 MB each |
| Count | Android TV needs at least 1 per language; up to 8. This package ships **5** per language. |
| Content | Must show the real app. No prices unless correct for that country; no Apple/Google device frames or logos. |

The egui UI is laid out on a 960 × 540 logical canvas at `pixels_per_point = 2.0`
(`src/ui/mod.rs`), so 1920 × 1080 is its native size. The renders match the TV's layout exactly,
with no scaling.

### Shot list (same 5 shots in every language)

Files: `docs/store/screenshots/<lang>/NN-name.png`. The generator picks the exact `name` slug.

| # | Screen | Must be visible | Notes |
|---|---|---|---|
| 01 | **Ready dashboard** | Ayna header with the 3-protocol line; live receiver status card ("waiting for connection" + receiver name); connection guide (Apple · Android · Windows) | First impression; upload first |
| 02 | **Mirroring HUD** | The telemetry bar shown when the user presses OK/Info while mirroring: codec (HEVC/H.264), resolution, fps, latency mode, connected device | The real video plane can't be captured, so the HUD sits on a neutral or self-made backdrop. Never Apple UI, a film/game frame, or anyone's real screen |
| 03 | **Now playing** (AirPlay music) | Cover art, title / artist / album, progress bar, codec line (ALAC 44.1 kHz / 16-bit) | Fictional track metadata and self-made cover art only |
| 04 | **Settings** | Settings panel with the **Language** row ("Dil · Language") visible, plus resolution, codec and latency rows | Shows the 10-language support |
| 05 | **Ayna Pro sheet** | "Get 4K and an ad-free experience with Ayna Pro", the benefits line (4K (2160p) mirroring • No ads • Support development), yearly / lifetime options, "Restore purchases" | Offscreen there is no Billing connection. Show a neutral placeholder, or a price that matches Play Console for that locale. Never show "$10" in a non-US screenshot if it could mislead |

### Locale folders → Play Console languages

| Folder | Play locale | Listing file |
|---|---|---|
| `tr` | tr-TR | `listing-tr.md` |
| `en` | en-US (default) | `listing-en.md` |
| `de` | de-DE | `listing-de.md` |
| `es` | es-ES (reuse for es-419) | `listing-es.md` |
| `fr` | fr-FR | `listing-fr.md` |
| `it` | it-IT | `listing-it.md` |
| `pt` | pt-BR | `listing-pt.md` |
| `ru` | ru-RU | `listing-ru.md` |
| `pl` | pl-PL | `listing-pl.md` |
| `id` | id (Indonesian) | `listing-id.md` |

### Regenerate

```sh
# from the repository root
cargo run --example shots
# → docs/store/screenshots/<lang>/NN-name.png, 1920×1080, for tr en de es fr it pt ru pl id
```

The example renders the same `src/ui` code the TV runs, so re-run it after any UI or i18n change
and re-upload. If the example's options change, the comment at the top of `examples/shots.rs` is
authoritative.

Check sizes and strip alpha (Play rejects PNGs with an alpha channel). This needs Pillow, which
`tools/make_icons.py` already uses:

```sh
python3 -I - docs/store/screenshots <<'EOF'
import sys, pathlib
from PIL import Image
root, bad = pathlib.Path(sys.argv[1]), 0
for lang in ["tr", "en", "de", "es", "fr", "it", "pt", "ru", "pl", "id"]:
    files = sorted((root / lang).glob("*.png"))
    print(f"{lang}: {len(files)} shots")
    bad += not 1 <= len(files) <= 8
    for f in files:
        im = Image.open(f)
        if im.size != (1920, 1080):
            print("  wrong size:", f, im.size); bad += 1
        if im.mode != "RGB":
            im.convert("RGB").save(f); print("  flattened to RGB:", f)
print("OK" if not bad else f"{bad} problem(s)")
EOF
```

**Phones/tablets:** the manifest marks leanback and touchscreen as optional, so Play will also
offer the app to phones. That form factor needs ≥ 2 phone screenshots and the feature graphic. The
1920×1080 PNGs fit Play's limits, but they show a D-pad TV UI. Consider limiting the device catalog
to TV until the UI is touch-friendly.

---

## 2. Play TV banner — 1280 × 720

| Item | Spec |
|---|---|
| Size / format | **1280 × 720**, JPEG or 24-bit PNG (**no alpha**) |
| Where it shows | Google Play on Android TV / Google TV |
| Not to be confused with | the launcher banner inside the APK: `android/res/drawable/banner.png`, 320 × 180, made by `tools/make_icons.py` |
| Text | Only the wordmark **Ayna**. Optionally one generic phrase under it ("Screen Mirroring" / "Ekran Yansıtma"). No protocol names, no "4K", no prices. One banner can serve all 10 languages. |
| Look | Same family as the app: dark navy gradient (≈ `#060A18` → blue), cyan accent (`rgb(0,225,255)`), white Inter Bold wordmark at ≈ 140 px cap height, left-aligned. **Use the new non-AirPlay-like emblem** (see the box at the top). |
| Margins | Keep text and emblem ≥ 64 px from every edge |

Once the script's subtitle and emblem are fixed, its own drawing code can produce the Play sizes.
This writes only into the output folder and never into `android/res/`:

```sh
python3 -I - assets/fonts/InterVariable-om.ttf /tmp/yansit-store-art <<'EOF'
import runpy, sys, pathlib
font, out = sys.argv[1], sys.argv[2]
pathlib.Path(out).mkdir(parents=True, exist_ok=True)
sys.argv = ["make_icons.py", font, out]       # the script's own 192/320 px outputs land in `out`
g = runpy.run_path("tools/make_icons.py")
g["banner"](1280, 720).save(f"{out}/play-tv-banner-1280x720.png")   # RGB, no alpha
g["icon"](512).save(f"{out}/play-icon-512.png")                     # RGBA
EOF
```

(Tested 2026-11-03: it produces a 1280×720 RGB banner and a 512×512 RGBA icon. Both still show
today's "AirPlay 4K Alıcı" subtitle and AirPlay-like emblem.)

---

## 3. Feature graphic — 1024 × 500

| Item | Spec |
|---|---|
| Size / format | **1024 × 500**, JPEG or 24-bit PNG (**no alpha**). Required for every store listing. |
| Layout | Left ≈ 40 %: emblem + "Ayna" wordmark. Right ≈ 60 %: tagline (≥ 48 px) and a smaller sub-line. Same navy-to-cyan background as the banner. Keep everything inside the central 924 × 400 area. |
| Never include | Prices, "free", "#1"/"best", Apple/Google device imagery, the AirPlay/Miracast/DLNA **logos**. |
| Optional third line | `AirPlay 2 · Miracast · DLNA` as plain text (a compatibility statement, not logos) |

Localized text (upload one graphic per language, or the English one everywhere):

| Lang | Tagline | Sub-line |
|---|---|---|
| en | Your screen, on the big screen | Fast · No account · No cloud |
| tr | Ekranınız, büyük ekranda | Hızlı · Hesapsız · Bulutsuz |
| de | Ihr Bildschirm – ganz groß | Schnell · Ohne Konto · Ohne Cloud |
| es | Tu pantalla, en grande | Rápido · Sin cuenta · Sin nube |
| fr | Votre écran, en grand | Rapide · Sans compte · Sans cloud |
| it | Il tuo schermo, in grande | Veloce · Senza account · Senza cloud |
| pt | Sua tela, em tamanho grande | Rápido · Sem conta · Sem nuvem |
| ru | Ваш экран — на большом экране | Быстро · Без аккаунта · Без облака |
| pl | Twój ekran na dużym ekranie | Szybko · Bez konta · Bez chmury |
| id | Layar Anda, di layar besar | Cepat · Tanpa akun · Tanpa cloud |

---

## 4. App icon — 512 × 512

| Item | Spec |
|---|---|
| Size / format | **512 × 512**, 32-bit PNG (alpha allowed), ≤ 1 MB |
| Shape | Full-bleed square. Play applies its own rounded mask, so don't bake in rounded corners, a transparent border or a drop shadow. Today's `icon()` output has transparent rounded corners and would be rounded twice. |
| Content | The new Ayna emblem (not the AirPlay-like screen + triangle). No text. |

## Generated assets (ready to upload)

Regenerate everything with:

```sh
cargo run --example shots                                   # UI screenshots, 10 languages
python3 -I tools/make_icons.py assets/fonts/InterVariable-full.ttf android/res/drawable docs/store
```

| Play Console field | File |
|---|---|
| App icon (512×512, square, Play applies the mask) | `docs/store/icon-512.png` |
| TV banner (1280×720) | `docs/store/tv-banner-1280x720.png` |
| Feature graphic (1024×500), one per listing language | `docs/store/feature-graphic/<lang>.png` |
| Android TV screenshots (1920×1080), per language | `docs/store/screenshots/<lang>/01-ready.png` … `05-pin.png` |

`06-about.png` is a review image of the in-app About sheet (GPL notice, source link, credits); it is
not meant as a store screenshot.

The mark is original (a TV on a stand with a bold "Y" inside): it replaced the earlier emblem,
which resembled Apple's AirPlay symbol, and the banner no longer says "AirPlay" (trademark risk).
