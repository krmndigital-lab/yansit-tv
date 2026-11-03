#!/usr/bin/env python3
"""Ayna (com.yansit.tv) — Google Play publishing over the Play Developer API v3.

Everything that Google exposes through the API is done here, so a release is one command (or one
git tag, see .github/workflows/release.yml):

  check                         auth + what Play currently has (tracks, releases, listings, products)
  listing [--no-images]         store listing in 10 languages, graphics, contact details
  products                      Ayna Pro: yearly subscription ($10) + lifetime one-time product ($20)
  datasafety                    Data safety section from docs/store/data-safety.csv
  release --track T             upload the AAB and put it on a track (internal/alpha/beta/production)
  testers --track T --group G   attach a Google Group as the testers of a closed track
  promote --from A --to B       copy the newest release of track A to track B
  all --track T                 listing + release (+ products) in one go

Credentials: a Google Cloud service account JSON that was invited in Play Console (Users and
permissions). Looked up in this order: --key PATH, env PLAY_SERVICE_ACCOUNT_JSON (a path or the
JSON itself, as in CI), ~/.config/krmn/play-service-account.json. Never put it in the repo.

Only depends on the Python standard library + `cryptography` (RS256 for the OAuth JWT).
GPL-3.0-or-later.
"""

import argparse
import base64
import hashlib
import json
import os
import pathlib
import re
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[2]
STORE = ROOT / "docs" / "store"
PACKAGE = "com.yansit.tv"
API = "https://androidpublisher.googleapis.com/androidpublisher/v3"
UPLOAD_API = "https://androidpublisher.googleapis.com/upload/androidpublisher/v3"
SCOPE = "https://www.googleapis.com/auth/androidpublisher"
DEFAULT_KEY = pathlib.Path.home() / ".config" / "krmn" / "play-service-account.json"

CONTACT_EMAIL = "krmndigital@gmail.com"
CONTACT_WEBSITE = "https://krmndigital-lab.github.io/yansit-tv/"

# docs/store/listing-<code>.md -> Play listing language
LANGS = {
    "en": "en-US", "tr": "tr-TR", "de": "de-DE", "es": "es-ES", "fr": "fr-FR",
    "it": "it-IT", "pt": "pt-BR", "ru": "ru-RU", "pl": "pl-PL", "id": "id",
}
STORE_SHOTS = re.compile(r"^0[1-5]-.*\.png$")  # 06-about.png is for review only, never uploaded

