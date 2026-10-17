---
name: Klify
colors:
  surface: '#10141d'
  surface-dim: '#070a0e'
  surface-bright: '#1a2230'
  surface-container-lowest: '#090c11'
  surface-container-low: '#121722'
  surface-container: '#1f2937'
  surface-container-high: '#2a3648'
  surface-container-highest: '#354358'
  surface-variant: '#171e2a'
  on-surface: '#e5e7eb'
  on-surface-variant: '#9aa4b2'
  inverse-surface: '#e5e7eb'
  inverse-on-surface: '#1f2937'
  outline: '#8b95a3'
  outline-variant: '#2f3a4a'
  surface-tint: '#ff0033'
  primary: '#ff0033'
  on-primary: '#ffffff'
  primary-container: '#440011'
  on-primary-container: '#ffd9de'
  inverse-primary: '#ff4d73'
  secondary: '#c3cad4'
  on-secondary: '#141a24'
  secondary-container: '#2c3848'
  on-secondary-container: '#e2e8f0'
  tertiary: '#ff5c7a'
  on-tertiary: '#2b000a'
  tertiary-container: '#4a0a1a'
  on-tertiary-container: '#ffd9e0'
  error: '#ffa198'
  on-error: '#5c1116'
  error-container: '#5c1116'
  on-error-container: '#ffdad9'
  primary-fixed: '#ff0033'
  primary-fixed-dim: '#cc0029'
  on-primary-fixed: '#ffffff'
  on-primary-fixed-variant: '#ffd9de'
  secondary-fixed: '#e2e8f0'
  secondary-fixed-dim: '#c3cad4'
  on-secondary-fixed: '#141a24'
  on-secondary-fixed-variant: '#2c3848'
  tertiary-fixed: '#ff4d6b'
  tertiary-fixed-dim: '#cc2a47'
  on-tertiary-fixed: '#2b000a'
  on-tertiary-fixed-variant: '#4a0a1a'
  background: '#0b0e14'
  on-background: '#e5e7eb'
typography:
  display-lg:
    fontFamily: Barlow Condensed
    fontSize: 64px
    fontWeight: '700'
    lineHeight: '1.05'
    letterSpacing: -0.01em
  headline-lg:
    fontFamily: Barlow Condensed
    fontSize: 48px
    fontWeight: '600'
    lineHeight: '1.1'
    letterSpacing: -0.005em
  headline-lg-mobile:
    fontFamily: Barlow Condensed
    fontSize: 32px
    fontWeight: '600'
    lineHeight: '1.15'
  headline-md:
    fontFamily: Barlow Condensed
    fontSize: 32px
    fontWeight: '500'
    lineHeight: '1.2'
  body-lg:
    fontFamily: Inter
    fontSize: 18px
    fontWeight: '400'
    lineHeight: '1.6'
  body-md:
    fontFamily: Inter
    fontSize: 16px
    fontWeight: '400'
    lineHeight: '1.6'
  label-md:
    fontFamily: Inter
    fontSize: 14px
    fontWeight: '600'
    lineHeight: '1.4'
    letterSpacing: 0.08em
rounded:
  sm: 0
  DEFAULT: 0.125rem
  md: 0.25rem
  lg: 0.5rem
  xl: 0.75rem
  full: 9999px
spacing:
  unit: 8px
  container-max: 1024px
  gutter: 24px
  margin-desktop: 48px
  margin-mobile: 20px
---

## Brand & Style

Klify es una herramienta de escritorio de estadísticas de League of Legends que opera
en tiempo real durante el champ select: detecta tu campeón, consulta millones de
partidas y recomienda las runas con mejor rendimiento para esa partida. El tagline de
la marca define su personalidad: **"Analiza · Mejora · Domina"** — Klify es precisa,
directa y competitiva. No decora: informa.

