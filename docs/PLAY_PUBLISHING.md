# Ayna — automated Google Play publishing

Releases go through the Google Play Developer API. Pushing a tag `vX.Y.Z` (equal to the
`version` in `Cargo.toml`) makes GitHub Actions run: tests → signed AAB → privacy check → Google
Play (store listing, graphics, products, release) → GitHub release. `./publish.sh` does the same
from a local machine.

## What `tools/play/publish.py` automates

| Command | What it does |
|---|---|
| `check` | Shows what Play has now: tracks, releases, listings, uploaded bundles, products |
| `listing` | Title, short and full description in 10 languages; icon, feature graphic, TV banner, TV screenshots (unchanged files are skipped); contact e-mail and website |
| `datasafety` | Data safety section from `docs/store/data-safety.csv` (Play Console CSV format; Google Mobile Ads SDK disclosures) |
| `products` | `yansit_pro_yearly` (yearly, US$10) and `yansit_pro_lifetime` (US$20): local prices for every country, activation. `PRICE_OVERRIDES` sets country prices (Turkey: TRY 199.99 / 399.99) |
| `release --track alpha` | Uploads the AAB and puts it on a track with release notes in 10 languages |
| `testers --group x@googlegroups.com` | Attaches a Google Group as the testers of a closed track |
| `promote --from alpha --to production` | Moves the newest release of one track to another (`--rollout 0.2` for a staged rollout) |
| `all --track T` | listing + products + release |

A never-published ("draft") app only accepts draft releases; the tool then saves the release as a
draft and says so. After the first rollout from Play Console, releases go out automatically.

The version has one source: `version` in `Cargo.toml`. versionCode is derived from it
(MAJOR·10000 + MINOR·100 + PATCH, so 1.0.0 → 10000). Play has already seen versionCode 2, so it
must never go below 3.

## One-time setup (done)

- Google Cloud project `krmn-play-publisher` with the Google Play Android Developer API enabled.
- Service account `play-publisher@krmn-play-publisher.iam.gserviceaccount.com`, invited in Play
  Console with app-level permissions for Ayna only (releases, testers, store listing, products,
  app content; no financial data, no user management).
- Its key lives outside the repository (`~/.config/krmn/play-service-account.json`, mode 600) and
  as the GitHub Actions secret `PLAY_SERVICE_ACCOUNT_JSON`.
- GitHub secrets `UPLOAD_KEYSTORE_B64`, `UPLOAD_STORE_PASSWORD`, `UPLOAD_KEY_ALIAS`,
  `UPLOAD_KEY_PASSWORD` (Play App Signing upload key) and the variable `PLAY_TRACK`
  (`alpha` = closed testing until production access is granted, then `production`).
- Payments profile (merchant account) in Play Console: required before any paid product exists.

## Play Console steps the API cannot do

1. **App content declarations** (once per app). Suggested answers:

   | Section | Answer |
   |---|---|
   | Privacy policy | `https://krmndigital-lab.github.io/yansit-tv/` |
   | App access | All functionality is available without special access |
   | Ads | Yes, the app contains ads |
   | Content rating | Category *Utility, Productivity, Communication or Other*; every question **No** except "users can purchase digital goods": **Yes** |
   | Target audience | **18 and over**; "appeals to children": **No** |
   | Data safety | Collects/shares data: **Yes** (Google Mobile Ads SDK, free tier only). Encrypted in transit: **Yes**. Deletion request: **No** (no accounts). Approximate location, Device or other IDs, App interactions: collected and shared for Advertising, Analytics, Fraud prevention; Diagnostics: collected for Analytics, Fraud prevention. All required. Details: appendix of `docs/store/privacy-policy.md` |
   | Advertising ID | Yes: advertising/marketing, fraud prevention |
   | Foreground service | Type *Connected device*: "Keeps the AirPlay/Miracast/DLNA receiver reachable so phones and PCs can connect while the TV shows other apps." |
   | Government, financial, health, news | No |
   | Category | App → Video Players & Editors |
   | Form factors | Test and release → Advanced settings → Form factors → **Android TV** |

2. **First rollout** of the draft release: Closed testing → Alpha → Countries/regions (all) →
   Edit release → Review → Start rollout → Publishing overview → Send changes for review.

3. **Closed test with 12+ testers for 14 days** (personal developer accounts created after
   13 Nov 2023, for every new app). Testers join the Google Group `ayna-test@googlegroups.com`
   (attached to the `alpha` track) and opt in via the signup page
   <https://krmndigital-lab.github.io/yansit-tv/test/>. Afterwards: Dashboard → Apply for
   production, then `publish.py promote --from alpha --to production` and set `PLAY_TRACK` to
   `production`. Organization accounts (D-U-N-S number) do not have this requirement.

## Every later release

Raise `version` in `Cargo.toml`, add notes under `docs/store/release-notes/<version>/`, then
`git tag vX.Y.Z && git push --tags`. GitHub Actions does the rest; the track comes from the
`PLAY_TRACK` variable.