# ---- Ayna Pro products (docs/MONETIZATION.md) -------------------------------------------------
SUB_ID, BASE_PLAN, SUB_USD = "yansit_pro_yearly", "yearly", "10"
LIFE_ID, LIFE_OPTION, LIFE_USD = "yansit_pro_lifetime", "lifetime", "20"
# Country prices that replace Google's USD conversion (tax-inclusive local prices). Turkey: Google
# suggests TRY 599.99 / 1199.99 for US$10 / US$20; Ayna sells at roughly a third of that there.
PRICE_OVERRIDES = {
    SUB_ID: {"TR": {"currencyCode": "TRY", "units": "199", "nanos": 990000000}},
    LIFE_ID: {"TR": {"currencyCode": "TRY", "units": "399", "nanos": 990000000}},
}
PRODUCT_TEXT = {  # lang: (yearly title, lifetime title, description)
    "en-US": ("Ayna Pro (yearly)", "Ayna Pro (lifetime)", "4K mirroring and no ads."),
    "tr-TR": ("Ayna Pro (yıllık)", "Ayna Pro (ömür boyu)", "4K yansıtma ve reklamsız kullanım."),
    "de-DE": ("Ayna Pro (jährlich)", "Ayna Pro (lebenslang)", "4K-Spiegelung und keine Werbung."),
    "es-ES": ("Ayna Pro (anual)", "Ayna Pro (de por vida)", "Duplicación en 4K y sin anuncios."),
    "fr-FR": ("Ayna Pro (annuel)", "Ayna Pro (à vie)", "Recopie en 4K et sans publicité."),
    "it-IT": ("Ayna Pro (annuale)", "Ayna Pro (a vita)", "Mirroring in 4K e nessuna pubblicità."),
    "pt-BR": ("Ayna Pro (anual)", "Ayna Pro (vitalício)", "Espelhamento em 4K e sem anúncios."),
    "ru-RU": ("Ayna Pro (на год)", "Ayna Pro (навсегда)", "Трансляция в 4K и без рекламы."),
    "pl-PL": ("Ayna Pro (roczna)", "Ayna Pro (dożywotnia)", "Kopia ekranu w 4K i bez reklam."),
    "id": ("Ayna Pro (tahunan)", "Ayna Pro (seumur hidup)", "Pencerminan 4K dan tanpa iklan."),
}
SUB_BENEFITS = {
    "en-US": ["4K (2160p) mirroring", "No ads"], "tr-TR": ["4K (2160p) yansıtma", "Reklam yok"],
    "de-DE": ["4K-Spiegelung (2160p)", "Keine Werbung"], "es-ES": ["Duplicación 4K (2160p)", "Sin anuncios"],
    "fr-FR": ["Recopie 4K (2160p)", "Sans publicité"], "it-IT": ["Mirroring 4K (2160p)", "Nessuna pubblicità"],
    "pt-BR": ["Espelhamento 4K (2160p)", "Sem anúncios"], "ru-RU": ["Трансляция 4K (2160p)", "Без рекламы"],
    "pl-PL": ["Kopia ekranu 4K (2160p)", "Bez reklam"], "id": ["Pencerminan 4K (2160p)", "Tanpa iklan"],
}

C = {"b": "\033[1m", "g": "\033[32m", "y": "\033[33m", "r": "\033[31m", "d": "\033[2m", "0": "\033[0m"}
if not sys.stdout.isatty():
    C = {k: "" for k in C}


def say(msg, kind="i"):
    mark = {"i": f"{C['d']}·{C['0']}", "ok": f"{C['g']}✔{C['0']}", "w": f"{C['y']}!{C['0']}",
            "e": f"{C['r']}✖{C['0']}", "h": f"\n{C['b']}==>{C['0']}"}[kind]
    print(f"{mark} {C['b'] if kind == 'h' else ''}{msg}{C['0'] if kind == 'h' else ''}", flush=True)


class ApiError(Exception):
    def __init__(self, status, message, body=""):
        super().__init__(f"HTTP {status}: {message}")
        self.status, self.message, self.body = status, message, body


# ---------------------------------------------------------------- auth + HTTP

def load_key(arg):
    src = arg or os.environ.get("PLAY_SERVICE_ACCOUNT_JSON", "").strip()
    if src.startswith("{"):
        return json.loads(src)
    path = pathlib.Path(src).expanduser() if src else DEFAULT_KEY
    if not path.is_file():
        raise SystemExit(
            f"Service account key not found ({path}).\n"
            "Create it once (docs/PLAY_PUBLISHING.md, step 2) and save it there, or set\n"
            "PLAY_SERVICE_ACCOUNT_JSON / --key.")
    if path.stat().st_mode & 0o077:
        say(f"{path} is readable by other users; run: chmod 600 {path}", "w")
    return json.loads(path.read_text(encoding="utf-8"))


def b64url(b):
    return base64.urlsafe_b64encode(b).rstrip(b"=").decode()


