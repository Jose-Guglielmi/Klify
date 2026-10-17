//! Tab ITEMS del champ select (FASE 11b-2b).
//!
//! Panel a lo ancho con el [`crate::stats::source::items::ItemBuild`] de
//! Lolalytics en secciones apiladas, según el prototipo (ui-preview.html,
//! panel `#panel-items`):
//!
//! - **// Inicio** (starters), **// Botas** (boots), **// Build Core**
//!   (core: tiles numerados 1→N con flechas `→` en orden de compra) y
//!   **// Situacionales** (situational).
//! - Cada tile: icono 44px biselado + nombre del catálogo de ítems de Data
//!   Dragon ([`item_name`], fallback `#id`) + descripción de una línea
//!   (blurb limpio y truncado, [`truncate_line`]) + WR en blanco + partidas
//!   en `on-surface-var`.
//!
//! Regla del rojo (`design.md`): los WR destacados van en BLANCO grande;
//! `#ff0033` queda reservado (solo el orden de compra es metadata neutra).
//!
//! Estados honestos: `Loading` → spinner; sin build (fallos en `Partial`
//! o `Unavailable`) → "Build no disponible · {razón}" con la razón corta
//! del analizador. Toda la derivación de filas es pura y testeada
//! ([`item_rows`], [`row_blurb`], [`truncate_line`]); el RSX solo pinta.

use std::sync::Arc;

use dioxus::prelude::*;

use crate::data_dragon::item::{item_name, ItemCatalog};
use crate::data_dragon::models::ChampionCatalog;
use crate::stats::source::analysis::{
    analysis_failure_reason, analysis_in_progress, analysis_items, AnalysisState,
};
use crate::stats::source::items::ItemStat;
use crate::ui::format::{format_thousands, strip_html};
use crate::ui::recommendations::format_winrate;

/// Máximo de caracteres de la descripción de una fila (una línea del
/// panel; el CSS además corta con line-clamp por si el ancho no da).
pub const BLURB_MAX_CHARS: usize = 90;

/// Fila de ítem lista para render (pura, testeada).
#[derive(Clone, Debug, PartialEq)]
pub struct ItemRow {
    /// Id numérico (clave del icono y fallback del nombre).
    pub item_id: i64,
    /// Nombre visible (`#id` sin catálogo o con nombre vacío).
    pub name: String,
    /// Descripción de una línea (blurb limpio y truncado; vacía sin
    /// catálogo — la fila solo muestra nombre y WR).
    pub desc: String,
    /// URL del icono en la CDN (`None` sin versión de Data Dragon).
    pub icon: Option<String>,
    /// Winrate en porcentaje (de la muestra de Lolalytics).
    pub winrate: f64,
    /// Partidas de la muestra.
    pub games: u64,
}

/// Trunca un texto a una línea de `max_chars` (puro, testeado). La
/// elipsis cuenta dentro del presupuesto: el resultado nunca excede
/// `max_chars` visibles. Un texto corto pasa tal cual (trim de bordes) y
/// el corte nunca parte a la mitad de un grafo Unicode (cuenta `char`s).
#[must_use]
pub fn truncate_line(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    if max_chars == 0 {
        return String::new();
    }
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }
    let cut: String = trimmed.chars().take(max_chars - 1).collect();
    format!("{}…", cut.trim_end())
}

/// Descripción de una fila desde el catálogo (puro, testeado): blurb del
/// ítem limpio de HTML y truncado. Sin catálogo o sin entrada → vacío.
#[must_use]
pub fn row_blurb(catalog: Option<&ItemCatalog>, item_id: i64) -> String {
    catalog
        .and_then(|catalog| catalog.get(item_id))
        .map(|info| truncate_line(&strip_html(&info.blurb), BLURB_MAX_CHARS))
        .unwrap_or_default()
}

/// Filas de una sección del build desde los stats crudos (puro, testeado):
/// nombre contra el catálogo (`#id` degradado), blurb de una línea e icono
/// de la CDN con la versión vigente (`None` sin versión — nunca una URL
/// rota con el componente de versión vacío).
#[must_use]
pub fn item_rows(
    stats: &[ItemStat],
    catalog: Option<&ItemCatalog>,
    version: &str,
) -> Vec<ItemRow> {
    stats
        .iter()
        .map(|stat| ItemRow {
            item_id: stat.item_id,
            name: item_name(catalog, stat.item_id),
            desc: row_blurb(catalog, stat.item_id),
            icon: (!version.is_empty()).then(|| stat.icon_url(version)),
            winrate: stat.winrate,
            games: stat.games,
        })
        .collect()
}

/// Partidas formateadas de una fila (los stats traen `u64` y el
/// formateador es `i64`): con saturación, nunca un pánico por overflow.
#[must_use]
pub fn row_games(games: u64) -> String {
    format_thousands(i64::try_from(games).unwrap_or(i64::MAX))
}

