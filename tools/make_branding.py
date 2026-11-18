#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2026 The Trair contributors
"""Erzeugt Logo-Dateien und die Symbol-Schrift der App neu.

  python3 tools/make_branding.py <Ordner mit entpacktem lucide-static 1.75.0>

Holen und entpacken (npm ist nur hierfuer noetig, die App braucht es nicht):
  npm pack lucide-static@1.75.0 && tar -xzf lucide-static-1.75.0.tgz

Schreibt:
  app/assets/branding/trair-app-icon.svg   App-Symbol (Logo D "Zylinder")
  app/assets/branding/trair-mark.svg       nur das Zeichen, einfarbig
  app/assets/branding/app_icon.ico         Windows-Symbol fuer Trair.exe
  app/assets/fonts/Lucide.ttf              nur die Symbole, die die App nutzt
  app/assets/fonts/LUCIDE-LICENSE.txt      Lizenz der Symbole (ISC, Teile MIT)

Braucht: pip install cairosvg pillow fonttools
"""
import hashlib
import io
import pathlib
import shutil
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
BRANDING = ROOT / "app" / "assets" / "branding"
FONTS = ROOT / "app" / "assets" / "fonts"

LUCIDE_TTF_SHA256 = "3d6ce197ac48c4b546500518a5de137d46c3b2269762dd26c3ac32e974ed9658"

# Muss zu app/lib/widgets/trair_icons.dart passen.
ICONS = {
    "arrow-right": 0xE049,
    "arrow-up": 0xE04A,
    "badge-check": 0xE241,
    "check": 0xE06C,
    "chevron-down": 0xE06D,
    "chevron-left": 0xE06E,
    "ellipsis": 0xE0B6,
    "eye": 0xE0BA,
    "eye-off": 0xE0BB,
    "file-text": 0xE0CC,
    "flask-conical": 0xE0D5,
    "lock": 0xE10B,
    "monitor": 0xE11D,
    "monitor-smartphone": 0xE3A2,
    "plus": 0xE13D,
    "qr-code": 0xE1DF,
    "search": 0xE151,
    "shield-check": 0xE1FF,
    "square-pen": 0xE172,
    "triangle-alert": 0xE193,
    "user": 0xE19F,
    "user-cog": 0xE342,
    "user-plus": 0xE1A2,
}

# Logo D "Zylinder" auf einem 256er Raster: weisse Scheibe mit T-Schlitz.
MARK = (
    '<path fill="{c}" fill-rule="evenodd" d="M40 128A88 88 0 1 0 216 128A88 88 0 1 0 40 128Z '
    'M95 92H161A13 13 0 0 1 161 118H141V167A13 13 0 0 1 115 167V118H95A13 13 0 0 1 95 92Z"/>'
)
APP_ICON = (
    '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256">'
    '<defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1">'
    '<stop offset="0" stop-color="#8A6CFF"/><stop offset="1" stop-color="#5A3BE6"/></linearGradient></defs>'
    '<rect width="256" height="256" rx="58" fill="url(#g)"/>'
    '<rect x="1" y="1" width="254" height="254" rx="57" fill="none" stroke="#fff" stroke-opacity=".16" stroke-width="2"/>'
    + MARK.format(c="#fff")
    + '<circle cx="128" cy="128" r="74" fill="none" stroke="#5A3BE6" stroke-opacity=".18" stroke-width="3"/>'
    "</svg>\n"
)
MARK_SVG = '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256">' + MARK.format(c="#fff") + "</svg>\n"

ICO_SIZES = [16, 20, 24, 32, 40, 48, 64, 128, 256]


def logo():
    import cairosvg
    from PIL import Image

    BRANDING.mkdir(parents=True, exist_ok=True)
    (BRANDING / "trair-app-icon.svg").write_text(APP_ICON, encoding="utf-8")
    (BRANDING / "trair-mark.svg").write_text(MARK_SVG, encoding="utf-8")
    images = []
    for size in ICO_SIZES:
        png = cairosvg.svg2png(bytestring=APP_ICON.encode(), output_width=size, output_height=size)
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