class Play:
    def __init__(self, sa, package, dry_run=False):
        self.sa, self.pkg, self.dry = sa, package, dry_run
        self._tok, self._exp = None, 0

    def token(self):
        if self._tok and time.time() < self._exp - 120:
            return self._tok
        from cryptography.hazmat.primitives import hashes, serialization
        from cryptography.hazmat.primitives.asymmetric import padding

        now = int(time.time())
        aud = self.sa.get("token_uri", "https://oauth2.googleapis.com/token")
        head = b64url(json.dumps({"alg": "RS256", "typ": "JWT"}).encode())
        claims = b64url(json.dumps({"iss": self.sa["client_email"], "scope": SCOPE, "aud": aud,
                                    "iat": now, "exp": now + 3600}).encode())
        key = serialization.load_pem_private_key(self.sa["private_key"].encode(), password=None)
        sig = key.sign(f"{head}.{claims}".encode(), padding.PKCS1v15(), hashes.SHA256())
        body = urllib.parse.urlencode({"grant_type": "urn:ietf:params:oauth:grant-type:jwt-bearer",
                                       "assertion": f"{head}.{claims}.{b64url(sig)}"}).encode()
        try:
            with urllib.request.urlopen(urllib.request.Request(aud, data=body), timeout=60) as r:
                data = json.loads(r.read())
        except urllib.error.HTTPError as e:
            raise SystemExit(f"Google sign-in failed ({e.code}): {e.read().decode(errors='replace')}")
        self._tok, self._exp = data["access_token"], now + int(data.get("expires_in", 3600))
        return self._tok

    def call(self, method, path, body=None, query=None, raw=None, ctype=None, base=API,
             headers=None, want_headers=False, ok404=False):
        url = (path if path.startswith("http") else f"{base}/applications/{self.pkg}{path}")
        if query:
            url += ("&" if "?" in url else "?") + urllib.parse.urlencode(query)
        data = raw if raw is not None else (json.dumps(body).encode() if body is not None else None)
        for attempt in range(6):
            h = {"Authorization": f"Bearer {self.token()}"}
            if data is not None or method in ("POST", "PUT", "PATCH"):
                h["Content-Type"] = ctype or "application/json"
            h.update(headers or {})
            req = urllib.request.Request(url, data=data if data is not None else
                                         (b"" if method in ("POST", "PUT", "PATCH") else None),
                                         method=method, headers=h)
            try:
                with urllib.request.urlopen(req, timeout=600) as r:
                    txt = r.read()
                    out = json.loads(txt) if txt.strip() else {}
                    return (out, dict(r.headers)) if want_headers else out
            except urllib.error.HTTPError as e:
                txt = e.read().decode(errors="replace")
                try:
                    msg = json.loads(txt).get("error", {}).get("message", txt)
                except ValueError:
                    msg = txt
                if e.code == 404 and ok404:
                    return None
                if e.code in (429, 500, 502, 503, 504) and attempt < 5:
                    time.sleep(2 ** attempt)
                    continue
                raise ApiError(e.code, msg, txt) from None
            except (urllib.error.URLError, TimeoutError) as e:
                if attempt < 5:
                    time.sleep(2 ** attempt)
                    continue
                raise ApiError(0, str(e)) from None

    # ---- edits
    def edit(self):
        return self.call("POST", "/edits", {})["id"]

    def discard(self, eid):
        try:
            self.call("DELETE", f"/edits/{eid}")
        except ApiError:
            pass

    def commit(self, eid):
        """Commits; if Play wants a human to press "Send for review", commits without sending."""
        if self.dry:
            self.call("POST", f"/edits/{eid}:validate")
            self.discard(eid)
            say("dry run: edit validated and discarded (nothing changed on Play)", "ok")
            return "dry"
        try:
            self.call("POST", f"/edits/{eid}:commit")
            say("committed and sent to Google Play", "ok")
            return "sent"
        except ApiError as e:
            if "changesNotSentForReview" not in e.message:
                raise
            self.call("POST", f"/edits/{eid}:commit", query={"changesNotSentForReview": "true"})
            say("committed. Play requires ONE manual click for this change: Play Console → "
                "Publishing overview → 'Send changes for review'", "w")
            return "needs-review-click"

    def upload(self, path_suffix, file, ctype):
        """Resumable upload (works for any size); returns the JSON resource."""
        size = file.stat().st_size
        _, hdr = self.call("POST", path_suffix, raw=b"", base=UPLOAD_API,
                           query={"uploadType": "resumable"}, want_headers=True,
                           headers={"X-Upload-Content-Type": ctype,
                                    "X-Upload-Content-Length": str(size)})
        loc = next((v for k, v in hdr.items() if k.lower() == "location"), None)
        if not loc:
            raise ApiError(0, "no resumable upload URL returned")
        return self.call("PUT", loc, raw=file.read_bytes(), ctype=ctype)


