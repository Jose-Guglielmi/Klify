#!/usr/bin/env python3
"""Preview + revisor de splasharts sin fondo.

- Sirve los PNG de assets/champs/ en /champs/{nombre}.png
- GET  /            → página con grilla, lightbox y auto-refresh cada 2s
- GET  /api/status  → JSON de progreso del batch (scripts/.splash_status.json)
- GET  /api/list    → JSON ["Ahri.png", ...]
- GET  /api/marks   → JSON {"Ahri": "bien", "Aatrox": "mal", ...}
- POST /api/mark    → body {"id": "Ahri", "mark": "bien"|"mal"|null}, persiste en
                      scripts/.splash_marks.json (lo usa el revisor)

Uso:
    .venv/bin/python scripts/serve_preview.py            # puerto 8899
    .venv/bin/python scripts/serve_preview.py 9000       # otro puerto
"""

import base64
import json
import re
import ssl
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

try:
    import certifi
    SSL_CTX = ssl.create_default_context(cafile=certifi.where())
except ImportError:  # pragma: no cover
    SSL_CTX = ssl.create_default_context()

ROOT = Path(__file__).resolve().parent.parent
CHAMPS_DIR = ROOT / "assets" / "champs"
STATUS_FILE = ROOT / "scripts" / ".splash_status.json"
MARKS_FILE = ROOT / "scripts" / ".splash_marks.json"
ORIGINALS_DIR = ROOT / "scripts" / ".originals"
DD_SPLASH = "https://ddragon.leagueoflegends.com/cdn/img/champion/splash/{id}_0.jpg"
PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 8899


def http_get(url: str) -> bytes:
    import urllib.request
    req = urllib.request.Request(url, headers={"User-Agent": "klipy-fixer"})
    with urllib.request.urlopen(req, timeout=60, context=SSL_CTX) as r:
        return r.read()


def read_json(path: Path, default):
    if path.exists():
        try:
            return json.loads(path.read_text())
        except Exception:
            pass
    return default


def read_status() -> dict:
    return read_json(STATUS_FILE, {"total": 0, "done": 0, "current": None,
                                   "errors": [], "finished": False})


def read_marks() -> dict:
    return read_json(MARKS_FILE, {})


