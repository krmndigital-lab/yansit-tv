"""Renders Ayna artwork: launcher icon + Android TV banner (in the APK) and the Google Play
store graphics (512 px icon, 1280x720 TV banner, 1024x500 feature graphic per language).

Mark: a TV screen on a stand with a bold round "a" inside (Ayna = "mirror"). Deliberately
original: no bottom triangle (Apple's AirPlay symbol), no corner waves (Google Cast), and no
third-party product names in any artwork — those are trademark-rejection risks on Play.

Usage: python3 -I tools/make_icons.py <InterVariable-full.ttf> <android/res/drawable> <docs/store>
"""
import os
import sys

from PIL import Image, ImageDraw, ImageFilter, ImageFont

FONT, RES_OUT, STORE_OUT = sys.argv[1], sys.argv[2], sys.argv[3]

STOPS = [(0.0, (8, 14, 32)), (0.45, (20, 38, 92)), (0.8, (40, 110, 210)), (1.0, (0, 205, 255))]

# Feature-graphic taglines (kept in sync with docs/store/listing-*.md).
TAGLINES = {
    "tr": "iPhone, iPad, Mac, Android ve Windows'tan TV'ye yansıtın",
    "en": "Mirror iPhone, iPad, Mac, Android & Windows to your TV",
    "de": "iPhone, iPad, Mac, Android & Windows auf den Fernseher",
    "es": "Duplica iPhone, iPad, Mac, Android y Windows en tu TV",
    "fr": "Recopiez iPhone, iPad, Mac, Android et Windows sur la TV",
    "it": "Duplica iPhone, iPad, Mac, Android e Windows sulla TV",
    "pt": "Espelhe iPhone, iPad, Mac, Android e Windows na TV",
    "ru": "iPhone, iPad, Mac, Android и Windows — на ваш телевизор",
    "pl": "Kopia ekranu iPhone, iPad, Mac, Android i Windows na TV",
    "id": "Cerminkan iPhone, iPad, Mac, Android & Windows ke TV",
}


def grad(t):
    t = min(1.0, max(0.0, t))
    for (a, ca), (b, cb) in zip(STOPS, STOPS[1:]):
        if t <= b:
            u = (t - a) / (b - a)
            return tuple(int(ca[i] + (cb[i] - ca[i]) * u) for i in range(3))
    return STOPS[-1][1]


