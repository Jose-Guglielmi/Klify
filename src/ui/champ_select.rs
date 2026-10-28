//! Vista CHAMP SELECT rediseñada (FASE 11b-2a).
//!
//! Según el prototipo aprobado (ui-preview.html, iteraciones 1-7):
//!
//! - **Headers de sección** (prototipo ui-preview-sylas.html): "// CHAMP
//!   SELECT" con nota de draft sobre la fila 5v5 y "// GUÍA DE MATCHUP" con
//!   el matchup corriente sobre las tabs, con el banner de matchup
//!   standalone ([`vs_guide::MatchupBanner`]) entre la fila y las tabs.
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

use crate::data_dragon::item::ItemCatalog;
use crate::data_dragon::models::ChampionCatalog;
use crate::data_dragon::runes::RuneCatalog;
use crate::lcu::models::{AppState, ChampionInfo, MatchContext, Position};
use crate::stats::aggregation::patch_of;
use crate::stats::source::analysis::AnalysisState;
use crate::stats::source::counters::fetch_counters_cached;
use crate::stats::source::RecommendationsState;
use crate::ui::shell::{KlifyFooter, KlifyHeader};
use vs_hero::rival_lane_from_context;

pub(crate) mod champ_art;
mod counters_panel;
mod items_panel;
mod players_panel;
mod runes_panel;
mod skills_panel;
mod vs_guide;
pub(crate) mod vs_guide_data;
mod vs_hero;
mod vs_row;

pub(crate) use vs_hero::RivalCountersState;


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
    /// Consejos de MI champ desde su ficha embebida (vs_guide).
    PlayerTips,
}

