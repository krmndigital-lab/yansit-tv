# Source code offer — Ayna (GPL-3.0-or-later)

> **Where this text must appear:** the short notice goes in the **Google Play full description**
> (every language; each `listing-<lang>.md` already has it as the "Open source" paragraph) **and in
> the app's About screen**. The long form lives in the public source repository.

GPLv3 §6(d) lets object code be offered from a network server (Google Play) as long as "clear
directions next to the object code" say where the Corresponding Source is. The Play listing
paragraph and the in-app About row are those directions.

## 1. Short notice (Play listing + in-app About)

**English**

> Ayna is free software, licensed under the GNU General Public License version 3 or (at your
> option) any later version, and comes with ABSOLUTELY NO WARRANTY. The complete corresponding
> source code of every released version is available free of charge at https://github.com/krmndigital-lab/yansit-tv.

**Türkçe**

> Ayna, GNU Genel Kamu Lisansı sürüm 3 veya (tercihinize göre) daha sonraki bir sürümü ile
> lisanslanmış özgür bir yazılımdır ve HİÇBİR GARANTİ olmaksızın sunulur. Yayımlanan her sürümün
> eksiksiz kaynak kodu https://github.com/krmndigital-lab/yansit-tv adresinden ücretsiz olarak edinilebilir.

For the other eight languages, use the "Open source" paragraph of the matching `listing-<lang>.md`.

## 2. Written offer (long form — publish in the repository)

Ayna (`com.yansit.tv`) is distributed in object-code form (APK / Android App Bundle) through
Google Play. As required by section 6 of the GNU General Public License version 3, the
**Corresponding Source** of every distributed version is available as follows:

- **Where:** https://github.com/krmndigital-lab/yansit-tv
- **Cost:** free of charge; no account is needed to download it.
- **Finding a version:** each release is tagged `v<versionName>` (for example `v1.0.0`). The
  tagged tree is exactly the source used to build the binary published on Google Play under that
  version.
- **Contents:** the Rust core (`src/`, `Cargo.toml`, `Cargo.lock`), the Android shell and manifest
  (`android/`), build scripts, bundled assets (`assets/fonts/`), tools (`tools/make_icons.py`),
  the notices and license texts (`docs/store/NOTICE.md`, `LICENSE`), and build instructions.
- **How long:** for as long as that version is offered on Google Play.
- **Fallback:** if the link above stops working, write to krmndigital@gmail.com and we will provide the
  Corresponding Source of any version we have distributed, at no charge.

---

## 3. Maintainer checklist (not part of the published text)

1. **Publish before the first release.** The public repository must exist, with its URL filled in
   everywhere, before the first Play upload. Every Play release needs a matching tag.
   MONETIZATION.md: "The published source must match the shipped binary."
2. **Replace the placeholders:**
   - `https://github.com/krmndigital-lab/yansit-tv`: all 10 listings, NOTICE, privacy policy, this file
   - `krmndigital@gmail.com`: privacy policy, listings, this file
   - `KRMN Digital`: privacy policy
   - `KRMN Digital` and `https://github.com/thebracket/shairplay-rust`: NOTICE

   Check with `grep -rn '<[A-Z_]*>' docs/store/`.
3. **Add an About row to the app.** Settings in `src/ui/mod.rs` has 12 rows (Language … AutoWake,
   Pro) and **no About entry**, but the GPL directions and MONETIZATION.md need one. It should show
   the short notice, the source URL, the privacy-policy URL and the license texts (NOTICE +
   THIRD-PARTY-LICENSES).
4. **Keep upstream attributions.** Don't strip the UxPlay / shairplay-rust / playfair references in
   `src/airplay/**`. Add a top-level `LICENSE` (full GPL-3.0 text) if the repository lacks one.
5. **Make builds reproducible from a tag.** Document the Rust and NDK versions and the build path
   (Gradle/AGP or `android/build_apk.sh`). Signing keys are **not** Corresponding Source:
   distribution through Play is not a "User Product" transfer, so GPLv3's Installation Information
   clause does not apply. A rebuilt APK just carries a different signature. Never commit keystores
   (`.gitignore` already excludes `android/debug.keystore`).
6. **Proprietary SDKs: get legal advice before shipping ads/billing.** The Android shell will
   bundle the Google Play Billing Library and the Google Mobile Ads SDK. Both are proprietary and are
   not GPLv3 "System Libraries", so their source cannot be supplied as Corresponding Source.
   - For code you own, a GPLv3 §7 additional permission solves this ("you may link this program
     with the Google Play Billing Library and the Google Mobile Ads SDK and distribute the
     combination").
   - **You cannot grant that permission for third-party GPL code.** The AirPlay modules follow
     UxPlay's GPL-3.0 C files (see NOTICE §1), and UxPlay's authors hold that copyright; the
     playfair-derived code is the same situation.
   - Options: (a) a legal review of the combination; (b) ask the UxPlay copyright holders for a
     linking exception; (c) replace the derived parts with independently written code under your
     own copyright, then add the §7 permission.
   - A separate "FOSS" build without the SDKs (e.g. for F-Droid) is worth keeping either way.
7. **Licenses this package could not confirm offline:**
   - whether UxPlay's files say "version 3 **or later**". If any are GPL-3.0-only, label the
     combined work GPL-3.0-only (Cargo.toml, listings, About);
   - playfair's license. If it is GPL-3.0, the `fairplay` module is effectively GPL rather than LGPL
     (still fine inside this app, but label it correctly);
   - shairplay-rust's upstream URL and copyright holders.
