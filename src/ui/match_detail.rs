//! Modal de detalle de partida (FASE 11b-1).
//!
//! Se abre con click en una fila del historial del perfil. El fetch
//! on-demand lo maneja el padre ([`crate::ui::profile::ProfileView`]) con
//! el handle global del cliente LCU ([`crate::profile::client_handle`]); el
//! modal solo renderiza los estados (loading / error / ready) y emite
//! `on_retry` / `on_close`.
//!
//! Contenido tolerante: header (champ + resultado + KDA), stats HUD (CS,
//! duración y KDA siempre; oro y daño solo si el payload los trajo), runas
//! usadas (nombres vía [`RuneCatalog`] o `#id`), build (iconos
//! `item_icon_url` con la versión del catálogo) y ambos equipos
//! ([`LcuMatchDetail::teams_split`]).
//!
//! Las resoluciones de nombre/icono son puras y testeadas: [`my_player`],
//! [`rune_names`], [`item_slots`], [`champ_label`] y [`champ_icon`].

use dioxus::prelude::*;

use crate::data_dragon::models::{item_icon_url, ChampionCatalog};
use crate::data_dragon::runes::RuneCatalog;
use crate::profile::models::{LcuMatchDetail, TeamPlayer};
use crate::ui::format::{
    format_duration_mmss, format_kda, format_queue_name, format_ratio, format_thousands,
};

/// Estado del modal de detalle (lo posee [`crate::ui::profile`]).
#[derive(Clone, Debug, PartialEq)]
pub enum MatchModalState {
    /// Cerrado (no renderiza nada).
    Closed,
    /// Fetch en curso para el game id dado.
    Loading {
        /// Partida pedida.
        game_id: i64,
    },
    /// Falló el fetch (sin cliente, error de transporte, ...).
    Failed {
        /// Partida pedida.
        game_id: i64,
        /// Mensaje corto para la UI.
        message: String,
    },
    /// Detalle listo para mostrar.
    Ready {
        /// Partida pedida.
        game_id: i64,
        /// Detalle deserializado del LCU.
        detail: LcuMatchDetail,
    },
}

/// El participante propio del detalle, resuelto por puuid vía las
/// identidades (puro, testeado). `None` si el puuid no figura en la
/// partida (el modal muestra el resto sin header propio).
#[must_use]
pub fn my_player(detail: &LcuMatchDetail, puuid: Option<&str>) -> Option<TeamPlayer> {
    let puuid = puuid?.trim();
    if puuid.is_empty() {
        return None;
    }
    let my_id = detail
        .participant_identities
        .iter()
        .find(|identity| identity.player.puuid.as_deref() == Some(puuid))
        .map(|identity| identity.participant_id)?;
    detail
        .participants
        .iter()
        .find(|player| player.participant_id == my_id)
        .cloned()
}

/// Nombres visibles de las runas: catálogo primero, `#id` si no figura
/// (puro, testeado).
#[must_use]
pub fn rune_names(rune_ids: &[i64], runes: Option<&RuneCatalog>) -> Vec<String> {
    rune_ids
        .iter()
        .map(|rune_id| {
            runes
                .and_then(|runes| runes.get(*rune_id))
                .map(|rune| rune.name.clone())
                .unwrap_or_else(|| format!("#{rune_id}"))
        })
        .collect()
}

/// Slots del build con iconos: solo los casilleros ocupados (`id != 0`),
/// con la URL de la versión dada (puro, testeado). Sin versión → lista
/// vacía (el modal muestra los ids crudos).
#[must_use]
pub fn item_slots(items: &[i64; 6], version: Option<&str>) -> Vec<(i64, String)> {
    let Some(version) = version.map(str::trim).filter(|version| !version.is_empty()) else {
        return Vec::new();
    };
    items
        .iter()
        .filter(|item_id| **item_id > 0)
        .map(|item_id| (*item_id, item_icon_url(version, *item_id)))
        .collect()
}

/// Nombre visible de un campeón: catálogo primero, `#id` como fallback
/// (puro, testeado).
#[must_use]
pub fn champ_label(catalog: Option<&ChampionCatalog>, champion_id: i64) -> String {
    catalog
        .and_then(|catalog| catalog.get(champion_id))
        .map(|entry| entry.name.clone())
        .unwrap_or_else(|| format!("#{champion_id}"))
}

