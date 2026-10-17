//! Análisis completo del champ select (FASE 11b-2a).
//!
//! Una consulta por champ elegido que junta en paralelo las tres fuentes que
//! la vista rediseñada consume:
//!
//! - **Detalle del campeón** (Data Dragon,
//!   [`load_champion_detail`]): pasiva + QWER para la tab HABILIDADES.
//! - **Counters** (Lolalytics `ep=counter`, [`fetch_counters_cached`]):
//!   métricas del hero (WR / pickrate / partidas analizadas).
//! - **Build de ítems** (Lolalytics `ep=build-itemset`,
//!   [`fetch_item_build_cached`]): queda listo para la tab ITEMS de
//!   11b-2b; hoy solo viaja en el estado.
//! - **Skill order** ([`fetch_skill_order`]): best-effort honesto (hoy
//!   `Unavailable`); cuando la fuente reviva el dato fluye solo.
//!
//! [`run_champ_analysis`] es la tarea de por vida (mismo patrón que
//! [`super::run_recommender`]): al entrar a champ select con pick y con
//! catálogo + parche disponibles lanza los cuatro fetches en paralelo y
//! publica [`AnalysisState`] en su señal. Un intento por
//! `(champ, posición, parche)` — las caches internas de cada fuente evitan
//! la segunda consulta de red; los fallos NO se cachean. Al cerrar el champ
//! select el estado vuelve a `Idle`, lo que también libera el anti-reintento.
//!
//! [`combine_analysis`] es la combinación pura (testeada sin red): todo OK →
//! `Ready`, todo mal → `Unavailable`, mixto → `Partial` con lo que llegó y
//! la razón corta de lo que falló.

use std::sync::Arc;

use dioxus::prelude::{ReadableExt, Signal, WritableExt};
use tokio::time::sleep;

use super::counters::{CounterSummary, fetch_counters_cached};
use super::items::{ItemBuild, fetch_item_build_cached};
use super::skills::{SkillOrder, fetch_skill_order};
use super::CacheKey;
use crate::data_dragon::champion_detail::{ChampionDetail, load_champion_detail};
use crate::data_dragon::models::ChampionCatalog;
use crate::lcu::models::{AppState, MatchContext, Position};
use crate::stats::source::{build_http_client, preview_target};

/// Pausa del poll del analizador (misma cadencia que el recomendador):
/// reacciona a la entrada al champ select en ~1.5 s sin martillar señales.
const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(1500);

/// Estado del análisis del champ select, publicado por [`run_champ_analysis`]
/// y consumido por [`crate::ui::champ_select`].
///
/// Los datos viajan en `Arc` (el detalle pesa ~15 KB y un `CounterSummary`
/// puede traer cientos de matchups): clonar el estado en cada render de la
/// UI es barato.
///
/// Sin `Eq` por los `f64`; las transiciones se publican con
/// [`set_analysis_if_changed`] antes de tocar la señal.
#[derive(Clone, Debug, PartialEq)]
pub enum AnalysisState {
    /// Fuera de champ select o sin champ elegido: la UI no muestra análisis.
    Idle,
    /// Consultando las fuentes para el campeón en curso.
    Loading {
        /// Id del campeón en curso.
        champion_id: i64,
    },
    /// Las tres fuentes respondieron.
    Ready {
        /// Id del campeón analizado.
        champion_id: i64,
        /// Pasiva + QWER (Data Dragon).
        detail: Arc<ChampionDetail>,
        /// Métricas del hero + matchups (Lolalytics).
        counters: Arc<CounterSummary>,
        /// Build de ítems (Lolalytics; tab ITEMS en 11b-2b).
        items: Arc<ItemBuild>,
        /// Skill order + summoners (best-effort honesto).
        skills: SkillOrder,
    },
    /// Alguna fuente falló: se muestran las que llegaron y la razón corta de
    /// las que no (cero datos fabricados en los huecos).
    Partial {
        /// Id del campeón analizado.
        champion_id: i64,
        /// Detalle si Data Dragon respondió.
        detail: Option<Arc<ChampionDetail>>,
        /// Counters si Lolalytics (`ep=counter`) respondió.
        counters: Option<Arc<CounterSummary>>,
        /// Build si Lolalytics (`ep=build-itemset`) respondió.
        items: Option<Arc<ItemBuild>>,
        /// Skill order (siempre presente: el probe nunca falla).
        skills: SkillOrder,
        /// Razón corta de las piezas que fallaron (ej. `"sin counters · sin build"`).
        reason: String,
    },
    /// Ninguna fuente respondió para este campeón.
    Unavailable {
        /// Id del campeón consultado.
        champion_id: i64,
        /// Motivo corto, listo para la UI.
        reason: String,
    },
}