# ---------------------------------------------------------------- local content

def version():
    m = re.search(r'(?m)^version\s*=\s*"(\d+)\.(\d+)\.(\d+)"', (ROOT / "Cargo.toml").read_text())
    name = ".".join(m.groups())
    code = int(m.group(1)) * 10000 + int(m.group(2)) * 100 + int(m.group(3))
    return name, code


def listings():
    out = {}
    for code, lang in LANGS.items():
        md = (STORE / f"listing-{code}.md").read_text(encoding="utf-8")
        f = {}
        for key, sec in (("title", "Title"), ("shortDescription", "Short description"),
                         ("fullDescription", "Full description")):
            m = re.search(r"##\s*%s\s*```text\n(.*?)\n```" % re.escape(sec), md, re.S)
            if not m:
                raise SystemExit(f"listing-{code}.md: section '{sec}' missing")
            f[key] = m.group(1).strip()
        for k, lim in (("title", 30), ("shortDescription", 80), ("fullDescription", 4000)):
            if len(f[k]) > lim:
                raise SystemExit(f"listing-{code}.md: {k} is {len(f[k])} chars (Play limit {lim})")
        out[lang] = (code, f)
    return out


def release_notes(ver):
    notes = []
    d = STORE / "release-notes" / ver
    for code, lang in LANGS.items():
        f = d / f"{code}.txt"
        if f.is_file():
            t = f.read_text(encoding="utf-8").strip()
            if len(t) > 500:
                raise SystemExit(f"{f}: {len(t)} chars (Play limit 500)")
            notes.append({"language": lang, "text": t})
    if not notes:
        say(f"no release notes in {d.relative_to(ROOT)} (release goes out without notes)", "w")
    return notes


def images_for(code):
    shots = sorted(p for p in (STORE / "screenshots" / code).glob("*.png") if STORE_SHOTS.match(p.name))
    return {
        "icon": [STORE / "icon-512.png"],
        "featureGraphic": [STORE / "feature-graphic" / f"{code}.png"],
        "tvBanner": [STORE / "tv-banner-1280x720.png"],
        "tvScreenshots": shots,
        # Play's main listing also demands 2-8 phone/tablet screenshots (16:9 is accepted), even
        # for a TV-only app: the TV screenshots are the honest picture of the app there too.
        "phoneScreenshots": shots,
    }


def sha256(p):
    return hashlib.sha256(p.read_bytes()).hexdigest()


# ---------------------------------------------------------------- commands

def cmd_check(play, _a):
    say("Google sign-in + app access", "h")
    play.token()
    say(f"service account: {play.sa['client_email']}", "ok")
    eid = play.edit()
    try:
        det = play.call("GET", f"/edits/{eid}/details")
        say(f"app {play.pkg}: default language {det.get('defaultLanguage')}, "
            f"contact {det.get('contactEmail', '-')}", "ok")
        say("tracks", "h")
        for t in play.call("GET", f"/edits/{eid}/tracks").get("tracks", []):
            rel = t.get("releases", [])
            desc = "; ".join(f"{r.get('name', '?')} {r.get('versionCodes', [])} {r.get('status')}"
                             + (f" {r['userFraction']:.0%}" if "userFraction" in r else "") for r in rel)
            say(f"{t['track']:<22} {desc or '(empty)'}")
        say("store listings", "h")
        ls = play.call("GET", f"/edits/{eid}/listings").get("listings", [])
        say(", ".join(sorted(l["language"] for l in ls)) or "(none)")
        bundles = play.call("GET", f"/edits/{eid}/bundles").get("bundles", [])
        say(f"uploaded bundles: {sorted(b['versionCode'] for b in bundles)}")
    finally:
        play.discard(eid)
    say("products", "h")
    try:
        subs = play.call("GET", "/subscriptions").get("subscriptions", [])
        for s in subs:
            plans = ", ".join(f"{b['basePlanId']}={b.get('state')}" for b in s.get("basePlans", []))
            say(f"subscription {s['productId']}: {plans}")
        if not subs:
            say("no subscriptions yet (run: products)")
    except ApiError as e:
        say(f"subscriptions: {e}", "w")
    try:
        otps = play.call("GET", "/oneTimeProducts").get("oneTimeProducts", [])
        for o in otps:
            st = ", ".join(f"{p['purchaseOptionId']}={p.get('state')}" for p in o.get("purchaseOptions", []))
            say(f"one-time {o['productId']}: {st}")
        if not otps:
            say("no one-time products yet (run: products)")
    except ApiError as e:
        say(f"one-time products: {e}", "w")
    name, code = version()
    say(f"local version {name} (versionCode {code})", "ok")