/// Champ select en vivo (11b-2a). Las señales llegan como [`ReadSignal`]
/// para que las piezas internas re-rendericen cuando los loops publiquen
/// (aliados lockeando, análisis, recomendaciones).
///
/// Con `manual == true` (modo manual, sin cliente de LoL) se omite el HUD
/// de draft y la fila 5v5: la vista queda como ficha de análisis pura
/// (Skills/Runes/Items/Counters/VsTips/Consejos) con botón de salida al
/// menú. Con `manual == false` el render es exactamente el de siempre.
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
    /// True cuando la vista se abrió desde el menú sin cliente de LoL.
    manual: bool,
    /// True cuando el cliente de LoL está conectado (lo consume la app;
    /// se reenvía a los paneles de Runas e Items).
    client_connected: bool,
    /// Vuelve al menú (solo se dispara en modo manual).
    on_exit_manual: EventHandler<()>,
) -> Element {
    // Snapshot corto: el guard no cruza la construcción del rsx.
    let state_snapshot: AppState = state.read().clone();
    let champion = current_champion(&state_snapshot);
    let in_game_snapshot = *in_game.read();

    // Señal del RIVAL elegido + carga de sus counters (compartidas con el
    // hero strip y la tab Counters); la tab "Cómo ganarle" lee la ficha
    // embebida sin red. El effect de carga vive acá (una sola vez por
    // elección de rival).
    let mut rival: Signal<Option<i64>> = use_signal(|| None);
    let mut rival_counters: Signal<RivalCountersState> = use_signal(RivalCountersState::default);

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
                if !matches!(rival_counters(), RivalCountersState::Idle) {
                    rival_counters.set(RivalCountersState::Idle);
                }
                return;
            };
            // ¿Ya cargado o cargando este mismo rival? Evita refetchs.
            match &*rival_counters.read() {
                RivalCountersState::Loading { rival_id: current }
                | RivalCountersState::Ready { rival_id: current, .. }
                    if *current == id =>
                {
                    return;
                }
                _ => {}
            }

            let Some(entry) = catalog_snapshot.as_ref().and_then(|data| data.get(id)) else {
                return;
            };

            // Lane del matchup: la posición DEL RIVAL si el lock la trajo,
            // si no MI posición; `None` → muestra agregada de la fuente.
            let context_snapshot = match_context.read().clone();
            let rival_lane = rival_lane_from_context(&context_snapshot, Some(id), my_position)
                .or(my_position);

            rival_counters.set(RivalCountersState::Loading { rival_id: id });
            let client = match crate::data_dragon::http_client() {
                Ok(client) => client,
                Err(_) => {
                    rival_counters.set(RivalCountersState::Failed { rival_id: id });
                    return;
                }
            };

            let counters_result =
                fetch_counters_cached(&client, &entry.slug, rival_lane, &patch).await;
            match counters_result {
                Ok(summary) => {
                    rival_counters.set(RivalCountersState::Ready { rival_id: id, summary });
                }
                Err(_) => rival_counters.set(RivalCountersState::Failed { rival_id: id }),
            }
        });
    });

    // Giro de diseño (modo manual): en la ficha el RIVAL es el propio
    // champ — no hay fila 5v5 que lo elija. Sincronizo la señal con el
    // champ vigente: leo `state` adentro (su fuente reactiva) para que al
    // entrar a una ficha o cambiar de ficha el rival apunte al de la
    // ficha. En champ select REAL el guard corta antes y el effect no
    // escribe nada (la fila 5v5 sigue mandando).
    use_effect(move || {
        if !manual {
            return;
        }
        let state_snapshot = state.read().clone();
        let ficha = current_champion(&state_snapshot);
        rival.set(ficha.map(|champ| champ.id));
    });

    rsx! {
        div { class: "view",
            KlifyHeader { state }

            main { class: "main-container",
               
                {match champion {
                    Some(champion) => {
                        // Snapshots y derivación ANTES del rsx (convención de
                        // la app): posición propia, rival lockeado, nombres
                        // del catálogo y notas de los headers de sección.
                        let my_position = champion_position(&state_snapshot, &match_context);
                        let context_snapshot = match_context.read().clone();
                        let rival_id_snapshot = rival();
                        let catalog_snapshot = catalog.read().clone();

                        // Nota del header de la fila: el parche solo si la
                        // prop lo trae (sin " · PATCH …" cuando es None).
                        let patch_note = patch_version
                            .as_deref()
                            .filter(|value| !value.is_empty())
                            .map(str::to_string);
                        let has_patch_note = patch_note.is_some();
                        let patch_note_value = patch_note.unwrap_or_default();

                        // Nota del header de las tabs: con rival lockeado
                        // suma el lado rival (posición desde el contexto,
                        // mismo criterio que el hero); sin rival, solo el
                        // propio. Rival sin catálogo degrada a "#id".
                        let rival_position_snapshot =
                            rival_lane_from_context(&context_snapshot, rival_id_snapshot, my_position);
                        let my_side = labeled_side(&champion.name, my_position);
                        let rival_side = rival_id_snapshot.map(|id| {
                            let rival_name = catalog_snapshot
                                .as_ref()
                                .and_then(|data| data.get(id))
                                .map(|entry| entry.name.clone())
                                .unwrap_or_else(|| format!("#{id}"));
                            format!(" · VS {}", labeled_side(&rival_name, rival_position_snapshot))
                        });
                        let guide_note = format!("{my_side}{}", rival_side.unwrap_or_default());

                        rsx! {
                            div { class: "frame cut-tr-bl cs-analysis",
                                div { class: "cut-tr-bl-i bg-low",
                                    // HUD del cliente (header "// CHAMP
                                    // SELECT" con el draft en vivo + fila
                                    // 5v5 con rivales clickeables): SOLO con
                                    // lobby real. En modo manual se omite
                                    // entero — sin draft ni fila 5v5, la
                                    // señal `rival` la sincroniza el effect
                                    // con el champ de la ficha.
                                    if !manual {
                                        // Header de la fila (prototipo
                                        // ui-preview-sylas.html): "// CHAMP
                                        // SELECT" + nota del draft a la derecha.
                                        // `margin-top: 0` inline: anula el genérico
                                        // de `.section-head--cs` (32px) para que el
                                        // frame no arrastre una franja gris vacía
                                        // sobre la fila 5v5.
                                        div { class: "section-head section-head--cs",
                                            "style": "margin-top: 0; margin-bottom: 8px; flex-wrap: wrap;",
                                            span { class: "bar-skew", "style": "width: 24px;" }
                                            span { class: "label label-md",
                                                span { class: "sl", "//" }
                                                " CHAMP SELECT"
                                            }
                                            span { class: "label fs-10 t-outline-var ml-auto",
                                                "style": "min-width: 0; overflow-wrap: anywhere; text-align: right;",
                                                "DRAFT EN VIVO "
                                                span { class: "hud-sep", "·" }
                                                " PLATINUM+"
                                                if has_patch_note {
                                                    span { class: "hud-sep", "·" }
                                                    " PATCH {patch_note_value}"
                                                }
                                            }
                                            // Indicador de partida en vivo (antes
                                            // vivía en el header "Análisis").
                                            if in_game_snapshot {
                                                span { class: "chip chip--soft", "Partida en vivo" }
                                            }
                                        }
                                        // Fila 5v5: los enemigos lockeados son
                                        // clickeables y eligen al objetivo de la
                                        // guía (misma signal `rival` de las tabs).
                                        vs_row::VsTeamRow {
                                            my_champion_id: champion.id,
                                            my_position,
                                            match_context,
                                            catalog,
                                            rival,
                                        }
                                        // Panel de los 10 jugadores del
                                        // draft (mi card + aliados + rivales
                                        // con lane y lock): debajo de la fila
                                        // 5v5, solo con lobby real (sin draft
                                        // no hay jugadores que mostrar).
                                        players_panel::PlayersPanel {
                                            match_context,
                                            catalog,
                                        }
                                    }
                                    // Hero strip (prototipo
                                    // ui-preview-sylas.html): DEBAJO de la
                                    // fila 5v5, con su hint `cs5-hint` entre
                                    // medias (emite `VsTeamRow`).
                                    vs_hero::VsHero {
                                        champion_id: champion.id,
                                        my_name: champion.name.clone(),
                                        position: my_position,
                                        match_context,
                                        catalog,
                                        analysis,
                                        items,
                                        rival,
                                        rival_counters,
                                    }
                                    // Banner de matchup standalone (entre la
                                    // fila y las tabs): misma fuente de datos
                                    // que la guía, siempre visible con lock.
                                    vs_guide::MatchupBanner {
                                        my_champion_id: champion.id,
                                        my_champion_name: champion.name.clone(),
                                        my_position,
                                        match_context,
                                        catalog,
                                        rival,
                                        analysis,
                                        version: patch_version.clone(),
                                    }
                                    // Header de las tabs: "// GUÍA DE
                                    // MATCHUP" + nota del matchup corriente.
                                    // `flex-wrap` + `overflow-wrap: anywhere`
                                    // en la nota: nunca se recorta por el
                                    // borde derecho (envuelve a la derecha).
                                    div { class: "section-head section-head--cs",
                                        "style": "margin-bottom: 8px; flex-wrap: wrap;",
                                        span { class: "bar-skew", "style": "width: 24px;" }
                                        span { class: "label label-md",
                                            span { class: "sl", "//" }
                                            " GUÍA DE MATCHUP"
                                        }
                                        span { class: "label fs-10 t-outline-var ml-auto",
                                            "style": "min-width: 0; overflow-wrap: anywhere; text-align: right;",
                                            "{guide_note}"
                                        }
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
                                        rival_counters,
                                        patch_version: patch_version.clone(),
                                        client_connected,
                                        champ_nombre: champion.name.clone(),
                                        manual,
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

/// Un lado de la nota del header de guía (puro, testeado): nombre + posición
/// corta, con un espacio solo si hay posición (`"SYLAS MID"`; sin rol queda
/// `"SYLAS"`, sin espacio colgante).
#[must_use]
fn labeled_side(name: &str, position: Option<Position>) -> String {
    let label = position_label(position);
    if label.is_empty() {
        name.to_string()
    } else {
        format!("{name} {label}")
    }
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
    rival_counters: ReadSignal<RivalCountersState>,
    patch_version: Option<String>,
    /// True cuando el cliente de LoL está conectado (reenviado a Runes/Items).
    client_connected: bool,
    /// Display name del champ vigente (reenviado a Runes/Items).
    champ_nombre: String,
    /// True en modo manual (sin cliente de LoL): no hay fila 5v5 que
    /// clickee, así que la señal `rival` apunta al champ de la ficha
    /// (la sincroniza ChampSelectView) y "Cómo ganarle" la consume.
    manual: bool,
) -> Element {
    let mut active_tab = use_signal(CsTab::default);

    // Snapshot ANTES del rsx (convención de la app): slug de MI champ para
    // la tab Consejos (ficha embebida, sin red). `None` sin champ o sin
    // entrada en el catálogo; slug vacío también degrada a `None` (sin
    // ficha que mostrar).
    let catalog_snapshot = catalog.read().clone();
    let my_slug: Option<String> = catalog_snapshot
        .as_ref()
        .and_then(|data| data.get(champion_id))
        .map(|entry| entry.slug.clone())
        .filter(|slug| !slug.is_empty());

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
                "Cómo ganarle"
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
                runes_panel::RunesPanel {
                    recommendations,
                    runes,
                    retry: retry_runes,
                    client_connected,
                    champ_nombre,
                }
            },
            CsTab::Items => rsx! {
                items_panel::ItemsPanel {
                    analysis,
                    champion_id,
                    catalog,
                    items,
                    client_connected,
                    champ_nombre,
                }
            },
            CsTab::Counters => rsx! {
                counters_panel::CountersPanel {
                    analysis,
                    champion_id,
                    match_context,
                    catalog,
                    items,
                    rival,
                    rival_counters,
                }
            },
            CsTab::VsTips => rsx! {
                // El rival llega ya sincronizado: en champ select real lo
                // elige la fila 5v5 y en manual es el champ de la ficha
                // (effect de ChampSelectView). Un solo panel para ambos.
                vs_guide::VsGuidePanel {
                    rival,
                    catalog,
                    version: patch_version,
                    manual,
                }
            },
            CsTab::PlayerTips => rsx! {
                vs_guide::ConsejosPanel { my_slug }
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

    /// Lado de la nota de guía: espacio solo cuando hay posición.
    #[test]
    fn labeled_side_concatena_solo_con_posicion() {
        assert_eq!(labeled_side("Sylas", Some(Position::Middle)), "Sylas MID");
        assert_eq!(labeled_side("Akali", Some(Position::Top)), "Akali TOP");
        assert_eq!(labeled_side("Sylas", None), "Sylas");
    }
}