def background(w, h):
    """Diagonal brand gradient with a soft cyan glow behind the mark."""
    sw, sh = max(48, w // 8), max(48, h // 8)  # smooth gradient: compute small, upscale
    im = Image.new("RGB", (sw, sh))
    px = im.load()
    for y in range(sh):
        for x in range(sw):
            px[x, y] = grad((x / sw) * 0.55 + (1 - y / sh) * 0.45)
    im = im.resize((w, h), Image.BICUBIC)
    glow = Image.new("L", (w, h), 0)
    ImageDraw.Draw(glow).ellipse((w * 0.15, h * 0.1, w * 0.85, h * 0.9), fill=110)
    glow = glow.filter(ImageFilter.GaussianBlur(min(w, h) * 0.18))
    im.paste(Image.new("RGB", (w, h), (90, 220, 255)), (0, 0), glow.point(lambda v: v // 3))
    return im


def draw_mark(im, cx, cy, s, color=(255, 255, 255)):
    """TV + "a" mark centred at (cx, cy); `s` = mark width in px."""
    d = ImageDraw.Draw(im)
    lw = max(2, round(s * 0.085))
    sw, sh = s, s * 0.66
    left, top = cx - sw / 2, cy - sh / 2 - s * 0.06
    d.rounded_rectangle((left, top, left + sw, top + sh), radius=s * 0.12, outline=color, width=lw)
    # stand: neck + foot (a plain bar, no triangle)
    foot_y = top + sh + s * 0.13
    d.line((cx, top + sh, cx, foot_y), fill=color, width=lw)
    d.line((cx - s * 0.2, foot_y, cx + s * 0.2, foot_y), fill=color, width=lw)
    for x in (cx - s * 0.2, cx + s * 0.2):
        d.ellipse((x - lw / 2, foot_y - lw / 2, x + lw / 2, foot_y + lw / 2), fill=color)
    # single-storey "a" inside the screen: an open ring + a straight stem flush with its right
    # edge, spanning exactly the ring's height (anything longer reads as "d" or "q").
    w = lw * 0.95
    R = sh * 0.27
    acx, acy = cx, top + sh * 0.5
    d.ellipse((acx - R, acy - R, acx + R, acy + R), outline=color, width=round(w))
    d.rectangle((acx + R - w, acy - R, acx + R, acy + R), fill=color)


def font(size, weight=700):
    f = ImageFont.truetype(FONT, size)
    try:  # set axes by name: Inter's order is (optical size, weight)
        vals = []
        for ax in f.get_variation_axes():
            name = str(ax.get("name", b"")).lower()
            lo, hi = ax["minimum"], ax["maximum"]
            v = weight if "weight" in name else min(32, max(14, size)) if "optical" in name else ax.get("default", lo)
            vals.append(max(lo, min(hi, v)))
        f.set_variation_by_axes(vals)
    except (OSError, AttributeError, KeyError):
        pass
    return f


def supersample(render, w, h, k=3):
    big = render(w * k, h * k)
    return big.resize((w, h), Image.LANCZOS)


def icon_square(n):
    """Full-bleed square: Google Play applies its own mask (no baked corners)."""
    def r(w, h):
        im = background(w, h)
        draw_mark(im, w / 2, h / 2 + h * 0.02, w * 0.58)
        return im
    return supersample(r, n, n)


def icon_tile(n):
    """In-APK launcher icon: rounded tile on transparent background."""
    def r(w, h):
        tile = icon_square(w).convert("RGBA")
        mask = Image.new("L", (w, h), 0)
        ImageDraw.Draw(mask).rounded_rectangle((0, 0, w - 1, h - 1), radius=w * 0.22, fill=255)
        out = Image.new("RGBA", (w, h), (0, 0, 0, 0))
        out.paste(tile, (0, 0), mask)
        return out
    return r(n, n)


def banner(w, h):
    """Android TV banner: mark + "Ayna" wordmark, language-neutral."""
    def r(W, H):
        im = background(W, H)
        draw_mark(im, W * 0.24, H * 0.5, H * 0.52)
        d = ImageDraw.Draw(im)
        d.text((W * 0.43, H * 0.5), "Ayna", font=font(round(H * 0.3)), fill=(255, 255, 255), anchor="lm")
        return im
    return supersample(r, w, h)


def feature_graphic(lang, w=1024, h=500):
    def r(W, H):
        im = background(W, H)
        draw_mark(im, W * 0.19, H * 0.47, H * 0.42)
        d = ImageDraw.Draw(im)
        d.text((W * 0.34, H * 0.4), "Ayna", font=font(round(H * 0.2)), fill=(255, 255, 255), anchor="lm")
        tag = TAGLINES[lang]
        size = round(H * 0.068)
        f = font(size, 500)
        while d.textlength(tag, font=f) > W * 0.62 and size > 10:
            size -= 1
            f = font(size, 500)
        d.text((W * 0.34, H * 0.58), tag, font=f, fill=(225, 240, 255), anchor="lm")
        proto = "AirPlay 2  •  Miracast  •  DLNA"
        d.text((W * 0.34, H * 0.7), proto, font=font(round(H * 0.045), 500), fill=(150, 205, 255), anchor="lm")
        return im
    return supersample(r, w, h, 2)


os.makedirs(RES_OUT, exist_ok=True)
os.makedirs(os.path.join(STORE_OUT, "feature-graphic"), exist_ok=True)
icon_tile(192).save(os.path.join(RES_OUT, "icon.png"))
banner(320, 180).save(os.path.join(RES_OUT, "banner.png"))
icon_square(512).convert("RGBA").save(os.path.join(STORE_OUT, "icon-512.png"))  # Play: 32-bit PNG
banner(1280, 720).save(os.path.join(STORE_OUT, "tv-banner-1280x720.png"))
for lang in TAGLINES:
    feature_graphic(lang).save(os.path.join(STORE_OUT, "feature-graphic", f"{lang}.png"))
print("artwork written")
