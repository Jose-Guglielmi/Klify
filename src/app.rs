//! Componente raíz de la aplicación.

use dioxus::prelude::*;

use std::sync::Arc;

use crate::data_dragon::item::ItemCatalog;
use crate::data_dragon::models::ChampionCatalog;
use crate::data_dragon::run_catalog_loader;
use crate::data_dragon::run_item_loader;
use crate::data_dragon::run_runes_loader;
use crate::data_dragon::runes::RuneCatalog;
use crate::lcu::client::run_lcu_loop;
use crate::lcu::models::{AppState, MatchContext, SummonerInfo};
use crate::profile::models::ProfileSummary;
use crate::profile::run_profile_loop;
use crate::stats::builder::run_dataset_sync;
use crate::stats::source::{AnalysisState, RecommendationsState, run_champ_analysis, run_recommender};
use crate::stats::storage::{data_dir, JsonlStatsStore, MATCHES_FILE_NAME};
use crate::ui::champ_select::ChampSelectView;
use crate::ui::profile::ProfileView;
use crate::ui::waiting::WaitingView;

const FAVICON: Asset = asset!("/assets/favicon.ico");
const MAIN_CSS: Asset = asset!("/assets/main.css");

/// Hoja de estilos de las fuentes de marca (design.md): Barlow Condensed
/// (itálicas verdaderas, pesos 600/700) + Inter (400/600, datos en
/// tabular-nums).
const FONTS_URL: &str = "https://fonts.googleapis.com/css2?family=Barlow+Condensed:ital,wght@0,600;0,700;1,600;1,700&family=Inter:wght@400;600&display=swap";

/// Vista activa derivada del estado global (pura, testeada abajo):
///
/// - Champ select activo ([`AppState::InChampSelect`]) → [`AppView::ChampSelect`].
/// - Resto: con perfil cargado → [`AppView::Perfil`], si no
///   [`AppView::Waiting`] (incluye "cliente conectado, sin champ select y
///   perfil todavía no cargado").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppView {
    /// Pantalla de espera (sin cliente o cliente sin champ select y sin
    /// perfil todavía).
    Waiting,
    /// Perfil del invocador.
    Perfil,
    /// Champ select en vivo.
    ChampSelect,
}

/// Deriva la vista desde `app_state` + la presencia de perfil.
#[must_use]
pub fn derive_view(state: &AppState, profile_loaded: bool) -> AppView {
    match state {
        AppState::InChampSelect(_) => AppView::ChampSelect,
        AppState::Disconnected | AppState::WaitingForChampSelect => {
            if profile_loaded {
                AppView::Perfil
            } else {
                AppView::Waiting
            }
        }
    }
}