def cmd_listing(play, a):
    ls = listings()
    eid = play.edit()
    try:
        say("contact details", "h")
        det = play.call("GET", f"/edits/{eid}/details")
        play.call("PATCH", f"/edits/{eid}/details",
                  {"contactEmail": CONTACT_EMAIL, "contactWebsite": CONTACT_WEBSITE,
                   "defaultLanguage": det.get("defaultLanguage", "en-US")})
        say(f"{CONTACT_EMAIL} · {CONTACT_WEBSITE}", "ok")
        say("store listing texts", "h")
        for lang, (code, f) in ls.items():
            play.call("PUT", f"/edits/{eid}/listings/{lang}", dict(language=lang, **f))
            say(f"{lang:<6} {f['title']}", "ok")
        if not a.no_images:
            say("graphics (unchanged files are skipped)", "h")
            for lang, (code, _f) in ls.items():
                sent = 0
                for itype, files in images_for(code).items():
                    files = [p for p in files if p.is_file()]
                    if not files:
                        continue
                    have = play.call("GET", f"/edits/{eid}/listings/{lang}/{itype}").get("images", [])
                    if [i.get("sha256") for i in have] == [sha256(p) for p in files]:
                        continue
                    if have:
                        play.call("DELETE", f"/edits/{eid}/listings/{lang}/{itype}")
                    for p in files:
                        play.upload(f"/edits/{eid}/listings/{lang}/{itype}", p, "image/png")
                        sent += 1
                say(f"{lang:<6} {sent} image(s) uploaded", "ok")
        return play.commit(eid)
    except BaseException:
        play.discard(eid)
        raise


DRAFT_APP_HINT = ("Play still treats the app as a draft (never published), so the release was saved "
                  "as a DRAFT on the track. Finish the Play Console setup checklist and start this "
                  "first rollout there once; later releases go out automatically.")


def put_release(play, eid, track, rel):
    """tracks.update + commit. A never-published (draft) app only accepts draft releases; Play
    may report that at tracks.update or only at commit, so both are retried as a draft."""
    def as_draft(r):
        r = dict(r, status="draft")
        r.pop("userFraction", None)
        return r

    try:
        play.call("PUT", f"/edits/{eid}/tracks/{track}", {"track": track, "releases": [rel]})
    except ApiError as e:
        if "draft app" not in e.message or rel["status"] == "draft":
            raise
        rel = as_draft(rel)
        play.call("PUT", f"/edits/{eid}/tracks/{track}", {"track": track, "releases": [rel]})
        say(DRAFT_APP_HINT, "w")
    try:
        return rel["status"], play.commit(eid)
    except ApiError as e:
        if "draft app" not in e.message or rel["status"] == "draft":
            raise
        rel = as_draft(rel)
        play.call("PUT", f"/edits/{eid}/tracks/{track}", {"track": track, "releases": [rel]})
        result = play.commit(eid)
        say(DRAFT_APP_HINT, "w")
        return "draft", result


