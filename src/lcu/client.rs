//! Orquestador de la conexión con el cliente de League of Legends (LCU).
//!
//! Flujo completo (cada ciclo del bucle exterior = un intento de conexión):
//!
//! 1. **Detección**: se crea un [`LcuWebSocket`] de `irelia`. Su hilo nativo
//!    localiza el cliente vía `sysinfo` (multiplataforma, sin rutas
//!    hardcodeadas). Si no hay cliente, el socket muere enseguida y este bucle
//!    reintenta cada [`DETECTION_RETRY`]. Estado: `Disconnected`.
//! 2. **Cliente detectado**: cuando el hilo procesa nuestra suscripción al
//!    endpoint de champ select (mensaje [`SessionMessage::Connected`]), hay
//!    cliente vivo. Estado: `WaitingForChampSelect`.
//! 3. **Sincronización inicial**: se conecta el cliente REST y se hace un GET
//!    de [`CHAMP_SELECT_SESSION_URI`] por si la app arrancó con un champ
//!    select ya en curso (el websocket solo avisa de cambios).
//! 4. **Sesión**: los eventos del websocket llegan al hilo nativo de irelia y
//!    se puentean a esta tarea con un canal `tokio::sync::mpsc` (el callback
//!    corre en un thread del SO, no es un Stream tokio). Cada payload se
//!    parsea y actualiza `AppState`: `InChampSelect(Some(champ))`,
//!    `InChampSelect(None)` o, si la sesión se cerró, `WaitingForChampSelect`.
//! 5. **Caída**: si el hilo del websocket muere (cliente cerrado) o el REST
//!    deja de responder, se vuelve a `Disconnected` y al paso 1.
//!
//! **Catálogo Data Dragon**: la resolución de nombre/imagen del campeón usa
//! una señal aparte `catalog: Signal<Option<ChampionCatalog>>` que alimenta
//! [`crate::data_dragon::run_catalog_loader`]. Cada evento champ select
//! resuelve contra el snapshot vigente del catálogo (lectura con guard de
//! ámbito corto, nunca sostenida sobre un `await`). Si el catálogo llega
//! tarde (la app arrancó sin internet y el champ select ya empezó), el
//! chequeo de salud de 5 s re-resuelve el placeholder en cuanto el catálogo
//! aparece ([`re_resolve_current`]); si no cambia nada, la transición es un
//! no-op.
//!
//! **Contexto de partida (FASE 6)**: además de `AppState`, cada evento de
//! sesión alimenta una señal `match_context: Signal<MatchContext>` con el
//! snapshot de la partida —ambos equipos, posiciones asignadas y pick local—
//! base del futuro recomendador de runas (RF-01..RF-04 del spec). Se resetea
//! a `MatchContext::default()` cuando el champ select se cierra (evento
//! `Delete`, GET `null`/404) y al desconectarse el cliente. Usa el mismo
//! patrón de espejo local + `.set()` condicional que `AppState`, de modo que
//! la señal solo cambia cuando el snapshot realmente cambió.
//!
//! **Versión del juego (FASE 7)**: al conectar el REST se hace un GET a
//! [`GAME_VERSION_URI`] y se publica en la señal
//! `client_version: Signal<Option<String>>`. Los loaders de Data Dragon la
//! usan para pinear los catálogos al parche instalado (ver
//! [`crate::data_dragon::choose_version`]). La versión se conserva aunque el
//! cliente se cierre (el parche instalado sigue siendo el mejor pin y evita
//! re-descargas innecesarias); un cliente nuevo con otro parche la actualiza
//! al reconectar.
//!
//! **Invocador actual (FASE 8a)**: en el mismo momento de la conexión REST
//! se consulta [`SUMMONER_URI`] y se publica en la señal
//! `summoner: Signal<Option<SummonerInfo>>` (mismo patrón de espejo local).
//! Es el puuid que la Riot API Match-V5 de [`crate::riot_api`] usará como
//! raíz del historial para el dataset de runas (FASE 8b). Igual que la
//! versión, se consulta una vez por conexión REST y se conserva al
//! desconectar; si el GET falla se loguea y la señal queda como esté
//! (`None` si nunca hubo dato).
//!
//! **Platform id (FASE 8b)**: en la misma conexión REST se consulta
//! `/riotclient/region-locale` (con `/lol-login/v1/session` de respaldo) y
//! se publica el platform id (`"LA1"` → `"la1"`) en la señal
//! `platform: Signal<Option<String>>`, normalizado y validado con
//! [`crate::riot_api::normalize_platform`] — es la pieza que la tarea de
//! sincronización del dataset ([`crate::stats::builder`]) necesita para
//! construir el cliente de la Riot API. Si faltó al conectar (login en
//! curso), el chequeo de salud lo reintenta en silencio.
//!
//! **Backstop anti-sesión fantasma (FASE 13)**: si el LCU desarma un draft
//! y el `Delete` nunca llega por websocket, el chequeo de salud re-consulta
//! el GET de la sesión y baja el estado a `WaitingForChampSelect` con el
//! contexto reseteado ([`backstop_procede`]) — sin tocar el espejo de la
//! Live API y fuera del modo manual.
//!
//! `irelia` 0.11 reconecta el socket pero **no** re-envía las suscripciones
//! tras un reconnect, por eso en vez de usar `Flow::TryReconnect` de forma
//! eterna, el error handler corta el hilo ([`BreakOnError`]) y el orquestador
//! recrea socket + suscripción desde cero en cada ciclo: más simple y sin
//! hilos ni suscripciones huérfanas.

use std::ops::ControlFlow;
use std::time::Duration;

use dioxus::prelude::{ReadableExt, Signal, WritableExt};
use irelia::requests::RequestClientType;
use irelia::rest::LcuClient;
use irelia::ws::types::{Event, EventKind, RequestType};
use irelia::ws::{ErrorHandler, Flow, LcuWebSocket, Subscriber, WebSocketError};
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::time::{interval, sleep};

use crate::data_dragon::models::{resolve_champion, ChampionCatalog};
use crate::lcu::events::{
    local_champion_id, parse_session_payload, ChampSelectSession, ChampSelectUpdate,
    SessionPayload, CHAMP_SELECT_SESSION_URI,
};
use crate::lcu::models::{AppState, MatchContext, SummonerInfo};
use crate::riot_api::normalize_platform;

/// Pausa entre intentos de detección del cliente (~2.5 s, según spec 2-3 s).
const DETECTION_RETRY: Duration = Duration::from_millis(2500);
/// Frecuencia del chequeo de salud durante una sesión activa.
const HEALTH_CHECK_INTERVAL: Duration = Duration::from_millis(5000);
/// Endpoint REST que reporta la versión del juego instalado; es el ancla
/// para pinear los catálogos de Data Dragon al parche del cliente.
///
/// NOTA (2026-10, verificado con cliente real de LA2 16.20): el endpoint
/// clásico `/lol-game-data/v1/version` devuelve **404** en los clientes
/// actuales. El válido es `/lol-patch/v1/game-version`, que devuelve la
/// versión completa de build (ej. `"16.20.8248524+branch.releases-16-20.
/// code.public.content.release.anticheat.vanguard"`); la metadata posterior
/// al `+` se recorta con [`clean_game_version`] antes de publicar la señal.
const GAME_VERSION_URI: &str = "/lol-patch/v1/game-version";
/// Endpoint REST con los datos del invocador actual: de acá sale el `puuid`
/// que la Riot API Match-V5 ([`crate::riot_api`]) necesita para el dataset
/// de runas (FASE 8b).
const SUMMONER_URI: &str = "/lol-summoner/v1/current-summoner";
/// Endpoint REST que reporta región/locale del cliente: es la fuente
/// principal del platform id (ej. `"LA1"`) que la FASE 8b usa para resolver
/// el routing regional de la Riot API.
const REGION_LOCALE_URI: &str = "/riotclient/region-locale";
/// Endpoint REST de la sesión de login: fuente de respaldo del platform id
/// por si [`REGION_LOCALE_URI`] no lo trae (campo `region`, ej. `"LA1"`).
const LOGIN_SESSION_URI: &str = "/lol-login/v1/session";

