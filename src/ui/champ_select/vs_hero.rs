//! Modo visual **VS** del champ select (opción 2, mockup del dueño del
//! proyecto): mini-franja de equipos en círculos + pantalla dividida
//! "MI CHAMP vs RIVAL" con rival ELEGIBLE manualmente.
//!
//! Diferencias con la vista clásica (opción 1, [`super::HeroStrip`]):
//! - El rival es una elección manual (picker con búsqueda sobre el
//!   catálogo): permite consultar "Akali vs Kayn" SIN que nadie lo
//!   lockee. Quitarlo vuelve al estado neutro ("vs nadie") y las tabs
//!   siguen mostrando los counters de los enemigos REALES lockeados.
//! - La franja de equipos refleja el champ select EN VIVO (locks reales);
//!   el rival manual vive SOLO en el split hero con chip "MANUAL" para no
//!   confundirlo con un lock real.
//!
//! Arte: `loading_url()` (art vertical de ddragon). Cuando el dueño
//! consiga PNGs sin fondo, el origen se cambia en [`ChampionEntry`]
//! sin tocar este layout.

use dioxus::prelude::*;

use std::sync::Arc;

use crate::data_dragon::champion_detail::ChampionDetail;
use crate::data_dragon::item::ItemCatalog;
use crate::data_dragon::models::{ChampionCatalog, ChampionEntry};
use crate::lcu::models::{MatchContext, Position, TeamMember};
use crate::stats::source::analysis::{analysis_counters, AnalysisState};
use crate::stats::source::counters::{wr_vs, CounterEntry, CounterSummary, MIN_COUNTER_GAMES};
use crate::stats::source::items::{suggest_vs_comp, suggestion_reason, CompRules, ItemBuild};
use crate::stats::source::player_tips::PlayerTip;
use crate::ui::champ_select::champ_art::champ_art;
use crate::ui::champ_select::items_panel::{item_rows, ItemRow};
use crate::ui::format::format_thousands;
use crate::ui::recommendations::format_winrate;

use super::position_label;

/// Cantidad de casillas por lado en la mini-franja (5v5).
const STRIP_SLOTS: usize = 5;

/// Tope de resultados del picker.
pub(crate) const PICKER_LIMIT: usize = 12;

/// Casilla de la mini-franja (pura, testeada).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct StripSlot {
    /// Id del campeón lockeado (`None` = casilla vacía/pending): es lo que
    /// hace al slot clickeable para elegirlo como rival.
    pub champion_id: Option<i64>,
    /// Icono cuadrado (catálogo) o `None` para casilla sin datos.
    pub icon: Option<String>,
    /// Nombre visible (tooltip/alt del círculo).
    pub label: String,
    /// `true` = campeón lockeado (círculo con arte); `false` = "?" punteado.
    pub filled: bool,
}

impl StripSlot {
    fn empty() -> Self {
        Self {
            champion_id: None,
            icon: None,
            label: String::from("?"),
            filled: false,
        }
    }
}

/// Lado izquierdo de la franja: MI champ primero + aliados, rellenado a
/// [`STRIP_SLOTS`]. Lado derecho: enemigos (sin mi champ), rellenado igual.
/// Los locks reales van con arte; pendientes/vacíos con "?" (puro,
/// testeado).
pub(crate) fn build_strip(
    my: Option<(i64, Option<Position>)>,
    allies: &[TeamMember],
    enemies: &[TeamMember],
    catalog: Option<&ChampionCatalog>,
    version: &str,
) -> (Vec<StripSlot>, Vec<StripSlot>) {
    let entry = |id: i64| -> StripSlot {
        match catalog.and_then(|data| data.get(id)) {
            Some(entry) => StripSlot {
                champion_id: Some(id),
                icon: Some(entry.icon_url(version)),
                label: entry.name.clone(),
                filled: true,
            },
            None => StripSlot {
                champion_id: Some(id),
                icon: None,
                label: format!("#{id}"),
                filled: true,
            },
        }
    };

    let mut left: Vec<StripSlot> = Vec::new();
    match my {
        Some((id, _)) => left.push(entry(id)),
        None => left.push(StripSlot::empty()),
    }
    for member in allies {
        left.push(match member.champion_id {
            Some(id) => entry(id),
            None => StripSlot::empty(),
        });
    }
    left.resize(STRIP_SLOTS, StripSlot::empty());

    let mut right: Vec<StripSlot> = enemies
        .iter()
        .map(|member| match member.champion_id {
            Some(id) => entry(id),
            None => StripSlot::empty(),
        })
        .collect();
    right.resize(STRIP_SLOTS, StripSlot::empty());

    (left, right)
}

