#!/usr/bin/env python3
"""Descarga splasharts originales de Data Dragon y les quita el fondo con rembg.

- Fuente de splashes: https://ddragon.leagueoflegends.com/cdn/img/champion/splash/{Id}_0.jpg
- Modelo de remoción de fondo: isnet-general-use
- Salida: assets/champs/{Id}.png (PNG con transparencia, resolución original 1215x717)
- Progreso: scripts/.splash_status.json (lo consume serve_preview.py)

Resume-safe: saltea cualquier campeón que ya tenga assets/champs/{Id}.png,
así que se puede interrumpir y re-ejecutar sin repetir trabajo.

Uso:
    .venv/bin/python scripts/process_splashes.py             # procesa todos los faltantes
    .venv/bin/python scripts/process_splashes.py --limit 5   # solo 5 campeones (prueba)
    .venv/bin/python scripts/process_splashes.py --only Aatrox,Ahri
"""

import argparse
import io
import json
import ssl
import sys
import time
import urllib.request
from datetime import datetime
from pathlib import Path

from PIL import Image
from rembg import new_session, remove

# Homebrew Python no trae CA bundle propio: usar certifi si está disponible
try:
    import certifi
    SSL_CTX = ssl.create_default_context(cafile=certifi.where())
except ImportError:  # pragma: no cover
    SSL_CTX = ssl.create_default_context()

ROOT = Path(__file__).resolve().parent.parent
CHAMPS_DIR = ROOT / "assets" / "champs"
STATUS_FILE = ROOT / "scripts" / ".splash_status.json"

DD_VERSIONS = "https://ddragon.leagueoflegends.com/api/versions.json"
DD_CHAMPS = "https://ddragon.leagueoflegends.com/cdn/{v}/data/en_US/champion.json"
DD_SPLASH = "https://ddragon.leagueoflegends.com/cdn/img/champion/splash/{id}_0.jpg"

MODEL = "isnet-general-use"
UA = {"User-Agent": "Mozilla/5.0 (klipy asset pipeline)"}


def fetch_json(url: str):
    req = urllib.request.Request(url, headers=UA)
    with urllib.request.urlopen(req, timeout=30, context=SSL_CTX) as r:
        return json.loads(r.read().decode())


def fetch_bytes(url: str) -> bytes:
    req = urllib.request.Request(url, headers=UA)
    with urllib.request.urlopen(req, timeout=60, context=SSL_CTX) as r:
        return r.read()


def write_status(**kw):
    status = {
        "model": MODEL,
        "startedAt": kw.get("startedAt"),
        "finished": kw.get("finished", False),
        "phase": kw.get("phase", "processing"),
        "version": kw.get("version"),
        "total": kw.get("total", 0),
        "done": kw.get("done", 0),
        "current": kw.get("current"),
        "errors": kw.get("errors", []),
        "updatedAt": datetime.now().isoformat(timespec="seconds"),
    }
    STATUS_FILE.write_text(json.dumps(status, indent=2, ensure_ascii=False))


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--limit", type=int, default=0, help="procesar como máximo N campeones")
    ap.add_argument("--only", type=str, default="", help="procesar solo estos ids (coma-separados)")
    args = ap.parse_args()

    CHAMPS_DIR.mkdir(parents=True, exist_ok=True)
    started = datetime.now().isoformat(timespec="seconds")

    print("[1/3] Obteniendo catálogo de campeones...", flush=True)
    version = fetch_json(DD_VERSIONS)[0]
    data = fetch_json(DD_CHAMPS.format(v=version))["data"]
    ids = sorted(data.keys())
    if args.only:
        wanted = {s.strip() for s in args.only.split(",") if s.strip()}
        ids = [i for i in ids if i in wanted]
    total = len(ids)

    todo = [i for i in ids if not (CHAMPS_DIR / f"{i}.png").exists()]
    done = total - len(todo)
    print(f"[1/3] Versión {version}: {total} campeones, {done} ya listos, {len(todo)} a procesar", flush=True)

    if args.limit and args.limit > 0:
        todo = todo[: args.limit]
        print(f"[1/3] --limit: procesando solo {len(todo)}", flush=True)

    write_status(startedAt=started, phase="model", version=version,
                 total=total, done=done, errors=[])
    print(f"[2/3] Cargando modelo {MODEL} (descarga ~170MB la primera vez)...", flush=True)
    session = new_session(MODEL)
    print("[2/3] Modelo listo", flush=True)

    errors = []
    write_status(startedAt=started, phase="processing", version=version,
                 total=total, done=done, errors=errors)

    for cid in todo:
        write_status(startedAt=started, phase="processing", version=version,
                     total=total, done=done, current=cid, errors=errors)
        try:
            t0 = time.time()
            raw = fetch_bytes(DD_SPLASH.format(id=cid))
            img = Image.open(io.BytesIO(raw)).convert("RGBA")
            out = remove(img, session=session)
            out.save(CHAMPS_DIR / f"{cid}.png")
            dt = time.time() - t0
            done += 1
            print(f"  ✔ {cid} ({dt:.1f}s) [{done}/{total}]", flush=True)
        except Exception as e:  # noqa: BLE001
            errors.append({"id": cid, "error": str(e)[:200]})
            print(f"  ✘ {cid}: {e}", flush=True)
        time.sleep(0.3)  # no saturar la CDN

    write_status(startedAt=started, phase="done", version=version,
                 total=total, done=done, errors=errors, finished=True)
    print(f"[3/3] Listo: {done}/{total} procesados, {len(errors)} errores", flush=True)
    for e in errors:
        print(f"  ✘ {e['id']}: {e['error']}", flush=True)


if __name__ == "__main__":
    sys.exit(main())