/// Subconjunto mínimo de `/riotclient/region-locale` (FASE 8b): solo el
/// platform id; `locale`, `webLanguage` y el resto se ignoran. Tolerante:
/// payload parcial o `region` ausente/`null` → `None` sin error.
#[derive(Debug, Deserialize)]
struct RegionLocaleDto {
    #[serde(default)]
    region: Option<String>,
}

/// Subconjunto mínimo de `/lol-login/v1/session` (respaldo): solo el campo
/// `region`. Tolerante: el resto del payload (`state`, `account`, ...) se
/// ignora.
#[derive(Debug, Deserialize)]
struct LoginSessionDto {
    #[serde(default)]
    region: Option<String>,
}

/// Mensajes que llegan del hilo nativo del websocket hacia la tarea asíncrona.
enum SessionMessage {
    /// La suscripción fue procesada por el hilo del websocket: hay cliente.
    Connected,
    /// Campo `data` del evento de champ select (`Null` = sesión cerrada).
    Session(Value),
}

/// Suscriptor de irelia que puentea los eventos al canal tokio.
struct SessionSubscriber {
    tx: UnboundedSender<SessionMessage>,
}

impl Subscriber for SessionSubscriber {
    /// Se invoca cuando el hilo del websocket procesa nuestra suscripción,
    /// lo que solo ocurre con la conexión ya establecida: es nuestra señal de
    /// "cliente detectado".
    fn on_subscribe(&mut self, _event_kind: &EventKind, _request_code: &RequestType) {
        let _ = self.tx.send(SessionMessage::Connected);
    }

    fn on_event(&mut self, event: &Event, _continues: &mut bool) {
        let _ = self.tx.send(SessionMessage::Session(event.2.data.clone()));
    }
}

/// Corta el hilo del websocket ante el primer error. El orquestador recrea
/// socket y suscripción en el siguiente ciclo (ver doc del módulo).
struct BreakOnError;

impl ErrorHandler for BreakOnError {
    fn on_error(&mut self, error: WebSocketError) -> ControlFlow<(), Flow> {
        // "Cliente no encontrado" es el caso esperado mientras no hay cliente;
        // cualquier otro error sí interesa en consola.
        if !matches!(error, WebSocketError::ProcessInfo(_)) {
            eprintln!("klipy [lcu]: error de websocket: {error}");
        }
        ControlFlow::Break(())
    }
}

/// Bucle de vida completo de la conexión LCU. Nunca retorna: vive mientras
/// viva la app, alternando entre detección y sesión según el cliente.
///
/// `match_context` es el snapshot de partida (FASE 6): se actualiza en cada
/// evento de sesión con [`MatchContext::from_session`] y se resetea al
/// cerrarse el champ select o al desconectarse el cliente.
///
/// `catalog` es el catálogo de Data Dragon (ver doc del módulo): puede llegar
/// en cualquier momento, incluso varios minutos después de arrancar la app.
///
/// `client_version` (FASE 7) recibe la versión del juego que reporta el
/// cliente al conectar (una sola vez por conexión REST); los loaders de
/// Data Dragon la leen para pinear los catálogos a ese parche.
///
/// `summoner` (FASE 8a) recibe los datos del invocador actual
/// ([`SummonerInfo`], puuid incluido) en el mismo momento de la conexión
/// REST; es la raíz del historial que la FASE 8b va a recorrer con la Riot
/// API Match-V5.
///
/// `platform` (FASE 8b) recibe el platform id que reporta el cliente en la
/// misma conexión REST (normalizado lowercase y validado contra la tabla de
/// routing con [`normalize_platform`]): es la pieza que falta para construir
/// el cliente de la Riot API. Igual que el invocador, se consulta una vez
/// por conexión y se conserva al desconectar; ver [`fetch_platform`].
///
/// FASE 12 · Canales del espejo de partida en curso (agrupados para mantener
/// la firma de [`run_lcu_loop`] acotada).
pub struct LiveMirror {
    /// Publicado por [`crate::live::run_live_fetch_loop`]: `Some` mientras
    /// hay partida con mi champ resuelto.
    pub rx: tokio::sync::watch::Receiver<Option<crate::live::LiveGame>>,
    /// Flag para la UI: la vista de champ select cambia su etiqueta a
    /// "Partida en vivo" mientras dura el espejo.
    pub in_game: Signal<bool>,
}

