//! Vista CHAMP SELECT rediseñada (FASE 11b-2a).
//!
//! Según el prototipo aprobado (ui-preview.html, iteraciones 1-7):
//!
//! - **Fila de equipo en vivo** (5 slots): mi card 200×440 con splash,
//!   degradado, borde inferior rojo + brackets rojos + glow y chips
//!   "BLOQUEADO"/posición; aliados 170×420 (splash + nombre + posición al
//!   lockear, "PICKING…" con spinner mientras eligen, "ESPERANDO" para las
//!   casillas sin miembro).
//! - **Hero strip**: retrato cuadrado del splash, nombre 40px, chips y
//!   métricas HUD reales del [`CounterSummary`]: `{wr}% WR / {pr}%
//!   PICKRATE / {n} PARTIDAS ANALIZADAS`. Sin badge de tier: no hay fuente
//!   honesta para él (el badge del prototipo se omite).
//! - **Tabs**: solo HABILIDADES y RUNAS — ITEMS y COUNTERS quedan FUERA de
//!   la UI hasta 11b-2b (decisión: pestañas visibles pero sin datoswould
//!   ser ruido; los datos de items ya viajan en [`AnalysisState`] para
//!   esa fase).
//! - **TAB HABILIDADES** ([`skills_panel`]): "// BASIC KIT" (pasiva + QWER
//!   con hover/click) y panel contextual con la ficha del hechizo o el
//!   estado honesto del orden de subida.
//! - **TAB RUNAS** ([`runes_panel`]): páginas verticales por árbol desde
//!   [`RecommendationsState`], con descripciones de runas y shards
//!   nombrados.
//! - **TAB ITEMS** ([`items_panel`], FASE 11b-2b): build de Lolalytics en
//!   secciones apiladas (Inicio/Botas/Build Core/Situacionales) con nombres
//!   y descripciones del catálogo de ítems de Data Dragon.
//! - **TAB COUNTERS** ([`counters_panel`], FASE 11b-2b): dos columnas
//!   55/45 — VS enemigos en vivo + matchups generales a la izquierda,
//!   sugerencias vs comp a la derecha (cruce honesto con la tabla
//!   anti-enemigo).
//!
//! Toda la derivación de datos para el render es pura y testeada:
//! [`current_champion`] y [`position_label`].

use dioxus::prelude::*;

use std::sync::Arc;

use crate::data_dragon::champion_detail::load_champion_detail;
use crate::data_dragon::item::ItemCatalog;
use crate::data_dragon::models::ChampionCatalog;
use crate::data_dragon::runes::RuneCatalog;
use crate::lcu::models::{AppState, ChampionInfo, MatchContext, Position};
use crate::stats::aggregation::patch_of;
use crate::stats::source::analysis::AnalysisState;
use crate::stats::source::counters::fetch_counters_cached;
use crate::stats::source::player_tips::{fetch_player_tips_cached, PlayerTip};
use crate::stats::source::RecommendationsState;
use crate::ui::shell::{KlifyFooter, KlifyHeader};
use vs_hero::rival_lane_from_context;

mod champ_art;
mod counters_panel;
mod items_panel;
mod rival_panels;
mod runes_panel;
mod skills_panel;
mod vs_hero;

pub(crate) use vs_hero::{RivalCountersState, RivalDetailState, RivalTipsState};


/// Etiqueta corta de una posición para chips y labels (pura, testeada):
/// convención del prototipo (`TOP/JGL/MID/BOT/SUP`); sin posición → vacío.
#[must_use]
pub fn position_label(position: Option<Position>) -> &'static str {
    match position {
        Some(Position::Top) => "TOP",
        Some(Position::Jungle) => "JGL",
        Some(Position::Middle) => "MID",
        Some(Position::Bottom) => "BOT",
        Some(Position::Utility) => "SUP",
        None => "",
    }
}

/// Pick local dentro del [`AppState`] (puro, testeado): `Some` solo en
/// `InChampSelect(Some(champion))`.
#[must_use]
pub fn current_champion(state: &AppState) -> Option<ChampionInfo> {
    match state {
        AppState::InChampSelect(champion) => champion.clone(),
        AppState::Disconnected | AppState::WaitingForChampSelect => None,
    }
}

