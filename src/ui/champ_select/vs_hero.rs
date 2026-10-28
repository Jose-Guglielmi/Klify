//! Hero strip del champ select: la franja superior con MI campeón — una
//! CARD con esquina cortada que adentro tiene el icono cuadrado en marco
//! rojo, el nombre y, a la derecha, un HUD honesto con WR / pickrate /
//! partidas del `CounterSummary`.
//!
//! El rival NO se elige acá: la fila 5v5 (`vs_row::VsTeamRow`) hace
//! clickeables a los enemigos lockeados y publica el objetivo en la señal
//! `rival`, compartida con las tabs "Cómo ganarle" / "Consejos". Este
//! componente solo la lee; el análisis del matchup (línea de WR y bloque
//! "Enfrentamiento") vive en la tab de counters (`counters_panel`), que
//! consume [`RivalCountersState`] vía re-export.
//!
//! Honradez del HUD (omisión > invención): celda sin muestra → `"—"`;
//! dato que el modelo NO trae → la celda directamente no existe. Hoy se
//! omiten BAN (`ChampionDetail` no tiene banrate), el subtítulo
//! (`ChampionEntry` no tiene título) y los chips de rol por lane (ninguna
//! fuente del análisis trae la distribución). Si el día de mañana hay
//! dato, la celda se agrega acá sin tocar el layout.
//!
//! Icono: cuadrado de Data Dragon (`champ_identity`, patrón
//! counters_panel). Sin catálogo o sin versión → placeholder "?".

use dioxus::prelude::*;

use std::sync::Arc;

use crate::data_dragon::item::ItemCatalog;
use crate::data_dragon::models::ChampionCatalog;
use crate::lcu::models::{MatchContext, Position};
use crate::stats::source::analysis::{analysis_counters, AnalysisState};
use crate::stats::source::counters::CounterSummary;
use crate::ui::champ_select::counters_panel::{champ_identity, is_unfavourable};
use crate::ui::format::format_thousands;
use crate::ui::recommendations::format_winrate;

use super::position_label;

/// Formato compacto de partidas (puro, testeado): millones con un decimal
/// (`48.4M`, convención del prototipo); debajo del millón, miles con
/// separador (`48.400`). Sin floats: truncar el decimal es honesto.
pub(crate) fn format_games_compact(games: i64) -> String {
    if games >= 1_000_000 {
        let decimas = games / 100_000; // 48_400_000 → 484
        format!("{}.{:01}M", decimas / 10, decimas % 10)
    } else {
        format_thousands(games)
    }
}

/// Textos del HUD del hero (puro, testeado): `(wr, pickrate, partidas)`
/// desde el `CounterSummary` de MI champ. Dato ausente → `"—"`: la franja
/// nunca muestra un número inventado.
pub(crate) fn hero_hud_texts(summary: Option<&CounterSummary>) -> (String, String, String) {
    let dash = || String::from("—");
    let Some(summary) = summary else {
        return (dash(), dash(), dash());
    };
    (
        summary
            .my_wr
            .map(|wr| format!("{}%", format_winrate(wr)))
            .unwrap_or_else(dash),
        summary
            .pickrate
            .map(|pr| format!("{}%", format_winrate(pr)))
            .unwrap_or_else(dash),
        summary
            .analysed
            .map(|games| format_games_compact(games.max(0)))
            .unwrap_or_else(dash),
    )
}

/// Clase de color del WR del hero (puro, testeado): rojo de marca si es
/// desfavorable (< 50, [`is_unfavourable`]); normal si no lo es; atenuado
/// si no hay muestra (el guión no pinta ni verde ni rojo).
pub(crate) fn hero_wr_class(summary: Option<&CounterSummary>) -> &'static str {
    match summary.and_then(|summary| summary.my_wr) {
        Some(wr) if is_unfavourable(wr) => "t-bright",
        Some(_) => "t-on",
        None => "t-dim",
    }
}

/// Estado de la carga de los counters DEL RIVAL (alimentan la tab de
/// counters: mejores/peores picks contra él). [`crate::ui::champ_select`]
/// lo re-exporta para `counters_panel`.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) enum RivalCountersState {
    #[default]
    Idle,
    Loading { rival_id: i64 },
    Ready { rival_id: i64, summary: CounterSummary },
    Failed { rival_id: i64 },
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

