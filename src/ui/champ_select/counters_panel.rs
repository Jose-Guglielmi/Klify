//! Tab COUNTERS del champ select (FASE 11b-2b).
//!
//! Dos columnas 55/45 según el prototipo (ui-preview.html, panel
//! `#panel-counters`):
//!
//! - **Izquierda · "// VS Enemigos en vivo"**: una fila por casilla enemiga
//!   del [`MatchContext`]. Lockeado con matchup en la muestra
//!   ([`wr_vs`]) → icono + nombre + posición + WR coloreado (desfavorable
//!   `< 50%` → `#ff3355`, favorable → blanco) + barra proporcional +
//!   partidas. Lockeado sin matchup → "Sin datos suficientes". Sin lock →
//!   fila fantasma "? Enemy · {posición} · se muestra al lockear".
//!   Debajo, "// Matchups generales" con [`CounterSummary::strong_against`]
//!   (Fuerte contra) y [`CounterSummary::weak_against`] (Débil contra, WR
//!   en rojo).
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
//! razón corta. Toda la derivación de filas es pura y testeada
//! ([`enemy_rows`], [`matchup_rows`], [`vs_comp_panel`]); el RSX solo
//! pinta.

use std::sync::Arc;

use dioxus::prelude::*;

use crate::data_dragon::item::ItemCatalog;
use crate::data_dragon::models::ChampionCatalog;
use crate::lcu::models::{MatchContext, TeamMember};
use crate::stats::source::analysis::{
    analysis_counters, analysis_failure_reason, analysis_in_progress, analysis_items,
    AnalysisState,
};
use crate::stats::source::counters::{wr_vs, CounterEntry, CounterSummary};
use crate::stats::source::items::{suggest_vs_comp, suggestion_reason, CompRules, ItemBuild, ItemStat};
use crate::ui::champ_select::position_label;
use crate::ui::champ_select::items_panel::{item_rows as build_item_rows, row_games, ItemRow};
use crate::ui::format::format_thousands;
use crate::ui::recommendations::format_winrate;

/// Umbral de la convención del prototipo: WR contra el rival por debajo de
/// 50% es matchup desfavorable (número y barra en rojo de marca).
pub const UNFAVOURABLE_THRESHOLD: f64 = 50.0;