/// **MODO MANUAL**: `manual_mode` en `true` congela las escrituras de este
/// loop sobre la señal de estado de la app (el champ actual lo eligió el
/// usuario desde el menú, sin juego abierto). La detección, la
/// sincronización REST y los eventos del websocket siguen corriendo
/// "en seco": la conexión se publica en versión/invocador/platform/contexto,
/// pero ninguna transición de `AppState` llega a la señal hasta que el flag
/// vuelva a `false`.
#[allow(clippy::too_many_arguments)]
pub async fn run_lcu_loop(
    mut app_state: Signal<AppState>,
    mut match_context: Signal<MatchContext>,
    catalog: Signal<Option<ChampionCatalog>>,
    mut client_version: Signal<Option<String>>,
    mut summoner: Signal<Option<SummonerInfo>>,
    mut platform: Signal<Option<String>>,
    live: LiveMirror,
    manual_mode: Signal<bool>,
) {
    let mut live_rx = live.rx;
    let mut in_game = live.in_game;
    // Espejo local del estado: evita leer la señal dentro de la tarea y
    // mantiene la lógica de transiciones pura y testeable (`next_state`).
    let mut current = AppState::Disconnected;
    // MODO MANUAL: si el usuario ya eligió champ, el reset inicial no pisa
    // la señal (el guard vive en `transition`).
    transition(
        &mut current,
        &mut app_state,
        *manual_mode.peek(),
        AppState::Disconnected,
    );

    // FASE 12 · Espejo de partida en curso: `Some(champ_id)` mientras la
    // app muestra una partida de la Live API (el árbitro es ESTE loop: si
    // hay champ select real activo, la Live API espera).
    let mut live_mirror: Option<i64> = None;

    // Espejo local del contexto de partida (mismo patrón que `current`).
    let mut current_context = MatchContext::default();
    set_match_context(
        &mut current_context,
        &mut match_context,
        MatchContext::default(),
    );

    // Espejo local de la versión del juego del cliente (mismo patrón).
    let mut current_client_version = Option::<String>::None;
    set_client_version(&mut current_client_version, &mut client_version, None);

    // Espejo local del invocador actual: solo se publica con fetch exitoso,
    // así un fallo puntual nunca borra el último dato válido.
    let mut current_summoner = Option::<SummonerInfo>::None;

    // Espejo local del platform id (FASE 8b): solo valores normalizados y
    // válidos según la tabla de routing; se conserva al desconectar.
    let mut current_platform = Option::<String>::None;

    loop {
        // FASE 1: detección del cliente vía el websocket de irelia.
        let Some((ws, mut session_rx)) = start_websocket() else {
            sleep(DETECTION_RETRY).await;
            continue;
        };

        // FASE 2: esperar conexión (o muerte del hilo si no hay cliente).
        match session_rx.recv().await {
            None => {
                // El hilo murió sin conectarse: cliente no está corriendo.
                sleep(DETECTION_RETRY).await;
                continue;
            }
            // No debería ocurrir un evento antes que `Connected`; se acepta
            // por robustez.
            Some(SessionMessage::Session(payload)) => {
                let catalog_snapshot = catalog.read();
                // MODO MANUAL: el guard de `transition` decide.
                transition(
                    &mut current,
                    &mut app_state,
                    *manual_mode.peek(),
                    AppState::WaitingForChampSelect,
                );
                apply_update(
                    &mut current,
                    &mut app_state,
                    &mut current_context,
                    &mut match_context,
                    payload,
                    catalog_snapshot.as_ref(),
                    *manual_mode.peek(),
                );
            }
            Some(SessionMessage::Connected) => {
                // MODO MANUAL: el guard de `transition` decide.
                transition(
                    &mut current,
                    &mut app_state,
                    *manual_mode.peek(),
                    AppState::WaitingForChampSelect,
                );
            }
        }

        // FASE 3: cliente REST + versión del juego + GET inicial de la sesión.
        let client = connect_rest().await;
        if let Some(client) = &client {
            // El parche instalado se consulta una vez por conexión REST y
            // queda expuesto para los loaders de catálogos. No se resetea al
            // desconectar (ver doc del módulo).
            let version = fetch_client_version(client).await;
            set_client_version(&mut current_client_version, &mut client_version, version);

            // FASE 8a: datos del invocador actual (puuid para Match-V5),
            // una vez por conexión REST. Si falla, se loguea y la señal
            // queda como esté (ver doc del módulo).
            if let Some(summoner_info) = fetch_current_summoner(client).await {
                set_summoner(&mut current_summoner, &mut summoner, summoner_info);
            }

            // FASE 8b: platform id para el routing regional de la Riot API,
            // en el mismo momento de conexión. Si falla, se loguea y la
            // señal queda como esté.
            if let Some(platform_id) = fetch_platform(client).await {
                set_platform(&mut current_platform, &mut platform, platform_id);
            }

            // Hoist del bool: el guard de `peek()` no debe vivir a través
            // del `await` (ver clippy.toml).
            let manual = *manual_mode.peek();
            refresh_initial_state(
                client,
                &mut current,
                &mut app_state,
                &mut current_context,
                &mut match_context,
                &catalog,
                manual,
            )
            .await;
        }

        // FASE 4: bucle de sesión, alimentado por el websocket.
        let mut health = interval(HEALTH_CHECK_INTERVAL);
        let mut session_alive = true;
        while session_alive {
            tokio::select! {
                message = session_rx.recv() => match message {
                    Some(SessionMessage::Session(payload)) => {
                        // Guard de ámbito corto: se suelta antes de cualquier
                        // `await` (ver clippy.toml).
                        let catalog_snapshot = catalog.read();
                        apply_update(
                            &mut current,
                            &mut app_state,
                            &mut current_context,
                            &mut match_context,
                            payload,
                            catalog_snapshot.as_ref(),
                            *manual_mode.peek(),
                        );
                    }
                    // El hilo nunca se reconecta (ver doc del módulo); si
                    // llegara a repetirse, re-sincronizamos con el GET.
                    Some(SessionMessage::Connected) => {
                        if let Some(client) = &client {
                            // Hoist del bool: el guard de `peek()` no debe
                            // vivir a través del `await` (ver clippy.toml).
                            let manual = *manual_mode.peek();
                            refresh_initial_state(
                                client,
                                &mut current,
                                &mut app_state,
                                &mut current_context,
                                &mut match_context,
                                &catalog,
                                manual,
                            )
                            .await;
                        }
                    }
                    // El hilo del websocket murió: el cliente se cerró o cayó.
                    None => session_alive = false,
                },
                // FASE 12: partida en curso publicada por la Live API
                // (el canal solo avisa en cambios reales).
                res = live_rx.changed() => {
                    let _ = res;
                    handle_live_update(
                        live_rx.borrow().clone(),
                        &mut current,
                        &mut live_mirror,
                        &mut app_state,
                        &mut current_context,
                        &mut match_context,
                        &mut in_game,
                        *manual_mode.peek(),
                    );
                }
                _ = health.tick() => {
                    // Catálogo tardío: si el campeón actual es un placeholder
                    // y el catálogo ya llegó, re-resolver. Es un no-op cuando
                    // nada cambió (mismo valor ⇒ `transition` no toca la señal).
                    {
                        let catalog_snapshot = catalog.read();
                        let next = re_resolve_current(&current, catalog_snapshot.as_ref());
                        // MODO MANUAL: el guard de `transition` decide.
                        transition(&mut current, &mut app_state, *manual_mode.peek(), next);
                    }
                    let ws_dead = ws.is_finished();
                    // `head` solo falla ante errores de transporte (cliente
                    // muerto); cualquier código HTTP indica cliente vivo.
                    let rest_dead = match &client {
                        Some(client) => !client.head(CHAMP_SELECT_SESSION_URI).await.is_ok(),
                        None => false,
                    };
                    if ws_dead || rest_dead {
                        session_alive = false;
                    } else {
                        // FASE 12: re-chequeo del live con el valor VIGENTE
                        // del canal — cubre el caso "la partida ya estaba
                        // arriba cuando el champ select terminó" (sin
                        // `changed()` nuevo, el tick vuelve a intentar el
                        // espejo; es no-op cuando nada cambió).
                        handle_live_update(
                            live_rx.borrow().clone(),
                            &mut current,
                            &mut live_mirror,
                            &mut app_state,
                            &mut current_context,
                            &mut match_context,
                            &mut in_game,
                            *manual_mode.peek(),
                        );
                        if let Some(client) = &client {
                        // FASE 8b: si conectamos durante la pantalla de
                        // login, invocador y platform llegan recién cuando
                        // el jugador inicia sesión. Reintento silencioso
                        // (sin log) cada tick mientras falten; el valor se
                        // publica apenas aparece y se conserva (ver doc del
                        // módulo).
                        if current_summoner.is_none() {
                            if let Ok(info) = client.get::<SummonerInfo>(SUMMONER_URI).await {
                                if info.puuid.as_deref().is_some_and(|p| !p.is_empty()) {
                                    set_summoner(&mut current_summoner, &mut summoner, info);
                                }
                            }
                        }
                        if current_platform.is_none() {
                            if let Some(platform_id) =
                                fetch_platform_quiet(client).await
                            {
                                set_platform(&mut current_platform, &mut platform, platform_id);
                            }
                        }

                        // FASE 13 · Backstop anti-sesión fantasma: si el estado sigue en
                        // champ select pero el LCU ya no reporta sesión (o reporta la
                        // fantasma de equipos vacías que deja al desarmar el draft),
                        // volvemos a WaitingForChampSelect aunque el websocket no haya
                        // avisado. Solo corre cuando NO hay partida en vivo espejándose
                        // (ese estado lo maneja la Live API) y fuera del modo manual.
                        // Hoist del bool: el guard de `peek()` no debe vivir a través
                        // del `await` del GET (ver clippy.toml).
                        let manual = *manual_mode.peek();
                        if !manual && backstop_procede(&current, &live_mirror) {
                            match client
                                .get::<Option<ChampSelectSession>>(CHAMP_SELECT_SESSION_URI)
                                .await
                            {
                                // Fantasma (equipos vacías): misma baja que un cierre limpio.
                                Ok(Some(session))
                                    if crate::lcu::events::sesion_es_fantasma(&session) =>
                                {
                                    bajar_del_champ_select(
                                        &mut current,
                                        &mut app_state,
                                        &mut current_context,
                                        &mut match_context,
                                    );
                                }
                                // La sesión ya no existe (404/null): idem. Los demás errores
                                // también bajan: el `head` de este mismo tick ya garantizó
                                // que hay cliente vivo, y algunos builds responden 204 con
                                // body vacío (parsea como error, indistinguible de "sin
                                // sesión" a efectos del backstop). Si el draft real sigue
                                // activo, el próximo evento del websocket restablece el
                                // estado; a efectos del bug (quedarse colgado para
                                // siempre), bajar es el único desenlace que autocorrige.
                                Err(_) | Ok(None) => {
                                    bajar_del_champ_select(
                                        &mut current,
                                        &mut app_state,
                                        &mut current_context,
                                        &mut match_context,
                                    );
                                }
                                // Draft real en curso: no-op.
                                Ok(Some(_)) => {}
                            }
                        }
                        }
                    }
                }
            }
        }

        // FASE 5: caída → `Disconnected`, reset del contexto y nueva ronda
        // de detección.
        let _ = ws.abort();
        // MODO MANUAL: la caída del cliente tampoco pisa el champ elegido a
        // mano; el espejo local sí vuelve a `Disconnected` para que, al salir
        // del modo manual, la próxima transición retome el control limpio.
        if *manual_mode.peek() {
            current = AppState::Disconnected;
        } else {
            transition(
                &mut current,
                &mut app_state,
                *manual_mode.peek(),
                AppState::Disconnected,
            );
        }
        set_match_context(
            &mut current_context,
            &mut match_context,
            MatchContext::default(),
        );
        sleep(DETECTION_RETRY).await;
    }
}