/// Tab ITEMS.
#[component]
pub(crate) fn ItemsPanel(
    analysis: ReadSignal<AnalysisState>,
    champion_id: i64,
    catalog: ReadSignal<Option<ChampionCatalog>>,
    items: ReadSignal<Option<Arc<ItemCatalog>>>,
) -> Element {
    // Snapshot corto: el guard no cruza la construcción del rsx.
    let snapshot: AnalysisState = analysis.read().clone();
    let build = analysis_items(&snapshot, champion_id);
    let loading = analysis_in_progress(&snapshot, champion_id);
    let failure = analysis_failure_reason(&snapshot, champion_id);
    let items_catalog: Option<Arc<ItemCatalog>> = items.read().clone();
    let version = catalog
        .read()
        .as_ref()
        .map(|data| data.version.clone())
        .unwrap_or_default();

    rsx! {
        div { class: "cs-items",
            {match build {
                Some(build) => {
                    let catalog = items_catalog.as_deref();
                    let starters = item_rows(&build.starters, catalog, &version);
                    let boots = item_rows(&build.boots, catalog, &version);
                    let core = item_rows(&build.core, catalog, &version);
                    let situational = item_rows(&build.situational, catalog, &version);

                    if starters.is_empty() && boots.is_empty() && core.is_empty()
                        && situational.is_empty()
                    {
                        rsx! {
                            p { class: "cs-hint", "La fuente no devolvió ítems para este pick." }
                        }
                    } else {
                        rsx! {
                            div { class: "cs-item-sections",
                                ItemSection { label: "Inicio", rows: starters, numbered: false }
                                ItemSection { label: "Botas", rows: boots, numbered: false }
                                ItemSection { label: "Build Core", rows: core, numbered: true }
                                ItemSection { label: "Situacionales", rows: situational, numbered: false }
                            }
                        }
                    }
                }
                None if loading => rsx! {
                    div { class: "cs-loading",
                        div { class: "spinner" }
                        span { class: "label label-md pulse-soft t-silver",
                            "Consultando build de ítems…"
                        }
                    }
                },
                None => {
                    let reason =
                        failure.unwrap_or_else(|| String::from("análisis no disponible"));
                    rsx! {
                        div { class: "cs-unavailable",
                            span { class: "label label-md t-error", "Build no disponible" }
                            p { class: "cs-unavailable-reason", "{reason}" }
                        }
                    }
                }
            }}
        }
    }
}

