//! Tabs "CÓMO JUGAR VS" y "CONSEJOS" del análisis (FASE 12): consejos
//! oficiales de Riot + sugerido, y consejos de jugadores
//! (counterstats/MOBAFire) — todos contra el RIVAL elegido y traducidos.

use dioxus::prelude::*;

use std::sync::Arc;

use crate::data_dragon::item::ItemCatalog;
use crate::data_dragon::models::ChampionCatalog;
use crate::stats::source::analysis::{analysis_items, AnalysisState};
use crate::ui::champ_select::items_panel::row_games;
use crate::ui::champ_select::vs_hero::{
    vs_suggestions, RivalDetailState, RivalTipsState, VsSuggestState,
};
use crate::ui::recommendations::format_winrate;

/// Nombre visible del rival (catálogo; fallback `#id`) — puro, testeado.
#[must_use]
fn rival_display(
    rival_id: Option<i64>,
    catalog: Option<&ChampionCatalog>,
) -> String {
    match rival_id.and_then(|id| catalog.and_then(|data| data.get(id))) {
        Some(entry) => entry.name.clone(),
        None => match rival_id {
            Some(id) => format!("#{id}"),
            None => String::from("—"),
        },
    }
}

/// Aviso cuando no hay rival elegido todavía.
#[component]
fn NoRivalHint() -> Element {
    rsx! {
        div { class: "cs-waiting",
            span { class: "cs-qmark", "?" }
            p { class: "cs-hint",
                "Elegí un rival para ver esta tab: click en un enemigo de la franja, en el botón ELEGIR RIVAL del split, o en la tab COUNTERS."
            }
        }
    }
}

/// Tab "CÓMO JUGAR VS": tips oficiales de Riot (traducidos) + sugerido de
/// ítems contra el rival.
#[component]
pub(crate) fn VsTipsPanel(
    my_champion_id: i64,
    rival: Signal<Option<i64>>,
    catalog: ReadSignal<Option<ChampionCatalog>>,
    analysis: ReadSignal<AnalysisState>,
    items: ReadSignal<Option<Arc<ItemCatalog>>>,
    detail: ReadSignal<RivalDetailState>,
    tips: ReadSignal<RivalTipsState>,
) -> Element {
    let rival_id = rival();
    let catalog_snapshot = catalog.read().clone();
    let version = catalog_snapshot
        .as_ref()
        .map(|data| data.version.clone())
        .unwrap_or_default();
    let rival_name = rival_display(rival_id, catalog_snapshot.as_ref());

    let detail_snapshot: RivalDetailState = detail.read().clone();
    let tips_snapshot: RivalTipsState = tips.read().clone();
    let oficiales: Vec<String> = match (&tips_snapshot, rival_id) {
        (RivalTipsState::Ready { rival_id: current, official, .. }, Some(rid))
            if *current == rid =>
        {
            official.iter().cloned().collect()
        }
        _ => Vec::new(),
    };

    let items_catalog = items.read().clone();
    let suggestions = rival_id
        .map(|id| {
            vs_suggestions(
                analysis_items(&analysis.read().clone(), my_champion_id).as_deref(),
                items_catalog.as_deref(),
                &version,
                id,
            )
        })
        .unwrap_or(VsSuggestState::NoBuild);

    rsx! {
        div { class: "vs-info-cols",
            div { class: "vs-info-col",
                div { class: "section-head",
                    span { class: "bar-skew", "style": "width: 20px;" }
                    span { class: "label label-md",
                        span { class: "sl", "//" }
                        " Cómo jugar contra {rival_name}"
                    }
                }
                {match rival_id {
                    None => rsx! { NoRivalHint {} },
                    Some(rid) => rsx! {
                        {match &detail_snapshot {
                            RivalDetailState::Ready { rival_id: current, detail } if *current == rid => {
                                let tips = if oficiales.is_empty() {
                                    detail.enemytips.clone()
                                } else {
                                    oficiales.clone()
                                };
                                if tips.is_empty() {
                                    rsx! {
                                        p { class: "cs-hint", "Sin tips oficiales para este campeón." }
                                    }
                                } else {
                                    rsx! {
                                        ul { class: "vs-tips",
                                            {tips.iter().enumerate().map(|(index, tip)| rsx! {
                                                li { key: "tip-{index}", "{tip}" }
                                            })}
                                        }
                                    }
                                }
                            }
                            RivalDetailState::Loading { .. } => rsx! {
                                div { class: "cs-loading",
                                    div { class: "spinner" }
                                    span { class: "label label-md pulse-soft t-silver", "Cargando consejos…" }
                                }
                            },
                            _ => rsx! {
                                p { class: "cs-hint", "Consejos no disponibles ahora." }
                            },
                        }}
                    },
                }}
            }

            div { class: "vs-info-col",
                div { class: "section-head",
                    span { class: "bar-skew", "style": "width: 20px;" }
                    span { class: "label label-md",
                        span { class: "sl", "//" }
                        " Sugerido vs {rival_name}"
                    }
                }
                {match rival_id {
                    None => rsx! { NoRivalHint {} },
                    Some(_) => rsx! {
                        {match &suggestions {
                            VsSuggestState::Rows(rows) => rsx! {
                                div { class: "vs-suggest",
                                    {rows.iter().map(|row| {
                                        let games = row_games(row.games);
                                        rsx! {
                                            div { class: "cs-vsitem", key: "sug-{row.item_id}",
                                                div { class: "tile", "style": "width: 44px; height: 44px;",
                                                    {match &row.icon {
                                                        Some(icon) => rsx! { img { src: "{icon}", alt: "{row.name}" } },
                                                        None => rsx! { span { class: "cs-tile-fallback", "#" } },
                                                    }}
                                                }
                                                div { class: "cs-vsitem-text",
                                                    div { class: "cs-vsitem-name", "{row.name}" }
                                                    if !row.desc.is_empty() {
                                                        div { class: "cs-vsitem-desc", "{row.desc}" }
                                                    }
                                                }
                                                div { class: "cs-vsitem-wrcol",
                                                    div { class: "num-display tnum cs-vsitem-wr t-on", "{format_winrate(row.winrate)}%" }
                                                    div { class: "tnum cs-vsitem-games", "{games} partidas" }
                                                }
                                            }
                                        }
                                    })}
                                }
                            },
                            VsSuggestState::Empty => rsx! {
                                p { class: "cs-hint",
                                    "Sin sugerencia específica contra este rival: tu build principal está en la tab ITEMS."
                                }
                            },
                            VsSuggestState::NoBuild => rsx! {
                                div { class: "cs-loading",
                                    div { class: "spinner" }
                                    span { class: "label label-md pulse-soft t-silver", "Leyendo tu build…" }
                                }
                            },
                        }}
                        p { class: "cs-hint vs-note",
                            "Las runas de la tab RUNAS ya están optimizadas para tu champ y posición; no existe fuente por-matchup."
                        }
                    },
                }}
            }
        }
    }
}