/// Clave del anti-reintento del analizador: misma identidad que la cache de
/// páginas (`champ + posición + parche`) más la versión de Data Dragon de la
/// que salió el detalle.
pub type AnalysisKey = CacheKey;

/// Objetivo de análisis derivado de las señales (puro, testeado): envuelve
/// [`preview_target`] (que ya exige pick + slug + parche) y agrega el
/// `ddragon_id` y la versión de Data Dragon que [`load_champion_detail`]
/// necesita.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnalysisTarget {
    /// Id numérico del campeón.
    pub champion_id: i64,
    /// Slug para Lolalytics (del catálogo).
    pub slug: String,
    /// Id de Data Dragon con casing original (clave del JSON por-campeón).
    pub ddragon_id: String,
    /// Versión de Data Dragon pineada por el loader del catálogo.
    pub ddragon_version: String,
    /// Posición asignada al jugador local (`None` = sin rol).
    pub position: Option<Position>,
    /// Parche (`major.minor`) para Lolalytics.
    pub patch: String,
}

/// Deriva el objetivo de análisis del estado actual. `None` = nada que
/// consultar todavía (sin pick, sin catálogo, sin parche o sin `ddragon_id`).
#[must_use]
pub fn analysis_target(
    state: &AppState,
    context: &MatchContext,
    catalog: Option<&ChampionCatalog>,
    client_version: Option<&str>,
) -> Option<AnalysisTarget> {
    let base = preview_target(state, context, catalog, client_version)?;
    let catalog = catalog?;
    let entry = catalog.get(base.champion_id)?;
    if entry.ddragon_id.is_empty() || catalog.version.is_empty() {
        return None;
    }

    Some(AnalysisTarget {
        champion_id: base.champion_id,
        slug: base.slug,
        ddragon_id: entry.ddragon_id.clone(),
        ddragon_version: catalog.version.clone(),
        position: base.position,
        patch: base.patch,
    })
}

/// Razón corta que nombra las piezas que fallaron (puro, testeado). La UI la
/// muestra tal cual; las causas detalladas quedan en el log de la tarea.
#[must_use]
pub fn failed_parts_reason(
    detail_failed: bool,
    counters_failed: bool,
    items_failed: bool,
) -> String {
    let mut parts: Vec<&str> = Vec::new();
    if detail_failed {
        parts.push("sin habilidades");
    }
    if counters_failed {
        parts.push("sin counters");
    }
    if items_failed {
        parts.push("sin build");
    }
    parts.join(" · ")
}

