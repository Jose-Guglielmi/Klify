# Plan de UI — Menú Principal de Klipy

> Estado: PROPUESTA · Fecha: 2026-10-22 · Referencias: `design.md` (tokens), `assets/main.css` (recetas de marca), prototipos aprobados `ui-preview.html`, `ui-preview-sylas.html`.

## 0. Decisiones aprobadas con el usuario

| Decisión | Elección |
|---|---|
| Navegación global | **Pantalla hub** (estilo launcher de juego) |
| Detección LCU (champ select / partida en vivo) | **Aviso + botón ENTRAR**, nunca salto automático |
| Búsqueda de perfiles | **Riot API oficial** (Summoner-V4/Account-V1, `RIOT_API_KEY` por env) |
| Ficha de campeón | **Completa**: stats + hechizos + build/runas (Lolalytics) + guía curada + historial propio |
| Vista EN VIVO | **Porofessor completo**: rank, WR, KDA reciente, maestría, avisos de riesgo |
| Entregable de esta fase | **Plan + prototipos HTML** (sin tocar código Rust) |

## 1. Modelo de navegación

**Cambio de paradigma**: hoy el estado LCU *deriva* la vista (`derive_view`). Con el menú
principal, la vista la controla el usuario y el LCU pasa a ser una **capa de alertas**.

Nuevo estado de navegación (propuesta de implementación futura):

```rust
enum AppView {
    Menu,                                  // Hub principal (raíz)
    Perfil(SummonerRef),                   // Ficha de perfil (propio o buscado)
    FichaCampeon(ChampionId),              // Ficha standalone de campeón
    PartidaEnVivo(LiveTab),                // Shell con 2 tabs
}
enum LiveTab { ChampSelect, EnVivo }
enum SummonerRef { LcuLocal, RiotId { game_name: String, tag: String } }
```

- Las señales LCU/Live dejan de mandar en la navegación: publican una `LcuAlert`
  (`ChampSelectDetectado`, `PartidaEnVivoDetectada`) que el Hub muestra como banner.
- Regla: **la app nunca te saca de donde estás**. Solo avisa.
- Breadcrumb en todas las vistas hijas: `INICIO › PERFILES › Nombre#TAG` /
  `INICIO › CAMPEONES › Ahri` / `INICIO › PARTIDA EN VIVO › EN VIVO`.
- El `WaitingView` actual se sustituye por el Hub; el estado de desconexión del LCU
  se muestra como badge de estado dentro del Hub, no como pantalla aparte.

## 2. Pantallas

### 2.1 HUB — `MenuView` (nueva, raíz)

```
┌──────────────────────────────────────────────────────────────┐
│ KLIY HEADER (logo, badge LCU: ● conectado / ○ desconectado,   │
│              badge parche 16.x, badge RIOT API ok/sin key)    │
├──────────────────────────────────────────────────────────────┤
│  ⚡ BANNER ALERTA LCU (solo si hay detección)                  │
│  "CHAMP SELECT DETECTADO — Ahri (mid) · [ ENTRAR → ]"         │
│  "PARTIDA EN VIVO DETECTADA · 12:43 · [ ENTRAR → ]"           │
├──────────────────────────────────────────────────────────────┤
│  ┌───────────────┐          ┌───────────────┐                │
│  │  PERFILES      │          │  CAMPEONES     │   <- 2 cards  │
│  │  launcher      │          │  launcher      │      grandes  │
│  │  [ Riot ID#TAG]│          │  [ Buscar… ]   │                │
│  │  resultados ▾  │          │  grid/suggest ▾│                │
│  │  "Buscar"      │          │  "Ver ficha"   │                │
│  └───────────────┘          └───────────────┘                │
│  (debajo: accesos rápidos — tu perfil LCU local, champs       │
│   recientes/recientes buscados)                               │
├──────────────────────────────────────────────────────────────┤
│  KLIY FOOTER                                                  │
└──────────────────────────────────────────────────────────────┘
```