/// Tab activa del análisis (pura, testeada).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum CsTab {
    /// Pasiva + QWER con ficha contextual.
    #[default]
    Skills,
    /// Páginas de runas recomendadas.
    Runes,
    /// Build de ítems del parche (Lolalytics + catálogo ddragon).
    Items,
    /// Counters en vivo contra la comp + matchups generales.
    Counters,
    /// Tips oficiales (Riot) + sugerido contra el rival elegido.
    VsTips,
    /// Consejos de jugadores (counterstats/MOBAFire) traducidos.
    PlayerTips,
}

/// Champ select en vivo (11b-2a). Las señales llegan como [`ReadSignal`]
/// para que las piezas internas re-rendericen cuando los loops publiquen
/// (aliados lockeando, análisis, recomendaciones).
#[component]
pub fn ChampSelectView(
    state: ReadSignal<AppState>,
    match_context: ReadSignal<MatchContext>,
    catalog: ReadSignal<Option<ChampionCatalog>>,
    runes: ReadSignal<Option<RuneCatalog>>,
    items: ReadSignal<Option<Arc<ItemCatalog>>>,
    recommendations: ReadSignal<RecommendationsState>,
    analysis: ReadSignal<AnalysisState>,
    patch_version: Option<String>,
    source_served: bool,
    retry_runes: Signal<u64>,
    in_game: ReadSignal<bool>,
) -> Element {
    // Snapshot corto: el guard no cruza la construcción del rsx.
    let state_snapshot: AppState = state.read().clone();
    let champion = current_champion(&state_snapshot);
    let in_game_snapshot = *in_game.read();

    // Señales del RIVAL elegido: compartidas entre la franja/split del modo
    // VS y las tabs "Cómo jugar vs" / "Consejos". El effect de carga vive
    // acá (una sola vez por elección de rival).
    let rival: Signal<Option<i64>> = use_signal(|| None);
    let mut rival_detail: Signal<RivalDetailState> = use_signal(RivalDetailState::default);
    let mut rival_counters: Signal<RivalCountersState> = use_signal(RivalCountersState::default);
    let mut rival_tips: Signal<RivalTipsState> = use_signal(RivalTipsState::default);

    let patch_for_effect = patch_version.clone();
    use_effect(move || {
        let rival_id = rival();
        let catalog_snapshot = catalog.read().clone();
        let state_for_position = state.read().clone();
        let my_position = champion_position(&state_for_position, &match_context);
        let patch = patch_for_effect
            .as_deref()
            .map(patch_of)
            .filter(|value| !value.is_empty())
            .or_else(|| {
                catalog_snapshot
                    .as_ref()
                    .map(|data| patch_of(&data.version))
                    .filter(|value| !value.is_empty())
            })
            .unwrap_or_default();

        spawn(async move {
            let Some(id) = rival_id else {
                if !matches!(rival_detail(), RivalDetailState::Idle) {
                    rival_detail.set(RivalDetailState::Idle);
                }
                if !matches!(rival_counters(), RivalCountersState::Idle) {
                    rival_counters.set(RivalCountersState::Idle);
                }
                if !matches!(rival_tips(), RivalTipsState::Idle) {
                    rival_tips.set(RivalTipsState::Idle);
                }
                return;
            };
            // ¿Ya cargado o cargando este mismo rival? Evita refetchs.
            match &*rival_detail.read() {
                RivalDetailState::Loading { rival_id: current }
                | RivalDetailState::Ready { rival_id: current, .. }
                    if *current == id =>
                {
                    return;
                }
                _ => {}
            }

            let Some(entry) = catalog_snapshot.as_ref().and_then(|data| data.get(id)) else {
                return;
            };
            let version = catalog_snapshot
                .as_ref()
                .map(|data| data.version.clone())
                .unwrap_or_default();
            if version.is_empty() {
                rival_detail.set(RivalDetailState::Failed { rival_id: id });
                rival_tips.set(RivalTipsState::Failed { rival_id: id });
                return;
            }

            // Lane del matchup: la posición DEL RIVAL si el lock la trajo,
            // si no MI posición; `None` → muestra agregada de la fuente.
            let context_snapshot = match_context.read().clone();
            let rival_lane = rival_lane_from_context(&context_snapshot, Some(id), my_position)
                .or(my_position);

            let ddragon_id = entry.ddragon_id.clone();
            rival_detail.set(RivalDetailState::Loading { rival_id: id });
            rival_counters.set(RivalCountersState::Loading { rival_id: id });
            rival_tips.set(RivalTipsState::Loading { rival_id: id });
            let client = match crate::data_dragon::http_client() {
                Ok(client) => client,
                Err(_) => {
                    rival_detail.set(RivalDetailState::Failed { rival_id: id });
                    rival_counters.set(RivalCountersState::Failed { rival_id: id });
                    rival_tips.set(RivalTipsState::Failed { rival_id: id });
                    return;
                }
            };

            let detail_task = load_champion_detail(&client, &version, &ddragon_id);
            let counters_task = fetch_counters_cached(&client, &entry.slug, rival_lane, &patch);
            let tips_task = fetch_player_tips_cached(&client, &entry.slug);
            let (detail_result, counters_result, tips_result) =
                tokio::join!(detail_task, counters_task, tips_task);

            match counters_result {
                Ok(summary) => {
                    rival_counters.set(RivalCountersState::Ready { rival_id: id, summary });
                }
                Err(_) => rival_counters.set(RivalCountersState::Failed { rival_id: id }),
            }

            // Tips: oficiales (enemytips del detalle) + jugadores, ambos
            // pasados por traducción EN→ES best-effort (falla → original).
            let detail_ok = detail_result.ok();
            let tips_ok = tips_result.ok();
            if detail_ok.is_none() {
                rival_detail.set(RivalDetailState::Failed { rival_id: id });
            }
            if detail_ok.is_none() && tips_ok.is_none() {
                rival_tips.set(RivalTipsState::Failed { rival_id: id });
            } else {
                let official_src: Vec<String> = detail_ok
                    .as_ref()
                    .map(|detail| detail.enemytips.clone())
                    .unwrap_or_default();
                let mut texts = official_src.clone();
                texts.extend(tips_ok.iter().flatten().map(|tip| tip.text.clone()));

                let translated =
                    crate::stats::source::translate::translate_all_en_to_es(&client, &texts)
                        .await;

                let official = Arc::new(translated[..official_src.len()].to_vec());
                let players = Arc::new(
                    tips_ok
                        .unwrap_or_default()
                        .into_iter()
                        .enumerate()
                        .map(|(index, mut tip)| {
                            tip.text = translated[official_src.len() + index].clone();
                            tip
                        })
                        .collect::<Vec<PlayerTip>>(),
                );
                rival_tips.set(RivalTipsState::Ready {
                    rival_id: id,
                    official,
                    players,
                });
            }
            if let Some(detail) = detail_ok {
                rival_detail.set(RivalDetailState::Ready {
                    rival_id: id,
                    detail: Box::new(detail),
                });
            }
        });
    });

    rsx! {
        div { class: "view",
            KlifyHeader { state }

            main { class: "main-container",

                    // ZONA 3 · ANÁLISIS DEL PICK
                {match champion {
                    Some(champion) => {
                        rsx! {
                            div { class: "section-head section",
                                span { class: "bar-skew", "style": "width: 32px;" }
                                span { class: "label label-md",
                                    span { class: "sl", "//" }
                                    " Análisis "
                                    span { class: "hud-sep", "·" }
                                    " {champion.name}"
                                }
                                if in_game_snapshot {
                                    span { class: "chip chip--soft", "Partida en vivo" }
                                }
                            }
                            div { class: "frame cut-tr-bl cs-analysis",
                                div { class: "cut-tr-bl-i bg-low",
                                    vs_hero::VsHero {
                                        champion_id: champion.id,
                                        my_name: champion.name.clone(),
                                        position: champion_position(&state_snapshot, &match_context),
                                        match_context,
                                        catalog,
                                        analysis,
                                        items,
                                        rival,
                                        rival_counters,
                                    }
                                    AnalysisTabs {
                                        champion_id: champion.id,
                                        analysis,
                                        match_context,
                                        catalog,
                                        items,
                                        runes,
                                        recommendations,
                                        retry_runes,
                                        rival,
                                        rival_detail,
                                        rival_tips,
                                    }
                                }
                            }
                        }
                    }
                    None => rsx! {
                        div { class: "frame cut-tr-bl cs-analysis",
                            div { class: "cut-tr-bl-i bg-low",
                                div { class: "cs-waiting",
                                    div { class: "spinner" }
                                    span { class: "label label-md pulse-soft t-silver",
                                        "Esperando selección…"
                                    }
                                }
                            }
                        }
                    },
                }}
            }

            KlifyFooter { source_served, version: patch_version }
        }
    }
}

