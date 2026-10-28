//! Tab COUNTERS del champ select (FASE 11b-2b + unificación del board).
//!
//! Dos columnas 55/45 según el prototipo (ui-preview.html, panel
//! `#panel-counters`):
//!
//! - **Izquierda · tres bloques en orden**:
//!   1. "// VS Enemigos en vivo": una fila por casilla enemiga del
//!      [`MatchContext`] ([`enemy_rows`], sin cambios).
//!   2. "// ENFRENTAMIENTO · {rival} {lane}": board de matchups con la
//!      muestra DEL RIVAL ([`RivalCountersState`], la carga champ_select
//!      cuando se elige rival en la fila 5v5). Columnas "MEJORES CONTRA
//!      {rival}" (los que le ganan) y "PEORES CONTRA {rival}" (los que
//!      domina), con la perspectiva invertida de `vs_hero::rival_rows`:
//!      el `vs_wr` del payload es DEL RIVAL contra cada campeón, así que
//!      la fila muestra `100 − vs_wr`. Header derecho con el WR/PR
//!      globales del rival.
//!   3. "// MIS MATCHUPS · {mi champ}": MIS counters de siempre
//!      ([`CounterSummary::strong_against`] /
//!      [`CounterSummary::weak_against`]) en el MISMO diseño: "FUERTE
//!      CONTRA" (plata) / "DÉBIL CONTRA" (roja).
//!
//!   Los dos boards comparten el diseño aprobado del enfrentamiento:
//!   [`BoardRow`] (icono tile 32px + WR grande plata ≥ 50 / rojo < 50 +
//!   barra + nombre a la derecha con "{n} partidas") dentro de
//!   [`BoardColumn`] (título grande + subtítulo) y [`BoardSection`]
//!   (encabezado "// ..." + stats + dos columnas). Las vistas `*View`
//!   solo pintan valores ya resueltos.
//!
//! - **Derecha · "// Items vs esta comp"**: [`suggest_vs_comp`] sobre los
//!   enemigos LOCKEADOS (cruce honesto con la tabla
//!   [`crate::stats::source::items::ANTI_ENEMY_ITEMS`]); con
//!   match → filas con la razón de la tabla y badge "Sugerido vs comp" en
//!   la primera (caja con borde rojo + paralelogramo primary-container);
//!   sin match → nota honesta + BUILD PRINCIPAL con los 3 primeros del
//!   core.
//!
//! Los estados de análisis faltante son coherentes con las otras tabs:
//! `Loading` → spinner; sin counters/build → bloque "no disponible" con la
//! razón corta; el board del rival, ídem según su estado
//! ([`RivalBoardState`]). Toda la derivación de filas es pura y testeada
//! ([`enemy_rows`], [`board_row`], [`rival_board_rows`],
//! [`rival_board_state`], [`vs_comp_panel`]); el RSX solo pinta.

use std::sync::Arc;

use dioxus::prelude::*;

use crate::data_dragon::item::ItemCatalog;
use crate::data_dragon::models::ChampionCatalog;
use crate::lcu::models::{MatchContext, TeamMember};
use crate::stats::source::analysis::{
    analysis_counters, analysis_failure_reason, analysis_in_progress, analysis_items,
    AnalysisState,
};
use crate::stats::source::counters::{
    wr_vs, CounterEntry, CounterSummary, MIN_COUNTER_GAMES,
};
use crate::stats::source::items::{suggest_vs_comp, suggestion_reason, CompRules, ItemBuild, ItemStat};
use crate::ui::champ_select::position_label;
use crate::ui::champ_select::items_panel::{item_rows as build_item_rows, row_games, ItemRow};
use crate::ui::champ_select::vs_hero::{rival_lane_from_context, RivalCountersState};
use crate::ui::format::format_thousands;
use crate::ui::recommendations::format_winrate;

/// Umbral de la convención del prototipo: WR por debajo de 50% es matchup
/// desfavorable (número y barra en rojo de marca).
pub const UNFAVOURABLE_THRESHOLD: f64 = 50.0;

/// ¿Matchup desfavorable? (puro, testeado).
#[must_use]
pub fn is_unfavourable(winrate: f64) -> bool {
    winrate < UNFAVOURABLE_THRESHOLD
}

/// Identidad de un campeón para las filas (puro, testeado): nombre del
/// catálogo (`#id` si no figura) e icono cuadrado solo con versión
/// (nunca una URL con el componente de versión vacío).
#[must_use]
pub fn champ_identity(
    champion_id: i64,
    catalog: Option<&ChampionCatalog>,
    version: &str,
) -> (String, Option<String>) {
    match catalog.and_then(|catalog| catalog.get(champion_id)) {
        Some(entry) => {
            let icon = (!version.is_empty()).then(|| entry.icon_url(version));
            (entry.name.clone(), icon)
        }
        None => (format!("#{champion_id}"), None),
    }
}

/// Fila de un enemigo del champ select (pura, testeada).
#[derive(Clone, Debug, PartialEq)]
pub enum EnemyRow {
    /// Enemigo lockeado con matchup dentro de la muestra de counters.
    Locked {
        /// Id numérico del rival.
        champion_id: i64,
        /// Nombre visible (catálogo o `#id`).
        name: String,
        /// Icono cuadrado (`None` sin versión).
        icon: Option<String>,
        /// Etiqueta de posición (`""` sin rol).
        position: &'static str,
        /// WR de MI campeón contra el rival.
        vs_winrate: f64,
        /// Partidas del matchup.
        games: i64,
    },
    /// Enemigo lockeado pero el matchup no alcanzó el umbral de la fuente
    /// (o no aparece): la UI muestra "sin datos suficientes", sin ceros.
    NoData {
        /// Id numérico del rival.
        champion_id: i64,
        /// Nombre visible (catálogo o `#id`).
        name: String,
        /// Icono cuadrado (`None` sin versión).
        icon: Option<String>,
        /// Etiqueta de posición (`""` sin rol).
        position: &'static str,
    },
    /// Casilla sin lock: fila fantasma numerada que espera el lock.
    Pending {
        /// Número de casilla enemiga (1-based) para "? Enemy {n}".
        slot: usize,
        /// Etiqueta de posición (`""` sin rol).
        position: &'static str,
    },
}

/// Filas de "VS ENEMIGOS EN VIVO" (puro, testeado): cada casilla enemiga
/// se clasifica contra el [`CounterSummary`] del análisis (lockeado con
/// dato / lockeado sin dato / pendiente).
#[must_use]
pub fn enemy_rows(
    enemies: &[TeamMember],
    counters: &CounterSummary,
    catalog: Option<&ChampionCatalog>,
    version: &str,
) -> Vec<EnemyRow> {
    enemies
        .iter()
        .enumerate()
        .map(|(index, enemy)| {
            let position = position_label(enemy.position);
            match enemy.champion_id {
                None => EnemyRow::Pending {
                    slot: index + 1,
                    position,
                },
                Some(champion_id) => {
                    let (name, icon) = champ_identity(champion_id, catalog, version);
                    match wr_vs(&counters.counters, champion_id) {
                        Some(entry) => EnemyRow::Locked {
                            champion_id,
                            name,
                            icon,
                            position,
                            vs_winrate: entry.vs_winrate,
                            games: entry.games,
                        },
                        None => EnemyRow::NoData {
                            champion_id,
                            name,
                            icon,
                            position,
                        },
                    }
                }
            }
        })
        .collect()
}