/// Resultado de la búsqueda del picker (puro, testeado): pares
/// `(champion_id, entry)` cuyo nombre contiene la query
/// (case-insensitive), excluyendo MI champ, orden alfabético y tope
/// [`PICKER_LIMIT`].
pub(crate) fn filter_champions<'a>(
    query: &str,
    catalog: &'a ChampionCatalog,
    exclude_champion_id: i64,
) -> Vec<(i64, &'a ChampionEntry)> {
    let needle = query.trim().to_lowercase();
    let mut rows: Vec<(i64, &ChampionEntry)> = catalog
        .by_id
        .iter()
        .filter(|(id, entry)| {
            **id != exclude_champion_id && entry.name.to_lowercase().contains(&needle)
        })
        .map(|(id, entry)| (*id, entry))
        .collect();
    rows.sort_by(|a, b| a.1.name.cmp(&b.1.name));
    rows.truncate(PICKER_LIMIT);
    rows
}

/// WR del matchup manual (puro, testeado): `Some((wr, games))` solo si la
/// muestra de counters trae el matchup; `None` → la UI muestra "sin datos
/// suficientes" (nunca un número inventado).
pub(crate) fn manual_wr(counters: Option<&CounterSummary>, rival_id: i64) -> Option<(f64, i64)> {
    let entry = wr_vs(&counters?.counters, rival_id)?;
    Some((entry.vs_winrate, entry.games))
}

/// Texto de la línea de WR (puro, testeado).
pub(crate) fn wr_line_text(counters: Option<&CounterSummary>, rival_id: i64) -> String {
    match manual_wr(counters, rival_id) {
        Some((wr, games)) => format!(
            "{}% WR · {} partidas contra este rival",
            format_winrate(wr),
            format_thousands(games.max(0))
        ),
        None => String::from("Sin datos suficientes para este matchup en la muestra del parche"),
    }
}

/// Estado de la carga del detalle del rival (tips oficiales `enemytips`).
/// El loader ya cachea por `(versión, id)`: re-elegir un rival es gratis.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) enum RivalDetailState {
    #[default]
    Idle,
    Loading { rival_id: i64 },
    Ready { rival_id: i64, detail: Box<ChampionDetail> },
    Failed { rival_id: i64 },
}

/// Sugerencias de ítems contra UN rival (puro, testeado): cruce honesto
/// [`suggest_vs_comp`] + razones; sin build propio o sin match de reglas
/// → estado explícito (la UI muestra su nota, nunca inventa ítems).
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum VsSuggestState {
    NoBuild,
    Empty,
    Rows(Vec<ItemRow>),
}

/// Estado de la carga de los counters DEL RIVAL (los que alimentan el
/// bloque estilo counterstats: mejores/peores picks contra él).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) enum RivalCountersState {
    #[default]
    Idle,
    Loading { rival_id: i64 },
    Ready { rival_id: i64, summary: CounterSummary },
    Failed { rival_id: i64 },
}

/// Estado de los consejos contra el rival, ya TRADUCIDOS al español
/// (best-effort vía Google gtx; sin traducción disponible → textos
/// originales en inglés). `official` = enemytips de Riot; `players` =
/// consejos de counterstats/MOBAFire.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) enum RivalTipsState {
    #[default]
    Idle,
    Loading { rival_id: i64 },
    Ready {
        rival_id: i64,
        official: Arc<Vec<String>>,
        players: Arc<Vec<PlayerTip>>,
    },
    Failed { rival_id: i64 },
}

/// Fila del bloque mejores/peores contra el rival (puro, testeado).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RivalRow {
    /// Campeón de la fila.
    pub champion_id: i64,
    /// Nombre visible (catálogo o `#id`).
    pub name: String,
    /// Icono cuadrado (`None` sin versión).
    pub icon: Option<String>,
    /// Winrate de ESTE campeón contra el rival (100 − `vs_wr` del rival).
    pub win_pct: f64,
    /// Partidas de la muestra del matchup.
    pub games: i64,
    /// `true` si la fila es MI campeón (highlight en la UI).
    pub is_me: bool,
}

