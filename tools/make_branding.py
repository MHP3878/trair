#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2026 The Trair contributors
"""Erzeugt Logo-Dateien und die Symbol-Schrift der App neu.

  python3 tools/make_branding.py <Ordner mit entpacktem lucide-static 1.75.0>

Holen und entpacken (npm ist nur hierfuer noetig, die App braucht es nicht):
  npm pack lucide-static@1.75.0 && tar -xzf lucide-static-1.75.0.tgz

Schreibt:
  app/assets/branding/trair-app-icon.svg   App-Symbol (Logo "Neon-Schild")
  app/assets/branding/trair-mark.svg       nur das Zeichen, einfarbig
  app/assets/branding/app_icon.ico         Windows-Symbol fuer Trair.exe
  app/assets/fonts/Lucide.ttf              nur die Symbole, die die App nutzt
  app/assets/fonts/LUCIDE-LICENSE.txt      Lizenz der Symbole (ISC, Teile MIT)

Braucht: rsvg-convert (librsvg), pip install pillow fonttools
"""
import hashlib
import io
import pathlib
import shutil
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
BRANDING = ROOT / "app" / "assets" / "branding"
FONTS = ROOT / "app" / "assets" / "fonts"

LUCIDE_TTF_SHA256 = "3d6ce197ac48c4b546500518a5de137d46c3b2269762dd26c3ac32e974ed9658"  # pragma: allowlist secret (Pruefsumme einer oeffentlichen Datei)

# Muss zu app/lib/widgets/trair_icons.dart passen.
ICONS = {
    "arrow-right": 0xE049,
    "arrow-up": 0xE04A,
    "badge-check": 0xE241,
    "check": 0xE06C,
    "chevron-down": 0xE06D,
    "chevron-left": 0xE06E,
    "clock": 0xE087,
    "copy": 0xE09E,
    "crown": 0xE1D6,
    "ellipsis": 0xE0B6,
    "eye": 0xE0BA,
    "eye-off": 0xE0BB,
    "file-text": 0xE0CC,
    "flask-conical": 0xE0D5,
    "gavel": 0xE0E0,
    "info": 0xE0F9,
    "link": 0xE102,
    "lock": 0xE10B,
    "log-out": 0xE10E,
    "monitor": 0xE11D,
    "monitor-smartphone": 0xE3A2,
    "plus": 0xE13D,
    "qr-code": 0xE1DF,
    "search": 0xE151,
    "server": 0xE153,
    "shield": 0xE158,
    "shield-check": 0xE1FF,
    "square-pen": 0xE172,
    "trash-2": 0xE18E,
    "triangle-alert": 0xE193,
    "user": 0xE19F,
    "user-cog": 0xE342,
    "user-minus": 0xE1A1,
    "user-plus": 0xE1A2,
    "users": 0xE1A4,
    "pencil": 0xE1F9,
    "x": 0xE1B2,
}

# Logo "Neon-Schild" (Julians Wahl, Mischung aus Schildblase und Neon-T)
# auf einem 512er Raster: leuchtender Schild, der zugleich Sprechblase ist,
# darin ein T mit drei Tipp-Punkten. Muss zu TrairLogo in
# app/lib/widgets/common.dart passen.
APP_ICON = """<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512"><defs><linearGradient id="abg" x1="0" y1="0" x2="1" y2="1">
 <stop offset="0" stop-color="#221b3d"/><stop offset="0.55" stop-color="#120f1e"/><stop offset="1" stop-color="#0a0910"/></linearGradient>
<radialGradient id="ahalo" cx="0.5" cy="0.45" r="0.55">
 <stop offset="0" stop-color="#7c5cff" stop-opacity="0.42"/><stop offset="1" stop-color="#7c5cff" stop-opacity="0"/></radialGradient>
<linearGradient id="aneon" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#5cc8ff"/><stop offset="0.5" stop-color="#a18dff"/><stop offset="1" stop-color="#ff7ac6"/></linearGradient>
<linearGradient id="acyan" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#8fe6ff"/><stop offset="1" stop-color="#2b8bff"/></linearGradient>
<linearGradient id="avio" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#c3b3ff"/><stop offset="0.6" stop-color="#8a6cff"/><stop offset="1" stop-color="#ff7ac6"/></linearGradient>
<linearGradient id="awhite" x1="0" y1="0" x2="0.4" y2="1"><stop offset="0" stop-color="#ffffff"/><stop offset="1" stop-color="#dcd4ff"/></linearGradient>
<linearGradient id="asolid" x1="0.15" y1="0" x2="0.85" y2="1"><stop offset="0" stop-color="#b9a6ff"/><stop offset="0.45" stop-color="#7c5cff"/><stop offset="1" stop-color="#4425d6"/></linearGradient>
<filter id="ablur" x="-50%" y="-50%" width="200%" height="200%"><feGaussianBlur stdDeviation="16"/></filter>
<filter id="asoft" x="-50%" y="-50%" width="200%" height="200%"><feGaussianBlur stdDeviation="6"/></filter>
<mask id="ad"><rect width="512" height="512" fill="#fff"/><g fill="#000"><circle cx="214" cy="190" r="9"/><circle cx="256" cy="190" r="9"/><circle cx="298" cy="190" r="9"/></g></mask></defs><rect width="512" height="512" rx="116" fill="url(#abg)"/>
<rect x="2" y="2" width="508" height="508" rx="114" fill="none" stroke="#fff" stroke-opacity="0.09" stroke-width="3"/>
<rect width="512" height="512" rx="116" fill="url(#ahalo)"/>
<path d="M 160 100 H 352 Q 410 100 410 158 V 262 Q 410 352 312 398 L 210 440 Q 216 408 196 396 Q 102 350 102 262 V 158 Q 102 100 160 100 Z" fill="none" stroke="url(#aneon)" stroke-width="32" stroke-linejoin="round" opacity="0.85" filter="url(#ablur)"/>
<path d="M 160 100 H 352 Q 410 100 410 158 V 262 Q 410 352 312 398 L 210 440 Q 216 408 196 396 Q 102 350 102 262 V 158 Q 102 100 160 100 Z" fill="#ffffff" fill-opacity="0.035" stroke="url(#aneon)" stroke-width="20" stroke-linejoin="round"/>
<path d="M 160 100 H 352 Q 410 100 410 158 V 262 Q 410 352 312 398 L 210 440 Q 216 408 196 396 Q 102 350 102 262 V 158 Q 102 100 160 100 Z" fill="none" stroke="#fff" stroke-opacity="0.65" stroke-width="4.5" stroke-linejoin="round"/>
<g fill="#a18dff" opacity="0.75" filter="url(#asoft)"><rect x="176" y="163" width="160" height="54" rx="27"/><rect x="229" y="163" width="54" height="180" rx="27"/></g>
<g fill="url(#awhite)" mask="url(#ad)"><rect x="176" y="163" width="160" height="54" rx="27"/><rect x="229" y="163" width="54" height="180" rx="27"/></g></svg>
"""
# Einfarbig: Schild-Umriss und T, die Punkte ausgestanzt.
SHIELD = ("M 160 100 H 352 Q 410 100 410 158 V 262 Q 410 352 312 398 L 210 440 Q 216 408 196 396 "
          "Q 102 350 102 262 V 158 Q 102 100 160 100 Z")