/// Tope de filas por columna del board DEL RIVAL: el tab ya muestra la
/// lista de enemigos viva arriba, así que queda compacto.
const RIVAL_BOARD_ROWS: usize = 5;

/// Tope de filas por columna de MIS matchups (igual que antes).
const MY_BOARD_ROWS: usize = 3;

/// Fila del board unificado de matchups (pura, testeada): icono + WR
/// grande + barra + nombre + partidas. `win_pct` ya viene desde la
/// perspectiva correcta de la fila (MI winrate contra el rival, o el del
/// campeón de la fila contra el rival en el board del rival).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct BoardRow {
    /// Id numérico del campeón de la fila.
    pub champion_id: i64,
    /// Nombre visible (catálogo o `#id`).
    pub name: String,
    /// Icono cuadrado (`None` sin versión).
    pub icon: Option<String>,
    /// Winrate mostrado (plata ≥ 50 / rojo < 50).
    pub win_pct: f64,
    /// Partidas de la muestra del matchup.
    pub games: i64,
}

/// Columna del board (pura, testeada): título grande + subtítulo + filas.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct BoardColumn {
    /// Título grande ("MEJORES CONTRA Jinx").
    pub title: String,
    /// Subtítulo 11px en mayúsculas ("GANAN AL RIVAL SEGÚN LA MUESTRA").
    pub subtitle: &'static str,
    /// Filas de la columna.
    pub rows: Vec<BoardRow>,
    /// `true` → columna "buena" (título en plata); `false` → roja.
    pub positive: bool,
}

/// Sección del board (pura, testeada): encabezado "// TÍTULO" con stats
/// opcionales a la derecha + las dos columnas de matchups.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct BoardSection {
    /// Título de la sección sin el "//" (lo aporta el `sl` del head):
    /// "ENFRENTAMIENTO · Jinx MID".
    pub title: String,
    /// Stats del header derecho ("51.2% WR · 4.1% PR · del rival").
    pub stats: Option<String>,
    /// Columnas del board (2 en el diseño).
    pub columns: Vec<BoardColumn>,
}

/// Qué sección del board se arma (define títulos y subtítulos).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BoardKind {
    /// Counters DEL rival (perspectiva invertida, columnas nombradas).
    Rival,
    /// MIS matchups (perspectiva propia, columnas genéricas).
    Mine,
}

/// Arma una fila del board desde un `CounterEntry` de MI muestra (puro,
/// testeado): el WR es el de la entrada (mi winrate contra ese rival),
/// sin inversión.
#[must_use]
pub(crate) fn board_row(
    entry: &CounterEntry,
    catalog: Option<&ChampionCatalog>,
    version: &str,
) -> BoardRow {
    let (name, icon) = champ_identity(entry.champion_id, catalog, version);
    BoardRow {
        champion_id: entry.champion_id,
        name,
        icon,
        win_pct: entry.vs_winrate,
        games: entry.games,
    }
}

/// Filas del board DEL RIVAL (puro, testeado), con la perspectiva
/// invertida de `vs_hero::rival_rows`: el `vs_wr` de la muestra es DEL
/// RIVAL contra cada campeón, así que el winrate DE CADA CAMPEÓN contra
/// el rival es `100 − vs_wr`. `best = true` → los que le ganan (su
/// `vs_wr` es más bajo). Filtra muestra corta ([`MIN_COUNTER_GAMES`]),
/// ordena y recorta a `n`.
#[must_use]
pub(crate) fn rival_board_rows(
    summary: &CounterSummary,
    catalog: Option<&ChampionCatalog>,
    version: &str,
    best: bool,
    n: usize,
) -> Vec<BoardRow> {
    let mut entries: Vec<&CounterEntry> = summary
        .counters
        .iter()
        .filter(|entry| entry.games >= MIN_COUNTER_GAMES)
        .collect();
    entries.sort_by(|a, b| {
        if best {
            a.vs_winrate
                .partial_cmp(&b.vs_winrate)
                .unwrap_or(std::cmp::Ordering::Equal)
        } else {
            b.vs_winrate
                .partial_cmp(&a.vs_winrate)
                .unwrap_or(std::cmp::Ordering::Equal)
        }
    });

    entries
        .into_iter()
        .take(n)
        .map(|entry| {
            let mut row = board_row(entry, catalog, version);
            row.win_pct = 100.0 - entry.vs_winrate;
            row
        })
        .collect()
}

/// Encabezados de las columnas del board según la sección (puro,
/// testeado): `((título, subtítulo) positiva, (título, subtítulo)
/// negativa)`. `subject` solo nombra las columnas del rival.
#[must_use]
pub(crate) fn board_column_headers(
    section: BoardKind,
    subject: &str,
) -> ((String, &'static str), (String, &'static str)) {
    match section {
        BoardKind::Rival => (
            (
                format!("MEJORES CONTRA {subject}"),
                "GANAN AL RIVAL SEGÚN LA MUESTRA",
            ),
            (
                format!("PEORES CONTRA {subject}"),
                "EL RIVAL LOS DOMINA",
            ),
        ),
        BoardKind::Mine => (
            (
                String::from("FUERTE CONTRA"),
                "TU MEJOR WR DE LA MUESTRA",
            ),
            (
                String::from("DÉBIL CONTRA"),
                "TU PEOR WR DE LA MUESTRA",
            ),
        ),
    }
}

/// Columnas del board DEL RIVAL (puro, testeado): "MEJORES CONTRA
/// {rival}" (plata) y "PEORES CONTRA {rival}" (roja) desde SU muestra.
#[must_use]
pub(crate) fn rival_board_columns(
    summary: &CounterSummary,
    rival_name: &str,
    catalog: Option<&ChampionCatalog>,
    version: &str,
    n: usize,
) -> Vec<BoardColumn> {
    let ((best_title, best_subtitle), (worst_title, worst_subtitle)) =
        board_column_headers(BoardKind::Rival, rival_name);
    vec![
        BoardColumn {
            title: best_title,
            subtitle: best_subtitle,
            rows: rival_board_rows(summary, catalog, version, true, n),
            positive: true,
        },
        BoardColumn {
            title: worst_title,
            subtitle: worst_subtitle,
            rows: rival_board_rows(summary, catalog, version, false, n),
            positive: false,
        },
    ]
}