/// Bloque estilo counterstats desde los counters DEL RIVAL (puro,
/// testeado): `vs_wr` de la muestra es el winrate del RIVAL contra cada
/// campeón, así que el winrate DE ESE campeón contra el rival es
/// `100 − vs_wr`. `best = true` → campeones que mejor le ganan al rival
/// (rival pierde: `vs_wr` más bajo). Filtradas por muestra mínima y tope
/// `n`, con MI campeón marcado si aparece.
pub(crate) fn rival_rows(
    summary: &CounterSummary,
    catalog: Option<&ChampionCatalog>,
    version: &str,
    my_champion_id: i64,
    best: bool,
    n: usize,
) -> Vec<RivalRow> {
    let mut rows: Vec<&CounterEntry> = summary
        .counters
        .iter()
        .filter(|entry| entry.games >= crate::stats::source::counters::MIN_COUNTER_GAMES)
        .collect();
    rows.sort_by(|a, b| {
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

    rows.into_iter()
        .take(n)
        .map(|entry| {
            let (name, icon) = match catalog.and_then(|data| data.get(entry.champion_id)) {
                Some(item) => (item.name.clone(), Some(item.icon_url(version))),
                None => (format!("#{}", entry.champion_id), None),
            };
            RivalRow {
                champion_id: entry.champion_id,
                name,
                icon,
                win_pct: 100.0 - entry.vs_winrate,
                games: entry.games,
                is_me: entry.champion_id == my_champion_id,
            }
        })
        .collect()
}

pub(crate) fn vs_suggestions(
    build: Option<&ItemBuild>,
    items_catalog: Option<&ItemCatalog>,
    version: &str,
    rival_id: i64,
) -> VsSuggestState {
    let Some(build) = build else {
        return VsSuggestState::NoBuild;
    };
    let suggestions = suggest_vs_comp(build, &[rival_id], &CompRules::default());
    if suggestions.is_empty() {
        return VsSuggestState::Empty;
    }
    let mut rows = item_rows(&suggestions, items_catalog, version);
    for row in &mut rows {
        if let Some(reason) = suggestion_reason(row.item_id, &[rival_id]) {
            row.desc = String::from(reason);
        }
    }
    VsSuggestState::Rows(rows)
}

/// Lane del matchup con el rival: la posición del ENEMIGO si el lock la
/// trajo (es la lane donde se cruzan), si no MI posición (puro, testeado).
pub(crate) fn rival_lane_from_context(
    context: &MatchContext,
    rival_id: Option<i64>,
    my_position: Option<Position>,
) -> Option<Position> {
    let rid = rival_id?;
    match context
        .enemies
        .iter()
        .find(|member| member.champion_id == Some(rid))
    {
        // El rival lockeado con lane → su lane; sin lane → la mía.
        Some(member) => member.position.or(my_position),
        // Rival manual (sin casilla en el draft) → mi lane.
        None => my_position,
    }
}

/// Modo visual VS: mini-franja + split "MI CHAMP vs RIVAL" + picker.
#[component]
pub(crate) fn VsHero(
    champion_id: i64,
    my_name: String,
    position: Option<Position>,
    match_context: ReadSignal<MatchContext>,
    catalog: ReadSignal<Option<ChampionCatalog>>,
    analysis: ReadSignal<AnalysisState>,
    items: ReadSignal<Option<Arc<ItemCatalog>>>,
    rival: Signal<Option<i64>>,
    rival_counters: Signal<RivalCountersState>,
) -> Element {
    let mut picker_open: Signal<bool> = use_signal(|| false);
    let mut query: Signal<String> = use_signal(String::new);

    // Snapshots cortos: los guards no cruzan la construcción del rsx.
    let catalog_snapshot = catalog.read().clone();
    let version = catalog_snapshot
        .as_ref()
        .map(|data| data.version.clone())
        .unwrap_or_default();
    let context: MatchContext = match_context.read().clone();
    let (left_slots, right_slots) = build_strip(
        Some((champion_id, position)),
        &context.allies,
        &context.enemies,
        catalog_snapshot.as_ref(),
        &version,
    );

    let my_entry = catalog_snapshot
        .as_ref()
        .and_then(|data| data.get(champion_id));
    // Arte: art sin fondo local (assets/champs) primero; fallback al art
    // vertical de Data Dragon (con fondo). `(url, es_cutout)`.
    let art_of = |entry: &ChampionEntry| -> (String, bool) {
        match champ_art(&entry.ddragon_id) {
            Some(asset) => (asset.to_string(), true),
            None => (entry.loading_url(), false),
        }
    };
    let my_art = my_entry.map(art_of);

    // Rival resuelto contra el catálogo (nombre, icono, art).
    let rival_id = rival();
    let rival_entry =
        rival_id.and_then(|id| catalog_snapshot.as_ref().and_then(|data| data.get(id)));

    // WR del matchup desde el análisis (counters de MI champ ya descargados).
    let analysis_snapshot: AnalysisState = analysis.read().clone();
    let counters = analysis_counters(&analysis_snapshot, champion_id);
    let wr_text = rival_id
        .map(|id| wr_line_text(counters.as_deref(), id))
        .unwrap_or_default();

    let rival_name = rival_entry
        .map(|entry| entry.name.clone())
        .unwrap_or_default();

    // Bloque estilo counterstats: counters DEL RIVAL resueltos contra el
    // catálogo (mejores/peores contra él, con MI champ marcado).
    let counters_snapshot: RivalCountersState = rival_counters();
    let (best_rows, worst_rows, rival_stats) = match (&counters_snapshot, rival_id) {
        (RivalCountersState::Ready { rival_id: current, summary }, Some(rid)) if *current == rid => {
            (
                rival_rows(summary, catalog_snapshot.as_ref(), &version, champion_id, true, 8),
                rival_rows(summary, catalog_snapshot.as_ref(), &version, champion_id, false, 8),
                Some((summary.my_wr, summary.pickrate)),
            )
        }
        _ => (Vec::new(), Vec::new(), None),
    };
    let lane_label = position_label(rival_lane_from_context(&context, rival_id, position));
    let lane_text = if lane_label.is_empty() {
        "agregada"
    } else {
        lane_label
    };

    // Búsqueda del picker (snapshot por render). La vaciedad se captura
    // antes del rsx (los resultados se consumen en el map).
    let query_value = query().clone();
    let mut results: Vec<(i64, &ChampionEntry)> = match catalog_snapshot.as_ref() {
        Some(data) => filter_champions(&query_value, data, champion_id),
        None => Vec::new(),
    };
    results.truncate(PICKER_LIMIT);
    let results_empty = results.is_empty();
    let picker_is_open = picker_open();

    // Selección de rival desde la franja: click en un enemigo lockeado lo
    // vuelve el rival del split; click de nuevo lo quita.
    let on_pick = move |picked: i64| {
        if rival() == Some(picked) {
            rival.set(None);
        } else {
            rival.set(Some(picked));
        }
    };

    rsx! {
        div { class: "vs",

            // ----- Mini-franja de equipos (locks EN VIVO; enemigos
            // clickeables para elegirlos como rival) -----
            div { class: "vs-strip",
                span { class: "vs-strip-label", "Mi equipo" }
                div { class: "vs-strip-side",
                    {left_slots.iter().enumerate().map(|(index, slot)| rsx! {
                        VsSlot {
                            slot: slot.clone(),
                            mine: index == 0,
                            enemy: false,
                            clickable: false,
                            selected: false,
                            on_pick,
                            key: "l{index}",
                        }
                    })}
                }
                span { class: "vs-strip-vs", "VS" }
                div { class: "vs-strip-side",
                    {right_slots.iter().enumerate().map(|(index, slot)| rsx! {
                        VsSlot {
                            slot: slot.clone(),
                            mine: false,
                            enemy: true,
                            clickable: slot.filled && slot.champion_id.is_some(),
                            selected: rival_id == slot.champion_id,
                            on_pick,
                            key: "r{index}",
                        }
                    })}
                }
                span { class: "vs-strip-label vs-strip-label--right", "Enemigos" }
            }

            // ----- Split hero: MI CHAMP vs RIVAL -----
            div { class: "vs-split",
                div { class: "vs-side vs-side--mine",
                    {match my_art {
                        Some((url, cutout)) => rsx! {
                            img {
                                class: if cutout { "vs-art vs-art--cutout" } else { "vs-art" },
                                src: "{url}",
                                alt: "{my_name}",
                            }
                        },
                        None => rsx! {
                            span { class: "vs-art-fallback", "?" }
                        },
                    }}
                    span { class: "vs-watermark", "{my_name}" }
                    div { class: "vs-foot vs-foot--mine",
                        span { class: "label fs-10 t-dim", "Mi champ" }
                        strong { class: "vs-name", "{my_name}" }
                    }
                }

                div { class: "vs-center",
                    span { class: "vs-badge", "VS" }
                }

                div { class: "vs-side vs-side--rival",
                    {match rival_entry {
                        Some(entry) => rsx! {
                            {let (art_url, cutout) = art_of(entry);
                            rsx! {
                                img {
                                    class: if cutout { "vs-art vs-art--cutout" } else { "vs-art" },
                                    src: "{art_url}",
                                    alt: "{entry.name}",
                                }
                            }}
                            span { class: "vs-watermark", "{entry.name}" }
                            button {
                                class: "vs-clear",
                                title: "Quitar rival",
                                onclick: move |_| rival.set(None),
                                "✕"
                            }
                            div { class: "vs-foot vs-foot--rival",
                                span { class: "label fs-10 t-dim", "Rival" }
                                strong { class: "vs-name", "{entry.name}" }
                            }
                        },
                        None => rsx! {
                            span { class: "vs-art-fallback", "?" }
                            button {
                                class: "vs-pick-btn btn-ghost",
                                onclick: move |_| {
                                    query.set(String::new());
                                    picker_open.set(true);
                                },
                                "Elegir rival"
                            }
                        },
                    }}
                }
            }

            // ----- WR del matchup manual (honesto) -----
            if rival_id.is_some() {
                div { class: "vs-wrline",
                    span { class: "sl sl--silver", "//" }
                    " {wr_text}"
                }
            }

            // ----- Enfrentamiento (estilo counterstats, datos Lolalytics):
            // mejores y peores picks CONTRA el rival según su propia muestra
            // de counters en la lane del cruce -----
            if rival_id.is_some() {
                div { class: "vs-matchup",
                    div { class: "section-head",
                        span { class: "bar-skew", "style": "width: 24px;" }
                        span { class: "label label-md",
                            span { class: "sl", "//" }
                            " Enfrentamiento "
                            span { class: "hud-sep", "·" }
                            " {rival_name}"
                        }
                        if !lane_label.is_empty() {
                            span { class: "chip chip--soft", "{lane_label}" }
                        }
                        if let Some((wr, pr)) = rival_stats {
                            span { class: "vs-mu-stats tnum",
                                "{format_winrate(wr.unwrap_or(0.0))}% WR"
                                span { class: "hud-sep", "·" }
                                "{format_winrate(pr.unwrap_or(0.0))}% PR"
                                span { class: "hud-sep", "·" }
                                "del rival"
                            }
                        }
                    }

                    {match (&counters_snapshot, rival_id) {
                        (RivalCountersState::Ready { rival_id: current, .. }, Some(rid)) if *current == rid => {
                            if best_rows.is_empty() && worst_rows.is_empty() {
                                rsx! {
                                    p { class: "cs-hint",
                                        "Sin muestra suficiente para los matchups de este rival en esta lane."
                                    }
                                }
                            } else {
                                rsx! {
                                    div { class: "vs-mu-cols",
                                        MuColumn {
                                            title: "Mejores contra {rival_name}",
                                            hint: "ganan al rival según la muestra",
                                            rows: best_rows.clone(),
                                            positive: true,
                                            key: "best",
                                        }
                                        MuColumn {
                                            title: "Peores contra {rival_name}",
                                            hint: "el rival los domina",
                                            rows: worst_rows.clone(),
                                            positive: false,
                                            key: "worst",
                                        }
                                    }
                                }
                            }
                        }
                        (RivalCountersState::Loading { rival_id: current }, Some(rid)) if *current == rid => rsx! {
                            div { class: "cs-loading",
                                div { class: "spinner" }
                                span { class: "label label-md pulse-soft t-silver", "Consultando matchups del rival…" }
                            }
                        },
                        _ => rsx! {
                            p { class: "cs-hint", "Matchups del rival no disponibles ahora." }
                        },
                    }}

                    p { class: "cs-hint vs-note",
                        "Datos Lolalytics · lane {lane_text} · matchups con muestra ≥ {MIN_COUNTER_GAMES} partidas. WR mostrado = de cada campeón CONTRA el rival."
                    }
                }
            }

            // ----- Picker modal -----
            if picker_is_open {
                div { class: "modal-overlay",
                    div { class: "modal-panel vs-picker",
                        div { class: "vs-picker-head",
                            span { class: "label label-md",
                                span { class: "sl sl--silver", "//" }
                                " Elegir rival"
                            }
                            button {
                                class: "modal-x",
                                onclick: move |_| picker_open.set(false),
                                "✕"
                            }
                        }
                        input {
                            class: "vs-picker-input",
                            placeholder: "Buscar campeón…",
                            value: "{query_value}",
                            autofocus: true,
                            oninput: move |event| query.set(event.value()),
                        }
                        div { class: "vs-picker-list",
                            {results.into_iter().map(|(id, entry)| {
                                let name = entry.name.clone();
                                let icon = entry.icon_url(&version);
                                rsx! {
                                    button {
                                        class: "vs-picker-row",
                                        key: "{id}",
                                        onclick: move |_| {
                                            rival.set(Some(id));
                                            picker_open.set(false);
                                        },
                                        img { class: "vs-picker-icon", src: "{icon}", alt: "{name}" }
                                        span { "{name}" }
                                    }
                                }
                            })}
                            if results_empty {
                                p { class: "cs-hint", "Sin resultados para \"{query_value}\"." }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Círculo de la mini-franja: arte lockeado con aro (azul mi equipo — el
/// mío con glow — y rojo los rivales) o "?" punteado. Los enemigos
/// lockeados son clickeables: los eligen como rival del split (click de
/// nuevo lo quita).
#[component]
fn VsSlot(
    slot: StripSlot,
    mine: bool,
    enemy: bool,
    clickable: bool,
    selected: bool,
    on_pick: EventHandler<i64>,
) -> Element {
    let ring = if mine {
        "vs-slot--mine"
    } else if enemy {
        "vs-slot--rival"
    } else {
        "vs-slot--ally"
    };
    let selected_class = if selected { " vs-slot--selected" } else { "" };
    let empty_class = if slot.filled { "" } else { " vs-slot--empty" };
    let label = slot.label.clone();
    let inner = match (slot.filled, slot.icon) {
        (true, Some(icon)) => rsx! {
            img { src: "{icon}", alt: "{label}", title: "{label}" }
        },
        _ => rsx! {
            span { class: "vs-slot-q", "?" }
        },
    };

    if clickable {
        rsx! {
            button {
                class: "vs-slot vs-slot--btn {ring}{empty_class}{selected_class}",
                title: if selected { "{label} · click para quitar" } else { "{label} · click para enfrentar" },
                onclick: move |_| {
                    if let Some(id) = slot.champion_id {
                        on_pick.call(id);
                    }
                },
                {inner}
            }
        }
    } else {
        rsx! {
            span { class: "vs-slot {ring}{empty_class}{selected_class}", title: "{label}", {inner} }
        }
    }
}

/// Columna del bloque de enfrentamiento (mejores/peores contra el rival).
#[component]
fn MuColumn(
    title: String,
    hint: &'static str,
    rows: Vec<RivalRow>,
    positive: bool,
) -> Element {
    let fill_class = if positive { "fill" } else { "fill fill--primary" };
    let wr_class = if positive { "t-on" } else { "t-bright" };
    rsx! {
        div { class: "vs-mu-col",
            div { class: "vs-mu-head",
                span { class: if positive { "vs-mu-title vs-mu-title--good" } else { "vs-mu-title vs-mu-title--bad" }, "{title}" }
                span { class: "label fs-10 t-dim", "{hint}" }
            }
            {rows.into_iter().map(|row| {
                let games = format_thousands(row.games.max(0));
                let fill_width = row.win_pct.clamp(0.0, 100.0);
                rsx! {
                    div {
                        class: if row.is_me { "vs-mu-row vs-mu-row--me" } else { "vs-mu-row" },
                        key: "mu-{row.champion_id}",
                        {match row.icon {
                            Some(icon) => rsx! {
                                img { class: "vs-mu-icon", src: "{icon}", alt: "{row.name}" }
                            },
                            None => rsx! {
                                span { class: "vs-mu-icon vs-mu-icon--fallback", "#" }
                            },
                        }}
                        div { class: "vs-mu-mid",
                            div { class: "vs-mu-wrline",
                                span { class: "num-display tnum vs-mu-wr {wr_class}",
                                    "{format_winrate(row.win_pct)}%"
                                }
                                if row.is_me {
                                    span { class: "chip chip--active vs-mu-me", "Tu champ" }
                                }
                            }
                            div { class: "track",
                                div { class: "{fill_class}", "style": "width: {fill_width}%;", "aria-hidden": "true" }
                            }
                        }
                        div { class: "vs-mu-right",
                            span { class: "vs-mu-name", "{row.name}" }
                            span { class: "vs-mu-games tnum", "{games} partidas" }
                        }
                    }
                }
            })}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::source::counters::CounterEntry;
    use std::collections::HashMap;

    fn catalog() -> ChampionCatalog {
        let mut by_id = HashMap::new();
        for (id, name, ddragon) in
            [(238, "Zed", "Zed"), (34, "Akali", "Akali"), (64, "Kayn", "Kayn")]
        {
            by_id.insert(
                id,
                ChampionEntry {
                    name: String::from(name),
                    image_url: format!("https://example/{ddragon}.png"),
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

    fn member(cell: i64, champion_id: Option<i64>) -> TeamMember {
        TeamMember {
            cell_id: cell,
            champion_id,
            position: None,
        }
    }

    /// Franja: mi champ primero, aliados después, relleno a 5; enemigos a
    /// la derecha rellenados a 5.
    #[test]
    fn build_strip_rellena_a_cinco_por_lado() {
        let catalog = catalog();
        let my = Some((34, Some(Position::Middle)));
        let allies = vec![member(0, Some(238)), member(1, None)];
        let enemies = vec![member(9, Some(64))];

        let (left, right) = build_strip(my, &allies, &enemies, Some(&catalog), "16.20.1");

        assert_eq!(left.len(), 5);
        assert_eq!(right.len(), 5);
        assert!(left[0].filled);
        assert_eq!(left[0].champion_id, Some(34));
        assert_eq!(left[0].label, "Akali");
        assert!(left[1].filled);
        assert_eq!(left[1].label, "Zed");
        assert!(!left[2].filled);
        assert_eq!(left[2].champion_id, None);
        assert!(!left[4].filled);
        assert!(right[0].filled);
        assert_eq!(right[0].champion_id, Some(64));
        assert_eq!(right[0].label, "Kayn");
        assert!(!right[1].filled);
    }

    /// Sin catálogo (o id fuera del catálogo) degrada a "#id" con casilla
    /// llena, sin panic.
    #[test]
    fn build_strip_sin_catalogo_degrada_a_id() {
        let (left, right) = build_strip(Some((999, None)), &[], &[], None, "");

        assert_eq!(left[0].label, "#999");
        assert!(left[0].filled && left[0].icon.is_none());
        assert_eq!(right.len(), 5);
        assert!(!right[0].filled);
    }

    /// Filtro del picker: case-insensitive, excluye MI champ, orden
    /// alfabético.
    #[test]
    fn filter_champions_filtra_excluye_y_ordena() {
        let catalog = catalog();

        let rows = filter_champions("KA", &catalog, 34);
        assert_eq!(rows.len(), 1, "solo Kayn contiene 'ka' (Akali soy yo)");
        assert_eq!(rows[0].1.name, "Kayn");
        assert_eq!(rows[0].0, 64);

        let todos = filter_champions("", &catalog, 34);
        assert_eq!(
            todos.iter().map(|(_, entry)| entry.name.as_str()).collect::<Vec<_>>(),
            vec!["Kayn", "Zed"],
            "sin query lista todos menos yo, en orden alfabético"
        );
    }

    /// WR manual: presente solo si el matchup está en la muestra.
    #[test]
    fn manual_wr_y_su_texto_son_honestos() {
        let summary = CounterSummary {
            my_wr: Some(51.0),
            pickrate: None,
            analysed: Some(10_000),
            counters: vec![CounterEntry {
                champion_id: 64,
                vs_winrate: 45.2,
                games: 1_234,
            }],
        };

        assert_eq!(manual_wr(Some(&summary), 64), Some((45.2, 1_234)));
        assert_eq!(
            wr_line_text(Some(&summary), 64),
            "45.2% WR · 1.234 partidas contra este rival"
        );
        assert_eq!(manual_wr(Some(&summary), 238), None);
        assert!(wr_line_text(Some(&summary), 238).starts_with("Sin datos"));
        assert!(wr_line_text(None, 64).starts_with("Sin datos"));
    }

    /// Bloque estilo counterstats: invierte la perspectiva de los counters
    /// del rival. La muestra trae el WR DEL RIVAL contra cada campeón: los
    /// "mejores contra él" son los de `vs_wr` más BAJO (su winrate contra
    /// ellos = 100 − vs_wr, alto).
    #[test]
    fn rival_rows_invierte_la_perspectiva_del_rival() {
        let catalog = catalog();
        // El rival (Kayn 64) pierde contra Akali (vs_wr 42) y aplasta a
        // Zed (vs_wr 58); muestra corta de Zed fuera del filtro.
        let summary = CounterSummary {
            my_wr: Some(51.4),
            pickrate: Some(8.8),
            analysed: Some(40_000),
            counters: vec![
                CounterEntry {
                    champion_id: 34,
                    vs_winrate: 42.0,
                    games: 2_000,
                },
                CounterEntry {
                    champion_id: 238,
                    vs_winrate: 58.0,
                    games: 500,
                },
                CounterEntry {
                    champion_id: 1,
                    vs_winrate: 10.0,
                    games: 50, // < MIN_COUNTER_GAMES → fuera
                },
            ],
        };

        let best = rival_rows(&summary, Some(&catalog), "16.20.1", 34, true, 8);
        assert_eq!(best.len(), 2, "la muestra corta se filtra");
        assert_eq!(best[0].champion_id, 34);
        assert_eq!(best[0].name, "Akali");
        assert_eq!(best[0].win_pct, 58.0, "100 − 42");
        assert!(best[0].is_me, "mi champ queda marcado");

        let worst = rival_rows(&summary, Some(&catalog), "16.20.1", 34, false, 8);
        assert_eq!(worst[0].champion_id, 238);
        assert_eq!(worst[0].win_pct, 42.0, "100 − 58: Zed la pasa mal");
        assert!(!worst[0].is_me);
    }

    /// Lane del matchup: la posición del enemigo lockeado manda; sin lock
    /// cae a mi posición.
    #[test]
    fn rival_lane_usa_la_del_enemigo_y_cae_a_la_mia() {
        let context = MatchContext {
            enemies: vec![TeamMember {
                cell_id: 100,
                champion_id: Some(64),
                position: Some(Position::Jungle),
            }],
            ..MatchContext::default()
        };

        assert_eq!(
            rival_lane_from_context(&context, Some(64), Some(Position::Middle)),
            Some(Position::Jungle)
        );
        assert_eq!(
            rival_lane_from_context(&context, Some(238), Some(Position::Middle)),
            Some(Position::Middle),
            "rival sin casilla → mi lane"
        );
        assert_eq!(rival_lane_from_context(&context, None, None), None);
    }

    /// Sugerido vs un rival: cruce honesto con la tabla de reglas.
    /// vs Zed (238) con Zhonya's (3157) en el core → fila con la razón;
    /// sin build → NoBuild; build sin match → Empty.
    #[test]
    fn vs_suggestions_cruza_la_tabla_con_mi_build() {
        let stat = |item_id: i64| crate::stats::source::items::ItemStat {
            item_id,
            games: 500,
            wins: 275,
            winrate: 55.0,
        };
        let build = ItemBuild {
            starters: vec![],
            boots: vec![],
            core: vec![stat(3157)],
            situational: vec![],
        };

        match vs_suggestions(Some(&build), None, "16.20.1", 238) {
            VsSuggestState::Rows(rows) => {
                assert_eq!(rows.len(), 1);
                assert_eq!(rows[0].item_id, 3157);
                assert_eq!(rows[0].name, "#3157", "sin catálogo degrada a #id");
                assert!(
                    rows[0].desc.contains("Zhonya"),
                    "la desc es la razón de la regla: {}",
                    rows[0].desc
                );
            }
            other => panic!("esperaba Rows, got {other:?}"),
        }

        assert_eq!(vs_suggestions(None, None, "", 238), VsSuggestState::NoBuild);

        let build_sin_match = ItemBuild {
            starters: vec![],
            boots: vec![],
            core: vec![stat(3020)],
            situational: vec![],
        };
        assert_eq!(
            vs_suggestions(Some(&build_sin_match), None, "", 238),
            VsSuggestState::Empty
        );
    }
}
