#!/usr/bin/env python3
"""Comprime los splasharts de assets/champs/ para producción.

- PNG masters → scripts/masters_png/ (intactos, para re-encodear)
- assets/champs/{Id}.webp → WebP con alpha, calidad 88, máx 1215px de ancho
  (no upscales: lo que ya es más chico queda igual)

Uso:
    .venv/bin/python scripts/compress_splashes.py            # procesa todos los *.png
    .venv/bin/python scripts/compress_splashes.py --only Ahri,Aatrox
"""

import argparse
import shutil
import sys
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parent.parent
CHAMPS = ROOT / "assets" / "champs"
MASTERS = ROOT / "scripts" / "masters_png"
MAX_W = 1215
QUALITY = 88

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--only", type=str, default="")
    args = ap.parse_args()

    pngs = sorted(CHAMPS.glob("*.png"))
    if args.only:
        wanted = {s.strip() for s in args.only.split(",") if s.strip()}
        pngs = [p for p in pngs if p.stem in wanted]
    if not pngs:
        print("nada para comprimir")
        return

    MASTERS.mkdir(parents=True, exist_ok=True)
    before = after = 0
    done = 0
    for p in pngs:
        src_size = p.stat().st_size
        img = Image.open(p)
        if img.width > MAX_W:
            h = round(img.height * MAX_W / img.width)
            img = img.resize((MAX_W, h), Image.LANCZOS)
        out = p.with_suffix(".webp")
        img.save(out, "WEBP", quality=QUALITY, method=6)
        dst_size = out.stat().st_size
        shutil.move(p, MASTERS / p.name)
        before += src_size
        after += dst_size
        done += 1
        if done % 20 == 0 or done == len(pngs):
            print(f"  {done}/{len(pngs)} | {before/1e6:.0f}MB → {after/1e6:.0f}MB", flush=True)

    print(f"✅ {done} archivos: {before/1e6:.1f}MB → {after/1e6:.1f}MB "
          f"({100*after/before:.0f}% del original, ahorro {100-100*after/before:.0f}%)", flush=True)

if __name__ == "__main__":
    sys.exit(main())