/// Columnas de MIS matchups (puro, testeado): "FUERTE CONTRA" (plata,
/// prefijo ya ordenado de [`CounterSummary::strong_against`]) y "DÉBIL
/// CONTRA" (roja, cola invertida de [`CounterSummary::weak_against`]).
#[must_use]
pub(crate) fn my_board_columns(
    summary: &CounterSummary,
    catalog: Option<&ChampionCatalog>,
    version: &str,
    n: usize,
) -> Vec<BoardColumn> {
    let ((strong_title, strong_subtitle), (weak_title, weak_subtitle)) =
        board_column_headers(BoardKind::Mine, "");
    vec![
        BoardColumn {
            title: strong_title,
            subtitle: strong_subtitle,
            rows: summary
                .strong_against(n)
                .iter()
                .map(|entry| board_row(entry, catalog, version))
                .collect(),
            positive: true,
        },
        BoardColumn {
            title: weak_title,
            subtitle: weak_subtitle,
            rows: summary
                .weak_against(n)
                .iter()
                .map(|entry| board_row(entry, catalog, version))
                .collect(),
            positive: false,
        },
    ]
}

/// Título de una sección del board (puro, testeado): "ENFRENTAMIENTO ·
/// Jinx MID"; sin lane resuelta no inventa posición (el "//" lo pone el
/// `sl` del section-head).
#[must_use]
pub(crate) fn board_section_title(kind: &str, subject: &str, lane: &str) -> String {
    if lane.is_empty() {
        format!("{kind} · {subject}")
    } else {
        format!("{kind} · {subject} {lane}")
    }
}

/// Stats del header derecho del board del rival (puro, testeado):
/// "51.2% WR · 4.1% PR · del rival". El segmento sin dato se omite
/// (honradez: nunca un 0 inventado); sin ningún dato → `None`.
#[must_use]
pub(crate) fn rival_stats_text(wr: Option<f64>, pr: Option<f64>) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    if let Some(wr) = wr {
        parts.push(format!("{}% WR", format_winrate(wr)));
    }
    if let Some(pr) = pr {
        parts.push(format!("{}% PR", format_winrate(pr)));
    }
    if parts.is_empty() {
        None
    } else {
        parts.push(String::from("del rival"));
        Some(parts.join(" · "))
    }
}

/// Vista del bloque "// ENFRENTAMIENTO" (pura, testeada): resuelve el
/// estado de [`RivalCountersState`] contra el rival vigente.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum RivalBoardState {
    /// Counters del rival listos: board armado con stats en el header.
    Ready {
        /// Sección completa (título + stats + columnas).
        section: BoardSection,
    },
    /// Cargando los counters del rival vigente.
    Loading,
    /// Falló la carga del rival vigente (o el estado es de OTRO rival en
    /// transición): mensaje honesto, nunca un board de otro rival.
    Failed,
    /// Ready pero sin filas: la muestra del rival no alcanzó
    /// [`MIN_COUNTER_GAMES`] en esta lane.
    Empty,
    /// Sin rival elegido: el bloque no se renderiza.
    Idle,
}

/// Resuelve el board del rival (puro, testeado) contra el rival pedido:
/// solo el estado `Ready` del MISMO rival arma la sección.
#[must_use]
pub(crate) fn rival_board_state(
    rival_id: Option<i64>,
    state: &RivalCountersState,
    rival_name: &str,
    lane: &str,
    catalog: Option<&ChampionCatalog>,
    version: &str,
    n: usize,
) -> RivalBoardState {
    let Some(rival_id) = rival_id else {
        return RivalBoardState::Idle;
    };
    match state {
        RivalCountersState::Ready { rival_id: current, summary } if *current == rival_id => {
            let columns = rival_board_columns(summary, rival_name, catalog, version, n);
            if columns.iter().any(|column| !column.rows.is_empty()) {
                RivalBoardState::Ready {
                    section: BoardSection {
                        title: board_section_title("ENFRENTAMIENTO", rival_name, lane),
                        stats: rival_stats_text(summary.my_wr, summary.pickrate),
                        columns,
                    },
                }
            } else {
                RivalBoardState::Empty
            }
        }
        RivalCountersState::Loading { rival_id: current } if *current == rival_id => {
            RivalBoardState::Loading
        }
        // Failed del vigente, estado de otro rival (transición de lock) o
        // Idle con rival elegido: mensaje honesto.
        _ => RivalBoardState::Failed,
    }
}

/// Estado de la columna derecha "ITEMS VS ESTA COMP" (puro, testeado).
#[derive(Clone, Debug, PartialEq)]
pub enum VsCompPanel {
    /// El cruce con la tabla anti-enemigo matcheó: filas sugeridas (la
    /// primera lleva el badge "Sugerido vs comp"); cada `desc` es la razón
    /// de la tabla para los enemigos lockeados vigentes.
    Suggestions(Vec<ItemRow>),
    /// Sin match (o sin enemigos lockeados): BUILD PRINCIPAL con los
    /// primeros 3 del core más una nota honesta; `no_enemies` distingue la
    /// nota "lockea un enemigo" de la de "sin sugerencias específicas".
    Fallback {
        /// Core recortado a 3.
        core: Vec<ItemRow>,
        /// `true` si no había ningún enemigo lockeado.
        no_enemies: bool,
    },
}

/// Deriva el panel "vs esta comp" (puro, testeado): sugerencias del cruce
/// contra los enemigos lockeados con las reglas por defecto; la
/// descripción de cada sugerencia es la razón de la tabla (con fallback al
/// blurb del catálogo). Sin match → core recortado + nota.
#[must_use]
pub fn vs_comp_panel(
    build: &ItemBuild,
    enemies: &[TeamMember],
    items_catalog: Option<&ItemCatalog>,
    version: &str,
) -> VsCompPanel {
    let locked: Vec<i64> = enemies
        .iter()
        .filter_map(|enemy| enemy.champion_id)
        .collect();
    let no_enemies = locked.is_empty();

    let suggestions = suggest_vs_comp(build, &locked, &CompRules::default());
    if !suggestions.is_empty() {
        let mut rows = build_item_rows(&suggestions, items_catalog, version);
        for row in &mut rows {
            if let Some(reason) = suggestion_reason(row.item_id, &locked) {
                row.desc = String::from(reason);
            }
        }
        return VsCompPanel::Suggestions(rows);
    }

    let core: Vec<ItemStat> = build.core.iter().take(3).copied().collect();
    VsCompPanel::Fallback {
        core: build_item_rows(&core, items_catalog, version),
        no_enemies,
    }
}

/// Razón corta para los bloques "no disponible" (sin análisis o fallo).
fn reason_text(loading: bool, failure: Option<String>) -> Option<String> {
    (!loading).then(|| failure.unwrap_or_else(|| String::from("análisis no disponible")))
}

