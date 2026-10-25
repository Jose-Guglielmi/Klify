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
use crate::lcu::models::{AppState, ChampionInfo, MatchContext, SummonerInfo};
use crate::profile::models::ProfileSummary;
use crate::profile::run_profile_loop;
use crate::stats::builder::run_dataset_sync;
use crate::stats::source::{AnalysisState, RecommendationsState, run_champ_analysis, run_recommender};
use crate::stats::storage::{data_dir, JsonlStatsStore, MATCHES_FILE_NAME};
use crate::ui::champ_select::ChampSelectView;
use crate::ui::menu::MenuView;
use crate::ui::profile::ProfileView;
use crate::ui::shell::KlifySidebar;
use crate::ui::vivo::VivoView;
use crate::ui::waiting::WaitingView;

const FAVICON: Asset = asset!("/assets/favicon.ico");
const MAIN_CSS: Asset = asset!("/assets/main.css");

/// Hoja de estilos de las fuentes de marca (design.md): Barlow Condensed
/// (itálicas verdaderas, pesos 600/700) + Inter (400/600, datos en
/// tabular-nums).
const FONTS_URL: &str = "https://fonts.googleapis.com/css2?family=Barlow+Condensed:ital,wght@0,600;0,700;1,600;1,700&family=Inter:wght@400;600&display=swap";

/// Vista activa derivada del estado global (pura, testeada abajo):
///
/// - Modo manual de ficha (elegida del menú sin cliente) o champ select
///   activo ([`AppState::InChampSelect`]) → [`AppView::ChampSelect`].
/// - Resto: navegación explícita del usuario ([`NavVista`]) si la hay; sin
///   navegación el inicio es SIEMPRE el menú de fichas ([`AppView::Menu`])
///   y el perfil se visita con un botón.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppView {
    /// Pantalla de espera (la derivación ya no la produce: el menú manual
    /// cubre el caso "sin cliente"; se conserva la vista por si vuelve a
    /// usarse).
    Waiting,
    /// Perfil del invocador.
    Perfil,
    /// Vista VIVO: espejo de la partida en curso (Live Client Data API),
    /// se visita desde la sidebar.
    Vivo,
    /// Champ select en vivo (o modo manual con ficha elegida).
    ChampSelect,
    /// Menú manual: búsqueda de campeones + fichas, sin cliente de LoL.
    Menu,
}

/// Destino de navegación explícito del usuario entre el menú de fichas, la
/// vista VIVO (partida en curso) y el perfil — los tres botones de la
/// sidebar. En la señal vive como `Option`: `None` = sin navegación
/// (estado inicial) → la app arranca en el menú.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum NavVista {
    /// Menú de fichas (inicio de la app).
    Menu,
    /// Vista VIVO: espejo de la partida en curso.
    Vivo,
    /// Perfil del invocador.
    Perfil,
}