/// Una sección del build: label `// {label}` + tiles con nombre/WR/juegos.
/// `numbered` (core) agrega el badge de orden 1..N sobre cada tile y la
/// flecha `→` entre tiles (orden de compra del build más jugado).
#[component]
fn ItemSection(label: &'static str, rows: Vec<ItemRow>, numbered: bool) -> Element {
    rsx! {
        section { class: "cs-item-section",
            div { class: "section-head",
                span { class: "bar-skew bar-skew--silver", "style": "width: 24px;" }
                span { class: "label label-md",
                    span { class: "sl sl--silver", "//" }
                    " {label}"
                }
            }

            div { class: "cs-item-rowlist",
                {rows.into_iter().enumerate().map(|(index, row)| {
                    let order = index + 1;
                    rsx! {
                        div { class: "cs-item-step", key: "{row.item_id}",
                            if numbered && index > 0 {
                                span { class: "cs-item-arrow", "aria-hidden": "true", "→" }
                            }
                            div { class: "cs-item",
                                div { class: "cs-item-tilewrap",
                                    {match row.icon {
                                        Some(icon) => rsx! {
                                            div { class: "tile",
                                                img { src: "{icon}", alt: "{row.name}" }
                                            }
                                        },
                                        None => rsx! {
                                            div { class: "tile",
                                                span { class: "cs-tile-fallback", "#" }
                                            }
                                        },
                                    }}
                                    if numbered {
                                        span { class: "cs-item-order", "{order}" }
                                    }
                                }
                                div { class: "cs-item-text",
                                    div { class: "cs-item-name", "{row.name}" }
                                    if !row.desc.is_empty() {
                                        div { class: "cs-item-desc", "{row.desc}" }
                                    }
                                    div { class: "cs-item-stats",
                                        span { class: "num-display tnum cs-item-wr",
                                            "{format_winrate(row.winrate)}%"
                                        }
                                        span { class: "cs-item-games tnum",
                                            span { class: "hud-sep", "·" }
                                            " {row_games(row.games)} partidas"
                                        }
                                    }
                                }
                            }
                        }
                    }
                })}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_dragon::item::ItemInfo;
    use std::collections::HashMap;

    fn stat(item_id: i64, games: u64, wins: u64) -> ItemStat {
        ItemStat {
            item_id,
            games,
            wins,
            winrate: wins as f64 / games as f64 * 100.0,
        }
    }

    /// Catálogo mínimo con dos ítems (uno sin nombre) y un blurb con HTML
    /// más largo que el tope de una línea.
    fn catalogo() -> ItemCatalog {
        let mut by_id = HashMap::new();
        by_id.insert(
            3157,
            ItemInfo {
                name: String::from("Zhonya's Hourglass"),
                blurb: String::from(
                    // Suficientemente largo para truncar con BLURB_MAX_CHARS = 90.
                    "<mainText>Activate to become invincible but unable to take actions for 2.5 seconds, additionally granting massive armor and spell resistance while it lasts</mainText>",
                ),
            },
        );
        by_id.insert(
            3020,
            ItemInfo {
                name: String::new(),
                blurb: String::from("Enhances Move Speed"),
            },
        );
        ItemCatalog {
            version: String::from("16.20.1"),
            by_id,
        }
    }

    /// `truncate_line`: corta con elipsis, respeta cortos y bordes.
    #[test]
    fn truncate_line_corta_con_ellipsis_y_respeta_cortos() {
        assert_eq!(truncate_line("  corto  ", 90), "corto");
        assert_eq!(truncate_line("1234567890", 10), "1234567890");
        assert_eq!(truncate_line("12345678901", 10), "123456789…");
        assert_eq!(truncate_line("", 10), "");
        // Cuenta chars, no bytes (el corte no parte un acento).
        assert_eq!(truncate_line("áéíóú áéíóú", 5), "áéíó…");
        // La elipsis cuenta dentro del presupuesto (6 visibles > 5).
        assert_eq!(truncate_line("123456", 5), "1234…");
        assert_eq!(truncate_line("", 10), "");
    }

    /// Filas con catálogo: nombre real, blurb limpio y truncado a una
    /// línea, icono con la versión y WR/juegos de la muestra.
    #[test]
    fn item_rows_resuelve_nombres_blurb_e_icono_con_catalogo() {
        let stats = vec![stat(3157, 775, 443), stat(3020, 9, 4)];
        let rows = item_rows(&stats, Some(&catalogo()), "16.20.1");

        assert_eq!(rows.len(), 2);

        let zhonyas = &rows[0];
        assert_eq!(zhonyas.item_id, 3157);
        assert_eq!(zhonyas.name, "Zhonya's Hourglass");
        assert_eq!(
            zhonyas.desc,
            truncate_line(
                &strip_html(
                    // Mismo blurb que el fixture de `catalogo()`.
                    "<mainText>Activate to become invincible but unable to take actions for 2.5 seconds, additionally granting massive armor and spell resistance while it lasts</mainText>"
                ),
                BLURB_MAX_CHARS
            )
        );
        assert!(zhonyas.desc.ends_with('…'), "el blurb largo se trunca");
        assert!(zhonyas.desc.len() <= BLURB_MAX_CHARS + "…".len());
        assert_eq!(
            zhonyas.icon.as_deref(),
            Some("https://ddragon.leagueoflegends.com/cdn/16.20.1/img/item/3157.png")
        );
        assert!((zhonyas.winrate - 443.0 / 775.0 * 100.0).abs() < 1e-9);
        assert_eq!(zhonyas.games, 775);

        // Entrada con nombre vacío → #id (degradación honesta).
        assert_eq!(rows[1].name, "#3020");
    }

    /// Sin catálogo: `#id`, descripción vacía; sin versión: sin icono
    /// (nunca una URL con la versión vacía).
    #[test]
    fn item_rows_sin_catalogo_degrada_a_id_y_sin_version_a_sin_icono() {
        let stats = vec![stat(4645, 100, 55)];

        let sin_catalogo = item_rows(&stats, None, "16.20.1");
        assert_eq!(sin_catalogo[0].name, "#4645");
        assert!(sin_catalogo[0].desc.is_empty());
        assert!(sin_catalogo[0].icon.is_some());

        // Con catálogo, el nombre resuelve; sin versión, el icono no se
        // arma con la URL vacía.
        let stats_named = vec![stat(3157, 100, 55)];
        let sin_version = item_rows(&stats_named, Some(&catalogo()), "");
        assert_eq!(sin_version[0].name, "Zhonya's Hourglass");
        assert_eq!(sin_version[0].icon, None);

        let nada = item_rows(&stats_named, None, "");
        assert_eq!(nada[0].name, "#3157");
        assert_eq!(nada[0].icon, None);
    }

    /// Partidas formateadas con miles y saturación para u64 grande.
    #[test]
    fn row_games_formatea_miles_y_satura() {
        assert_eq!(row_games(0), "0");
        assert_eq!(row_games(48_200), "48.200");
        assert_eq!(row_games(u64::MAX), format_thousands(i64::MAX));
    }
}