/// Componente raíz. Mantiene el estado global de la app y renderiza la
/// vista activa según [`derive_view`] (sistema de vistas de la FASE 11b-1).
///
/// Tareas de por vida de la app, lanzadas una única vez desde el effect:
///
/// - [`run_catalog_loader`]: descarga el catálogo de campeones de Data
///   Dragon en `catalog`, pineado al parche del cliente.
/// - [`run_runes_loader`] (FASE 7): ídem para el catálogo de runas en
///   `runes`, que el recomendador (FASE 10) usa para resolver nombres de
///   runas y árboles en los logs y en la UI.
/// - [`run_item_loader`] (FASE 11b-2b): ídem para el catálogo de ítems en
///   `items` (nombres y blurbs de las tabs ITEMS/COUNTERS del champ
///   select), también pineado al parche del cliente.
/// - [`run_lcu_loop`]: detección del cliente, champ select vía websocket +
///   GET inicial REST, resolviendo los campeones contra `catalog` y
///   publicando el snapshot de partida en `match_context`; al conectar
///   expone la versión del juego en `client_version`, que los dos loaders
///   anteriores leen para elegir el parche correcto (latest si no hay
///   cliente), el invocador en `summoner` y el platform id en `platform`.
/// - [`run_dataset_sync`] (FASE 8b): recorre el historial ranked del
///   invocador con la Riot API y lo persiste en el dataset JSONL de
///   `data_dir()`; sin `RIOT_API_KEY` queda deshabilitado con un solo log.
/// - [`run_recommender`] (FASE 10): al entrar a champ select con pick,
///   descarga las páginas de runas recomendadas (Lolalytics con fallback a
///   U.GG, cache en memoria), las loguea (validación en vivo de FASE 9) y
///   publica [`RecommendationsState`] en la señal `recommendations`, que la
///   tab RUNAS de [`crate::ui::champ_select`] renderiza. El contador
///   `retry_runes` es el canal del botón REINTENTAR (11b-2a).
/// - [`run_champ_analysis`] (FASE 11b-2a): análisis del champ select —
///   detalle del campeón (Data Dragon), counters y build de ítems
///   (Lolalytics) y skill order (best-effort) en paralelo, publicado como
///   [`AnalysisState`] para el hero y la tab HABILIDADES.
/// - [`run_profile_loop`] (FASE 11a-2): perfil del invocador por LCU
///   (segunda conexión REST propia, re-fetch cada 5 min, reset a None al
///   desconectar). También publica el handle global del cliente
///   ([`crate::profile::client_handle`]) para el modal de detalle.
#[component]
pub fn App() -> Element {
    let app_state: Signal<AppState> = use_signal(|| AppState::Disconnected);
    let catalog: Signal<Option<ChampionCatalog>> = use_signal(|| None);
    // FASE 7: catálogo de runas de Data Dragon (perkId → RuneDef, árboles).
    // Lo lee `run_recommender` (logs), `RuneRecommendations` (UI, FASE 10)
    // y el modal de detalle (nombres de runas, FASE 11b-1).
    let runes: Signal<Option<RuneCatalog>> = use_signal(|| None);
    // FASE 11b-2b: catálogo de ítems de Data Dragon (id → nombre + blurb)
    // para las tabs ITEMS y COUNTERS del champ select. Viaja en `Arc`
    // porque el JSON completo pesa ~200 KB y la UI lo clona por render.
    let items: Signal<Option<Arc<ItemCatalog>>> = use_signal(|| None);
    // FASE 6: snapshot de la partida (ambos equipos, posiciones, pick local).
    // `run_recommender` (FASE 10) lee de acá la posición del jugador local.
    let match_context: Signal<MatchContext> = use_signal(MatchContext::default);
    // FASE 7: versión del juego que reporta el cliente LCU al conectar
    // (ej. "16.7.623.1234"); `None` mientras no haya cliente, en cuyo caso
    // los catálogos usan la versión más reciente de Data Dragon.
    let client_version: Signal<Option<String>> = use_signal(|| None);
    // FASE 8a: invocador actual (puuid, Riot ID) que reporta el LCU al
    // conectar; raíz del historial que la Riot API Match-V5 de
    // `riot_api` recorrerá para el dataset de runas (fase 8b).
    let summoner: Signal<Option<SummonerInfo>> = use_signal(|| None);
    // FASE 8b: platform id que reporta el LCU al conectar (ej. "la1",
    // normalizado y validado); con `RIOT_PLATFORM` el dataset también puede
    // sincronizar sin cliente. Ninguno de los dos valores llega: el sync
    // queda deshabilitado limpio.
    let platform: Signal<Option<String>> = use_signal(|| None);
    // FASE 10: recomendaciones de runas para el pick en curso. Arranca Idle
    // (fuera de champ select no se muestra nada); `run_recommender` la
    // alimenta y la tab RUNAS del champ select (11b-2a) la renderiza.
    let recommendations: Signal<RecommendationsState> = use_signal(|| RecommendationsState::Idle);
    // FASE 11b-2a: análisis del champ select (detalle + counters + items +
    // skill order en un solo estado); `run_champ_analysis` lo publica y la
    // vista lo consume para el hero y la tab HABILIDADES.
    let champ_analysis: Signal<AnalysisState> = use_signal(|| AnalysisState::Idle);
    // FASE 11b-2a: contador de REINTENTAR de la tab RUNAS. La UI lo
    // incrementa; `run_recommender` lo observa y re-consulta el target
    // vigente (los fallos no se cachean, así que el reintento es real).
    let retry_runes: Signal<u64> = use_signal(|| 0);
    // FASE 11a-2: perfil del invocador (rank + top maestría + historial +
    // agregados), 100% por LCU. `None` mientras no hay cliente (la UI 11b
    // muestra su pantalla "sin cliente"); el loop de perfil lo llena al
    // conectar y lo refresca cada 5 min.
    let profile: Signal<Option<ProfileSummary>> = use_signal(|| None);
    // FASE 12: hay partida en curso (espejo Live API activo). La vista de
    // champ select cambia su etiqueta de sección para no mentir.
    let in_game: Signal<bool> = use_signal(|| false);
    // FASE 11b-1: la fuente de stats sirvió al menos una tanda de páginas
    // (footer "SYNC · LOLALYTICS" en vez de "EN ESPERA"). Una vez encendida
    // queda encendida para la sesión.
    let mut sync_served: Signal<bool> = use_signal(|| false);

    // El effect no lee señales, así que no se re-ejecuta.
    use_effect(move || {
        // Catálogos Data Dragon: tareas independientes del loop LCU para no
        // bloquear la detección del cliente esperando la red. Ambas son
        // tareas de por vida: se pinean al parche del cliente y re-descargan
        // si el parche cambia.
        spawn(async move {
            run_catalog_loader(catalog, client_version).await;
        });

        spawn(async move {
            run_runes_loader(runes, client_version).await;
        });

        // FASE 11b-2b: catálogo de ítems. Mismo patrón de tarea de por
        // vida que los dos loaders anteriores (pinedo al parche del
        // cliente + reintento cada RETRY_INTERVAL): es la pieza más simple
        // que reusa `resolve_target_version`/`http_client` y no exige
        // cablear una carga on-demand al entrar al champ select.
        spawn(async move {
            run_item_loader(items, client_version).await;
        });

        // FASE 12: sondeo de la Live Client Data API (2999). Publica por
        // canal `watch`; el ÁRBITRO del espejo es run_lcu_loop.
        let (live_tx, live_rx) = tokio::sync::watch::channel(None);
        spawn(async move {
            crate::live::run_live_fetch_loop(live_tx, catalog).await;
        });

        spawn(async move {
            run_lcu_loop(
                app_state,
                match_context,
                catalog,
                client_version,
                summoner,
                platform,
                crate::lcu::client::LiveMirror { rx: live_rx, in_game },
            )
            .await;
        });

        // FASE 8b: sincronización del dataset de runas contra la Riot API.
        // El store abre el JSONL de `data_dir()` (override con
        // `KLIPY_DATA_DIR`); sin `RIOT_API_KEY` la tarea termina limpia con
        // un único log.
        spawn(async move {
            let store = JsonlStatsStore::new(data_dir().join(MATCHES_FILE_NAME));
            run_dataset_sync(summoner, platform, store).await;
        });

        // FASE 10: recomendador de runas. Reemplaza al logger de FASE 9:
        // misma lógica de preview_target/cache/un-intento-por-clave, pero
        // ahora publica `RecommendationsState` (la tab RUNAS lo consume) y
        // sigue logueando los resultados para la validación en vivo. El
        // contador `retry_runes` es el canal del botón REINTENTAR.
        spawn(async move {
            run_recommender(
                recommendations,
                app_state,
                match_context,
                catalog,
                runes,
                client_version,
                retry_runes,
            )
            .await;
        });

        // FASE 11b-2a: analizador del champ select. Mismo patrón de tarea de
        // por vida que el recomendador: un intento por (champ, posición,
        // parche), fetch paralelo de detalle/counters/items/skills y reset a
        // Idle al cerrar el champ select.
        spawn(async move {
            run_champ_analysis(
                champ_analysis,
                app_state,
                match_context,
                catalog,
                client_version,
            )
            .await;
        });

        // FASE 11a-2: perfil del invocador por LCU (segunda conexión REST
        // propia, re-fetch cada 5 min, reset a None al desconectar).
        spawn(async move {
            run_profile_loop(profile).await;
        });
    });

    // FASE 11b-1: encender el flag de sync apenas llegue una tanda de
    // páginas listas (el footer lo consume en las tres vistas).
    use_effect(move || {
        if let RecommendationsState::Ready { pages, .. } = recommendations.read().clone() {
            if !pages.is_empty() {
                sync_served.set(true);
            }
        }
    });

    // Vista activa (snapshot corto: el guard no cruza el rsx).
    let view = derive_view(&app_state.read(), profile.read().is_some());
    let ddragon_version: Option<String> = catalog.read().as_ref().map(|data| data.version.clone());

    rsx! {
        document::Link { rel: "icon", href: FAVICON }
        document::Link { rel: "stylesheet", href: MAIN_CSS }
        document::Link { rel: "stylesheet", href: FONTS_URL }

        // Capas de fondo de marca (decorativas)
        div { class: "bg-glow", "aria-hidden": "true" }
        div { class: "bg-stripes", "aria-hidden": "true" }
        div { class: "bg-vignette", "aria-hidden": "true" }

        {match view {
            AppView::Waiting => rsx! {
                WaitingView {
                    state: app_state,
                    platform: platform.read().clone(),
                    feature_version: ddragon_version.clone(),
                    patch_version: ddragon_version.clone(),
                    source_served: *sync_served.read(),
                }
            },
            AppView::Perfil => rsx! {
                ProfileView {
                    state: app_state,
                    profile: profile.read().clone().unwrap_or_default(),
                    summoner: summoner.read().clone(),
                    platform: platform.read().clone(),
                    catalog: catalog.read().clone(),
                    runes: runes.read().clone(),
                    source_served: *sync_served.read(),
                }
            },
            AppView::ChampSelect => rsx! {
                ChampSelectView {
                    state: app_state,
                    match_context,
                    catalog,
                    runes,
                    items,
                    recommendations,
                    analysis: champ_analysis,
                    patch_version: ddragon_version,
                    source_served: *sync_served.read(),
                    retry_runes,
                    in_game,
                }
            },
        }}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lcu::events::champion_placeholder;
    use crate::ui::shell::sync_label;

    /// Champ select activo gana siempre, con o sin pick.
    #[test]
    fn champ_select_activo_es_champ_select() {
        assert_eq!(
            derive_view(&AppState::InChampSelect(None), false),
            AppView::ChampSelect
        );
        assert_eq!(
            derive_view(&AppState::InChampSelect(Some(champion_placeholder(103))), true),
            AppView::ChampSelect
        );
    }

    /// Sin champ select: perfil cargado → PERFIL.
    #[test]
    fn con_perfil_es_perfil() {
        assert_eq!(
            derive_view(&AppState::WaitingForChampSelect, true),
            AppView::Perfil
        );
        // incluso desconectado en el instante en que el perfil viejo sigue
        // en la señal (el loop lo resetea en su próximo ciclo).
        assert_eq!(derive_view(&AppState::Disconnected, true), AppView::Perfil);
    }

    /// Sin champ select y sin perfil → pantalla de espera.
    #[test]
    fn sin_perfil_es_waiting() {
        assert_eq!(
            derive_view(&AppState::Disconnected, false),
            AppView::Waiting
        );
        assert_eq!(
            derive_view(&AppState::WaitingForChampSelect, false),
            AppView::Waiting
        );
    }

    /// El label de sync del footer se resuelve desde el módulo shell
    /// (smoke de integración del sistema de vistas).
    #[test]
    fn label_de_sync_para_el_footer() {
        assert_eq!(sync_label(false), "EN ESPERA");
        assert_eq!(sync_label(true), "LOLALYTICS");
    }
}