/// Crea el websocket de irelia y se suscribe al endpoint de champ select.
///
/// La suscripción se envía a la cola interna del hilo y se procesa apenas el
/// socket se conecta; `None` = el hilo murió antes de suscribirse.
fn start_websocket() -> Option<(LcuWebSocket, UnboundedReceiver<SessionMessage>)> {
    let (session_tx, session_rx) = mpsc::unbounded_channel::<SessionMessage>();
    let mut ws = LcuWebSocket::new_with_error_handler(BreakOnError);

    let event_kind = EventKind::json_api_event_callback(CHAMP_SELECT_SESSION_URI);
    ws.subscribe(event_kind, SessionSubscriber { tx: session_tx })?;

    Some((ws, session_rx))
}

/// Conecta el cliente REST de irelia. Es bloqueante (escaneo de procesos con
/// `sysinfo`), así que corre en `spawn_blocking`.
async fn connect_rest() -> Option<LcuClient<RequestClientType>> {
    match tokio::task::spawn_blocking(LcuClient::connect).await {
        Ok(Ok(client)) => Some(client),
        Ok(Err(error)) => {
            eprintln!("klipy [lcu]: no se pudo conectar por REST: {error}");
            None
        }
        Err(error) => {
            eprintln!("klipy [lcu]: tarea de conexión REST falló: {error}");
            None
        }
    }
}

/// GET de [`GAME_VERSION_URI`]: la versión del juego instalado (ej.
/// `"16.20.8248524+branch....vanguard"`), que los loaders de Data Dragon
/// usan para pinear los catálogos al parche del cliente. La metadata de
/// build posterior al `+` se recorta ([`clean_game_version`]). `None` si el
/// endpoint falla o trae un string vacío: en ese caso los loaders caen al
/// latest, como siempre.
async fn fetch_client_version(client: &LcuClient<RequestClientType>) -> Option<String> {
    match client.get::<String>(GAME_VERSION_URI).await {
        Ok(raw) => {
            let version = clean_game_version(&raw);
            (!version.is_empty()).then_some(version)
        }
        Err(error) => {
            eprintln!("klipy [lcu]: no se pudo leer la versión del juego: {error}");
            None
        }
    }
}

/// Recorta la versión cruda de [`GAME_VERSION_URI`] a la forma utilizable:
/// descarta la metadata de build posterior al `+` (ej. `"+branch.releases-
/// 16-20..."`) y los espacios de más. Data Dragon solo matchea por los
/// primeros componentes del parche (`choose_version`), pero publicar la
/// versión limpia mantiene los logs y la señal legibles.
#[must_use]
fn clean_game_version(raw: &str) -> String {
    raw.split('+').next().unwrap_or_default().trim().to_string()
}

/// GET de [`SUMMONER_URI`]: datos del invocador actual (puuid incluido).
/// `None` si el endpoint falla: se loguea y se sigue (la señal queda como
/// esté; no es fatal para el resto de la app).
async fn fetch_current_summoner(client: &LcuClient<RequestClientType>) -> Option<SummonerInfo> {
    match client.get::<SummonerInfo>(SUMMONER_URI).await {
        Ok(info) => Some(info),
        Err(error) => {
            eprintln!("klipy [lcu]: no se pudo leer el invocador actual: {error}");
            None
        }
    }
}

/// Platform id de la cuenta conectada, con log de errores (ruta de conexión
/// REST inicial). Ver [`fetch_platform_quiet`].
async fn fetch_platform(client: &LcuClient<RequestClientType>) -> Option<String> {
    fetch_platform_impl(client, true).await
}

/// Platform id sin log: versión para los reintentos periódicos del chequeo
/// de salud, donde un fallo sostenido (cliente sin sesión iniciada) no debe
/// llenar la consola.
async fn fetch_platform_quiet(client: &LcuClient<RequestClientType>) -> Option<String> {
    fetch_platform_impl(client, false).await
}

/// Resuelve el platform id (ej. `"la1"`) del cliente conectado:
///
/// 1. [`REGION_LOCALE_URI`] (`region`, la fuente principal).
/// 2. [`LOGIN_SESSION_URI`] (`region`, respaldo por si la primera no lo
///    trae).
///
/// El valor crudo (ej. `"LA1"`) pasa por [`normalize_platform`], así que la
/// salida ya está en minúsculas y validada contra la tabla de routing; un
/// valor inválido se descarta y se prueba la otra fuente. `None` si ninguna
/// fuente dio un platform utilizable.
async fn fetch_platform_impl(
    client: &LcuClient<RequestClientType>,
    log_errors: bool,
) -> Option<String> {
    if let Ok(dto) = client.get::<RegionLocaleDto>(REGION_LOCALE_URI).await {
        if let Some(platform) = dto.region.as_deref().and_then(normalize_platform) {
            return Some(platform);
        }
    }

    match client.get::<LoginSessionDto>(LOGIN_SESSION_URI).await {
        Ok(dto) => dto.region.as_deref().and_then(normalize_platform),
        Err(error) => {
            if log_errors {
                eprintln!("klipy [lcu]: no se pudo leer el platform id: {error}");
            }
            None
        }
    }
}