HTML = r"""<!DOCTYPE html>
<html lang="es">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Splasharts sin fondo — klipy</title>
<style>
  :root {
    --bg: #0a1420; --panel: #0f1e2e; --line: #1e2f42;
    --gold: #c8aa6e; --gold-dark: #79592c; --text: #ece8e1; --muted: #7d8b99;
    --green: #3c9b6e; --red: #d14d4d;
  }
  * { box-sizing: border-box; margin: 0; }
  body {
    background: radial-gradient(ellipse at top, #10233a 0%, var(--bg) 60%);
    color: var(--text); font-family: "Segoe UI", system-ui, sans-serif;
    min-height: 100vh; padding: 24px;
  }
  header { display: flex; align-items: baseline; gap: 16px; flex-wrap: wrap; margin-bottom: 8px; }
  h1 { font-size: 20px; letter-spacing: 1px; color: var(--gold); text-transform: uppercase; }
  .sub { color: var(--muted); font-size: 13px; }
  .stats { margin-left: auto; font-size: 13px; color: var(--muted); display: flex; gap: 12px; }
  .stats b.ok { color: var(--green); } .stats b.bad { color: var(--red); }
  .progress-wrap { margin: 12px 0 14px; }
  .progress-label { font-size: 13px; color: var(--muted); margin-bottom: 6px; display: flex; justify-content: space-between; }
  .progress-label b { color: var(--text); }
  .bar { height: 8px; background: var(--panel); border: 1px solid var(--line); border-radius: 4px; overflow: hidden; }
  .bar > div { height: 100%; background: linear-gradient(90deg, var(--gold-dark), var(--gold)); transition: width .6s ease; }
  .current {
    display: inline-flex; align-items: center; gap: 10px; margin: 0 0 16px;
    padding: 8px 16px; border: 1px solid var(--gold-dark); border-radius: 6px;
    background: rgba(200,170,110,.06); font-size: 14px;
  }
  .spinner {
    width: 14px; height: 14px; border: 2px solid var(--gold-dark);
    border-top-color: var(--gold); border-radius: 50%; animation: spin .8s linear infinite;
  }
  @keyframes spin { to { transform: rotate(360deg); } }
  .done-badge { color: var(--green); font-size: 13px; }
  .hint { font-size: 12px; color: var(--muted); margin-bottom: 14px; }
  .hint kbd {
    background: var(--panel); border: 1px solid var(--line); border-bottom-width: 2px;
    border-radius: 4px; padding: 1px 6px; font-size: 11px; color: var(--text);
  }
  .filters { display: flex; gap: 8px; margin-bottom: 16px; flex-wrap: wrap; }
  .filters button {
    background: var(--panel); color: var(--muted); border: 1px solid var(--line);
    border-radius: 6px; padding: 6px 14px; font-size: 13px; cursor: pointer; transition: all .15s;
  }
  .filters button:hover { color: var(--text); border-color: var(--gold-dark); }
  .filters button.active { color: var(--gold); border-color: var(--gold); background: rgba(200,170,110,.08); }
  .grid {
    display: grid; gap: 14px;
    grid-template-columns: repeat(auto-fill, minmax(180px, 1fr));
  }
  .card {
    background: var(--panel); border: 1px solid var(--line); border-radius: 8px;
    overflow: hidden; cursor: pointer; position: relative;
    transition: transform .15s ease, border-color .15s ease;
    animation: fadein .4s ease;
  }
  .card:hover { transform: translateY(-3px); border-color: var(--gold-dark); }
  .card.ok { border-color: var(--green); }
  .card.bad { border-color: var(--red); }
  @keyframes fadein { from { opacity: 0; transform: scale(.97); } to { opacity: 1; } }
  .card .img {
    aspect-ratio: 1215 / 717; display: flex; align-items: center; justify-content: center;
    background:
      linear-gradient(45deg, #0c1826 25%, transparent 25%, transparent 75%, #0c1826 75%),
      linear-gradient(45deg, #0c1826 25%, #10233a 25%, #10233a 75%, #0c1826 75%);
    background-size: 24px 24px; background-position: 0 0, 12px 12px;
  }
  .card img { width: 100%; height: 100%; object-fit: contain; display: block; }
  .card .name {
    padding: 8px 10px; font-size: 12px; color: var(--muted);
    border-top: 1px solid var(--line); text-align: center;
  }
  .badge {
    position: absolute; top: 6px; right: 6px; width: 22px; height: 22px;
    border-radius: 50%; display: none; align-items: center; justify-content: center;
    font-size: 12px; font-weight: bold; color: #fff;
  }
  .card.ok .badge { display: flex; background: var(--green); }
  .card.bad .badge { display: flex; background: var(--red); }
  /* ---- lightbox ---- */
  .lb {
    position: fixed; inset: 0; z-index: 100; display: none;
    background: rgba(3, 8, 14, .94); backdrop-filter: blur(4px);
  }
  .lb.open { display: flex; align-items: center; justify-content: center; gap: 12px; padding: 20px; }
  .lb-nav {
    background: var(--panel); color: var(--text); border: 1px solid var(--line);
    width: 52px; height: 52px; border-radius: 50%; font-size: 22px; cursor: pointer;
    flex-shrink: 0; transition: all .15s;
  }
  .lb-nav:hover { border-color: var(--gold); color: var(--gold); }
  .lb-center { display: flex; flex-direction: column; align-items: center; gap: 14px; max-width: 88vw; }
  .lb-img {
    display: flex; align-items: center; justify-content: center;
    max-width: 88vw; max-height: calc(100vh - 210px);
    background:
      linear-gradient(45deg, #0c1826 25%, transparent 25%, transparent 75%, #0c1826 75%),
      linear-gradient(45deg, #0c1826 25%, #12283f 25%, #12283f 75%, #0c1826 75%);
    background-size: 32px 32px; background-position: 0 0, 16px 16px;
    border: 1px solid var(--line); border-radius: 10px; overflow: hidden;
  }
  .lb-img img { max-width: 88vw; max-height: calc(100vh - 210px); object-fit: contain; display: block; }
  .lb-name { font-size: 18px; color: var(--gold); letter-spacing: .5px; }
  .lb-count { font-size: 12px; color: var(--muted); margin-top: 2px; }
  .lb-actions { display: flex; gap: 10px; align-items: center; margin-top: 4px; flex-wrap: wrap; justify-content: center; }
  .lb-actions button {
    border-radius: 6px; padding: 9px 18px; font-size: 14px; cursor: pointer;
    border: 1px solid var(--line); background: var(--panel); color: var(--text); transition: all .15s;
  }
  .lb-actions button:hover { border-color: var(--gold); }
  #btnBien.on { background: var(--green); border-color: var(--green); color: #fff; }
  #btnMal.on { background: var(--red); border-color: var(--red); color: #fff; }
  #btnClear { color: var(--muted); }
  .lb-actions kbd {
    opacity: .65; font-size: 11px; background: rgba(0,0,0,.3);
    border-radius: 3px; padding: 1px 5px; margin-left: 4px;
  }
  .lb-auto { display: flex; align-items: center; gap: 6px; font-size: 12px; color: var(--muted); cursor: pointer; user-select: none; }
  .lb-close {
    position: fixed; top: 16px; right: 20px; background: none; border: none;
    color: var(--muted); font-size: 28px; cursor: pointer; line-height: 1;
  }
  .lb-close:hover { color: var(--text); }
</style>
</head>
<body>
<header>
  <h1>Splasharts sin fondo</h1>
  <span class="sub" id="meta">cargando…</span>
  <a class="sub" href="/fixer" style="color:var(--gold);text-decoration:none">🛠️ Fixer con IA</a>
  <div class="stats" id="stats"></div>
</header>
<div class="progress-wrap">
  <div class="progress-label"><span id="label">…</span><span id="pct">0%</span></div>
  <div class="bar"><div id="fill" style="width:0%"></div></div>
</div>
<div class="hint">
  Click en una imagen para verla en grande · marcar con
  <kbd>B</kbd> bien / <kbd>M</kbd> mal · navegar con <kbd>←</kbd> <kbd>→</kbd> · cerrar con <kbd>Esc</kbd>
</div>
<div class="filters" id="filters">
  <button data-f="todos" class="active">Todos</button>
  <button data-f="sin">Sin revisar</button>
  <button data-f="bien">✔ Bien</button>
  <button data-f="mal">✘ Mal</button>
</div>
<div id="currentbox"></div>
<div class="grid" id="grid"></div>

<div class="lb" id="lb">
  <button class="lb-close" id="lbClose" title="Cerrar (Esc)">✕</button>
  <button class="lb-nav" id="lbPrev" title="Anterior (←)">←</button>
  <div class="lb-center">
    <div class="lb-img"><img id="lbimg" alt=""></div>
    <div class="lb-name" id="lbname"></div>
    <div class="lb-count" id="lbcount"></div>
    <div class="lb-actions">
      <button id="btnBien">✔ Bien<kbd>B</kbd></button>
      <button id="btnMal">✘ Mal<kbd>M</kbd></button>
      <button id="btnClear">Quitar marca</button>
      <label class="lb-auto"><input type="checkbox" id="autoNext" checked> avanzar al marcar</label>
    </div>
  </div>
  <button class="lb-nav" id="lbNext" title="Siguiente (→)">→</button>
</div>

<script>
const grid = document.getElementById("grid");
const lb = document.getElementById("lb");
let rendered = new Set();
let marks = {};        // { "Ahri": "bien" | "mal" }
let filter = "todos";
let lbIndex = -1;      // índice dentro de la lista visible
let statusData = {};

async function tick() {
  try {
    const [statusRes, listRes] = await Promise.all([
      fetch("/api/status"), fetch("/api/list")
    ]);
    statusData = await statusRes.json();
    renderList(await listRes.json());
    renderStatus(statusData);
  } catch (e) { /* server reiniciando: ignorar tick */ }
}

function renderList(files) {
  for (const f of files) {
    if (rendered.has(f)) continue;
    rendered.add(f);
    const name = f.replace(/\.png$/i, "");
    const card = document.createElement("div");
    card.className = "card";
    card.dataset.name = name;
    card.innerHTML = `<div class="img"><img src="/champs/${encodeURIComponent(f)}" loading="lazy" alt="${name}"></div>` +
                     `<div class="name">${name}</div><div class="badge"></div>`;
    card.addEventListener("click", () => openLb(name));
    // insertar en orden alfabético
    const names = [...grid.children].map(c => c.dataset.name);
    const idx = names.findIndex(n => n.toLowerCase() > name.toLowerCase());
    if (idx === -1) grid.appendChild(card); else grid.insertBefore(card, grid.children[idx]);
    applyMarkToCard(name);
  }
  applyFilter();
}

function renderStatus(s) {
  const count = grid.children.length;
  const total = Math.max(s.total || 0, count);
  const pct = total ? Math.round(100 * count / total) : 0;
  document.getElementById("meta").textContent = `patch ${s.version || "?"} · modelo ${s.model || "?"}`;
  document.getElementById("label").innerHTML =
    `<b>${count}</b> / ${total} splasharts sin fondo` + (s.finished ? ` <span class="done-badge">✔ terminado</span>` : "");
  document.getElementById("pct").textContent = pct + "%";
  document.getElementById("fill").style.width = pct + "%";
  document.getElementById("currentbox").innerHTML =
    (s.current && !s.finished)
      ? `<div class="current"><div class="spinner"></div> quitando fondo: <b>${s.current}</b>…</div>` : "";
}

/* ---------- marcas ---------- */
function applyMarkToCard(name) {
  const card = [...grid.children].find(c => c.dataset.name === name);
  if (!card) return;
  const m = marks[name];
  card.classList.toggle("ok", m === "bien");
  card.classList.toggle("bad", m === "mal");
  card.querySelector(".badge").textContent = m === "bien" ? "✔" : m === "mal" ? "✘" : "";
}

function updateStats() {
  const total = grid.children.length;
  const bien = Object.values(marks).filter(v => v === "bien").length;
  const mal = Object.values(marks).filter(v => v === "mal").length;
  document.getElementById("stats").innerHTML =
    `revisadas <b>${bien + mal}</b>/${total} · <b class="ok">✔ ${bien}</b> · <b class="bad">✘ ${mal}</b>`;
}

function applyFilter() {
  [...grid.children].forEach(c => {
    const m = marks[c.dataset.name];
    let show = true;
    if (filter === "sin") show = !m;
    else if (filter === "bien") show = m === "bien";
    else if (filter === "mal") show = m === "mal";
    c.style.display = show ? "" : "none";
  });
  updateStats();
}

async function setMark(name, mark) {
  marks[name] = mark;
  if (mark === null) delete marks[name];
  applyMarkToCard(name);
  applyFilter();
  if (lb.classList.contains("open")) syncLbButtons();
  try { await fetch("/api/mark", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ id: name, mark })
  }); } catch (e) { console.error("no se pudo guardar la marca", e); }
}

/* ---------- lightbox ---------- */
function visibleNames() {
  return [...grid.children].filter(c => c.style.display !== "none").map(c => c.dataset.name);
}

function openLb(name) {
  const vis = visibleNames();
  lbIndex = Math.max(0, vis.indexOf(name));
  lb.classList.add("open");
  showLb();
}

function showLb() {
  const vis = visibleNames();
  if (!vis.length) { closeLb(); return; }
  if (lbIndex >= vis.length) lbIndex = vis.length - 1;
  if (lbIndex < 0) lbIndex = 0;
  const name = vis[lbIndex];
  document.getElementById("lbimg").src = `/champs/${encodeURIComponent(name)}.png`;
  document.getElementById("lbname").textContent = name;
  document.getElementById("lbcount").textContent = `${lbIndex + 1} / ${vis.length}`;
  syncLbButtons();
}

function syncLbButtons() {
  const vis = visibleNames();
  const name = vis[lbIndex];
  const m = name ? marks[name] : undefined;
  document.getElementById("btnBien").classList.toggle("on", m === "bien");
  document.getElementById("btnMal").classList.toggle("on", m === "mal");
}

function lbNav(delta) {
  lbIndex += delta;
  showLb();
}

function markCurrent(mark) {
  const vis = visibleNames();
  if (lbIndex < 0 || lbIndex >= vis.length) return;
  const name = vis[lbIndex];
  setMark(name, mark);
  if (document.getElementById("autoNext").checked) lbNav(1);
}

function closeLb() { lb.classList.remove("open"); }

document.getElementById("lbPrev").addEventListener("click", () => lbNav(-1));
document.getElementById("lbNext").addEventListener("click", () => lbNav(1));
document.getElementById("lbClose").addEventListener("click", closeLb);
document.getElementById("btnBien").addEventListener("click", () => markCurrent("bien"));
document.getElementById("btnMal").addEventListener("click", () => markCurrent("mal"));
document.getElementById("btnClear").addEventListener("click", () => markCurrent(null));
lb.addEventListener("click", e => { if (e.target === lb) closeLb(); });

document.addEventListener("keydown", e => {
  if (!lb.classList.contains("open")) return;
  if (e.key === "ArrowLeft") lbNav(-1);
  else if (e.key === "ArrowRight") lbNav(1);
  else if (e.key === "Escape") closeLb();
  else if (e.key.toLowerCase() === "b") markCurrent("bien");
  else if (e.key.toLowerCase() === "m") markCurrent("mal");
});

/* ---------- filtros ---------- */
document.getElementById("filters").addEventListener("click", e => {
  const btn = e.target.closest("button");
  if (!btn) return;
  filter = btn.dataset.f;
  document.querySelectorAll("#filters button").forEach(b => b.classList.toggle("active", b === btn));
  applyFilter();
});

/* ---------- init ---------- */
fetch("/api/marks").then(r => r.json()).then(m => { marks = m; [...grid.children].forEach(c => applyMarkToCard(c.dataset.name)); applyFilter(); });
tick();
setInterval(tick, 2000);
</script>
</body>
</html>
"""


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *args):  # silenciar logs por request
        pass

    def _send(self, code: int, body: bytes, ctype: str, cache: str = "no-store"):
        self.send_response(code)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", cache)
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        if self.path in ("/", "/index.html"):
            self._send(200, HTML.encode(), "text/html; charset=utf-8")
        elif self.path == "/fixer":
            body = (ROOT / "scripts" / "fixer.html").read_bytes()
            self._send(200, body, "text/html; charset=utf-8")
        elif self.path.startswith("/original/"):
            cid = self.path.removeprefix("/original/").split("?")[0]
            if not re.fullmatch(r"[A-Za-z0-9]+", cid):
                self._send(400, b"bad id", "text/plain")
                return
            ORIGINALS_DIR.mkdir(parents=True, exist_ok=True)
            f = ORIGINALS_DIR / f"{cid}.jpg"
            if not f.exists():
                try:
                    f.write_bytes(http_get(DD_SPLASH.format(id=cid)))
                except Exception as e:  # noqa: BLE001
                    self._send(502, str(e).encode(), "text/plain")
                    return
            self._send(200, f.read_bytes(), "image/jpeg", cache="max-age=86400")
        elif self.path == "/api/status":
            self._send(200, json.dumps(read_status()).encode(), "application/json")
        elif self.path == "/api/list":
            files = sorted(
                [p.name for p in CHAMPS_DIR.glob("*.png")] +
                [p.name for p in CHAMPS_DIR.glob("*.webp")]
            )
            self._send(200, json.dumps(files).encode(), "application/json")
        elif self.path == "/api/marks":
            self._send(200, json.dumps(read_marks()).encode(), "application/json")
        elif self.path.startswith("/champs/"):
            name = Path(self.path.removeprefix("/champs/")).name  # anti path traversal
            f = CHAMPS_DIR / name
            ctype = {"png": "image/png", "webp": "image/webp"}.get(f.suffix.lower().lstrip("."))
            if ctype and f.is_file():
                self._send(200, f.read_bytes(), ctype, cache="max-age=3600")
            else:
                self._send(404, b"not found", "text/plain")
        else:
            self._send(404, b"not found", "text/plain")

    def do_POST(self):
        if self.path != "/api/mark" and self.path != "/api/save":
            self._send(404, b"not found", "text/plain")
            return
        try:
            length = int(self.headers.get("Content-Length", 0))
            body = json.loads(self.rfile.read(length))

            if self.path == "/api/save":
                cid = body.get("id", "")
                data_url = body.get("dataUrl", "")
                if not re.fullmatch(r"[A-Za-z0-9]+", cid):
                    raise ValueError("id inválido")
                if not data_url.startswith("data:image/"):
                    raise ValueError("dataUrl inválido")
                _, b64 = data_url.split(",", 1)
                png = base64.b64decode(b64)
                (CHAMPS_DIR / f"{cid}.png").write_bytes(png)
                mark = body.get("mark", "bien")
                marks = read_marks()
                if mark in ("bien", "mal"):
                    marks[cid] = mark
                else:
                    marks.pop(cid, None)
                MARKS_FILE.write_text(json.dumps(marks, indent=2, ensure_ascii=False))
                self._send(200, json.dumps({"ok": True, "marks": marks}).encode(), "application/json")
                return

            cid, mark = body.get("id"), body.get("mark")
            if not isinstance(cid, str) or mark not in ("bien", "mal", None):
                raise ValueError("payload inválido")
            marks = read_marks()
            if mark is None:
                marks.pop(cid, None)
            else:
                marks[cid] = mark
            MARKS_FILE.write_text(json.dumps(marks, indent=2, ensure_ascii=False))
            self._send(200, json.dumps(marks).encode(), "application/json")
        except Exception as e:  # noqa: BLE001
            self._send(400, json.dumps({"error": str(e)}).encode(), "application/json")


if __name__ == "__main__":
    server = ThreadingHTTPServer(("127.0.0.1", PORT), Handler)
    print(f"Preview en http://localhost:{PORT}  (sirviendo {CHAMPS_DIR})", flush=True)
    server.serve_forever()