/// URL del icono cuadrado de un campeón con la versión del propio
/// catálogo; `None` sin catálogo o id ausente (puro, testeado).
#[must_use]
pub fn champ_icon(catalog: Option<&ChampionCatalog>, champion_id: i64) -> Option<String> {
    let catalog = catalog?;
    let entry = catalog.get(champion_id)?;
    Some(entry.icon_url(&catalog.version))
}

/// Fila de equipo ya formateada para el render del modal.
#[derive(Clone, Debug, PartialEq)]
struct TeamRow {
    champ: String,
    icon: Option<String>,
    kda: String,
    ratio: String,
    cs: i32,
    win: bool,
}

/// Convierte los participantes de un equipo en filas (puro, testeado).
#[must_use]
fn team_rows(players: &[TeamPlayer], catalog: Option<&ChampionCatalog>) -> Vec<TeamRow> {
    players
        .iter()
        .map(|player| TeamRow {
            champ: champ_label(catalog, player.champion_id),
            icon: champ_icon(catalog, player.champion_id),
            kda: format_kda(player.kills, player.deaths, player.assists),
            ratio: format_ratio(player.kda()),
            cs: player.cs,
            win: player.win,
        })
        .collect()
}

/// Modal de detalle de partida.
#[component]
pub fn MatchDetailModal(
    state: ReadSignal<MatchModalState>,
    catalog: Option<ChampionCatalog>,
    runes: Option<RuneCatalog>,
    my_puuid: Option<String>,
    on_close: EventHandler<()>,
    on_retry: EventHandler<i64>,
) -> Element {
    // Snapshot corto: el guard no cruza la construcción del rsx.
    let snapshot: MatchModalState = state.read().clone();

    // Contenido central según el estado. `Closed` → nada en absoluto.
    let content: Element = match snapshot {
        MatchModalState::Closed => return rsx! {},
        MatchModalState::Loading { .. } => rsx! {
            div { "style": "display: flex; align-items: center; gap: 16px; padding: 32px 0;",
                div { class: "spinner" }
                span { class: "label label-md t-dim pulse-soft",
                    "Cargando detalle desde el cliente…"
                }
            }
        },
        MatchModalState::Failed { game_id, message } => rsx! {
            div { "style": "padding: 24px 0;",
                p { class: "label label-md t-error", "No se pudo cargar el detalle" }
                p { class: "mt-2 t-dim", "style": "font-size: 13px;", "{message}" }
                button { class: "btn-ghost-sm mt-4",
                    onclick: move |_| on_retry.call(game_id),
                    "Reintentar"
                }
            }
        },
        MatchModalState::Ready { detail, .. } => {
            let version = catalog.as_ref().map(|data| data.version.clone());
            let catalog_ref = catalog.as_ref();

            let me = my_player(&detail, my_puuid.as_deref());
            // Reparto de equipos con la pieza oficial del modelo: devuelve
            // dos listas vacías si el puuid no figura (modal sin equipos
            // antes que equipos inventados).
            let (my_team, enemy_team) = detail.teams_split(my_puuid.as_deref().unwrap_or(""));
            let my_rows = team_rows(&my_team, catalog_ref);
            let enemy_rows = team_rows(&enemy_team, catalog_ref);

            rsx! {
                {match me.as_ref() {
                    Some(me) => rsx! {
                        div { "style": "display: flex; flex-wrap: wrap; align-items: center; gap: 16px; margin-top: 16px;",
                            div { class: "tile", "style": "width: 56px; height: 56px;",
                                {match champ_icon(catalog_ref, me.champion_id) {
                                    Some(icon) => rsx! {
                                        img { src: "{icon}", alt: "{champ_label(catalog_ref, me.champion_id)}" }
                                    },
                                    None => rsx! {
                                        span { class: "num-display t-outline-var",
                                            "style": "display: flex; align-items: center; justify-content: center; height: 100%; font-size: 22px;",
                                            "?"
                                        }
                                    },
                                }}
                            }
                            span {
                                class: if me.win { "chip-result chip-win" } else { "chip-result chip-loss" },
                                if me.win { "Victoria" } else { "Derrota" }
                            }
                            div { class: "num-display tnum",
                                "style": "font-size: 28px; line-height: 1; color: var(--on-surface);",
                                "{format_kda(me.kills, me.deaths, me.assists)}"
                            }
                            div { class: "stat-line",
                                span { class: "num-display tnum t-on", "style": "font-size: 20px;", "{format_ratio(me.kda())}" }
                                span { class: "label fs-10", "KDA" }
                            }
                        }
                        div { class: "detail-stats",
                            div { class: "stat-line",
                                span { class: "num-display tnum t-on", "style": "font-size: 24px;", "{me.cs}" }
                                span { class: "label fs-10", "CS" }
                            }
                            span { class: "hud-sep", "style": "font-size: 20px;", "/" }
                            div { class: "stat-line",
                                span { class: "num-display tnum t-on", "style": "font-size: 24px;", "{format_duration_mmss(detail.game_duration_secs)}" }
                                span { class: "label fs-10", "Duración" }
                            }
                            span { class: "hud-sep", "style": "font-size: 20px;", "/" }
                            div { class: "stat-line",
                                span { class: "label fs-10 t-on", "{format_queue_name(detail.queue_id)}" }
                            }
                            {me.gold_earned.map(|gold| rsx! {
                                span { class: "hud-sep", "style": "font-size: 20px;", "/" }
                                div { class: "stat-line",
                                    span { class: "num-display tnum t-on", "style": "font-size: 24px;", "{format_thousands(gold)}" }
                                    span { class: "label fs-10", "Oro" }
                                }
                            })}
                            {me.damage_to_champions.map(|damage| rsx! {
                                span { class: "hud-sep", "style": "font-size: 20px;", "/" }
                                div { class: "stat-line",
                                    span { class: "num-display tnum t-on", "style": "font-size: 24px;", "{format_thousands(damage)}" }
                                    span { class: "label fs-10", "Daño" }
                                }
                            })}
                        }

                        // Build (con iconos si hay versión de Data Dragon)
                        {(me.items.iter().any(|item| *item > 0)).then(|| {
                            let slots = item_slots(&me.items, version.as_deref());
                            rsx! {
                                div { class: "mt-4",
                                    div { class: "section-head",
                                        span { class: "bar-skew bar-skew--silver", "style": "width: 20px;" }
                                        span { class: "label",
                                            span { class: "sl--silver", "//" }
                                            " Build"
                                        }
                                    }
                                    if slots.is_empty() {
                                        p { class: "label fs-10 mt-2 t-outline-var",
                                            "Sin catálogo para los iconos del build"
                                        }
                                    } else {
                                        div { class: "build-row",
                                            for (item_id, icon) in slots {
                                                div { class: "tile", "title": "Item {item_id}",
                                                    img { src: "{icon}", alt: "Item {item_id}" }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        })}

                        // Runas usadas (si el payload las trajo)
                        {(!me.rune_ids.is_empty()).then(|| {
                            let names = rune_names(&me.rune_ids, runes.as_ref());
                            rsx! {
                                div { class: "mt-4",
                                    div { class: "section-head",
                                        span { class: "bar-skew bar-skew--silver", "style": "width: 20px;" }
                                        span { class: "label",
                                            span { class: "sl--silver", "//" }
                                            " Runas"
                                        }
                                    }
                                    div { class: "build-row",
                                        for name in names {
                                            span { class: "chip chip--soft", "{name}" }
                                        }
                                    }
                                }
                            }
                        })}
                    },
                    None => rsx! {
                        p { class: "label label-md t-dim mt-4",
                            "No se pudo identificar tu participante en esta partida."
                        }
                    },
                }}

                hr { class: "divider" }

                div { class: "team-grid",
                    div {
                        div { class: "section-head",
                            span { class: "bar-skew", "style": "width: 20px;" }
                            span { class: "label label-md",
                                span { class: "sl", "//" }
                                " Mi equipo"
                            }
                        }
                        div { class: "mt-2",
                            for row in my_rows {
                                div { class: "team-row",
                                    div { class: "tile", "style": "width: 32px; height: 32px;",
                                        {match row.icon {
                                            Some(icon) => rsx! { img { src: "{icon}", alt: "{row.champ}" } },
                                            None => rsx! {
                                                span { "style": "display: flex; align-items: center; justify-content: center; height: 100%; color: var(--outline-var); font-size: 12px;", "?" }
                                            },
                                        }}
                                    }
                                    span { class: "team-row-name", "{row.champ}" }
                                    if row.win {
                                        span { class: "chip-result chip-win", "style": "padding: 2px 8px;", "V" }
                                    } else {
                                        span { class: "chip-result chip-loss", "style": "padding: 2px 8px;", "D" }
                                    }
                                    span { class: "tnum t-dim ml-auto", "style": "font-size: 12px;", "{row.kda} · {row.ratio}" }
                                    span { class: "tnum t-outline-var", "style": "font-size: 12px;", "{row.cs} cs" }
                                }
                            }
                        }
                    }
                    div {
                        div { class: "section-head",
                            span { class: "bar-skew bar-skew--silver", "style": "width: 20px;" }
                            span { class: "label label-md",
                                span { class: "sl--silver", "//" }
                                " Enemigo"
                            }
                        }
                        div { class: "mt-2",
                            for row in enemy_rows {
                                div { class: "team-row",
                                    div { class: "tile", "style": "width: 32px; height: 32px;",
                                        {match row.icon {
                                            Some(icon) => rsx! { img { src: "{icon}", alt: "{row.champ}" } },
                                            None => rsx! {
                                                span { "style": "display: flex; align-items: center; justify-content: center; height: 100%; color: var(--outline-var); font-size: 12px;", "?" }
                                            },
                                        }}
                                    }
                                    span { class: "team-row-name", "{row.champ}" }
                                    if row.win {
                                        span { class: "chip-result chip-win", "style": "padding: 2px 8px;", "V" }
                                    } else {
                                        span { class: "chip-result chip-loss", "style": "padding: 2px 8px;", "D" }
                                    }
                                    span { class: "tnum t-dim ml-auto", "style": "font-size: 12px;", "{row.kda} · {row.ratio}" }
                                    span { class: "tnum t-outline-var", "style": "font-size: 12px;", "{row.cs} cs" }
                                }
                            }
                        }
                    }
                }

                p { class: "label fs-10 source-note t-outline-var",
                    "FUENTE: CLIENTE LCU · MATCH-HISTORY V1"
                }
            }
        }
    };

    rsx! {
        div { class: "modal-overlay",
            onclick: move |_| on_close.call(()),
            div { class: "modal-panel",
                onclick: move |event: Event<MouseData>| event.stop_propagation(),
                div { "style": "display: flex; align-items: center; gap: 12px;",
                    span { class: "bar-skew", "style": "width: 24px;" }
                    span { class: "label label-md",
                        span { class: "sl", "//" }
                        " Detalle de partida"
                    }
                    button { class: "modal-x ml-auto", "aria-label": "Cerrar",
                        onclick: move |_| on_close.call(()),
                        "✕"
                    }
                }
                {content}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_dragon::models::ChampionListResponse;
    use crate::data_dragon::runes::RuneTreeDto;
    use serde_json::json;

    /// Catálogo chico con Ahri (103) para probar resolución de nombres e
    /// iconos.
    fn catalogo() -> ChampionCatalog {
        let response: ChampionListResponse = serde_json::from_value(json!({
            "data": {
                "Ahri": {
                    "id": "Ahri", "key": "103", "name": "Ahri",
                    "image": { "full": "Ahri.png" }
                }
            }
        }))
        .expect("champion.json válido");
        ChampionCatalog::build(String::from("16.20.1"), response)
    }

    /// Catálogo de runas chico (Dominación con Electrocute).
    fn runas() -> RuneCatalog {
        let trees: Vec<RuneTreeDto> = serde_json::from_value(json!([
            {
                "id": 8100,
                "key": "Domination",
                "icon": "perk-images/Styles/7200_Domination.png",
                "name": "Domination",
                "slots": [
                    { "runes": [
                        { "id": 8112, "key": "Electrocute", "icon": "a.png", "name": "Electrocute" }
                    ]}
                ]
            }
        ]))
        .expect("runesReforged.json válido");
        RuneCatalog::build(String::from("16.20.1"), trees)
    }

    /// Detalle mínimo con identidad propia + un enemigo.
    fn detalle() -> LcuMatchDetail {
        serde_json::from_value(json!({
            "gameId": 1,
            "gameDuration": 1688,
            "queueId": 400,
            "participants": [
                { "participantId": 1, "championId": 103, "teamId": 100, "stats": { "win": true, "kills": 6, "deaths": 5, "assists": 6 } },
                { "participantId": 2, "championId": 106, "teamId": 200, "stats": { "win": false, "kills": 1, "deaths": 9, "assists": 2 } }
            ],
            "participantIdentities": [
                { "participantId": 1, "player": { "puuid": "mi-puuid" } },
                { "participantId": 2, "player": { "puuid": "otro" } }
            ]
        }))
        .expect("detalle parsea")
    }

    /// `my_player`: match por puuid; puuid ausente/ajeno → `None`.
    #[test]
    fn my_player_resuelve_por_puuid() {
        let detail = detalle();

        let me = my_player(&detail, Some("mi-puuid")).expect("yo en la partida");
        assert_eq!(me.champion_id, 103);
        assert!(me.win);

        assert!(my_player(&detail, Some("otro-puuid")).is_none());
        assert!(my_player(&detail, None).is_none());
        assert!(my_player(&detail, Some("   ")).is_none());
    }

    /// `rune_names`: con catálogo usa el nombre; sin catálogo `#id`.
    #[test]
    fn nombres_de_runas_con_y_sin_catalogo() {
        let runes = runas();

        assert_eq!(
            rune_names(&[8112, 999_999], Some(&runes)),
            vec![String::from("Electrocute"), String::from("#999999")]
        );
        assert_eq!(rune_names(&[8112], None), vec![String::from("#8112")]);
        assert!(rune_names(&[], None).is_empty());
    }

    /// `item_slots`: solo casilleros ocupados, con URL de la versión; sin
    /// versión → vacío (el modal muestra el aviso).
    #[test]
    fn slots_de_items_con_y_sin_version() {
        let items = [3157, 3020, 0, 3165, 0, 0];

        let slots = item_slots(&items, Some("16.20.1"));
        assert_eq!(slots.len(), 3);
        assert_eq!(
            slots[0],
            (
                3157,
                String::from("https://ddragon.leagueoflegends.com/cdn/16.20.1/img/item/3157.png")
            )
        );

        assert!(item_slots(&items, None).is_empty());
        assert!(item_slots(&items, Some("  ")).is_empty());
        assert!(item_slots(&[0; 6], Some("16.20.1")).is_empty());
    }

    /// `champ_label`/`champ_icon`: catálogo presente y ausente.
    #[test]
    fn etiqueta_e_icono_de_campeon() {
        let catalog = catalogo();

        assert_eq!(champ_label(Some(&catalog), 103), "Ahri");
        assert_eq!(champ_label(Some(&catalog), 7), "#7");
        assert_eq!(champ_label(None, 103), "#103");

        assert_eq!(
            champ_icon(Some(&catalog), 103),
            Some(String::from(
                "https://ddragon.leagueoflegends.com/cdn/16.20.1/img/champion/Ahri.png"
            ))
        );
        assert_eq!(champ_icon(Some(&catalog), 7), None);
        assert_eq!(champ_icon(None, 103), None);
    }

    /// `team_rows`: KDA formateado + ratio con un decimal por jugador.
    #[test]
    fn filas_de_equipo_formateadas() {
        let detail = detalle();
        let (mine, enemies) = detail.teams_split("mi-puuid");
        assert_eq!(mine.len(), 1);
        assert_eq!(enemies.len(), 1);

        let catalog = catalogo();
        let my_rows = team_rows(&mine, Some(&catalog));
        assert_eq!(my_rows[0].champ, "Ahri");
        assert_eq!(my_rows[0].kda, "6/5/6");
        assert_eq!(my_rows[0].ratio, "2.4");
        assert!(my_rows[0].win);

        let enemy_rows = team_rows(&enemies, Some(&catalog));
        assert_eq!(enemy_rows[0].champ, "#106");
        assert!(!enemy_rows[0].win);
    }
}