/// Tab COUNTERS. Re-renderiza cuando el análisis, el contexto o los
/// counters del rival publican (locks de enemigos en vivo y elección de
/// rival en la fila 5v5). Toda la derivación ocurre ANTES del rsx: el
/// bloque RSX solo pinta valores ya resueltos.
#[component]
pub(crate) fn CountersPanel(
    analysis: ReadSignal<AnalysisState>,
    champion_id: i64,
    match_context: ReadSignal<MatchContext>,
    catalog: ReadSignal<Option<ChampionCatalog>>,
    items: ReadSignal<Option<Arc<ItemCatalog>>>,
    rival: Signal<Option<i64>>,
    rival_counters: ReadSignal<RivalCountersState>,
    version: Option<String>,
) -> Element {
    // Snapshots cortos: los guards no cruzan la construcción del rsx.
    let snapshot: AnalysisState = analysis.read().clone();
    let counters = analysis_counters(&snapshot, champion_id);
    let build = analysis_items(&snapshot, champion_id);
    let loading = analysis_in_progress(&snapshot, champion_id);
    let failure = analysis_failure_reason(&snapshot, champion_id);

    let context: MatchContext = match_context.read().clone();
    let champ_catalog = catalog.read().clone();
    // Versión del caller; sin dato cae a la del catálogo (nunca una URL
    // con el componente de versión vacío).
    let version = version
        .filter(|value| !value.is_empty())
        .or_else(|| champ_catalog.as_ref().map(|data| data.version.clone()))
        .unwrap_or_default();
    let items_catalog = items.read().clone();

    let (my_name, _) = champ_identity(champion_id, champ_catalog.as_ref(), &version);

    // Rival elegido en la fila 5v5 (solo lectura) + estado de sus counters.
    let rival_id: Option<i64> = rival();
    let rival_state: RivalCountersState = rival_counters.read().clone();
    let rival_name = rival_id
        .and_then(|id| champ_catalog.as_ref().and_then(|data| data.get(id)))
        .map(|entry| entry.name.clone())
        .or_else(|| rival_id.map(|id| format!("#{id}")))
        .unwrap_or_default();
    let rival_lane = position_label(rival_lane_from_context(&context, rival_id, None));

    // Derivación pura (antes del rsx).
    let reason = reason_text(loading, failure).unwrap_or_default();
    let enemy_view: Option<Vec<EnemyRow>> = counters
        .as_deref()
        .map(|summary| enemy_rows(&context.enemies, summary, champ_catalog.as_ref(), &version));
    let rival_view = rival_board_state(
        rival_id,
        &rival_state,
        &rival_name,
        rival_lane,
        champ_catalog.as_ref(),
        &version,
        RIVAL_BOARD_ROWS,
    );
    // Board de MIS matchups (solo si alguna columna trae filas).
    let my_board: Option<BoardSection> = counters
        .as_deref()
        .map(|summary| BoardSection {
            title: board_section_title("MIS MATCHUPS", &my_name, ""),
            stats: None,
            columns: my_board_columns(summary, champ_catalog.as_ref(), &version, MY_BOARD_ROWS),
        })
        .filter(|section| section.columns.iter().any(|column| !column.rows.is_empty()));
    let vs_comp: Option<VsCompPanel> = build.as_deref().map(|build| {
        vs_comp_panel(build, &context.enemies, items_catalog.as_deref(), &version)
    });
    let fallback_note = match &vs_comp {
        Some(VsCompPanel::Fallback { no_enemies: true, .. }) => {
            "Lockea un enemigo para sugerencias"
        }
        Some(VsCompPanel::Fallback { .. }) => "Sin sugerencias específicas contra esta comp",
        _ => "",
    };

    rsx! {
        div { class: "cs-counters",

            // COLUMNA IZQUIERDA (55%) · VS ENEMIGOS EN VIVO + BOARDS
            div { class: "cs-counters-left",
                div { class: "section-head",
                    span { class: "bar-skew", "style": "width: 24px;" }
                    span { class: "label label-md",
                        span { class: "sl", "//" }
                        " VS Enemigos en vivo"
                    }
                }
                p { class: "cs-hint cs-hint--gap",
                    "El WR es de tu campeón contra cada rival (Lolalytics, muestra del parche)."
                }

                {match enemy_view {
                    Some(rows) if !rows.is_empty() => rsx! {
                        div { class: "cs-enemy-list",
                            {rows.into_iter().map(|row| {
                                let row_key = row_key(&row);
                                rsx! { EnemyLine { row, key: "{row_key}" } }
                            })}
                        }
                    },
                    Some(_) => rsx! {
                        p { class: "cs-hint cs-hint--gap",
                            "La cola todavía no reporta casillas enemigas."
                        }
                    },
                    None if loading => rsx! {
                        div { class: "cs-loading cs-hint--gap",
                            div { class: "spinner" }
                            span { class: "label label-md pulse-soft t-silver",
                                "Consultando counters…"
                            }
                        }
                    },
                    None => rsx! {
                        div { class: "cs-unavailable cs-hint--gap",
                            span { class: "label label-md t-error", "Counters no disponibles" }
                            p { class: "cs-unavailable-reason", "{reason}" }
                        }
                    },
                }}

                // ENFRENTAMIENTO · {rival} {lane}: board con la muestra
                // DEL RIVAL (misma grilla que "mis matchups").
                {match rival_view {
                    RivalBoardState::Ready { section } => {
                        let key = "board-rival";
                        rsx! { BoardSectionView { section, key: "{key}" } }
                    }
                    RivalBoardState::Loading => rsx! {
                        div { class: "cs-loading cs-hint--gap",
                            div { class: "spinner" }
                            span { class: "label label-md pulse-soft t-silver",
                                "Consultando matchups del rival…"
                            }
                        }
                    },
                    RivalBoardState::Empty => rsx! {
                        p { class: "cs-hint cs-hint--gap",
                            "Sin muestra suficiente para los matchups de este rival en esta lane."
                        }
                    },
                    RivalBoardState::Failed => rsx! {
                        p { class: "cs-hint cs-hint--gap",
                            "Matchups del rival no disponibles ahora."
                        }
                    },
                    RivalBoardState::Idle => rsx! {},
                }}

                // MIS MATCHUPS · {mi champ}: mismos datos de siempre,
                // ahora en el diseño board unificado.
                {match my_board {
                    Some(section) => {
                        let key = "board-my";
                        rsx! { BoardSectionView { section, key: "{key}" } }
                    }
                    None => rsx! {},
                }}
            }

            // COLUMNA DERECHA (45%) · ITEMS VS ESTA COMP
            div { class: "cs-counters-right",
                div { class: "section-head",
                    span { class: "bar-skew", "style": "width: 24px;" }
                    span { class: "label label-md",
                        span { class: "sl", "//" }
                        " Items vs esta comp"
                    }
                }

                {match vs_comp {
                    Some(VsCompPanel::Suggestions(rows)) => rsx! {
                        {rows.into_iter().enumerate().map(|(index, row)| {
                            let item_key = row.item_id;
                            if index == 0 {
                                rsx! {
                                    div { class: "cs-vscomp-suggested", key: "suggested-{item_key}",
                                        span { class: "cs-vscomp-tag", "Sugerido vs comp" }
                                        VsItemLine { row }
                                    }
                                }
                            } else {
                                rsx! { VsItemLine { row, key: "vsitem-{item_key}" } }
                            }
                        })}
                    },
                    Some(VsCompPanel::Fallback { core, .. }) => rsx! {
                        p { class: "cs-vscomp-note cs-hint--gap", "{fallback_note}" }
                        BuildPrincipal { core }
                    },
                    None if loading => rsx! {
                        div { class: "cs-loading cs-hint--gap",
                            div { class: "spinner" }
                            span { class: "label label-md pulse-soft t-silver",
                                "Consultando build…"
                            }
                        }
                    },
                    None => rsx! {
                        div { class: "cs-unavailable cs-hint--gap",
                            span { class: "label label-md t-error", "Build no disponible" }
                            p { class: "cs-unavailable-reason", "{reason}" }
                        }
                    },
                }}

                span { class: "label fs-10 t-outline-var source-note",
                    "Se recalcula cuando lockea un enemigo"
                }
            }
        }
    }
}