/// Tab "CONSEJOS": grilla de consejos de jugadores traducidos.
#[component]
pub(crate) fn PlayerTipsPanel(
    rival: Signal<Option<i64>>,
    tips: ReadSignal<RivalTipsState>,
) -> Element {
    let rival_id = rival();
    let tips_snapshot: RivalTipsState = tips.read().clone();
    let player_tips: Vec<crate::stats::source::player_tips::PlayerTip> =
        match (&tips_snapshot, rival_id) {
            (RivalTipsState::Ready { rival_id: current, players, .. }, Some(rid))
                if *current == rid =>
            {
                players.iter().take(12).cloned().collect()
            }
            _ => Vec::new(),
        };

    rsx! {
        div { class: "vs-ptips-grid",
            {match rival_id {
                None => rsx! { NoRivalHint {} },
                Some(rid) => rsx! {
                    {match &tips_snapshot {
                        RivalTipsState::Ready { rival_id: current, .. } if *current == rid => {
                            if player_tips.is_empty() {
                                rsx! {
                                    p { class: "cs-hint", "Sin consejos de jugadores para este campeón." }
                                }
                            } else {
                                rsx! {
                                    {player_tips.iter().enumerate().map(|(index, tip)| rsx! {
                                        div { class: "vs-ptip", key: "ptip-{index}",
                                            p { class: "vs-ptip-text", "“{tip.text}”" }
                                            div { class: "vs-ptip-meta",
                                                span { class: "vs-ptip-author", "{tip.author}" }
                                                if let Some(role) = &tip.author_role {
                                                    span { class: "vs-ptip-role tnum", " · {role}" }
                                                }
                                            }
                                        }
                                    })}
                                }
                            }
                        }
                        RivalTipsState::Loading { .. } => rsx! {
                            div { class: "cs-loading",
                                div { class: "spinner" }
                                span { class: "label label-md pulse-soft t-silver", "Buscando consejos de jugadores…" }
                            }
                        },
                        RivalTipsState::Failed { .. } => rsx! {
                            p { class: "cs-hint", "Consejos de jugadores no disponibles ahora." }
                        },
                        _ => rsx! {
                            p { class: "cs-hint", "Elegí un rival para ver los consejos." }
                        },
                    }}
                },
            }}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Nombre del rival: catálogo, degradación a #id y vacío sin rival.
    #[test]
    fn rival_display_cubre_los_tres_estados() {
        use crate::data_dragon::models::ChampionEntry;
        use std::collections::HashMap;

        let mut by_id = HashMap::new();
        by_id.insert(
            64,
            ChampionEntry {
                name: String::from("Kayn"),
                image_url: String::new(),
                slug: String::from("kayn"),
                ddragon_id: String::from("Kayn"),
            },
        );
        let catalog = ChampionCatalog {
            version: String::from("16.20.1"),
            by_id,
        };

        assert_eq!(rival_display(Some(64), Some(&catalog)), "Kayn");
        assert_eq!(rival_display(Some(999), Some(&catalog)), "#999");
        assert_eq!(rival_display(None, Some(&catalog)), "—");
    }
}