def cmd_release(play, a):
    name, code = version()
    aab = pathlib.Path(a.aab) if a.aab else ROOT / "android" / "build" / "gradle" / "outputs" / \
        "bundle" / "release" / "Yansit-release.aab"
    if not aab.is_file():
        raise SystemExit(f"AAB not found: {aab}\nBuild it: cd android && ./gradlew bundleRelease")
    notes = release_notes(name)
    eid = play.edit()
    try:
        say(f"upload {aab.name} ({aab.stat().st_size / 1e6:.1f} MB)", "h")
        try:
            b = play.upload(f"/edits/{eid}/bundles", aab, "application/octet-stream")
        except ApiError as e:
            if "version code" in e.message.lower() and "already" in e.message.lower():
                raise SystemExit(f"Play already has versionCode {code}. Bump `version` in Cargo.toml "
                                 "(or build with -PversionCode=N).") from None
            raise
        vc = int(b["versionCode"])
        say(f"versionCode {vc}, sha256 {b.get('sha256', '')[:16]}…", "ok")
        rel = {"name": f"{name} ({vc})", "versionCodes": [str(vc)], "releaseNotes": notes,
               "status": a.status if a.status != "auto" else "completed"}
        if a.rollout and a.rollout < 1.0:
            rel["status"], rel["userFraction"] = "inProgress", a.rollout
        st, result = put_release(play, eid, a.track, rel)
        say(f"track {a.track}: {name} ({vc}) → {st}", "ok")
    except BaseException:
        play.discard(eid)
        raise
    if a.track != "production" and result == "sent" and st != "draft":
        say(f"testers get it from https://play.google.com/apps/testing/{play.pkg}", "i")
    return result


def cmd_testers(play, a):
    eid = play.edit()
    try:
        play.call("PUT", f"/edits/{eid}/testers/{a.track}", {"googleGroups": a.group})
        say(f"{a.track}: testers = {', '.join(a.group)}", "ok")
        play.commit(eid)
    except BaseException:
        play.discard(eid)
        raise
    say(f"opt-in link for testers: https://play.google.com/apps/testing/{play.pkg}", "i")


def cmd_promote(play, a):
    eid = play.edit()
    try:
        src = play.call("GET", f"/edits/{eid}/tracks/{a.source}")
        rels = [r for r in src.get("releases", []) if r.get("versionCodes")]
        if not rels:
            raise SystemExit(f"track {a.source} has no release to promote")
        top = max(rels, key=lambda r: max(int(v) for v in r["versionCodes"]))
        rel = {"name": top.get("name"), "versionCodes": top["versionCodes"],
               "releaseNotes": top.get("releaseNotes", []), "status": "completed"}
        if a.rollout and a.rollout < 1.0:
            rel["status"], rel["userFraction"] = "inProgress", a.rollout
        st, _ = put_release(play, eid, a.target, rel)
        say(f"{a.source} → {a.target}: {rel['name']} {rel['versionCodes']} ({st})", "ok")
    except BaseException:
        play.discard(eid)
        raise


def money(m):
    return {"currencyCode": m["currencyCode"], "units": str(m.get("units", "0")),
            "nanos": int(m.get("nanos", 0))}


def converted(play, usd):
    r = play.call("POST", "/pricing:convertRegionPrices", {"price": {"currencyCode": "USD", "units": usd}})
    regions = {code: money(v["price"]) for code, v in r.get("convertedRegionPrices", {}).items()}
    other = r.get("convertedOtherRegionsPrice", {})
    return regions, money(other["usdPrice"]), money(other["eurPrice"]), r["regionVersion"]["version"]


def create_dropping_regions(fn, regions):
    """Retries `fn(regions)` while Play names regions it won't accept (billing not available, …)."""
    regions = dict(regions)
    for _ in range(25):
        try:
            return fn(regions)
        except ApiError as e:
            bad = {c for c in re.findall(r"\b([A-Z]{2})\b", e.message) if c in regions}
            if not bad:
                raise
            say(f"Play rejected region(s) {', '.join(sorted(bad))}: retrying without them", "w")
            for c in bad:
                regions.pop(c)
    raise ApiError(0, "too many rejected regions")