Componentes:
- `LcuStatusBadge`: estado del cliente (conectado/desconectado) + cola (norma/ranked).
- `LcuAlertBanner`: recetas de marca (glow rojo, esquinas cortadas), con botón ENTRAR
  grande. Distingue champ select (muestra tu champ/lane si ya está locked) de partida
  en vivo (muestra modo y tiempo).
- `SearchCard` ×2 (perfiles / campeones): input con autocomplete en vivo.
  - Perfiles: Riot ID `Nombre#TAG` → hasta ~6 resultados con icono de invocador, nivel
    y tag. Enter o click → `PerfilView`.
  - Campeones: por nombre (ES/EN), con mini-retratos; también grid con los 173 champs
    del catálogo. Click → `FichaCampeonView`.
- Accesos rápidos: "Mi perfil" (LCU local) y champs buscados recientemente (persistido
  en `settings.json`).

### 2.2 Ficha de PERFIL — `PerfilView` (adaptación de la actual)

- Se reutiliza la vista existente (hero con rank solo-q, TOP CHAMPS, HISTORIAL con
  modal `MatchDetailModal`) extendida para aceptar `SummonerRef`:
  - `LcuLocal`: como hoy, vía LCU.
  - `RiotId`: fetch vía Riot API (summoner + rank + match history Match-V5).
- Se añade breadcrumb `INICIO › PERFILES › Nombre#TAG` y botón volver.
- Badge "TÚ" cuando el perfil es el del LCU local.

### 2.3 Ficha de CAMPEÓN — `FichaCampeonView` (nueva)

Hero: arte del campeón (`assets/champs/`), nombre, título, rol, dificultad. Barra con
WR global / pickrate / banrate (fuente Lolalytics, la misma del champ select).

Tabs (mismo patrón visual que champ select):

| Tab | Contenido | Fuente de datos |
|---|---|---|
| GENERAL | Stats base (HP, AD, armadura… por nivel), pasiva + hechizos QWER con cooldown/costo/rango | Data Dragon `champion_detail.rs` (ya existe) |
| BUILD | Runas recomendadas, build de ítems, orden de skills, WR por build | `stats/source/lolalytics` + `ugg` (ya existe) |
| COUNTERS | Campeones que le ganan / pierde con WR | `stats/source/counters` (ya existe) |
| GUÍA | Ficha cualitativa curada: counter tips, momentos de peligro, contrajugadas | `design/data/*.json` vía `vs_guide_data.rs` (ya existe; hoy solo Akali y Annie) |
| TU HISTORIAL | Tus partidos con ese champ: KDA, WR, builds usadas | dataset propio `data/matches.jsonl` |

- Si no hay ficha curada para el champ: la tab GUÍA muestra estado vacío elegante
  ("Aún no hay guía curada para X") en vez de romper.
- El contenido reutiliza los paneles existentes del champ select (skills, runes,
  items, counters) como componentes compartidos donde sea posible.

### 2.4 PARTIDA EN VIVO — shell `PartidaEnVivoView` (nueva)

Shell de sección con dos tabs grandes:
- **CHAMP SELECT** → monta la `ChampSelectView` actual sin cambios (fuente: señales LCU
  + espejo Live).
- **EN VIVO** → nueva `LiveGameView` (fuente: `LiveGame` de `live.rs` + enriquecimiento
  por Riot API).

### 2.5 EN VIVO — `LiveGameView` (nueva, estilo Porofessor)

```
┌──────────────────────────────────────────────────────────────┐
│ RANKED SOLO/DUO · 14:32 · [refresco auto 5s ●]                │
├──────────────────────────────┬───────────────────────────────┤
│ TU EQUIPO (azul)             │ EQUIPO ENEMIGO (rojo, foco)   │
│ ┌──────────────────────────┐ │ ┌───────────────────────────┐ │
│ │ [art] Champ   Lv 12      │ │ │ [art] Champ   Lv 14       │ │
│ │ Nombre#TAG  EM 2         │ │ │ Nombre#TAG   EM 4   ⚠️     │ │
│ │ Oro IV · WR 54% (120p)   │ │ │ Platino I · WR 61% (88p)  │ │
│ │ KDA 6.2 · 45% c this champ│ │ │ KDA 3.1 · primera vez c.  │ │
│ │ chips: [main Ahri] [duo] │ │ │ chips: 🔴racha 0/6 💀smurf?│ │
│ └──────────────────────────┘ │ └───────────────────────────┘ │
│  … ×5                        │  … ×5                         │
└──────────────────────────────┴───────────────────────────────┘
```