La identidad visual oficial (brand board) fija la paleta exacta: **`#FF0033`** (rojo
principal), **`#0B0E14`** (fondo), **`#1F2937`** (superficie) y **`#E5E7EB`** (texto),
con **Barlow Condensed** para títulos y **Inter** para textos/UI. Los neutros son
slate fríos (azul-acero), no cálidos: el contraste entre la frialdad del fondo y el
rojo eléctrico de la marca es el corazón del estilo. Elementos gráficos de marca:
**formas angulares, líneas de impacto y estilo gamer/competitivo** — bordes rectos,
esquinas cortadas, corner brackets y barras sesgadas.

Tres principios rectores. Primero, **legibilidad en menos de 2 segundos**: un jugador
tiene ~30 segundos para decidir su pick, la jerarquía es brutal y los datos van
primero. Segundo, **el rojo es escaso y eléctrico**: `#FF0033` aparece solo en lo que
exige acción o marca lo mejor; el resto es slate. Tercero, **duotono rojo-negro para
campeones**: las cards de campeón tratan el splash art en escala de grises lavado con
rojo y hundido en negro (referencia: card de Irelia), con el nombre en itálica
condensada gigante.

## Colors

El sistema se apoya en los cuatro colores de la marca y sus derivados:

- **`primary` `#ff0033`** — el rojo eléctrico oficial. CTAs, borde/glow del campeón
  activo, la página de runas recomendada #1, tab activa. `on-primary` blanco: el texto
  sobre rojo va siempre en ≥18px bold (umbral AA de texto grande). La familia
  `primary-container` `#440011` / `on-primary-container` `#ffd9de` cubre rellenos
  tenues; `primary-fixed-dim` `#cc0029` para estados pressed.
- **`background` `#0b0e14` y `surface` `#10141d`** — el negro azulado del entorno. La
  escalera `surface-container-lowest` → `surface-container-highest` sube por slate
  (`#121722` → `#354358`) hasta la superficie oficial `#1f2937` como nivel base de
  cards y paneles.
- **`on-surface` `#e5e7eb` / `on-surface-variant` `#9aa4b2`** — el gris claro oficial
  para texto principal y su secundario slate. Datos fríos (runas, partidas, shards) en
  `on-surface-variant`.
- **`secondary` `#c3cad4`** — plata slate para labels de apoyo y el tagline; bordes con
  énfasis en `outline` `#8b95a3` y divisores en `outline-variant` `#2f3a4a`.
- **`tertiary` `#ff5c7a`** — rojo rosado de la misma familia para estados de
  selección/hover alternativos; **no hay dorado en la paleta oficial**: los winrates
  destacados van en blanco (`on-surface`) o rojo (`primary-fixed`), nunca en un color
  fuera de la marca.
- **`error` `#ffa198`** — salmon de aviso (reconexión, fallo de fuentes), distinguible
  del rojo puro de marca: error informa, `primary` actúa.

Regla de oro: `#ff0033` en máximo 2-3 elementos por pantalla.

## Typography

- **Barlow Condensed** (Google Fonts) — la voz de Klify, coincidente con el lettering
  del brand board ("KLIFY", títulos/destacados). Condensada, con itálicas verdaderas;
  en mayúsculas para display, headlines, nombres de campeones y tabs. Pesos 600-700,
  `letter-spacing` levemente negativo.
- **Inter** (Google Fonts) — textos/UI según el board. Runas, winrates, contadores y
  explicaciones; números tabulares (`tabular-nums`) para que `53.08% WR · 60.675`
  no "baile".
- **Labels** (`label-md`) en Inter 600, mayúsculas y `letter-spacing: 0.08em`, el ritmo
  de "ANALIZA · MEJORA · DOMINA".
- Nunca mezclar roles: un winrate nunca va en Barlow, un título nunca va en Inter.
  `headline-lg-mobile` (32px) aplica a la ventana compacta.

## Layout & Spacing