def cmd_products(play, _a):
    say(f"subscription {SUB_ID} (base plan {BASE_PLAN}, US${SUB_USD}/year)", "h")
    sub_listings = [{"languageCode": l, "title": t[0], "description": t[2], "benefits": SUB_BENEFITS[l]}
                    for l, t in PRODUCT_TEXT.items()]
    have = play.call("GET", f"/subscriptions/{SUB_ID}", ok404=True)
    if have is None:
        regions, usd, eur, rv = converted(play, SUB_USD)
        regions.update(PRICE_OVERRIDES.get(SUB_ID, {}))

        def create(regs):
            if play.dry:
                return say(f"dry run: would create with {len(regs)} regional prices", "ok")
            return play.call("POST", "/subscriptions", query={"productId": SUB_ID, "regionsVersion.version": rv},
                             body={"packageName": play.pkg, "productId": SUB_ID, "listings": sub_listings,
                                   "basePlans": [{
                                       "basePlanId": BASE_PLAN,
                                       "autoRenewingBasePlanType": {
                                           "billingPeriodDuration": "P1Y", "gracePeriodDuration": "P7D",
                                           "accountHoldDuration": "P53D",
                                           "resubscribeState": "RESUBSCRIBE_STATE_ACTIVE",
                                           "prorationMode": "SUBSCRIPTION_PRORATION_MODE_CHARGE_ON_NEXT_BILLING_DATE",
                                           "legacyCompatible": True, "legacyCompatibleSubscriptionOfferId": ""},
                                       "regionalConfigs": [{"regionCode": c, "newSubscriberAvailability": True,
                                                            "price": p} for c, p in sorted(regs.items())],
                                       "otherRegionsConfig": {"usdPrice": usd, "eurPrice": eur,
                                                              "newSubscriberAvailability": True}}]})
        create_dropping_regions(create, regions)
        say(f"created ({len(regions)} countries, local prices converted from US${SUB_USD})", "ok")
    else:
        if not play.dry:
            play.call("PATCH", f"/subscriptions/{SUB_ID}", query={"updateMask": "listings",
                      "regionsVersion.version": "2022/02"},
                      body={"packageName": play.pkg, "productId": SUB_ID, "listings": sub_listings})
        say("exists: listings updated (prices are left as they are)", "ok")
    plan = next((b for b in (play.call("GET", f"/subscriptions/{SUB_ID}", ok404=True) or {}).get("basePlans", [])
                 if b["basePlanId"] == BASE_PLAN), None)
    if plan and plan.get("state") != "ACTIVE" and not play.dry:
        play.call("POST", f"/subscriptions/{SUB_ID}/basePlans/{BASE_PLAN}:activate",
                  {"packageName": play.pkg, "productId": SUB_ID, "basePlanId": BASE_PLAN})
        say("base plan activated", "ok")
    elif plan:
        say(f"base plan state: {plan.get('state')}", "ok")

    say(f"one-time product {LIFE_ID} (US${LIFE_USD})", "h")
    otp_listings = [{"languageCode": l, "title": t[1], "description": t[2]} for l, t in PRODUCT_TEXT.items()]
    have = play.call("GET", f"/oneTimeProducts/{LIFE_ID}", ok404=True)
    if have is None:
        regions, usd, eur, rv = converted(play, LIFE_USD)
        regions.update(PRICE_OVERRIDES.get(LIFE_ID, {}))

        def create(regs):
            if play.dry:
                return say(f"dry run: would create with {len(regs)} regional prices", "ok")
            return play.call("PATCH", f"/onetimeproducts/{LIFE_ID}",
                             query={"allowMissing": "true", "regionsVersion.version": rv,
                                    "updateMask": "listings,purchaseOptions"},
                             body={"packageName": play.pkg, "productId": LIFE_ID, "listings": otp_listings,
                                   "purchaseOptions": [{
                                       "purchaseOptionId": LIFE_OPTION,
                                       "buyOption": {"legacyCompatible": True, "multiQuantityEnabled": False},
                                       "regionalPricingAndAvailabilityConfigs": [
                                           {"regionCode": c, "price": p, "availability": "AVAILABLE"}
                                           for c, p in sorted(regs.items())],
                                       "newRegionsConfig": {"usdPrice": usd, "eurPrice": eur,
                                                            "availability": "AVAILABLE"}}]})
        create_dropping_regions(create, regions)
        say(f"created ({len(regions)} countries)", "ok")
        have = play.call("GET", f"/oneTimeProducts/{LIFE_ID}", ok404=True) or {}
    elif not play.dry:
        play.call("PATCH", f"/onetimeproducts/{LIFE_ID}",
                  query={"updateMask": "listings", "regionsVersion.version": "2022/02"},
                  body={"packageName": play.pkg, "productId": LIFE_ID, "listings": otp_listings})
        say("exists: listings updated (prices are left as they are)", "ok")
    opt = next((o for o in have.get("purchaseOptions", []) if o.get("purchaseOptionId") == LIFE_OPTION), None)
    if opt and opt.get("state") != "ACTIVE" and not play.dry:
        play.call("POST", f"/oneTimeProducts/{LIFE_ID}/purchaseOptions:batchUpdateStates",
                  {"requests": [{"activatePurchaseOptionRequest": {
                      "packageName": play.pkg, "productId": LIFE_ID, "purchaseOptionId": LIFE_OPTION}}]})
        say("purchase option activated", "ok")
    elif opt:
        say(f"purchase option state: {opt.get('state')}", "ok")