MARK_SVG = (
    '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512">'
    f'<path d="{SHIELD}" fill="none" stroke="#fff" stroke-width="22" stroke-linejoin="round"/>'
    '<path fill="#fff" fill-rule="evenodd" d="M203 163H309A27 27 0 0 1 309 217H283V316A27 27 0 0 1 229 316V217H203'
    'A27 27 0 0 1 203 163Z M214 181A9 9 0 1 0 214 199A9 9 0 1 0 214 181Z M256 181A9 9 0 1 0 256 199'
    'A9 9 0 1 0 256 181Z M298 181A9 9 0 1 0 298 199A9 9 0 1 0 298 181Z"/>'
    "</svg>\n"
)

ICO_SIZES = [16, 20, 24, 32, 40, 48, 64, 128, 256]


def logo():
    import subprocess

    from PIL import Image

    BRANDING.mkdir(parents=True, exist_ok=True)
    (BRANDING / "trair-app-icon.svg").write_text(APP_ICON, encoding="utf-8")
    (BRANDING / "trair-mark.svg").write_text(MARK_SVG, encoding="utf-8")
    images = []
    for size in ICO_SIZES:
        # rsvg-convert (librsvg) kann das Leuchten (Weichzeichner), cairosvg nicht.
        png = subprocess.run(["rsvg-convert", "-w", str(size), "-h", str(size)],
                             input=APP_ICON.encode(), capture_output=True, check=True).stdout
        images.append(Image.open(io.BytesIO(png)).convert("RGBA"))
    biggest = images[-1]
    biggest.save(BRANDING / "app_icon.ico", format="ICO",
                 sizes=[(s, s) for s in ICO_SIZES], append_images=images[:-1])
    print("Logo-Dateien geschrieben.")


def icon_font(lucide_dir):
    from fontTools import subset
    from fontTools.ttLib import TTFont

    src = pathlib.Path(lucide_dir) / "font" / "lucide.ttf"
    digest = hashlib.sha256(src.read_bytes()).hexdigest()
    if digest != LUCIDE_TTF_SHA256:
        sys.exit(f"lucide.ttf hat nicht die erwartete Pruefsumme ({digest}). Version 1.75.0 verwenden.")
    font = TTFont(src)
    cmap = font.getBestCmap()
    missing = [n for n, cp in ICONS.items() if cp not in cmap]
    if missing:
        sys.exit("Nicht in lucide.ttf: " + ", ".join(missing))
    # Die Ligaturen-Tabellen in lucide.ttf sind fehlerhaft und werden nicht
    # gebraucht: die App waehlt Symbole ueber ihre Codepunkte.
    for tag in ("GSUB", "GPOS", "GDEF"):
        if tag in font:
            del font[tag]
    options = subset.Options()
    options.hinting = False
    options.layout_features = []
    options.name_IDs = ["*"]
    sub = subset.Subsetter(options)
    sub.populate(unicodes=sorted(ICONS.values()))
    sub.subset(font)
    FONTS.mkdir(parents=True, exist_ok=True)
    font.save(FONTS / "Lucide.ttf")
    shutil.copyfile(pathlib.Path(lucide_dir) / "LICENSE", FONTS / "LUCIDE-LICENSE.txt")
    print(f"Lucide.ttf mit {len(ICONS)} Symbolen geschrieben.")


def main():
    if len(sys.argv) != 2:
        print(__doc__)
        return 2
    logo()
    icon_font(sys.argv[1])
    return 0


if __name__ == "__main__":
    sys.exit(main())