- Card por jugador: arte del champ, nivel en juego, hechizos de invocador, Riot ID,
  rank + división, WR solo-q (partidas), KDA reciente (últimos ~10), WR con el champ
  actual, maestría (champ main).
- Chips de aviso (receta de marca, rojo escaso):
  - 🔴 racha perdedora (≥3 derrotas seguidas)
  - 💀 posible smurf (WR alto + poco nivel de cuenta)
  - ⚠️ primera vez con el campeón
  - 🏆 duo/premade detectado
  - 📉 one-trick invertido (main de otro rol/champ)
- Énfasis visual en el equipo enemigo (borde/duotono rojo); tu equipo en tono neutro.
- Estados: cargando (skeleton), sin datos de un jugador ("sin datos"), partida terminada.
- El poll cada 5 s ya existe en `live.rs`; la vista solo consume la señal.

## 3. Necesidades de datos por pantalla (notas para implementación futura)

| Pantalla | Ya existe | Falta backend |
|---|---|---|
| Hub | catálogo champs, settings | persistencia de búsquedas recientes |
| Búsqueda perfiles | Riot API Match-V5, env key | Summoner-V4/Account-V1 (por Riot ID), league-v4 rank |
| Ficha perfil | perfil LCU completo | fetch Riot API para invocadores remotos |
| Ficha champ | casi todo (DD + Lolalytics + guías + JSONL) | unificar acceso + estado vacío de guía |
| LiveGameView | `LiveGame` (champs, hechizos, nivel) | enriquecimiento: puuids de la sala → rank/WR/KDA/maestría (Riot API con rate-limit consciente, caché por partida) |

## 4. Fases de prototipado (entregable actual)

| Fase | Prototipo | Contenido |
|---|---|---|
| A | `ui-preview-menu.html` | Hub: header + badges, banner alerta LCU, 2 search cards, accesos rápidos |
| B | `ui-preview-ficha.html` | Ficha de campeón: hero + 5 tabs con contenido de ejemplo (Ahri) |
| C | `ui-preview-live.html` | EN VIVO: 10 cards Porofessor con chips de aviso + estados |

Se prototipan en paralelo con los tokens de `design.md` y las recetas de marca de
`main.css` (esquinas cortadas, brackets, slashes, glow, Barlow Condensed + Inter).
NO se sobreescriben los prototipos aprobados existentes (`ui-preview.html`,
`ui-preview-sylas.html`).

## 5. Fases de implementación (futuras, tras aprobar prototipos)

1. **F13 · Navegación**: `AppView` nuevo, breadcrumbs, alertas LCU, Hub estático.
2. **F14 · Ficha de campeón**: búsqueda con autocomplete + vista con tabs reutilizando
   paneles existentes.
3. **F15 · Búsqueda de perfiles**: endpoints Riot API + adaptar `PerfilView` a
   `SummonerRef` + recientes.
4. **F16 · En vivo**: enriquecimiento Riot API de `LiveGame` + `LiveGameView`.
5. **F17 · Integración**: tests unitarios de derivación pura, `code-checker` por
   plataforma, `platform-tester` en macOS (target actual), pulido.

## 6. Riesgos / decisiones abiertas

- **Rate limit de Riot API** en la vista en vivo: 10 puuids × (summoner + rank +
  matches) por partida → caché por `gameId` y peticiones diferidas. Decidir en F16.
- **Guías curadas**: solo existen Akali y Annie. La ficha necesita política de estado
  vacío (definida en 2.3).
- **Fuentes de Google Fonts** en desktop: ya se cargan en `app.rs`, sin cambio.
- El sistema de pestañas del champ select no cambia; solo se envuelve en un shell
  con la tab EN VIVO al lado.