/// Clave estable de una fila de enemigo para el reconciliador de Dioxus.
fn row_key(row: &EnemyRow) -> String {
    match row {
        EnemyRow::Locked { champion_id, .. } | EnemyRow::NoData { champion_id, .. } => {
            format!("locked-{champion_id}")
        }
        EnemyRow::Pending { slot, .. } => format!("pending-{slot}"),
    }
}

/// Tile de icono genérico (champ o ítem): imagen o fallback `#` chico
/// (nunca un `?` gigante recortado en tiles de 32-48px).
#[component]
fn IconTile(name: String, icon: Option<String>, size: &'static str) -> Element {
    rsx! {
        {match icon {
            Some(icon) => rsx! {
                div { class: "tile", "style": "width: {size}; height: {size};",
                    img { src: "{icon}", alt: "{name}" }
                }
            },
            None => rsx! {
                div { class: "tile", "style": "width: {size}; height: {size};",
                    span { class: "cs-tile-fallback", "#" }
                }
            },
        }}
    }
}

/// Una fila de enemigo del panel izquierdo en sus tres estados.
#[component]
fn EnemyLine(row: EnemyRow) -> Element {
    match row {
        EnemyRow::Locked {
            name,
            icon,
            position,
            vs_winrate,
            games,
            ..
        } => {
            let fill_width = vs_winrate.clamp(0.0, 100.0);
            let fill_class = if is_unfavourable(vs_winrate) {
                "fill fill--primary"
            } else {
                "fill"
            };
            let wr_class = if is_unfavourable(vs_winrate) {
                "cs-enemy-wr t-bright"
            } else {
                "cs-enemy-wr t-on"
            };
            rsx! {
                div { class: "cs-enemy-row",
                    IconTile { name: name.clone(), icon, size: "48px" }
                    div { class: "cs-enemy-mid",
                        div { class: "cs-enemy-titlerow",
                            span { class: "cs-enemy-name", "{name}" }
                            if !position.is_empty() {
                                span { class: "label fs-10 t-dim", "{position}" }
                            }
                        }
                        div { class: "cs-enemy-trackrow",
                            div { class: "track",
                                div { class: "{fill_class}",
                                    "style": "width: {fill_width}%;",
                                    "aria-hidden": "true"
                                }
                            }
                            span { class: "cs-enemy-games tnum",
                                "{format_thousands(games)} partidas"
                            }
                        }
                    }
                    div { class: "cs-enemy-wrcol",
                        div { class: "num-display tnum {wr_class}",
                            "{format_winrate(vs_winrate)}%"
                        }
                        span { class: "label fs-10 t-dim", "WR" }
                    }
                }
            }
        }
        EnemyRow::NoData {
            name,
            icon,
            position,
            ..
        } => rsx! {
            div { class: "cs-enemy-row",
                IconTile { name: name.clone(), icon, size: "48px" }
                div { class: "cs-enemy-mid",
                    div { class: "cs-enemy-titlerow",
                        span { class: "cs-enemy-name", "{name}" }
                        if !position.is_empty() {
                            span { class: "label fs-10 t-dim", "{position}" }
                        }
                    }
                }
                span { class: "cs-enemy-nodata", "Sin datos suficientes" }
            }
        },
        EnemyRow::Pending { slot, position } => rsx! {
            div { class: "cs-enemy-row cs-enemy-row--pending",
                div { class: "cs-pending-tile", "?" }
                div { class: "cs-enemy-mid",
                    div { class: "cs-enemy-titlerow",
                        span { class: "cs-enemy-name cs-enemy-name--dim", "? Enemy {slot}" }
                        if !position.is_empty() {
                            span { class: "label fs-10 cs-enemy-name--dim", "{position}" }
                        }
                    }
                }
                span { class: "cs-enemy-nodata", "Se muestra al lockear" }
            }
        },
    }
}

/// Fila unificada del board: icono tile 32px + WR grande (plata ≥ 50 /
/// rojo < 50, Barlow italic vía `vs-mu-wr`) + barra + nombre a la derecha
/// con "{n} partidas" debajo. El color sigue al VALOR del WR (no a la
/// columna), como en la captura aprobada.
#[component]
fn BoardRowView(row: BoardRow) -> Element {
    let fill_width = row.win_pct.clamp(0.0, 100.0);
    let fill_class = if is_unfavourable(row.win_pct) {
        "fill fill--primary"
    } else {
        "fill"
    };
    let wr_class = if is_unfavourable(row.win_pct) {
        "t-bright"
    } else {
        "t-on"
    };
    let games = format_thousands(row.games.max(0));
    let name = row.name.clone();
    rsx! {
        div { class: "vs-mu-row cmb-row",
            IconTile { name: row.name.clone(), icon: row.icon, size: "32px" }
            div { class: "vs-mu-mid",
                div { class: "vs-mu-wrline",
                    span { class: "num-display tnum vs-mu-wr {wr_class}",
                        "{format_winrate(row.win_pct)}%"
                    }
                }
                div { class: "track",
                    div { class: "{fill_class}",
                        "style": "width: {fill_width}%;",
                        "aria-hidden": "true"
                    }
                }
            }
            div { class: "vs-mu-right",
                span { class: "vs-mu-name", "{name}" }
                span { class: "vs-mu-games tnum", "{games} partidas" }
            }
        }
    }
}

/// Columna del board: título grande (plata/roja según `positive`) +
/// subtítulo 11px + filas unificadas.
#[component]
fn BoardColumnView(column: BoardColumn) -> Element {
    let title = column.title;
    let subtitle = column.subtitle;
    let title_class = if column.positive {
        "vs-mu-title vs-mu-title--good"
    } else {
        "vs-mu-title vs-mu-title--bad"
    };
    let rows = column.rows;
    rsx! {
        div { class: "vs-mu-col",
            div { class: "vs-mu-head",
                span { class: "{title_class}", "{title}" }
                span { class: "label fs-10 t-dim", "{subtitle}" }
            }
            {rows.into_iter().map(|row| {
                let row_key = row.champion_id;
                rsx! { BoardRowView { row, key: "board-{row_key}" } }
            })}
        }
    }
}