/// GET inicial de la sesión de champ select, por si la app abrió con una
/// sesión ya en curso. Un 404 (o `null`) solo confirma que se está esperando.
/// También siembra `match_context` con la sesión encontrada (o lo resetea si
/// no hay sesión).
async fn refresh_initial_state(
    client: &LcuClient<RequestClientType>,
    current: &mut AppState,
    app_state: &mut Signal<AppState>,
    current_context: &mut MatchContext,
    match_context: &mut Signal<MatchContext>,
    catalog: &Signal<Option<ChampionCatalog>>,
    manual_mode: bool,
) {
    match client
        .get::<Option<ChampSelectSession>>(CHAMP_SELECT_SESSION_URI)
        .await
    {
        Ok(Some(session)) => {
            // El guard se crea y se suelta dentro del closure, después del
            // `await` del GET: nunca cruza un punto de suspensión.
            let champion = local_champion_id(&session).map(|id| {
                let catalog_snapshot = catalog.read();
                resolve_champion(id, catalog_snapshot.as_ref())
            });
            transition(current, app_state, manual_mode, AppState::InChampSelect(champion));
            set_match_context(
                current_context,
                match_context,
                MatchContext::from_session(&session),
            );
        }
        Ok(None) => {
            transition(current, app_state, manual_mode, AppState::WaitingForChampSelect);
            set_match_context(current_context, match_context, MatchContext::default());
        }
        Err(irelia::requests::Error::Request(status)) if status.as_u16() == 404 => {
            transition(current, app_state, manual_mode, AppState::WaitingForChampSelect);
            set_match_context(current_context, match_context, MatchContext::default());
        }
        Err(error) => {
            // No es fatal: el websocket mantendrá la UI al día con los cambios.
            eprintln!("klipy [lcu]: el GET inicial de champ select falló: {error}");
        }
    }
}

/// Aplica un payload de champ select al estado y al contexto de partida, si
/// es interpretable. `catalog` es el snapshot vigente (puede ser `None` si
/// aún no cargó).
fn apply_update(
    current: &mut AppState,
    app_state: &mut Signal<AppState>,
    current_context: &mut MatchContext,
    match_context: &mut Signal<MatchContext>,
    payload: Value,
    catalog: Option<&ChampionCatalog>,
    manual_mode: bool,
) {
    let Some(parsed) = parse_session_payload(payload) else {
        return;
    };

    // `AppState`: mismo camino de siempre (Active → InChampSelect, Closed →
    // Waiting si había sesión).
    let update = ChampSelectUpdate::from(&parsed);
    let resolved = resolve_update(&update, catalog);
    transition(current, app_state, manual_mode, next_state(current, &resolved));

    // Contexto de partida para el recomendador: sesión activa → snapshot,
    // cierre → reset (ver `match_context_from_parsed`).
    let next_context = match_context_from_parsed(parsed);
    set_match_context(current_context, match_context, next_context);
}

/// Contexto de partida que produce un payload ya interpretado: la sesión
/// activa se proyecta con [`MatchContext::from_session`]; el cierre resetea a
/// [`MatchContext::default`]. Puro y testeado sin señales.
fn match_context_from_parsed(parsed: SessionPayload) -> MatchContext {
    match parsed {
        SessionPayload::Active(session) => MatchContext::from_session(&session),
        SessionPayload::Closed => MatchContext::default(),
    }
}