/// Posición del jugador local desde el snapshot del contexto (los chips del
/// hero la muestran). Helper interno sin señales.
fn champion_position(
    state: &AppState,
    match_context: &ReadSignal<MatchContext>,
) -> Option<Position> {
    current_champion(state)?;
    match_context.read().my_position
}

/// Tabs del análisis + panel activo (FASE 11b-2b: las cuatro con datos
/// reales).
#[component]
fn AnalysisTabs(
    champion_id: i64,
    analysis: ReadSignal<AnalysisState>,
    match_context: ReadSignal<MatchContext>,
    catalog: ReadSignal<Option<ChampionCatalog>>,
    items: ReadSignal<Option<Arc<ItemCatalog>>>,
    runes: ReadSignal<Option<RuneCatalog>>,
    recommendations: ReadSignal<RecommendationsState>,
    retry_runes: Signal<u64>,
    rival: Signal<Option<i64>>,
    rival_detail: Signal<RivalDetailState>,
    rival_tips: Signal<RivalTipsState>,
) -> Element {
    let mut active_tab = use_signal(CsTab::default);

    rsx! {
        div { class: "cs-tabs",
            button {
                class: if active_tab() == CsTab::Skills { "tab tab--active" } else { "tab" },
                "role": "tab",
                "aria-selected": if active_tab() == CsTab::Skills { "true" } else { "false" },
                onclick: move |_| active_tab.set(CsTab::Skills),
                "Habilidades"
            }
            button {
                class: if active_tab() == CsTab::Runes { "tab tab--active" } else { "tab" },
                "role": "tab",
                "aria-selected": if active_tab() == CsTab::Runes { "true" } else { "false" },
                onclick: move |_| active_tab.set(CsTab::Runes),
                "Runas"
            }
            button {
                class: if active_tab() == CsTab::Items { "tab tab--active" } else { "tab" },
                "role": "tab",
                "aria-selected": if active_tab() == CsTab::Items { "true" } else { "false" },
                onclick: move |_| active_tab.set(CsTab::Items),
                "Items"
            }
            button {
                class: if active_tab() == CsTab::Counters { "tab tab--active" } else { "tab" },
                "role": "tab",
                "aria-selected": if active_tab() == CsTab::Counters { "true" } else { "false" },
                onclick: move |_| active_tab.set(CsTab::Counters),
                "Counters"
            }
            button {
                class: if active_tab() == CsTab::VsTips { "tab tab--active" } else { "tab" },
                "role": "tab",
                "aria-selected": if active_tab() == CsTab::VsTips { "true" } else { "false" },
                onclick: move |_| active_tab.set(CsTab::VsTips),
                "Cómo jugar vs"
            }
            button {
                class: if active_tab() == CsTab::PlayerTips { "tab tab--active" } else { "tab" },
                "role": "tab",
                "aria-selected": if active_tab() == CsTab::PlayerTips { "true" } else { "false" },
                onclick: move |_| active_tab.set(CsTab::PlayerTips),
                "Consejos"
            }
            span { class: "bracket", "aria-hidden": "true",
                i {}
                i {}
                i {}
                i {}
            }
        }

        {match active_tab() {
            CsTab::Skills => rsx! {
                skills_panel::SkillsPanel { analysis, champion_id, catalog }
            },
            CsTab::Runes => rsx! {
                runes_panel::RunesPanel { recommendations, runes, retry: retry_runes }
            },
            CsTab::Items => rsx! {
                items_panel::ItemsPanel { analysis, champion_id, catalog, items }
            },
            CsTab::Counters => rsx! {
                counters_panel::CountersPanel { analysis, champion_id, match_context, catalog, items }
            },
            CsTab::VsTips => rsx! {
                rival_panels::VsTipsPanel {
                    my_champion_id: champion_id,
                    rival,
                    catalog,
                    analysis,
                    items,
                    detail: rival_detail,
                    tips: rival_tips,
                }
            },
            CsTab::PlayerTips => rsx! {
                rival_panels::PlayerTipsPanel { rival, tips: rival_tips }
            },
        }}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Etiquetas cortas de posición y vacío sin posición.
    #[test]
    fn position_label_usa_la_convencion_del_prototipo() {
        assert_eq!(position_label(Some(Position::Top)), "TOP");
        assert_eq!(position_label(Some(Position::Jungle)), "JGL");
        assert_eq!(position_label(Some(Position::Middle)), "MID");
        assert_eq!(position_label(Some(Position::Bottom)), "BOT");
        assert_eq!(position_label(Some(Position::Utility)), "SUP");
        assert_eq!(position_label(None), "");
    }
}