/// Combina los resultados de las fuentes en el estado final (puro, testeado
/// sin red):
///
/// - Todo OK → [`AnalysisState::Ready`].
/// - Todo mal → [`AnalysisState::Unavailable`] (motivo genérico: las causas
///   por fuente ya quedaron en el log).
/// - Mixto → [`AnalysisState::Partial`] con los datos que llegaron y
///   [`failed_parts_reason`] como razón.
#[must_use]
pub fn combine_analysis(
    champion_id: i64,
    detail: Result<ChampionDetail, String>,
    counters: Result<CounterSummary, String>,
    items: Result<ItemBuild, String>,
    skills: SkillOrder,
) -> AnalysisState {
    let detail = detail.map(Arc::new);
    let counters = counters.map(Arc::new);
    let items = items.map(Arc::new);

    match (&detail, &counters, &items) {
        (Ok(detail), Ok(counters), Ok(items)) => AnalysisState::Ready {
            champion_id,
            detail: Arc::clone(detail),
            counters: Arc::clone(counters),
            items: Arc::clone(items),
            skills,
        },
        (Err(_), Err(_), Err(_)) => AnalysisState::Unavailable {
            champion_id,
            reason: String::from("ninguna fuente de análisis respondió"),
        },
        _ => {
            let reason =
                failed_parts_reason(detail.is_err(), counters.is_err(), items.is_err());
            AnalysisState::Partial {
                champion_id,
                detail: detail.ok(),
                counters: counters.ok(),
                items: items.ok(),
                skills,
                reason,
            }
        }
    }
}

/// Publica `next` en la señal solo si difiere del valor vigente (mismo
/// criterio que el recomendador: el poll corre cada [`POLL_INTERVAL`] y
/// re-publicar el mismo estado dispararía re-renders inútiles).
fn set_analysis_if_changed(analysis: &mut Signal<AnalysisState>, next: AnalysisState) {
    if *analysis.read() != next {
        analysis.set(next);
    }
}

/// Detalle del análisis solo si corresponde al campeón pedido (puro,
/// testeado): la UI lo usa para no mostrar datos de un pick anterior.
#[must_use]
pub fn analysis_detail(
    state: &AnalysisState,
    champion_id: i64,
) -> Option<Arc<ChampionDetail>> {
    match state {
        AnalysisState::Ready { detail, .. } if state_champ(state) == champion_id => {
            Some(Arc::clone(detail))
        }
        AnalysisState::Partial {
            detail: Some(detail),
            ..
        } if state_champ(state) == champion_id => Some(Arc::clone(detail)),
        _ => None,
    }
}

/// Counters del análisis solo si corresponde al campeón pedido (puro).
#[must_use]
pub fn analysis_counters(
    state: &AnalysisState,
    champion_id: i64,
) -> Option<Arc<CounterSummary>> {
    match state {
        AnalysisState::Ready { counters, .. } if state_champ(state) == champion_id => {
            Some(Arc::clone(counters))
        }
        AnalysisState::Partial {
            counters: Some(counters),
            ..
        } if state_champ(state) == champion_id => Some(Arc::clone(counters)),
        _ => None,
    }
}

/// Build de ítems del análisis solo si corresponde al campeón pedido (puro,
/// testeado): la tab ITEMS y las sugerencias vs comp de la tab COUNTERS
/// (11b-2b) lo consumen igual que el hero a counters.
#[must_use]
pub fn analysis_items(state: &AnalysisState, champion_id: i64) -> Option<Arc<ItemBuild>> {
    match state {
        AnalysisState::Ready { items, .. } if state_champ(state) == champion_id => {
            Some(Arc::clone(items))
        }
        AnalysisState::Partial {
            items: Some(items),
            ..
        } if state_champ(state) == champion_id => Some(Arc::clone(items)),
        _ => None,
    }
}

/// Skill order del análisis si corresponde al campeón pedido (puro). En
/// `Loading`/`Unavailable` no hay (el probe corre junto al resto).
#[must_use]
pub fn analysis_skills(state: &AnalysisState, champion_id: i64) -> Option<SkillOrder> {
    match state {
        AnalysisState::Ready { skills, .. } if state_champ(state) == champion_id => {
            Some(skills.clone())
        }
        AnalysisState::Partial { skills, .. } if state_champ(state) == champion_id => {
            Some(skills.clone())
        }
        _ => None,
    }
}

/// ¿La consulta para este campeón sigue en curso? (puro, testeado). La UI lo
/// usa para distinguir "cargando" de "falló".
#[must_use]
pub fn analysis_in_progress(state: &AnalysisState, champion_id: i64) -> bool {
    matches!(
        state,
        AnalysisState::Loading {
            champion_id: loading,
        } if *loading == champion_id
    )
}