/// Deriva la vista desde `app_state` + el flag de modo manual + la
/// navegación explícita (`nav`).
///
/// Prioridad: modo manual → champ select ACTIVO → navegación explícita →
/// default (menú de fichas, SIEMPRE: `profile_loaded` ya no decide la
/// vista). Solo el draft real en curso ([`AppState::InChampSelect`]) pisa
/// la sidebar: cuando arranca el champ select la vista salta sola estés
/// donde estés, y al cerrarse el draft la navegación vuelve. "Cliente
/// conectado sin draft" ([`AppState::WaitingForChampSelect`]) navega libre
/// igual que [`AppState::Disconnected`] — FICHAS / VIVO / PERFIL desde la
/// sidebar — en vez de encerrar al usuario en la pantalla de espera.
/// `Waiting` ([`AppView::Waiting`]) sigue sin producirse.
///
/// Cuidado con el orden de escrituras: los tres handlers de nav de la
/// sidebar salen del modo manual ANTES de fijar nav (patrón
/// `on_exit_manual`, estado base `Disconnected`), así que el
/// corto-circuito por manual solo retiene la vista mientras la ficha está
/// abierta sin click de nav. El champ select REAL no pasa por ese escape:
/// con `manual_mode` falsa los handlers no tocan el estado y el draft
/// activo pisa igual. Los tests de `derive_view` fijan los post-estados de
/// esos clicks (manual=false); la secuencia de escrituras vive en los
/// handlers de `App`, no acá.
#[must_use]
pub fn derive_view(
    state: &AppState,
    profile_loaded: bool,
    manual_mode: bool,
    nav: &Option<NavVista>,
) -> AppView {
    // El modo manual gana siempre: pisa la derivación automática (y la
    // navegación) hasta que el usuario salga explícitamente del modo — por
    // el botón VOLVER de la ficha (`on_exit_manual`) o por cualquier click
    // de nav de la sidebar, que lo bajan antes de fijar la navegación.
    if manual_mode {
        return AppView::ChampSelect;
    }
    match state {
        // Champ select REAL ACTIVO: la partida pisa la navegación (cuando
        // arranca el draft, la vista salta sola esté donde esté).
        AppState::InChampSelect(_) => AppView::ChampSelect,
        // Cliente conectado sin draft activo (o desconectado): navegación
        // libre. Al salir de un champ select se vuelve acá: el usuario
        // recupera la sidebar en vez de quedar en la pantalla de espera.
        AppState::WaitingForChampSelect | AppState::Disconnected => {
            // Sin navegación explícita el inicio es el menú de fichas (el
            // perfil se visita con su botón). El flag de perfil quedó
            // fuera de la decisión: solo expresaba "hay cliente", que acá
            // no cambia nada (se conserva en la firma por los
            // tests/callers que lo pasan).
            let _ = profile_loaded;
            match nav {
                Some(NavVista::Menu) => AppView::Menu,
                Some(NavVista::Perfil) => AppView::Perfil,
                // Vista VIVO: también es un destino de la sidebar; champ
                // select real y modo manual ya volvieron antes, así que
                // acá no pisan.
                Some(NavVista::Vivo) => AppView::Vivo,
                // Default (sin navegación explícita): SIEMPRE el menú.
                None => AppView::Menu,
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
    let mut app_state: Signal<AppState> = use_signal(|| AppState::Disconnected);
    // Modo manual de ficha: activo cuando el usuario elige un campeón del
    // menú (sin cliente de LoL) — la app entra al champ select "a mano" y
    // sale cuando el usuario lo pide (botón VOLVER de la ficha o un click
    // de nav de la sidebar: los handlers de nav lo bajan antes de fijar la
    // nav). Leído en `derive_view` (abajo) y en los handlers de nav para
    // que App re-renderice al togglear.
    let mut manual_mode: Signal<bool> = use_signal(|| false);
    // Navegación explícita entre el menú de fichas, la vista VIVO y el
    // perfil (los tres botones de la sidebar): `None` = sin navegación →
    // el inicio SIEMPRE es el menú; el perfil se visita con el botón del
    // menú (`on_ver_perfil`) y se vuelve con el botón del perfil
    // (`on_ver_fichas`). Leída en `derive_view` (abajo) para que App
    // re-renderice al navegar.
    let mut nav_vista: Signal<Option<NavVista>> = use_signal(|| None);
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
                manual_mode,
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

    // Handlers del modo manual (los consume el menú y el champ select):
    //
    // - `on_pick`: el usuario eligió una ficha del menú → entramos al modo
    //   manual con el champ como pick en curso (mismo estado que usaría el
    //   cliente real, así toda la cadena recommender/análisis funciona igual).
    // - `on_exit_manual`: salir del modo → de vuelta a desconectado, lo que
    //   re-deriva la vista hacia el menú. Es el botón VOLVER de la ficha;
    //   los handlers de nav de acá abajo reutilizan el mismo patrón para
    //   dejar que la navegación gane sin tocar el champ select real.
    let on_pick = EventHandler::new(move |champ: ChampionInfo| {
        manual_mode.set(true);
        app_state.set(AppState::InChampSelect(Some(champ)));
    });
    let on_exit_manual = EventHandler::new(move |_| {
        manual_mode.set(false);
        app_state.set(AppState::Disconnected);
    });

    // Handlers de navegación (los consume el menú, el perfil y la sidebar):
    //
    // - Si el usuario está leyendo una ficha (modo manual), la navegación
    //   GANA: se sale del modo con el mismo patrón de `on_exit_manual`
    //   (estado base `Disconnected`) para que `derive_view` derive la vista
    //   pedida. Un champ select REAL no se corta acá: `manual_mode` es
    //   falsa, los handlers solo tocan nav y el estado pisa como siempre
    //   (el draft "agarra solo", ver `in_match`).
    // - `on_ver_perfil`: botón del menú/sidebar → visitar el perfil.
    // - `on_ver_fichas`: botón del perfil/sidebar → volver al menú de fichas.
    // - `on_ir_vivo`: botón de la sidebar → vista VIVO (partida en curso).
    let on_ver_perfil = EventHandler::new(move |_| {
        if *manual_mode.read() {
            manual_mode.set(false);
            app_state.set(AppState::Disconnected);
        }
        nav_vista.set(Some(NavVista::Perfil));
    });
    let on_ver_fichas = EventHandler::new(move |_| {
        if *manual_mode.read() {
            manual_mode.set(false);
            app_state.set(AppState::Disconnected);
        }
        nav_vista.set(Some(NavVista::Menu));
    });
    let on_ir_vivo = EventHandler::new(move |_| {
        if *manual_mode.read() {
            manual_mode.set(false);
            app_state.set(AppState::Disconnected);
        }
        nav_vista.set(Some(NavVista::Vivo));
    });

    // Vista activa (snapshot corto: el guard no cruza el rsx). Se leen
    // `manual_mode` y `nav_vista` acá para que App re-renderice cuando el
    // modo o la navegación cambien.
    let view = derive_view(
        &app_state.read(),
        profile.read().is_some(),
        *manual_mode.read(),
        &nav_vista.read(),
    );
    let ddragon_version: Option<String> = catalog.read().as_ref().map(|data| data.version.clone());

    // Sidebar: "en partida" = champ select real (cliente vivo) o modo
    // manual de ficha. Snapshot corto: el guard no cruza el rsx.
    let state_snapshot: AppState = app_state.read().clone();
    let in_match = matches!(state_snapshot, AppState::InChampSelect(_)) || *manual_mode.read();

    rsx! {
        document::Link { rel: "icon", href: FAVICON }
        document::Link { rel: "stylesheet", href: MAIN_CSS }
        document::Link { rel: "stylesheet", href: FONTS_URL }

        // Capas de fondo de marca (decorativas)
        div { class: "bg-glow", "aria-hidden": "true" }
        div { class: "bg-stripes", "aria-hidden": "true" }
        div { class: "bg-vignette", "aria-hidden": "true" }

        // Shell con sidebar fija: FICHAS · VIVO · PERFIL siempre visibles;
        // la vista activa (derivada) se renderiza al lado.
        div { class: "klify-shell",
            KlifySidebar {
                nav: nav_vista,
                in_match,
                on_ver_fichas: on_ver_fichas,
                on_ir_vivo: on_ir_vivo,
                on_ver_perfil: on_ver_perfil,
            }

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
                        on_ver_fichas,
                    }
                },
                AppView::Vivo => rsx! {
                    VivoView {
                        state: app_state,
                        catalog: catalog,
                        on_pick: on_pick,
                    }
                },
                AppView::Menu => rsx! {
                    MenuView {
                        catalog,
                        profile,
                        manual_mode,
                        app_state,
                        on_pick,
                        on_ver_perfil,
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
                        manual: manual_mode(),
                        // El perfil LCU solo se carga con cliente vivo: lo uso
                        // como proxy de "cliente conectado" (lee `profile` acá
                        // para que App re-renderice al conectar/desconectar).
                        client_connected: profile.read().is_some(),
                        on_exit_manual,
                    }
                },
            }}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lcu::events::champion_placeholder;
    use crate::ui::shell::sync_label;

    /// Champ select activo gana siempre, con o sin pick (y pisa la
    /// navegación explícita).
    #[test]
    fn champ_select_activo_es_champ_select() {
        assert_eq!(
            derive_view(&AppState::InChampSelect(None), false, false, &None),
            AppView::ChampSelect
        );
        assert_eq!(
            derive_view(
                &AppState::InChampSelect(Some(champion_placeholder(103))),
                true,
                false,
                &Some(NavVista::Perfil)
            ),
            AppView::ChampSelect
        );
    }

    /// El modo manual de ficha pisa cualquier derivación automática (y la
    /// navegación explícita).
    #[test]
    fn modo_manual_fuerza_champ_select() {
        assert_eq!(
            derive_view(&AppState::Disconnected, false, true, &Some(NavVista::Menu)),
            AppView::ChampSelect
        );
        assert_eq!(
            derive_view(
                &AppState::WaitingForChampSelect,
                true,
                true,
                &Some(NavVista::Perfil)
            ),
            AppView::ChampSelect
        );
    }

    /// Sin champ select y sin navegación: el inicio es SIEMPRE el menú de
    /// fichas, incluso con perfil cargado (el perfil se visita con botón).
    #[test]
    fn con_perfil_y_sin_nav_es_menu() {
        assert_eq!(
            derive_view(&AppState::WaitingForChampSelect, true, false, &None),
            AppView::Menu
        );
        // incluso desconectado en el instante en que el perfil viejo sigue
        // en la señal (el loop lo resetea en su próximo ciclo).
        assert_eq!(
            derive_view(&AppState::Disconnected, true, false, &None),
            AppView::Menu
        );
    }

    /// Sin champ select y sin perfil → menú manual de fichas (sin cliente
    /// la app se comporta como si detectara el cliente).
    #[test]
    fn sin_perfil_es_menu() {
        assert_eq!(
            derive_view(&AppState::Disconnected, false, false, &None),
            AppView::Menu
        );
        assert_eq!(
            derive_view(&AppState::WaitingForChampSelect, false, false, &None),
            AppView::Menu
        );
    }

    /// Navegación explícita al perfil: con perfil cargado muestra el
    /// perfil aunque el default sea el menú.
    #[test]
    fn nav_perfil_muestra_perfil() {
        assert_eq!(
            derive_view(
                &AppState::WaitingForChampSelect,
                true,
                false,
                &Some(NavVista::Perfil)
            ),
            AppView::Perfil
        );
    }

    /// Navegación explícita al menú: pisa el default (que ya es menú) y
    /// vuelve al menú aunque el perfil siga cargado.
    #[test]
    fn nav_menu_pisa_default() {
        assert_eq!(
            derive_view(
                &AppState::WaitingForChampSelect,
                true,
                false,
                &Some(NavVista::Menu)
            ),
            AppView::Menu
        );
    }

    /// Navegación explícita a VIVO desde la sidebar: muestra la vista VIVO
    /// (con o sin cliente conectado).
    #[test]
    fn nav_vivo_muestra_vivo() {
        assert_eq!(
            derive_view(
                &AppState::WaitingForChampSelect,
                true,
                false,
                &Some(NavVista::Vivo)
            ),
            AppView::Vivo
        );
        assert_eq!(
            derive_view(&AppState::Disconnected, false, false, &Some(NavVista::Vivo)),
            AppView::Vivo
        );
    }

    /// Champ select real y modo manual SIGUEN pisando la navegación a VIVO
    /// (regresión de la prioridad con la variante nueva de `NavVista`).
    #[test]
    fn champ_select_y_manual_pisan_nav_vivo() {
        // Champ select real, con o sin pick.
        assert_eq!(
            derive_view(
                &AppState::InChampSelect(None),
                false,
                false,
                &Some(NavVista::Vivo)
            ),
            AppView::ChampSelect
        );
        assert_eq!(
            derive_view(
                &AppState::InChampSelect(Some(champion_placeholder(103))),
                true,
                false,
                &Some(NavVista::Vivo)
            ),
            AppView::ChampSelect
        );
        // Modo manual de ficha (sin cliente).
        assert_eq!(
            derive_view(&AppState::Disconnected, false, true, &Some(NavVista::Vivo)),
            AppView::ChampSelect
        );
    }

    /// FIX de navegación desde una ficha (modo manual): los tres handlers
    /// de nav de la sidebar salen del modo con el patrón `on_exit_manual`
    /// (manual=false + estado base `Disconnected`) antes de fijar nav — la
    /// escritura vive en los handlers de `App` (no hay infraestructura de
    /// señales testeable acá), así que este test fija los post-estados que
    /// esos clicks deben dejar: la misma nav que el modo pisaba ahora
    /// deriva la vista pedida.
    #[test]
    fn post_estado_de_nav_en_modo_manual_deriva_vista_pedida() {
        // Referencia pre-click: leyendo una ficha, la vista queda fija
        // aunque ya hubiera nav pedida.
        assert_eq!(
            derive_view(
                &AppState::InChampSelect(Some(champion_placeholder(103))),
                false,
                true,
                &Some(NavVista::Perfil)
            ),
            AppView::ChampSelect
        );
        // Post-click de los tres handlers: manual pago por el handler y
        // estado base `Disconnected` → cada nav llega a su vista.
        for (nav, esperada) in [
            (NavVista::Menu, AppView::Menu),
            (NavVista::Perfil, AppView::Perfil),
            (NavVista::Vivo, AppView::Vivo),
        ] {
            assert_eq!(
                derive_view(&AppState::Disconnected, false, false, &Some(nav)),
                esperada
            );
        }
    }

    /// FIX navegación libre: con cliente conectado sin draft
    /// (`WaitingForChampSelect`) la sidebar navega igual que desconectado —
    /// cada destino llega a su vista en vez de encerrarte en champ select.
    #[test]
    fn waiting_deja_navegar() {
        assert_eq!(
            derive_view(
                &AppState::WaitingForChampSelect,
                true,
                false,
                &Some(NavVista::Menu)
            ),
            AppView::Menu
        );
        assert_eq!(
            derive_view(
                &AppState::WaitingForChampSelect,
                true,
                false,
                &Some(NavVista::Perfil)
            ),
            AppView::Perfil
        );
        assert_eq!(
            derive_view(
                &AppState::WaitingForChampSelect,
                true,
                false,
                &Some(NavVista::Vivo)
            ),
            AppView::Vivo
        );
    }

    /// Regresión: el champ select real ACTIVO pisa cualquier navegación —
    /// la vista salta sola al draft estés donde estés (FICHAS/PERFIL/VIVO).
    #[test]
    fn champ_select_activo_pisa_nav() {
        for nav in [
            Some(NavVista::Menu),
            Some(NavVista::Perfil),
            Some(NavVista::Vivo),
        ] {
            assert_eq!(
                derive_view(
                    &AppState::InChampSelect(Some(champion_placeholder(103))),
                    true,
                    false,
                    &nav
                ),
                AppView::ChampSelect
            );
        }
    }

    /// Secuencia exacta del bug: entrar a un champ select real, salir (el
    /// cliente queda abierto → `WaitingForChampSelect`) y pedir VIVO en la
    /// sidebar. La navegación responde: no hay encierro en la pantalla de
    /// "esperando champ select".
    #[test]
    fn al_salir_del_champ_select_vuelve_la_nav() {
        // Mientras duró el draft, la partida pisó la navegación aunque el
        // usuario hubiera pedido VIVO.
        assert_eq!(
            derive_view(
                &AppState::InChampSelect(Some(champion_placeholder(103))),
                true,
                false,
                &Some(NavVista::Vivo)
            ),
            AppView::ChampSelect
        );
        // Al cerrarse el draft (cliente abierto, sin partida) la MISMA
        // navegación a VIVO ya responde.
        assert_eq!(
            derive_view(
                &AppState::WaitingForChampSelect,
                true,
                false,
                &Some(NavVista::Vivo)
            ),
            AppView::Vivo
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