App de escritorio contenida: `container-max` **1024px**, `gutter` 24px, márgenes 48px.
Todo el ritmo vertical sobre el **`unit` de 8px**. Grid de dos zonas verticales: equipo
(campeones) arriba, análisis del campeón en pestañas debajo. Excepción de marca:
**líneas de impacto** — barras y slashes diagonales (`skewX(-15°)`) en headers y
divisores, y **cortes de esquina** 14-18px en paneles y cards (una vez por zona,
nunca en campos de texto).

## Elevation & Depth

Profundidad en tres capas, sin sombras difusas genéricas:

1. **Escalera tonal slate**: `surface-container-low` → `surface-container` (la
   superficie oficial `#1f2937`) → `surface-container-high` según jerarquía.
2. **Bordes de 1px** en `outline-variant`; `outline` para foco. Corner brackets en L
   (2px, 16px) sobre el elemento activo — plata en reposo, `#ff0033` en destacados.
3. **Glow de marca**: borde + resplandor `primary-fixed` al 20-25% (blur 12-24px) solo
   en el elemento #1 de cada zona (mi campeón, página recomendada #1).

Overlays: `surface-dim` al 70% con blur 8px.

## Shapes

Angular total: `rounded.sm` = **0px** default para cards, botones e inputs; `DEFAULT`
(2px) y `md` (4px) para interactivos chicos; `lg` (8px) solo modals; `full` para chips.
**Formas angulares de marca**: esquinas cortadas con `clip-path`, paralelogramos para
badges (tier, MEJOR PICK), slashes decorativos de 2-3px como "líneas de impacto" cerca
del wordmark y headers de zona.

## Tratamiento de campeones (duotono rojo-negro)

Las cards de campeón (propio y aliados) tratan el splash art así:

1. Splash en escala de grises (`grayscale(1) contrast(1.1)`).
2. Capa de color `#ff0033` en `mix-blend-mode: multiply` al 30-45% (mi campeón) o
   15-25% (aliados, más blanco y negro; al hover suben color).
3. Degradado negro desde abajo (`rgba(11,14,20,0.95) → transparent`) ocupando el 55%
   inferior para asentar el nombre.
4. Nombre del campeón en Barlow Condensed itálica, mayúsculas, blanco `#e5e7eb`,
   posición debajo en `label-md` slate.

## Components

- **Primary Button** — fondo `primary`, texto `on-primary` (Barlow Condensed uppercase
  ≥18px bold), radius 0. Hover: `#ff1a4d` (tinte); pressed: `primary-fixed-dim`; focus:
  anillo 2px `outline`. Versión destacada con esquina cortada.
- **Secondary / Ghost Button** — borde 1px `outline`, texto `on-surface`. Hover: fondo
  `surface-container-high`. Disabled: borde `outline-variant`, texto al 50%.
- **Input Fields** — fondo `surface-container-lowest`, borde 1px `outline-variant`;
  focus: borde `primary` + glow 10%. Label `label-md` uppercase arriba.
- **Card de runas** — fondo `surface-container` `#1f2937`, borde `outline-variant`,
  radius 0, padding 16px. Fuente en `label-md` (`on-surface-variant`); winrate grande
  en Barlow Condensed italic blanco (destacado en `primary-fixed`); partidas en
  `on-surface-variant` tabular; keystone con anillo `primary` si es la destacada.
  **Card #1**: borde `primary` + glow + badge "MEJOR PICK" en paralelogramo
  `primary-container`.
- **Chips / Tags** — radius `full`, fondo `surface-container-high`, texto
  `on-surface-variant` en `label-md`; para rol ("MID") y fuente ("LOLALYTICS"). Activo:
  `primary-container` + `on-primary-container`.
- **Modal / Overlay** — `surface-dim` 70% blur 8px; panel `surface-container-high`,
  radius `lg`, título Barlow Condensed, acciones Primary + Ghost.
- **Estados de recomendaciones** — Loading: "Consultando estadísticas de runas…" en
  `on-surface-variant` con shimmer; Unavailable: "Recomendaciones no disponibles:
  {motivo}" en `error` con Ghost "Reintentar".