/// Razón corta del fallo (parcial o total) si corresponde al campeón pedido
/// (puro, testeado): `None` en `Ready`/`Loading`/estados de otro pick.
#[must_use]
pub fn analysis_failure_reason(state: &AnalysisState, champion_id: i64) -> Option<String> {
    match state {
        AnalysisState::Partial { reason, .. }
        | AnalysisState::Unavailable { reason, .. }
            if state_champ(state) == champion_id =>
        {
            Some(reason.clone())
        }
        _ => None,
    }
}

/// Id del campeón al que corresponde el estado (0 en `Idle`).
fn state_champ(state: &AnalysisState) -> i64 {
    match state {
        AnalysisState::Idle => 0,
        AnalysisState::Loading { champion_id }
        | AnalysisState::Ready { champion_id, .. }
        | AnalysisState::Partial { champion_id, .. }
        | AnalysisState::Unavailable { champion_id, .. } => *champion_id,
    }
}

/// Tarea de por vida del analizador (FASE 11b-2a): al entrar a champ select
/// con pick (y con catálogo + parche disponibles) consulta las fuentes en
/// paralelo y publica [`AnalysisState`]. Transiciones:
///
/// - Champ select cerrado / sin pick → `Idle` (y `last_done = None`, así
///   re-entrar con el mismo champ vuelve a pasar por `Loading`; las caches
///   evitan la consulta de red repetida).
/// - Pick sin catálogo/parche → `Loading` hasta que estén.
/// - Target nuevo (champ, posición o parche distinto) → `Loading` + fetch.
/// - Resultado → `Ready`/`Partial`/`Unavailable`; `last_done` evita repetir
///   el intento hasta que cambie el target (los fallos no se cachean, así
///   que re-entrar al champ select reintenta limpio).
pub async fn run_champ_analysis(
    mut analysis: Signal<AnalysisState>,
    app_state: Signal<AppState>,
    match_context: Signal<MatchContext>,
    catalog: Signal<Option<ChampionCatalog>>,
    client_version: Signal<Option<String>>,
) {
    // Último objetivo ya procesado: un intento por (champ, posición, parche).
    let mut last_done: Option<AnalysisKey> = None;

    loop {
        // Snapshots cortos: ningún guard de señal cruza un `await`.
        let state: AppState = (*app_state.read()).clone();
        let context: MatchContext = (*match_context.read()).clone();
        let catalog_snapshot: Option<ChampionCatalog> = (*catalog.read()).clone();
        let version_snapshot: Option<String> = (*client_version.read()).clone();

        // Fuera de champ select (o sin pick): Idle + liberar anti-reintento.
        if !matches!(state, AppState::InChampSelect(Some(_))) {
            last_done = None;
            set_analysis_if_changed(&mut analysis, AnalysisState::Idle);
            sleep(POLL_INTERVAL).await;
            continue;
        }

        let champion_id = match &state {
            AppState::InChampSelect(Some(champion)) => champion.id,
            _ => 0,
        };

        match analysis_target(
            &state,
            &context,
            catalog_snapshot.as_ref(),
            version_snapshot.as_deref(),
        ) {
            None => {
                // Pick sin catálogo/parche todavía: Loading hasta que estén
                // (los loaders de Data Dragon reintentan por su cuenta).
                set_analysis_if_changed(
                    &mut analysis,
                    AnalysisState::Loading { champion_id },
                );
                sleep(POLL_INTERVAL).await;
            }
            Some(target) => {
                let key = AnalysisKey {
                    champion_id: target.champion_id,
                    position: target.position,
                    patch: target.patch.clone(),
                };
                if last_done.as_ref() == Some(&key) {
                    // Mismo target ya resuelto: la UI queda como está.
                    sleep(POLL_INTERVAL).await;
                    continue;
                }

                set_analysis_if_changed(
                    &mut analysis,
                    AnalysisState::Loading {
                        champion_id: target.champion_id,
                    },
                );

                let client = match build_http_client() {
                    Ok(client) => client,
                    Err(error) => {
                        eprintln!(
                            "klipy [analysis]: no se pudo crear el cliente HTTP: {error}"
                        );
                        set_analysis_if_changed(
                            &mut analysis,
                            AnalysisState::Unavailable {
                                champion_id: target.champion_id,
                                reason: String::from("no se pudo iniciar la consulta"),
                            },
                        );
                        last_done = Some(key);
                        sleep(POLL_INTERVAL).await;
                        continue;
                    }
                };

                // Los cuatro fetches en paralelo; cada uno con su propia
                // cache en memoria.
                let (detail, counters, items, skills) = tokio::join!(
                    load_champion_detail(
                        &client,
                        &target.ddragon_version,
                        &target.ddragon_id,
                    ),
                    fetch_counters_cached(
                        &client,
                        &target.slug,
                        target.position,
                        &target.patch,
                    ),
                    fetch_item_build_cached(
                        &client,
                        &target.slug,
                        target.position,
                        &target.patch,
                    ),
                    fetch_skill_order(&client, &target.slug, target.position, &target.patch),
                );

                eprintln!(
                    "klipy [analysis]: {} ({}): detalle {}, counters {}, build {}, skills {}",
                    target.ddragon_id,
                    target.patch,
                    detail.as_ref().map(|_| "ok").unwrap_or("fallo"),
                    counters.as_ref().map(|_| "ok").unwrap_or("fallo"),
                    items.as_ref().map(|_| "ok").unwrap_or("fallo"),
                    skills.source,
                );

                let combined = combine_analysis(
                    target.champion_id,
                    detail.map_err(|error| error.to_string()),
                    counters.map_err(|error| error.to_string()),
                    items.map_err(|error| error.to_string()),
                    skills,
                );
                set_analysis_if_changed(&mut analysis, combined);
                last_done = Some(key);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Detalle mínimo para las combinaciones.
    fn detalle() -> ChampionDetail {
        ChampionDetail {
            id: String::from("Veigar"),
            name: String::from("Veigar"),
            ..ChampionDetail::default()
        }
    }

    fn counters() -> CounterSummary {
        CounterSummary {
            my_wr: Some(52.02),
            pickrate: Some(4.15),
            analysed: Some(34_745_768),
            counters: Vec::new(),
        }
    }

    fn build() -> ItemBuild {
        ItemBuild::default()
    }

    /// Todo OK → `Ready` con las tres piezas (y el skill order viaja dentro).
    #[test]
    fn combine_con_todo_ok_da_ready() {
        let state = combine_analysis(
            45,
            Ok(detalle()),
            Ok(counters()),
            Ok(build()),
            SkillOrder::unavailable(),
        );

        match state {
            AnalysisState::Ready {
                champion_id,
                detail,
                counters,
                items,
                skills,
            } => {
                assert_eq!(champion_id, 45);
                assert_eq!(detail.name, "Veigar");
                assert_eq!(counters.my_wr, Some(52.02));
                assert_eq!(*items, ItemBuild::default());
                assert_eq!(skills.source, crate::stats::source::skills::SkillSource::Unavailable);
            }
            other => panic!("esperaba Ready, hubo {other:?}"),
        }
    }

    /// Todo mal → `Unavailable` con el motivo genérico.
    #[test]
    fn combine_con_todo_mal_da_unavailable() {
        let state = combine_analysis(
            45,
            Err(String::from("404")),
            Err(String::from("timeout")),
            Err(String::from("timeout")),
            SkillOrder::unavailable(),
        );

        assert_eq!(
            state,
            AnalysisState::Unavailable {
                champion_id: 45,
                reason: String::from("ninguna fuente de análisis respondió"),
            }
        );
    }

    /// Mixto → `Partial` con lo que llegó y la razón corta de lo que falló.
    #[test]
    fn combine_mixto_da_partial_con_lo_que_llego() {
        let state = combine_analysis(
            45,
            Ok(detalle()),
            Err(String::from("Lolalytics caída")),
            Err(String::from("Lolalytics caída")),
            SkillOrder::unavailable(),
        );

        match state {
            AnalysisState::Partial {
                champion_id,
                detail,
                counters,
                items,
                reason,
                ..
            } => {
                assert_eq!(champion_id, 45);
                assert_eq!(detail.as_ref().map(|d| d.name.as_str()), Some("Veigar"));
                assert!(counters.is_none());
                assert!(items.is_none());
                assert_eq!(reason, "sin counters · sin build");
            }
            other => panic!("esperaba Partial, hubo {other:?}"),
        }

        // Solo counters OK: razón que nombra las otras dos piezas.
        let solo_counters = combine_analysis(
            45,
            Err(String::from("404")),
            Ok(counters()),
            Err(String::from("404")),
            SkillOrder::unavailable(),
        );
        assert_eq!(
            solo_counters,
            AnalysisState::Partial {
                champion_id: 45,
                detail: None,
                counters: Some(Arc::new(counters())),
                items: None,
                skills: SkillOrder::unavailable(),
                reason: String::from("sin habilidades · sin build"),
            }
        );
    }

    /// `failed_parts_reason` nombra solo las piezas caídas, en orden fijo.
    #[test]
    fn failed_parts_reason_nombra_solo_las_caidas() {
        assert_eq!(failed_parts_reason(false, false, false), "");
        assert_eq!(failed_parts_reason(true, false, false), "sin habilidades");
        assert_eq!(
            failed_parts_reason(false, true, true),
            "sin counters · sin build"
        );
        assert_eq!(
            failed_parts_reason(true, true, true),
            "sin habilidades · sin counters · sin build"
        );
    }

    /// Catálogo mínimo para `analysis_target`.
    fn catalogo() -> ChampionCatalog {
        let mut by_id = std::collections::HashMap::new();
        by_id.insert(
            45,
            crate::data_dragon::models::ChampionEntry {
                name: String::from("Veigar"),
                image_url: String::from("https://ddragon.leagueoflegends.com/cdn/x.png"),
                slug: String::from("veigar"),
                ddragon_id: String::from("Veigar"),
            },
        );
        ChampionCatalog {
            version: String::from("16.20.1"),
            by_id,
        }
    }

    /// Caso nominal: target completo con slug, ddragon_id, versión de Data
    /// Dragon, posición y parche.
    #[test]
    fn analysis_target_arma_el_objetivo_completo() {
        let context = MatchContext {
            my_position: Some(Position::Middle),
            ..MatchContext::default()
        };
        let champion = crate::lcu::events::champion_placeholder(45);

        let target = analysis_target(
            &AppState::InChampSelect(Some(champion)),
            &context,
            Some(&catalogo()),
            Some("16.20.8248524"),
        )
        .expect("target");

        assert_eq!(target.champion_id, 45);
        assert_eq!(target.slug, "veigar");
        assert_eq!(target.ddragon_id, "Veigar");
        assert_eq!(target.ddragon_version, "16.20.1");
        assert_eq!(target.position, Some(Position::Middle));
        assert_eq!(target.patch, "16.20");
    }

    /// Sin pick, sin catálogo, sin parche, sin ddragon_id o sin versión de
    /// Data Dragon no hay análisis.
    #[test]
    fn analysis_target_exige_pick_catalogo_parche_y_ddragon_id() {
        let context = MatchContext::default();
        let catalog = catalogo();
        let champion = crate::lcu::events::champion_placeholder(45);

        assert_eq!(
            analysis_target(&AppState::Disconnected, &context, Some(&catalog), Some("16.20.1")),
            None
        );
        assert_eq!(
            analysis_target(
                &AppState::InChampSelect(None),
                &context,
                Some(&catalog),
                Some("16.20.1")
            ),
            None
        );
        assert_eq!(
            analysis_target(
                &AppState::InChampSelect(Some(champion.clone())),
                &context,
                None,
                Some("16.20.1")
            ),
            None
        );
        // Sin client_version cae al patch del catálogo (16.20.1 → 16.20),
        // así que SÍ hay análisis (comportamiento heredado de preview_target).
        let sin_cliente = analysis_target(
            &AppState::InChampSelect(Some(champion.clone())),
            &context,
            Some(&catalog),
            None,
        )
        .expect("el catálogo aporta el parche");
        assert_eq!(sin_cliente.patch, "16.20");

        // Entrada sin ddragon_id → no hay JSON por-campeón que pedir.
        let mut sin_id = catalogo();
        sin_id.by_id.get_mut(&45).expect("entrada").ddragon_id.clear();
        assert_eq!(
            analysis_target(
                &AppState::InChampSelect(Some(champion.clone())),
                &context,
                Some(&sin_id),
                Some("16.20.1")
            ),
            None
        );

        // Catálogo sin versión (payload vacío) → tampoco.
        let mut sin_version = catalogo();
        sin_version.version.clear();
        assert_eq!(
            analysis_target(
                &AppState::InChampSelect(Some(champion)),
                &context,
                Some(&sin_version),
                Some("16.20.1")
            ),
            None
        );
    }

    /// Los extractores por campeón ignoran estados de otro pick (stale).
    #[test]
    fn extractores_descartan_el_estado_de_otro_campeon() {
        let ready = combine_analysis(
            45,
            Ok(detalle()),
            Ok(counters()),
            Ok(build()),
            SkillOrder::unavailable(),
        );

        assert!(analysis_detail(&ready, 45).is_some());
        assert!(analysis_counters(&ready, 45).is_some());
        assert!(analysis_items(&ready, 45).is_some());
        assert!(analysis_skills(&ready, 45).is_some());

        // Pick distinto → nada (ni datos ni skill order stale).
        assert!(analysis_detail(&ready, 103).is_none());
        assert!(analysis_counters(&ready, 103).is_none());
        assert!(analysis_items(&ready, 103).is_none());
        assert!(analysis_skills(&ready, 103).is_none());

        // Idle no trae nada.
        assert!(analysis_detail(&AnalysisState::Idle, 45).is_none());
        assert!(analysis_counters(&AnalysisState::Idle, 45).is_none());
        assert!(analysis_items(&AnalysisState::Idle, 45).is_none());
    }

    /// `analysis_in_progress` y `analysis_failure_reason` distinguen los
    /// estados del pick vigente y descartan los de otro campeón.
    #[test]
    fn in_progress_y_failure_reason_son_por_campeon() {
        let champion = crate::lcu::events::champion_placeholder(45);
        let loading = AnalysisState::Loading { champion_id: 45 };

        assert!(analysis_in_progress(&loading, 45));
        assert!(!analysis_in_progress(&AnalysisState::Idle, 45));
        assert_eq!(analysis_failure_reason(&loading, 45), None);

        let failed = combine_analysis(
            45,
            Err(String::from("404")),
            Err(String::from("timeout")),
            Err(String::from("timeout")),
            SkillOrder::unavailable(),
        );
        assert!(!analysis_in_progress(&failed, 45));
        assert_eq!(
            analysis_failure_reason(&failed, 45).as_deref(),
            Some("ninguna fuente de análisis respondió")
        );
        // Otro pick: ni loading ni razón.
        assert!(!analysis_in_progress(&loading, 103));
        assert_eq!(analysis_failure_reason(&failed, 103), None);

        // Ready no tiene razón.
        let ready = combine_analysis(
            45,
            Ok(detalle()),
            Ok(counters()),
            Ok(build()),
            SkillOrder::unavailable(),
        );
        assert_eq!(analysis_failure_reason(&ready, 45), None);
        let _ = champion;
    }
}