/// FASE 12 · Árbitro del espejo de partida en curso. La Live API publica
/// `Option<LiveGame>`; ESTE loop decide cuándo proyectarla al estado:
///
/// - Con partida y **sin** champ select real con pick confirmado → espeja
///   `InChampSelect(mi champ)` + contexto sintético (toda la UI de análisis
///   reusa ese estado). Un champ select REAL con pick tiene prioridad; un
///   `InChampSelect(None)` (casillero vacío que el LCU deja colgado al
///   cerrar el draft) NO bloquea: la partida lo reemplaza.
/// - Sin partida, si había espejo propio → vuelve a `WaitingForChampSelect`
///   y resetea el contexto, pero SOLO si el estado vigente sigue siendo MI
///   espejo (si el LCU ya escribió otra cosa, no lo piso).
///
/// Idempotente: llamadas repetidas con el mismo valor son no-op.
///
/// ¿Procede espejar la partida ahora? (puro, testeado).
fn should_mirror(current: &AppState, live_mirror: Option<i64>) -> bool {
    match current {
        AppState::InChampSelect(Some(_)) => live_mirror.is_some(),
        _ => true,
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_live_update(
    live: Option<crate::live::LiveGame>,
    current: &mut AppState,
    live_mirror: &mut Option<i64>,
    app_state: &mut Signal<AppState>,
    current_context: &mut MatchContext,
    match_context: &mut Signal<MatchContext>,
    in_game: &mut Signal<bool>,
    manual_mode: bool,
) {
    match live {
        Some(game) => {
            // Champ select real con pick confirmado: la Live API no compite.
            if !should_mirror(current, *live_mirror) {
                return;
            }
            // Ya espejando esta misma partida → no-op.
            if *live_mirror == Some(game.my_champion_id)
                && matches!(current, AppState::InChampSelect(_))
            {
                return;
            }

            eprintln!(
                "klipy [live]: espejando partida {} · {} ({} aliados, {} enemigos)",
                game.game_mode,
                game.my_champion_name,
                game.allies.len(),
                game.enemies.len()
            );
            let info = crate::lcu::models::ChampionInfo {
                id: game.my_champion_id,
                name: game.my_champion_name.clone(),
                image_url: String::new(),
            };
            // MODO MANUAL: el champ mostrado lo eligió el usuario; el guard
            // de `transition` evita pisarlo con el espejo de la Live API.
            transition(current, app_state, manual_mode, AppState::InChampSelect(Some(info)));
            set_match_context(current_context, match_context, game.match_context());
            *live_mirror = Some(game.my_champion_id);
            in_game.set(true);
        }
        None => {
            let Some(was_id) = live_mirror.take() else {
                return;
            };
            in_game.set(false);
            // Reset solo si el estado vigente sigue siendo MI espejo (si el
            // LCU ya escribió otra cosa — ej. un champ select nuevo — no
            // piso nada).
            if matches!(&*current, AppState::InChampSelect(Some(info)) if info.id == was_id) {
                transition(current, app_state, manual_mode, AppState::WaitingForChampSelect);
                set_match_context(current_context, match_context, MatchContext::default());
            }
        }
    }
}

/// FASE 13 · ¿Procede el backstop anti-sesión fantasma? (puro, testeado).
///
/// Dos condiciones, ambas necesarias:
///
/// - El estado vigente es `InChampSelect(_)` (con o sin pick): es el único
///   estado que puede quedar colgado en la sesión fantasma cuando el LCU
///   desarma el draft sin mandar el `Delete`. `WaitingForChampSelect` y
///   `Disconnected` ya están sanos y el backstop no los toca.
/// - No hay partida en vivo espejándose (`live_mirror` en `None`): el
///   espejo de la Live API y el champ select son excluyentes. En plena
///   partida el GET de champ select da 404 (la sesión ya no existe) y el
///   backstop "vería" un cierre que en realidad es la partida: mataría el
///   espejo y toda la UI de análisis sin motivo. Ese estado lo administra
///   exclusivamente [`handle_live_update`]: el espejo se suelta recién
///   cuando la propia Live API avisa que la partida terminó.
#[must_use]
fn backstop_procede(current: &AppState, live_mirror: &Option<i64>) -> bool {
    live_mirror.is_none() && matches!(current, AppState::InChampSelect(_))
}

/// FASE 13 · Baja correlativa de [`transition`]: salida del champ select
/// cuando confirmamos que la sesión terminó (fantasma de equipos vacías,
/// `null`/404 del GET). Equivale al cierre limpio: el [`AppState`]
/// regresa a `WaitingForChampSelect` y el contexto de partida se resetea
/// (mismo patrón de espejo + `.set()` condicional, idempotente).
///
/// Le pasa `manual_mode: false` a [`transition`] porque el caller ya
/// verificó el flag (`!manual_mode.peek()` en la condición del backstop);
/// reproducir el guard acá duplicaría la misma condición en el único
/// camino posible y encarecería los tests de baja (que corren con el modo
/// manual ya descartado).
fn bajar_del_champ_select(
    current: &mut AppState,
    app_state: &mut Signal<AppState>,
    current_context: &mut MatchContext,
    match_context: &mut Signal<MatchContext>,
) {
    transition(current, app_state, false, AppState::WaitingForChampSelect);
    set_match_context(current_context, match_context, MatchContext::default());
}

/// Sustituye el placeholder del update por la resolución del catálogo.
///
/// Con catálogo `None` o id ausente el resultado es idéntico al update de
/// entrada: resolver es idempotente.
fn resolve_update(
    update: &ChampSelectUpdate,
    catalog: Option<&ChampionCatalog>,
) -> ChampSelectUpdate {
    match update {
        ChampSelectUpdate::Active(Some(champion)) => {
            ChampSelectUpdate::Active(Some(resolve_champion(champion.id, catalog)))
        }
        other => other.clone(),
    }
}

/// Re-resuelve el campeón del estado actual contra el catálogo (para cuando
/// el catálogo llega con el champ select ya en curso). Devuelve el estado
/// sin cambios si no hay campeón, o si resolver no mejora el placeholder.
fn re_resolve_current(current: &AppState, catalog: Option<&ChampionCatalog>) -> AppState {
    match current {
        AppState::InChampSelect(Some(champion)) => {
            AppState::InChampSelect(Some(resolve_champion(champion.id, catalog)))
        }
        other => other.clone(),
    }
}

/// Transición de estado pura ante un [`ChampSelectUpdate`].
///
/// - Sesión activa → `InChampSelect(champ)`.
/// - Sesión cerrada: solo baja a `WaitingForChampSelect` si había una sesión
///   activa; no toca `Disconnected` ni un `Waiting` previo.
fn next_state(current: &AppState, update: &ChampSelectUpdate) -> AppState {
    match update {
        ChampSelectUpdate::Active(champion) => AppState::InChampSelect(champion.clone()),
        ChampSelectUpdate::Closed => {
            if matches!(current, AppState::InChampSelect(_)) {
                AppState::WaitingForChampSelect
            } else {
                current.clone()
            }
        }
    }
}

/// Sincroniza el espejo local con la señal de Dioxus solo si cambió el valor,
/// evitando re-renders innecesarios.
///
/// **MODO MANUAL**: es el único camino por el que este loop escribe el estado
/// de la app, así que el guard vive acá. Con `manual_mode` en `true` no se
/// toca la señal NI el espejo local (`current` queda congelado): el champ
/// elegido por el usuario desde el menú permanece intacto y, al volver el
/// flag a `false`, la próxima transición retoma el control desde el estado
/// real del loop.
fn transition(
    current: &mut AppState,
    app_state: &mut Signal<AppState>,
    manual_mode: bool,
    next: AppState,
) {
    // MODO MANUAL: con el flag activo el loop no escribe el estado de la app.
    if manual_mode {
        return;
    }
    if *current != next {
        *current = next.clone();
        app_state.set(next);
    }
}

/// Sincroniza el espejo del contexto de partida con su señal solo si cambió
/// el valor (mismo patrón que [`transition`]).
fn set_match_context(
    current: &mut MatchContext,
    signal: &mut Signal<MatchContext>,
    next: MatchContext,
) {
    if *current != next {
        *current = next.clone();
        signal.set(next);
    }
}

/// Sincroniza el espejo de la versión del juego del cliente con su señal solo
/// si cambió el valor (mismo patrón que [`transition`]).
fn set_client_version(
    current: &mut Option<String>,
    signal: &mut Signal<Option<String>>,
    next: Option<String>,
) {
    if *current != next {
        *current = next.clone();
        signal.set(next);
    }
}

/// Sincroniza el espejo del invocador actual con su señal solo si cambió el
/// valor (mismo patrón que [`transition`]). Solo se llama con un fetch
/// exitoso: un fallo puntual nunca borra el último dato válido.
fn set_summoner(
    current: &mut Option<SummonerInfo>,
    signal: &mut Signal<Option<SummonerInfo>>,
    next: SummonerInfo,
) {
    if current.as_ref() != Some(&next) {
        *current = Some(next.clone());
        signal.set(Some(next));
    }
}

/// Sincroniza el espejo del platform id con su señal solo si cambió el valor
/// (mismo patrón que [`transition`]). El valor entra ya normalizado y
/// validado ([`fetch_platform_impl`]); la señal nunca contiene un platform
/// que la tabla de routing desconozca.
fn set_platform(current: &mut Option<String>, signal: &mut Signal<Option<String>>, next: String) {
    if current.as_ref() != Some(&next) {
        *current = Some(next.clone());
        signal.set(Some(next));
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use serde_json::json;

    use super::*;
    use crate::data_dragon::models::ChampionEntry;
    use crate::lcu::events::{champion_placeholder, ChampSelectMember};

    /// MODO MANUAL: el guard de `transition` es el único camino por el que
    /// el loop escribe el estado de la app. Con `manual_mode` en `true` no
    /// toca NI la señal NI el espejo local (`current` queda congelado); al
    /// volver el flag a `false`, la próxima transición retoma el control
    /// desde el estado real del espejo.
    ///
    /// Los `Signal` requieren un runtime de Dioxus (un `Owner` creado a mano
    /// aborta al dropear el storage fuera del TLS del runtime), así que la
    /// lógica corre dentro de un componente montado en un `VirtualDom` de
    /// prueba (mismo patrón que los tests de dioxus-signals).
    #[test]
    fn transition_con_manual_mode_no_escribe_la_senal() {
        use dioxus::prelude::*;

        fn transicion_harness() -> Element {
            let elegido_a_mano = AppState::InChampSelect(Some(champion_placeholder(103)));
            let mut current = AppState::WaitingForChampSelect;
            let mut app_state = Signal::new(elegido_a_mano.clone());

            // Con el flag activo: ni la señal ni el espejo local se tocan,
            // aunque el loop quiera pisar el champ elegido a mano.
            transition(
                &mut current,
                &mut app_state,
                true,
                AppState::InChampSelect(Some(champion_placeholder(412))),
            );
            assert_eq!(
                *app_state.read(),
                elegido_a_mano,
                "el guard no debe dejar que el loop pise el champ del usuario"
            );
            assert_eq!(current, AppState::WaitingForChampSelect);

            // Con el flag en false: la transición retoma el control y escribe
            // señal + espejo desde el estado real (el Waiting congelado).
            transition(&mut current, &mut app_state, false, AppState::Disconnected);
            assert_eq!(*app_state.read(), AppState::Disconnected);
            assert_eq!(current, AppState::Disconnected);

            // Transición no-op (mismo valor): no hay escritura de señal y el
            // espejo queda igual (idempotencia del camino sin guard).
            transition(
                &mut current,
                &mut app_state,
                false,
                AppState::Disconnected,
            );
            assert_eq!(*app_state.read(), AppState::Disconnected);
            assert_eq!(current, AppState::Disconnected);

            rsx! { div {} }
        }

        let mut dom = VirtualDom::new(transicion_harness);
        let _muts = dom.rebuild_to_vec();
    }

    /// El guard del espejo: un champ select REAL con pick bloquea a la Live
    /// API; un casillero vacío (`InChampSelect(None)`) que el LCU dejó
    /// colgado al cerrar el draft NO bloquea (era el bug de "se queda en
    /// esperando selección" al entrar a partida).
    #[test]
    fn should_mirror_bloquea_solo_picks_reales() {
        let real = AppState::InChampSelect(Some(crate::lcu::models::ChampionInfo {
            id: 64,
            name: String::from("Kayn"),
            image_url: String::new(),
        }));
        let vacio = AppState::InChampSelect(None);

        assert!(!should_mirror(&real, None), "pick real sin espejo propio → espera");
        assert!(should_mirror(&real, Some(64)), "re-espejo de la misma partida → ok");
        assert!(should_mirror(&vacio, None), "casillero vacío no bloquea");
        assert!(should_mirror(&AppState::WaitingForChampSelect, None));
    }
    use crate::lcu::models::Position;

    /// Catálogo mínimo (solo Ahri) para las pruebas de resolución.
    fn catalogo_de_prueba() -> ChampionCatalog {
        let mut by_id = HashMap::new();
        by_id.insert(
            103,
            ChampionEntry {
                name: String::from("Ahri"),
                image_url: String::from(
                    "https://ddragon.leagueoflegends.com/cdn/15.21.1/img/champion/Ahri.png",
                ),
                slug: String::from("ahri"),
                ddragon_id: String::from("Ahri"),
            },
        );
        ChampionCatalog {
            version: String::from("15.21.1"),
            by_id,
        }
    }

    fn ahri_real() -> crate::lcu::models::ChampionInfo {
        crate::lcu::models::ChampionInfo {
            id: 103,
            name: String::from("Ahri"),
            image_url: String::from(
                "https://ddragon.leagueoflegends.com/cdn/15.21.1/img/champion/Ahri.png",
            ),
        }
    }

    #[test]
    fn sesion_cerrada_baja_a_waiting_desde_champ_select() {
        let current = AppState::InChampSelect(Some(champion_placeholder(103)));

        assert_eq!(
            next_state(&current, &ChampSelectUpdate::Closed),
            AppState::WaitingForChampSelect
        );
    }

    #[test]
    fn sesion_cerrada_no_toca_waiting_ni_disconnected() {
        assert_eq!(
            next_state(&AppState::WaitingForChampSelect, &ChampSelectUpdate::Closed),
            AppState::WaitingForChampSelect
        );
        assert_eq!(
            next_state(&AppState::Disconnected, &ChampSelectUpdate::Closed),
            AppState::Disconnected
        );
    }

    #[test]
    fn sesion_activa_actualiza_el_campeon() {
        assert_eq!(
            next_state(
                &AppState::InChampSelect(None),
                &ChampSelectUpdate::Active(Some(champion_placeholder(412)))
            ),
            AppState::InChampSelect(Some(champion_placeholder(412)))
        );
        assert_eq!(
            next_state(
                &AppState::WaitingForChampSelect,
                &ChampSelectUpdate::Active(None)
            ),
            AppState::InChampSelect(None)
        );
    }

    /// FASE 3: con catálogo cargado, el update deja de traer placeholder y
    /// pasa a traer nombre/imagen reales.
    #[test]
    fn update_con_catalogo_resuelve_nombre_real() {
        let catalog = catalogo_de_prueba();
        let update = ChampSelectUpdate::Active(Some(champion_placeholder(103)));

        assert_eq!(
            resolve_update(&update, Some(&catalog)),
            ChampSelectUpdate::Active(Some(ahri_real()))
        );
    }

    /// Sin catálogo (o con un id que no está en él) el update queda igual:
    /// resolver es idempotente y el placeholder se mantiene.
    #[test]
    fn update_sin_catalogo_o_id_desconocido_mantiene_placeholder() {
        let catalog = catalogo_de_prueba();
        let update = ChampSelectUpdate::Active(Some(champion_placeholder(412)));

        assert_eq!(resolve_update(&update, None), update);
        assert_eq!(resolve_update(&update, Some(&catalog)), update);
    }

    /// FASE 3: si el catálogo llega con el champ select ya en curso, el
    /// placeholder del estado actual se re-resuelve al nombre real.
    #[test]
    fn catalogo_tardio_re_resuelve_el_campeon_en_curso() {
        let current = AppState::InChampSelect(Some(champion_placeholder(103)));
        let catalog = catalogo_de_prueba();

        assert_eq!(
            re_resolve_current(&current, Some(&catalog)),
            AppState::InChampSelect(Some(ahri_real()))
        );
    }

    /// Estados sin campeón no se tocan al re-resolver.
    #[test]
    fn re_resolucion_no_toca_estados_sin_campeon() {
        let catalog = catalogo_de_prueba();

        assert_eq!(
            re_resolve_current(&AppState::Disconnected, Some(&catalog)),
            AppState::Disconnected
        );
        assert_eq!(
            re_resolve_current(&AppState::WaitingForChampSelect, Some(&catalog)),
            AppState::WaitingForChampSelect
        );
        assert_eq!(
            re_resolve_current(&AppState::InChampSelect(None), Some(&catalog)),
            AppState::InChampSelect(None)
        );
    }

    /// Re-resolver con catálogo `None` (o id ausente) devuelve el mismo
    /// estado: `transition` lo trata como no-op y no re-renderiza.
    #[test]
    fn re_resolucion_sin_catalogo_es_no_op() {
        let current = AppState::InChampSelect(Some(champion_placeholder(412)));

        assert_eq!(re_resolve_current(&current, None), current);

        let catalog = catalogo_de_prueba();
        assert_eq!(re_resolve_current(&current, Some(&catalog)), current);
    }

    /// FASE 6: una sesión activa se proyecta al contexto y el cierre
    /// (`data: null`) lo resetea al default — helpers puros, sin señales.
    #[test]
    fn contexto_de_sesion_activa_y_reseteo_al_cerrar() {
        let activa = SessionPayload::Active(ChampSelectSession {
            local_member_cell_id: Some(1),
            my_team: vec![
                ChampSelectMember {
                    cell_id: 0,
                    champion_id: 22,
                    champion_pick_intent: 0,
                    assigned_position: Some(Position::Bottom),
                },
                ChampSelectMember {
                    cell_id: 1,
                    champion_id: 0,
                    champion_pick_intent: 103,
                    assigned_position: None,
                },
            ],
            enemy_team: vec![ChampSelectMember {
                cell_id: 5,
                champion_id: 86,
                champion_pick_intent: 0,
                assigned_position: Some(Position::Top),
            }],
            timer: None,
        });

        let context = match_context_from_parsed(activa);
        assert_eq!(context.my_cell_id, Some(1));
        assert_eq!(context.my_champion_id, Some(103));
        assert_eq!(context.allies.len(), 1);
        assert_eq!(context.enemies.len(), 1);

        assert_eq!(
            match_context_from_parsed(SessionPayload::Closed),
            MatchContext::default()
        );
    }

    /// FASE 6: el payload de cierre real (`null`, evento `Delete`) pasa por
    /// `parse_session_payload` → `Closed` → contexto default.
    #[test]
    fn payload_null_resetea_el_contexto() {
        let parsed = parse_session_payload(Value::Null).expect("null es interpretable");

        assert_eq!(match_context_from_parsed(parsed), MatchContext::default());
    }

    /// FASE 8b: el payload real de `/riotclient/region-locale` trae muchos
    /// campos más (`locale`, `webLanguage`, `webRegion`, ...): se parsea el
    /// subconjunto y el resto se ignora.
    #[test]
    fn region_locale_parsea_el_subconjunto_e_ignora_el_resto() {
        let dto: RegionLocaleDto = serde_json::from_value(json!({
            "locale": "es_ES",
            "region": "LA1",
            "webLanguage": "es_ES",
            "webRegion": "PBE1",
            "type": "leagues"
        }))
        .expect("payload real parsea");

        assert_eq!(dto.region.as_deref(), Some("LA1"));
    }

    /// Tolerancia de ambos DTOs: objeto vacío, `region` `null` o ausente →
    /// `None`, nunca error de parsing (el fallback decide).
    #[test]
    fn dtos_de_platform_tolerantes_a_payloads_parciales() {
        let vacio: RegionLocaleDto = serde_json::from_value(json!({})).expect("vacio parsea");
        assert_eq!(vacio.region, None);

        let nulo: RegionLocaleDto =
            serde_json::from_value(json!({"region": null})).expect("null parsea");
        assert_eq!(nulo.region, None);

        let login: LoginSessionDto = serde_json::from_value(json!({
            "state": "SUCCEEDED",
            "connected": true,
            "region": "KR"
        }))
        .expect("login parsea");
        assert_eq!(login.region.as_deref(), Some("KR"));

        let login_vacio: LoginSessionDto =
            serde_json::from_value(json!({"state": "/logging-in"})).expect("sin region parsea");
        assert_eq!(login_vacio.region, None);
    }

    /// El platform crudo del LCU (mayúsculas, espacios) ya sale normalizado
    /// vía `normalize_platform`; lo desconocido se descarta.
    #[test]
    fn normalizacion_del_platform_reportado_por_el_lcu() {
        assert_eq!(normalize_platform("LA1"), Some(String::from("la1")));
        assert_eq!(normalize_platform("  kr "), Some(String::from("kr")));
        assert_eq!(normalize_platform("lan"), None);
    }

    /// La versión cruda de `/lol-patch/v1/game-version` (con metadata de
    /// build posterior al `+`) se publica limpia: solo la parte del parche.
    #[test]
    fn clean_game_version_recorta_la_metadata_de_build() {
        // Valor real del cliente 16.20 (verificado 2026-10 con LCU de LA2).
        assert_eq!(
            clean_game_version(
                "16.20.8248524+branch.releases-16-20.code.public.content.release.anticheat.vanguard"
            ),
            "16.20.8248524"
        );

        // Formato corto estilo endpoint viejo: no se toca.
        assert_eq!(clean_game_version("15.21.1"), "15.21.1");

        // Sin `+`, con espacios de más.
        assert_eq!(clean_game_version("  16.7.623.1234  "), "16.7.623.1234");

        // Vacío sigue vacío (y `fetch_client_version` lo convierte en None).
        assert_eq!(clean_game_version(""), "");
    }

    /// FASE 13: el backstop solo actúa si el estado está colgado en champ
    /// select (con o sin pick) y NO hay partida en vivo espejándose — en
    /// partida el GET de champ select da 404 y el "cierre" que él vería
    /// sería la propia partida: matar el espejo sería el bug inverso. Los
    /// estados sanos (`Waiting`, `Disconnected`) no necesitan rescate.
    #[test]
    fn backstop_procede_solo_en_champ_select_sin_espejo() {
        let real = AppState::InChampSelect(Some(champion_placeholder(103)));
        let fantasma = AppState::InChampSelect(None);

        // Colgado en champ select y sin espejo: procede (fantasma o cierre
        // sin evento `Delete`, sea cual sea el pick del estado).
        assert!(backstop_procede(&real, &None));
        assert!(backstop_procede(&fantasma, &None));

        // Espejando una partida: el estado lo maneja la Live API, no se toca.
        assert!(!backstop_procede(&real, &Some(103)));
        assert!(!backstop_procede(&fantasma, &Some(103)));

        // Estados sanos: no hay nada que rescatar.
        assert!(!backstop_procede(&AppState::WaitingForChampSelect, &None));
        assert!(!backstop_procede(&AppState::Disconnected, &None));
    }

    /// FASE 13: la baja del backstop equivale a un cierre limpio: `AppState`
    /// vuelve a `WaitingForChampSelect` y el contexto de partida (que venía
    /// con la sesión real colgada) se resetea al default. Los `Signal`
    /// requieren el `VirtualDom` de prueba, mismo patrón que el test de
    /// `transition` con modo manual.
    #[test]
    fn bajar_del_champ_select_resetea_estado_y_contexto() {
        use dioxus::prelude::*;

        fn baja_harness() -> Element {
            // Sesión real colgada (con pick del local y miembros en ambos
            // equipos): su contexto es NO default — si fuera default, el
            // reset del contexto sería un no-op sin sobre el que verificar.
            let sesion = ChampSelectSession {
                local_member_cell_id: Some(1),
                my_team: vec![
                    ChampSelectMember {
                        cell_id: 0,
                        champion_id: 22,
                        champion_pick_intent: 0,
                        assigned_position: Some(Position::Bottom),
                    },
                    ChampSelectMember {
                        cell_id: 1,
                        champion_id: 103,
                        champion_pick_intent: 0,
                        assigned_position: None,
                    },
                ],
                enemy_team: vec![ChampSelectMember {
                    cell_id: 5,
                    champion_id: 86,
                    champion_pick_intent: 0,
                    assigned_position: Some(Position::Top),
                }],
                timer: None,
            };
            let contexto_previo = MatchContext::from_session(&sesion);
            assert_ne!(contexto_previo, MatchContext::default());

            let mut current = AppState::InChampSelect(Some(champion_placeholder(103)));
            let mut app_state = Signal::new(current.clone());
            let mut current_context = contexto_previo.clone();
            let mut match_context = Signal::new(contexto_previo.clone());

            // El caller YA descartó el modo manual (condición del backstop)
            // y confirmó el cierre: la baja es la correlativa de un cierre
            // limpio.
            bajar_del_champ_select(
                &mut current,
                &mut app_state,
                &mut current_context,
                &mut match_context,
            );

            assert_eq!(current, AppState::WaitingForChampSelect);
            assert_eq!(*app_state.read(), AppState::WaitingForChampSelect);
            assert_eq!(current_context, MatchContext::default());
            assert_eq!(*match_context.read(), MatchContext::default());

            rsx! { div {} }
        }

        let mut dom = VirtualDom::new(baja_harness);
        let _muts = dom.rebuild_to_vec();
    }
}