/// Sección del board: encabezado "// {título}" (con stats opcionales a la
/// derecha, estilo `vs-mu-stats` del hero) + las dos columnas.
#[component]
fn BoardSectionView(section: BoardSection) -> Element {
    let title = section.title;
    let stats = section.stats;
    let columns = section.columns;
    rsx! {
        div { class: "cmb-board",
            div { class: "section-head",
                span { class: "bar-skew", "style": "width: 24px;" }
                span { class: "label label-md",
                    span { class: "sl", "//" }
                    " {title}"
                }
                if let Some(stats) = stats {
                    span { class: "vs-mu-stats tnum", "{stats}" }
                }
            }
            div { class: "vs-mu-cols",
                {columns.into_iter().map(|column| {
                    let col_key = column.title.clone();
                    rsx! { BoardColumnView { column, key: "{col_key}" } }
                })}
            }
        }
    }
}

/// Fila de ítem sugerido (o del build principal) de la columna derecha.
#[component]
fn VsItemLine(row: ItemRow) -> Element {
    let games = row_games(row.games);
    rsx! {
        div { class: "cs-vsitem",
            IconTile { name: row.name.clone(), icon: row.icon.clone(), size: "48px" }
            div { class: "cs-vsitem-text",
                div { class: "cs-vsitem-name", "{row.name}" }
                if !row.desc.is_empty() {
                    div { class: "cs-vsitem-desc", "{row.desc}" }
                }
            }
            div { class: "cs-vsitem-wrcol",
                div { class: "num-display tnum cs-vsitem-wr t-on",
                    "{format_winrate(row.winrate)}%"
                }
                div { class: "tnum cs-vsitem-games", "{games} partidas" }
            }
        }
    }
}