def cmd_datasafety(play, _a):
    """Data safety section from docs/store/data-safety.csv (Play Console's CSV format)."""
    csv_path = STORE / "data-safety.csv"
    if play.dry:
        return say(f"dry run: would send {csv_path.relative_to(ROOT)}", "ok")
    play.call("POST", "/dataSafety", {"safetyLabels": csv_path.read_text(encoding="utf-8")})
    say("data safety section updated", "ok")


def cmd_all(play, a):
    cmd_listing(play, a)
    try:
        cmd_products(play, a)
    except ApiError as e:
        say(f"products skipped: {e.message}", "w")
    return cmd_release(play, a)


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--key", help="service account JSON (path)")
    ap.add_argument("--package", default=PACKAGE)
    ap.add_argument("--dry-run", action="store_true", help="validate everything, change nothing")
    sp = ap.add_subparsers(dest="cmd", required=True)
    sp.add_parser("check")
    p = sp.add_parser("listing")
    p.add_argument("--no-images", action="store_true")
    sp.add_parser("products")
    sp.add_parser("datasafety")
    for n in ("release", "all"):
        p = sp.add_parser(n)
        p.add_argument("--track", default="alpha", help="internal | alpha (closed) | beta (open) | production")
        p.add_argument("--aab")
        p.add_argument("--status", default="auto", choices=["auto", "draft", "completed"])
        p.add_argument("--rollout", type=float, help="staged rollout fraction, e.g. 0.2")
        p.add_argument("--no-images", action="store_true")
    p = sp.add_parser("testers")
    p.add_argument("--track", default="alpha")
    p.add_argument("--group", action="append", required=True, help="Google Group e-mail (repeatable)")
    p = sp.add_parser("promote")
    p.add_argument("--from", dest="source", default="alpha")
    p.add_argument("--to", dest="target", default="production")
    p.add_argument("--rollout", type=float)
    a = ap.parse_args()

    if a.cmd in ("listing", "all"):
        listings()  # validate local files before touching Play
    play = Play(load_key(a.key), a.package, a.dry_run)
    fn = {"check": cmd_check, "listing": cmd_listing, "products": cmd_products, "release": cmd_release,
          "testers": cmd_testers, "promote": cmd_promote, "all": cmd_all,
          "datasafety": cmd_datasafety}[a.cmd]
    try:
        fn(play, a)
    except ApiError as e:
        say(f"Google Play API: {e}", "e")
        sys.exit(1)


if __name__ == "__main__":
    main()