/// ¿Matchup desfavorable? (puro, testeado).
#[must_use]
pub fn is_unfavourable(vs_winrate: f64) -> bool {
    vs_winrate < UNFAVOURABLE_THRESHOLD
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

/// Fila compacta de matchup general (pura, testeada).
#[derive(Clone, Debug, PartialEq)]
pub struct MatchupRow {
    /// Id numérico del rival.
    pub champion_id: i64,
    /// Nombre visible (catálogo o `#id`).
    pub name: String,
    /// Icono cuadrado (`None` sin versión).
    pub icon: Option<String>,
    /// WR de MI campeón contra el rival.
    pub vs_winrate: f64,
    /// Partidas del matchup.
    pub games: i64,
}

/// Filas compactas para "MATCHUPS GENERALES" desde una lista ya recortada
/// (el resultado de [`CounterSummary::strong_against`] o
/// [`CounterSummary::weak_against`]) (puro, testeado).
#[must_use]
pub fn matchup_rows(
    entries: &[CounterEntry],
    catalog: Option<&ChampionCatalog>,
    version: &str,
) -> Vec<MatchupRow> {
    entries
        .iter()
        .map(|entry| {
            let (name, icon) = champ_identity(entry.champion_id, catalog, version);
            MatchupRow {
                champion_id: entry.champion_id,
                name,
                icon,
                vs_winrate: entry.vs_winrate,
                games: entry.games,
            }
        })
        .collect()
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

/// Tab COUNTERS. Re-renderiza cuando el análisis o el contexto publican
/// (locks de enemigos en vivo). Toda la derivación ocurre ANTES del rsx
/// (filas, notas y claves de fila): el bloque RSX solo pinta valores ya
/// resueltos.
#[component]
pub(crate) fn CountersPanel(
    analysis: ReadSignal<AnalysisState>,
    champion_id: i64,
    match_context: ReadSignal<MatchContext>,
    catalog: ReadSignal<Option<ChampionCatalog>>,
    items: ReadSignal<Option<Arc<ItemCatalog>>>,
) -> Element {
    // Snapshots cortos: los guards no cruzan la construcción del rsx.
    let snapshot: AnalysisState = analysis.read().clone();
    let counters = analysis_counters(&snapshot, champion_id);
    let build = analysis_items(&snapshot, champion_id);
    let loading = analysis_in_progress(&snapshot, champion_id);
    let failure = analysis_failure_reason(&snapshot, champion_id);

    let context: MatchContext = match_context.read().clone();
    let champ_catalog = catalog.read().clone();
    let version = champ_catalog
        .as_ref()
        .map(|data| data.version.clone())
        .unwrap_or_default();
    let items_catalog = items.read().clone();

    // Derivación pura (antes del rsx).
    let reason = reason_text(loading, failure).unwrap_or_default();
    let enemy_view: Option<Vec<EnemyRow>> = counters
        .as_deref()
        .map(|summary| enemy_rows(&context.enemies, summary, champ_catalog.as_ref(), &version));
    let matchups_view: Option<(Vec<MatchupRow>, Vec<MatchupRow>)> = counters.as_deref().map(
        |summary| {
            (
                matchup_rows(summary.strong_against(3), champ_catalog.as_ref(), &version),
                matchup_rows(&summary.weak_against(3), champ_catalog.as_ref(), &version),
            )
        },
    );
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

            // COLUMNA IZQUIERDA (55%) · VS ENEMIGOS EN VIVO
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

                // MATCHUPS GENERALES (siempre que la muestra trae counters)
                {match matchups_view {
                    Some((strong, weak)) if !strong.is_empty() || !weak.is_empty() => rsx! {
                        div { class: "cs-matchups",
                            span { class: "label fs-10 t-dim", "Matchups generales" }
                            MatchupList { title: "Fuerte contra", rows: strong, weak: false }
                            MatchupList { title: "Débil contra", rows: weak, weak: true }
                        }
                    },
                    _ => rsx! {},
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

/// Lista compacta de matchups (Fuerte/Débil contra). `weak` pinta los WR
/// en `#ff3355` (matchups que me tratan mal).
#[component]
fn MatchupList(title: &'static str, rows: Vec<MatchupRow>, weak: bool) -> Element {
    let wr_class = if weak {
        "cs-matchup-wr t-bright"
    } else {
        "cs-matchup-wr t-on"
    };
    rsx! {
        div { class: "cs-matchup-group",
            span { class: "label fs-10 t-outline-var", "{title}" }
            {rows.into_iter().map(|row| {
                let name = row.name.clone();
                let icon = row.icon.clone();
                rsx! {
                    div { class: "cs-matchup-row", key: "{row.champion_id}",
                        IconTile { name, icon, size: "32px" }
                        span { class: "cs-matchup-name", "{row.name}" }
                        span { class: "cs-matchup-games tnum",
                            "{format_thousands(row.games)}"
                        }
                        span { class: "num-display tnum {wr_class}",
                            "{format_winrate(row.vs_winrate)}%"
                        }
                    }
                }
            })}
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

    /// Matchups generales: strong toma el prefijo (mejores WR) y weak la
    /// cola invertida (peor primero); los nombres vienen del catálogo.
    #[test]
    fn matchup_rows_recorta_strong_y_weak_con_nombres() {
        let summary = counters();
        let catalog = catalogo();

        let strong = matchup_rows(summary.strong_against(3), Some(&catalog), "16.20.1");
        let ids: Vec<i64> = strong.iter().map(|row| row.champion_id).collect();
        assert_eq!(ids, vec![106, 111, 238]);
        assert_eq!(strong[0].name, "#106", "id sin entrada cae a #id");
        assert_eq!(strong[1].name, "Nautilus");
        assert!((strong[1].vs_winrate - 52.7).abs() < f64::EPSILON);

        let weak = matchup_rows(&summary.weak_against(2), Some(&catalog), "16.20.1");
        let ids: Vec<i64> = weak.iter().map(|row| row.champion_id).collect();
        assert_eq!(ids, vec![34, 238], "peor matchup primero");
        assert!((weak[0].vs_winrate - 43.81).abs() < f64::EPSILON);
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