/// BUILD PRINCIPAL: los primeros ítems del core cuando no hay sugerencias
/// contra la comp (fallback honesto del prototipo).
#[component]
fn BuildPrincipal(core: Vec<ItemRow>) -> Element {
    rsx! {
        div { class: "cs-buildprincipal",
            span { class: "label fs-10 t-dim", "Build principal" }
            {core.into_iter().map(|row| {
                let item_key = row.item_id;
                rsx! { VsItemLine { row, key: "core-{item_key}" } }
            })}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Catálogo de campeones mínimo (Zed 238, Nautilus 111, Ryze 34).
    fn catalogo() -> ChampionCatalog {
        let mut by_id = HashMap::new();
        for (id, name, ddragon) in [
            (238, "Zed", "Zed"),
            (111, "Nautilus", "Nautilus"),
            (34, "Ryze", "Ryze"),
        ] {
            by_id.insert(
                id,
                crate::data_dragon::models::ChampionEntry {
                    name: String::from(name),
                    image_url: format!("https://ddragon.leagueoflegends.com/cdn/x/{ddragon}.png"),
                    slug: String::from(name),
                    ddragon_id: String::from(ddragon),
                },
            );
        }
        ChampionCatalog {
            version: String::from("16.20.1"),
            by_id,
        }
    }

    /// Counters con tres matchups ordenados: 106 (62.76) > 111 (52.7) >
    /// 238 (45.2) > 34 (43.81). El fixture respeta el orden del parser.
    fn counters() -> CounterSummary {
        CounterSummary {
            my_wr: Some(52.02),
            pickrate: Some(4.15),
            analysed: Some(34_745_768),
            counters: vec![
                CounterEntry {
                    champion_id: 106,
                    vs_winrate: 62.76,
                    games: 333,
                },
                CounterEntry {
                    champion_id: 111,
                    vs_winrate: 52.7,
                    games: 88_400,
                },
                CounterEntry {
                    champion_id: 238,
                    vs_winrate: 45.2,
                    games: 142_900,
                },
                CounterEntry {
                    champion_id: 34,
                    vs_winrate: 43.81,
                    games: 210,
                },
            ],
        }
    }

    fn miembro(cell: i64, champion_id: Option<i64>, position: Option<crate::lcu::models::Position>) -> TeamMember {
        TeamMember {
            cell_id: cell,
            champion_id,
            position,
            summoner_id: None,
            summoner_name: None,
        }
    }

    /// Convención de color: <50% desfavorable, ≥50% favorable.
    #[test]
    fn is_unfavourable_usa_el_umbral_de_50() {
        assert!(is_unfavourable(45.2));
        assert!(is_unfavourable(49.99));
        assert!(!is_unfavourable(50.0));
        assert!(!is_unfavourable(52.7));
    }

    /// Identidad de champ: nombre + icono con versión; sin catálogo →
    /// `#id`; sin versión → sin icono.
    #[test]
    fn champ_identity_resuelve_contra_el_catalogo() {
        let catalog = catalogo();
        let (name, icon) = champ_identity(238, Some(&catalog), "16.20.1");
        assert_eq!(name, "Zed");
        assert_eq!(
            icon.as_deref(),
            Some("https://ddragon.leagueoflegends.com/cdn/16.20.1/img/champion/Zed.png")
        );

        let (sin_nombre, sin_icono) = champ_identity(999, Some(&catalog), "16.20.1");
        assert_eq!(sin_nombre, "#999");
        assert!(
            sin_icono.is_none(),
            "sin entrada no hay ddragon_id para armar la URL del icono"
        );

        let (_, sin_version) = champ_identity(238, Some(&catalog), "");
        assert_eq!(sin_version, None);

        let (sin_catalogo, _) = champ_identity(238, None, "16.20.1");
        assert_eq!(sin_catalogo, "#238");
    }

    /// Filas de enemigos por estado: lockeado con dato, lockeado sin dato
    /// (matchup fuera del umbral de la fuente) y pendiente de lock.
    #[test]
    fn enemy_rows_clasifica_locked_nodata_y_pending() {
        let summary = counters();
        let enemies = vec![
            miembro(5, Some(238), Some(crate::lcu::models::Position::Middle)),
            miembro(6, Some(999), Some(crate::lcu::models::Position::Utility)),
            miembro(7, None, Some(crate::lcu::models::Position::Jungle)),
        ];

        let rows = enemy_rows(&enemies, &summary, Some(&catalogo()), "16.20.1");

        assert_eq!(rows.len(), 3);
        // Lockeado CON matchup: Zed 45.2% desfavorable, 142.900 juegos.
        match &rows[0] {
            EnemyRow::Locked {
                champion_id,
                name,
                icon,
                position,
                vs_winrate,
                games,
            } => {
                assert_eq!(*champion_id, 238);
                assert_eq!(name, "Zed");
                assert!(icon.is_some());
                assert_eq!(*position, "MID");
                assert!((vs_winrate - 45.2).abs() < f64::EPSILON);
                assert_eq!(*games, 142_900);
            }
            other => panic!("esperaba Locked, hubo {other:?}"),
        }
        // Lockeado SIN matchup (id fuera de la muestra): NoData.
        match &rows[1] {
            EnemyRow::NoData {
                champion_id, name, position, ..
            } => {
                assert_eq!(*champion_id, 999);
                assert_eq!(name, "#999");
                assert_eq!(*position, "SUP");
            }
            other => panic!("esperaba NoData, hubo {other:?}"),
        }
        // Sin lock: pendiente numerado con su posición.
        assert_eq!(
            rows[2],
            EnemyRow::Pending {
                slot: 3,
                position: "JGL",
            }
        );

        // Lista enemiga vacía → sin filas.
        assert!(enemy_rows(&[], &summary, Some(&catalogo()), "16.20.1").is_empty());
    }

    /// Board unificado: fila armada desde un `CounterEntry` de MI muestra
    /// (WR sin invertir) con identidad resuelta contra el catálogo.
    #[test]
    fn board_row_arma_fila_desde_counter_entry() {
        let catalog = catalogo();
        let entry = CounterEntry {
            champion_id: 111,
            vs_winrate: 52.7,
            games: 88_400,
        };

        let row = board_row(&entry, Some(&catalog), "16.20.1");
        assert_eq!(row.champion_id, 111);
        assert_eq!(row.name, "Nautilus");
        assert_eq!(
            row.icon.as_deref(),
            Some("https://ddragon.leagueoflegends.com/cdn/16.20.1/img/champion/Nautilus.png")
        );
        assert!(
            (row.win_pct - 52.7).abs() < f64::EPSILON,
            "en MI board el WR no se invierte"
        );
        assert_eq!(row.games, 88_400);

        // Sin catálogo → #id y sin icono (nunca URL rota).
        let anonima = board_row(&entry, None, "16.20.1");
        assert_eq!(anonima.name, "#111");
        assert_eq!(anonima.icon, None);
    }

    /// Board del rival: invierte la perspectiva de la muestra (el WR del
    /// payload es DEL RIVAL contra cada campeón), filtra muestra corta y
    /// ordena mejor/peor.
    #[test]
    fn rival_board_rows_invierte_la_perspectiva_del_rival() {
        let catalog = catalogo();
        // El rival pierde contra Nautilus (vs_wr 42) y aplasta a Zed
        // (vs_wr 58); Ryze con muestra corta (< MIN_COUNTER_GAMES) fuera.
        let summary = CounterSummary {
            my_wr: Some(51.4),
            pickrate: Some(8.8),
            analysed: Some(40_000),
            counters: vec![
                CounterEntry {
                    champion_id: 111,
                    vs_winrate: 42.0,
                    games: 2_000,
                },
                CounterEntry {
                    champion_id: 238,
                    vs_winrate: 58.0,
                    games: 500,
                },
                CounterEntry {
                    champion_id: 34,
                    vs_winrate: 10.0,
                    games: 50,
                },
            ],
        };

        let best = rival_board_rows(&summary, Some(&catalog), "16.20.1", true, 8);
        assert_eq!(best.len(), 2, "la muestra corta se filtra");
        assert_eq!(best[0].champion_id, 111, "el rival pierde contra Nautilus");
        assert_eq!(best[0].name, "Nautilus");
        assert!(
            (best[0].win_pct - 58.0).abs() < f64::EPSILON,
            "100 − 42: el WR de Nautilus CONTRA el rival"
        );

        let worst = rival_board_rows(&summary, Some(&catalog), "16.20.1", false, 1);
        assert_eq!(worst.len(), 1, "el recorte a n se respeta");
        assert_eq!(worst[0].champion_id, 238, "Zed es el que más sufre");
        assert!(
            (worst[0].win_pct - 42.0).abs() < f64::EPSILON,
            "100 − 58: Zed la pasa mal contra el rival"
        );
    }

    /// Encabezados por sección: el rival nombra las columnas; las mías son
    /// genéricas. Subtítulos en mayúsculas, como en la captura aprobada.
    #[test]
    fn board_column_headers_por_seccion() {
        let ((mejores, hint_mejores), (peores, hint_peores)) =
            board_column_headers(BoardKind::Rival, "Jinx");
        assert_eq!(mejores, "MEJORES CONTRA Jinx");
        assert_eq!(hint_mejores, "GANAN AL RIVAL SEGÚN LA MUESTRA");
        assert_eq!(peores, "PEORES CONTRA Jinx");
        assert_eq!(hint_peores, "EL RIVAL LOS DOMINA");

        let ((fuerte, hint_fuerte), (debil, hint_debil)) =
            board_column_headers(BoardKind::Mine, "");
        assert_eq!(fuerte, "FUERTE CONTRA");
        assert_eq!(hint_fuerte, "TU MEJOR WR DE LA MUESTRA");
        assert_eq!(debil, "DÉBIL CONTRA");
        assert_eq!(hint_debil, "TU PEOR WR DE LA MUESTRA");
    }

    /// Board de MIS matchups: strong toma el prefijo (mejores WR, ya
    /// ordenado) y weak la cola invertida (peor primero); los nombres
    /// vienen del catálogo y el WR es el mío, sin invertir.
    #[test]
    fn my_board_columns_arma_fuerte_y_debil() {
        let summary = counters();
        let columns = my_board_columns(&summary, Some(&catalogo()), "16.20.1", 3);

        assert_eq!(columns.len(), 2);
        assert!(columns[0].positive, "FUERTE CONTRA es la columna plata");
        assert_eq!(columns[0].title, "FUERTE CONTRA");
        let ids: Vec<i64> = columns[0].rows.iter().map(|row| row.champion_id).collect();
        assert_eq!(ids, vec![106, 111, 238]);
        assert!(
            (columns[0].rows[1].win_pct - 52.7).abs() < f64::EPSILON,
            "mi WR contra Nautilus, sin inversión"
        );

        assert!(!columns[1].positive, "DÉBIL CONTRA es la columna roja");
        assert_eq!(columns[1].title, "DÉBIL CONTRA");
        let ids: Vec<i64> = columns[1].rows.iter().map(|row| row.champion_id).collect();
        assert_eq!(ids, vec![34, 238, 111], "peor matchup primero");
    }

    /// Título de sección: la lane solo si está resuelta (nunca una
    /// posición inventada).
    #[test]
    fn board_section_title_con_lane_opcional() {
        assert_eq!(
            board_section_title("ENFRENTAMIENTO", "Jinx", "MID"),
            "ENFRENTAMIENTO · Jinx MID"
        );
        assert_eq!(
            board_section_title("MIS MATCHUPS", "Zed", ""),
            "MIS MATCHUPS · Zed"
        );
    }

    /// Stats del header del rival: los segmentos sin dato se omiten y sin
    /// ninguno no hay texto (nunca un 0 inventado).
    #[test]
    fn rival_stats_text_es_honesto() {
        assert_eq!(
            rival_stats_text(Some(51.2), Some(4.1)).as_deref(),
            Some("51.2% WR · 4.1% PR · del rival")
        );
        assert_eq!(
            rival_stats_text(None, Some(4.1)).as_deref(),
            Some("4.1% PR · del rival")
        );
        assert_eq!(rival_stats_text(None, None), None);
    }

    /// Estado del board del rival: solo el Ready del MISMO rival arma la
    /// sección; Loading/Failed/vacío/sin rival dan estados honestos y un
    /// estado de OTRO rival no se muestra como Ready.
    #[test]
    fn rival_board_state_resuelve_contra_el_rival_vigente() {
        let catalog = catalogo();
        let summary = CounterSummary {
            my_wr: Some(51.2),
            pickrate: Some(4.1),
            analysed: Some(40_000),
            counters: vec![
                CounterEntry {
                    champion_id: 111,
                    vs_winrate: 42.0,
                    games: 2_000,
                },
                CounterEntry {
                    champion_id: 238,
                    vs_winrate: 58.0,
                    games: 500,
                },
            ],
        };
        let ready = RivalCountersState::Ready {
            rival_id: 64,
            summary: summary.clone(),
        };

        let estado = rival_board_state(
            Some(64),
            &ready,
            "Kayn",
            "JGL",
            Some(&catalog),
            "16.20.1",
            8,
        );
        let RivalBoardState::Ready { section } = estado else {
            panic!("esperaba Ready");
        };
        assert_eq!(section.title, "ENFRENTAMIENTO · Kayn JGL");
        assert_eq!(
            section.stats.as_deref(),
            Some("51.2% WR · 4.1% PR · del rival")
        );
        assert_eq!(section.columns.len(), 2);
        assert_eq!(section.columns[0].title, "MEJORES CONTRA Kayn");
        assert_eq!(section.columns[0].subtitle, "GANAN AL RIVAL SEGÚN LA MUESTRA");
        assert_eq!(
            section.columns[0].rows[0].champion_id, 111,
            "Nautilus le gana a Kayn"
        );
        assert!(
            (section.columns[0].rows[0].win_pct - 58.0).abs() < f64::EPSILON,
            "100 − 42"
        );
        assert_eq!(section.columns[1].title, "PEORES CONTRA Kayn");
        assert_eq!(
            section.columns[1].rows[0].champion_id, 238,
            "Zed es el que más sufre"
        );

        // Estado de OTRO rival (transición de lock): mensaje honesto, no
        // un board ajeno.
        assert_eq!(
            rival_board_state(
                Some(238),
                &ready,
                "Zed",
                "",
                Some(&catalog),
                "16.20.1",
                8
            ),
            RivalBoardState::Failed
        );

        // Loading y Failed del rival vigente.
        assert_eq!(
            rival_board_state(
                Some(64),
                &RivalCountersState::Loading { rival_id: 64 },
                "Kayn",
                "",
                Some(&catalog),
                "16.20.1",
                8
            ),
            RivalBoardState::Loading
        );
        assert_eq!(
            rival_board_state(
                Some(64),
                &RivalCountersState::Failed { rival_id: 64 },
                "Kayn",
                "",
                Some(&catalog),
                "16.20.1",
                8
            ),
            RivalBoardState::Failed
        );

        // Ready pero sin filas → Empty (la muestra no alcanzó el mínimo).
        let vacio = RivalCountersState::Ready {
            rival_id: 64,
            summary: CounterSummary::default(),
        };
        assert_eq!(
            rival_board_state(
                Some(64),
                &vacio,
                "Kayn",
                "",
                Some(&catalog),
                "16.20.1",
                8
            ),
            RivalBoardState::Empty
        );

        // Sin rival elegido → Idle (el bloque no se renderiza).
        assert_eq!(
            rival_board_state(None, &ready, "", "", Some(&catalog), "16.20.1", 8),
            RivalBoardState::Idle
        );
    }

    /// Build con Zhonya's en el core y Banshee's en situational (los ids
    /// de la tabla ANTI_ENEMY_ITEMS para Zed 238 y Anivia 84).
    fn build_con_antis() -> ItemBuild {
        let stat = |item_id: i64, games: u64| ItemStat {
            item_id,
            games,
            wins: games / 2,
            winrate: 52.0,
        };
        ItemBuild {
            starters: vec![stat(6655, 4206)],
            boots: vec![stat(3020, 9)],
            core: vec![stat(6655, 775), stat(4646, 775), stat(3089, 775), stat(3157, 775)],
            situational: vec![stat(3102, 120), stat(3135, 549)],
        }
    }

    /// Wiring de sugerencias vs comp: con Zed lockeado sale Zhonya's (del
    /// core) con la razón de la tabla; el panel trae la caja destacada.
    #[test]
    fn vs_comp_panel_sugiere_con_enemigos_lockeados() {
        let build = build_con_antis();
        let enemies = vec![
            miembro(5, Some(238), Some(crate::lcu::models::Position::Middle)),
            miembro(6, None, None),
        ];

        let panel = vs_comp_panel(&build, &enemies, None, "16.20.1");
        let VsCompPanel::Suggestions(rows) = panel else {
            panic!("esperaba Suggestions");
        };
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].item_id, 3157);
        assert_eq!(
            rows[0].desc,
            "Zhonya's: stasis esquiva el burst de Zed",
            "la descripción es la razón de la tabla"
        );
        assert_eq!(rows[0].name, "#3157", "sin catálogo de ítems cae a #id");
    }

    /// Sin match de reglas o SIN enemigos lockeados → BUILD PRINCIPAL con
    /// el core recortado a 3 y la nota correspondiente.
    #[test]
    fn vs_comp_panel_sin_match_o_sin_lockeados_cae_al_core() {
        let build = build_con_antis();

        // Enemigo lockeado sin ítem anti en el build (Yi 11 → Frozen Heart
        // 3110, que Veigar-clone no juega).
        let sin_match = vs_comp_panel(
            &build,
            &[miembro(5, Some(11), None)],
            None,
            "16.20.1",
        );
        assert_eq!(
            sin_match,
            VsCompPanel::Fallback {
                core: build_item_rows(&build.core[..3], None, "16.20.1"),
                no_enemies: false,
            }
        );

        // Con enemigos pero ninguno lockeado: mismo fallback, nota de lock.
        let sin_lock = vs_comp_panel(
            &build,
            &[miembro(5, None, None), miembro(6, None, None)],
            None,
            "16.20.1",
        );
        assert_eq!(
            sin_lock,
            VsCompPanel::Fallback {
                core: build_item_rows(&build.core[..3], None, "16.20.1"),
                no_enemies: true,
            }
        );

        // Sin lista enemiga: idem.
        let vacio = vs_comp_panel(&build, &[], None, "16.20.1");
        assert!(matches!(
            vacio,
            VsCompPanel::Fallback { no_enemies: true, .. }
        ));
    }

    /// Claves estables de fila para el reconciliador.
    #[test]
    fn row_key_distingue_locked_nodata_y_pending() {
        assert_eq!(
            row_key(&EnemyRow::Locked {
                champion_id: 238,
                name: String::new(),
                icon: None,
                position: "",
                vs_winrate: 45.2,
                games: 1,
            }),
            "locked-238"
        );
        assert_eq!(
            row_key(&EnemyRow::NoData {
                champion_id: 999,
                name: String::new(),
                icon: None,
                position: "",
            }),
            "locked-999"
        );
        assert_eq!(
            row_key(&EnemyRow::Pending {
                slot: 3,
                position: "",
            }),
            "pending-3"
        );
    }
}