/// Hero strip del champ select: MI campeón arriba (retrato + nombre +
/// HUD WR/PR). El análisis del matchup contra el rival (línea de WR y
/// bloque "Enfrentamiento") vive en la tab de counters.
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
    // Snapshots cortos: los guards no cruzan la construcción del rsx.
    let catalog_snapshot = catalog.read().clone();
    let version = catalog_snapshot
        .as_ref()
        .map(|data| data.version.clone())
        .unwrap_or_default();

    // Icono cuadrado de MI champ (patrón counters_panel): URL solo con
    // versión cargada; sin catálogo/versión → `None` y va el placeholder.
    let (_, my_icon) = champ_identity(champion_id, catalog_snapshot.as_ref(), &version);

    // HUD del hero: métricas globales de MI champ desde el análisis.
    let analysis_snapshot: AnalysisState = analysis.read().clone();
    let counters = analysis_counters(&analysis_snapshot, champion_id);
    let (hud_wr, hud_pr, hud_games) = hero_hud_texts(counters.as_deref());
    let wr_class = hero_wr_class(counters.as_deref());

    let my_lane = position_label(position);

    rsx! {
        div { class: "vs",

            // ----- Hero strip: CARD con esquina cortada (frame aporta el
            // borde 1px) con icono en marco rojo + nombre + HUD a la derecha.
            // Sin subtítulo ni chips de rol: el modelo no trae título ni
            // distribución por lane (ver doc del módulo) -----
            div { class: "frame cut-tr-bl",
                div { class: "cs-hero cut-tr-bl-i bg-surface",
                    "style": "border-bottom: none;",

                    // Icono 112px en marco rojo con esquina cortada: el
                    // background del `.frame` ES el borde (técnica del
                    // sistema, igual que `.is-target`); sin icono → "?".
                    {match my_icon {
                        Some(url) => rsx! {
                            div {
                                class: "frame cs-hero-portrait cut-tr",
                                "style": "background: var(--primary);",
                                img {
                                    class: "cut-tr-i",
                                    src: "{url}",
                                    alt: "{my_name}",
                                    "style": "width: 100%; height: 100%; object-fit: cover; display: block;",
                                }
                            }
                        },
                        None => rsx! {
                            div {
                                class: "cs-hero-portrait",
                                "style": "display: flex; align-items: center; justify-content: center; background: var(--surface-dim);",
                                span { class: "cs-qmark", "?" }
                            }
                        },
                    }}

                    div { class: "cs-hero-body",
                        div { class: "cs-hero-titlerow",
                            h1 { class: "cs-hero-name", "{my_name}" }
                            if !my_lane.is_empty() {
                                span { class: "chip chip--soft", "{my_lane}" }
                            }
                        }
                    }

                    // HUD honesto a la derecha: celdas apiladas separadas
                    // por border-left (como `.hud-item` del prototipo).
                    // BAN se omite: no hay banrate en el modelo.
                    div { class: "cs-hero-hud",
                        "style": "display: flex; flex-wrap: wrap;",
                        div {
                            "style": "display: flex; flex-direction: column; gap: 5px;",
                            span { class: "label fs-10 t-dim", "WR" }
                            span {
                                class: "num-display tnum {wr_class}",
                                "style": "font-size: 26px; line-height: 1;",
                                "{hud_wr}"
                            }
                        }
                        div {
                            "style": "display: flex; flex-direction: column; gap: 5px; padding: 0 18px; border-left: 1px solid var(--outline-var);",
                            span { class: "label fs-10 t-dim", "PR" }
                            span {
                                class: "num-display tnum t-on",
                                "style": "font-size: 26px; line-height: 1;",
                                "{hud_pr}"
                            }
                        }
                        div {
                            "style": "display: flex; flex-direction: column; gap: 5px; padding: 0 18px; border-left: 1px solid var(--outline-var);",
                            span { class: "label fs-10 t-dim", "PARTIDAS" }
                            span {
                                class: "num-display tnum t-on",
                                "style": "font-size: 26px; line-height: 1;",
                                "{hud_games}"
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lcu::models::TeamMember;

    /// HUD del hero: porcentajes formateados y "—" cuando falta la muestra
    /// (nunca un número inventado). Partidas en formato compacto del
    /// prototipo ("48.4M" para millones).
    #[test]
    fn hud_del_hero_es_honesto_sin_muestra() {
        assert_eq!(
            hero_hud_texts(None),
            (
                String::from("—"),
                String::from("—"),
                String::from("—")
            )
        );

        let vacio = CounterSummary {
            my_wr: None,
            pickrate: None,
            analysed: None,
            counters: Vec::new(),
        };
        assert_eq!(
            hero_hud_texts(Some(&vacio)),
            (
                String::from("—"),
                String::from("—"),
                String::from("—")
            ),
            "summary capado sin stats → guiones"
        );

        let summary = CounterSummary {
            my_wr: Some(51.2),
            pickrate: Some(8.8),
            analysed: Some(48_400),
            counters: Vec::new(),
        };
        assert_eq!(
            hero_hud_texts(Some(&summary)),
            (
                String::from("51.2%"),
                String::from("8.8%"),
                String::from("48.400")
            )
        );

        let masivo = CounterSummary {
            my_wr: Some(49.5),
            pickrate: Some(6.7),
            analysed: Some(48_400_000),
            counters: Vec::new(),
        };
        assert_eq!(
            hero_hud_texts(Some(&masivo)).2,
            String::from("48.4M"),
            "millones en formato compacto del prototipo"
        );
    }

    /// Formato compacto de partidas: millones con un decimal, abajo del
    /// millón miles con separador (y los negativos no llegan: el caller
    /// pasa `games.max(0)`).
    #[test]
    fn format_games_compact_usa_m_para_millones() {
        assert_eq!(format_games_compact(48_400_000), "48.4M");
        assert_eq!(format_games_compact(1_500_000), "1.5M");
        assert_eq!(format_games_compact(48_400), "48.400");
        assert_eq!(format_games_compact(0), "0");
    }

    /// Color del WR del hero: rojo de marca solo con muestra desfavorable
    /// (< 50); sin muestra va atenuado, sin semántica.
    #[test]
    fn hero_wr_class_sigue_el_umbral_de_50() {
        assert_eq!(hero_wr_class(None), "t-dim", "sin muestra → neutro");

        let malo = CounterSummary {
            my_wr: Some(49.9),
            pickrate: None,
            analysed: Some(1_000),
            counters: Vec::new(),
        };
        assert_eq!(hero_wr_class(Some(&malo)), "t-bright");

        let bueno = CounterSummary {
            my_wr: Some(50.0),
            pickrate: None,
            analysed: Some(1_000),
            counters: Vec::new(),
        };
        assert_eq!(hero_wr_class(Some(&bueno)), "t-on", "50.0 no es desfavorable");
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
                summoner_id: None,
                summoner_name: None,
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
}
