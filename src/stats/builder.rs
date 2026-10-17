//! Tarea de sincronización del dataset de runas contra la Riot API
//! oficial (FASE 8b).
//!
//! Flujo de [`run_dataset_sync`] (una única instancia por app, spawneada
//! desde `app.rs` junto a las demás tareas de por vida):
//!
//! 1. **Key**: sin `RIOT_API_KEY` en el entorno, un único log
//!    (`dataset sync disabled: set RIOT_API_KEY`) y la tarea termina, sin
//!    crash ni spam. El resto de la app sigue normal.
//! 2. **Platform id**: precedencia `RIOT_PLATFORM` (normalizada y validada
//!    con [`normalize_platform`]) → la señal que publica el LCU al conectar
//!    (ver [`crate::lcu::client`]). Sin ninguna fuente tras [`SOURCE_WAIT`]
//!    → log y sync deshabilitado por esta sesión.
//! 3. **Puuid de la API (namespace Account-V1)**: el puuid que el LCU trae
//!    en `/lol-summoner/v1/current-summoner` es el puuid **de juego** y
//!    Match-V5 lo rechaza con `HTTP 400 "Exception decrypting"` (bug de
//!    producción verificado 2026-10). El puuid que Match-V5 espera es el
//!    **puuid unificado de la cuenta Riot**, que se resuelve con
//!    [`RiotApi::account_puuid_by_riot_id`] a partir del Riot ID
//!    (`gameName`/`tagLine`, [`riot_id_of`]) que el mismo payload del LCU
//!    sí trae. Sin Riot ID (cuenta legacy o payload parcial) no hay forma
//!    de resolverlo: log claro y sync deshabilitado limpio.
//! 4. **Historial**: con el puuid de la API resuelto, se pagina `match_ids`
//!    (cola 420, `type=ranked`, páginas de [`PAGE_SIZE`]) hasta página
//!    vacía, página cuyos ids ya están todos en el store (historia
//!    conocida: no re-descargar detalles) o el tope [`MAX_SYNC_MATCHES`].
//! 5. **Detalles**: solo de ids nuevos; cada uno se appendea al store apenas
//!    baja (si la app muere a mitad, lo bajado queda) y los errores
//!    consecutivos cortan el sync sin perder lo acumulado.
//! 6. **Una vez por Riot ID**: terminado el sync, la tarea se queda mirando
//!    la señal del invocador con un poll liviano y solo re-sincroniza si el
//!    Riot ID cambia (cambio de cuenta). Los reconectos del LCU re-publican
//!    el mismo valor (patrón espejo), así que no disparan re-syncs. No hay
//!    loop de re-sincronización periódica: el dataset crece entre sesiones.
//!
//! Los puuids que aparecen en los logs se reportan truncados
//! ([`puuid_resumido`], últimos 4 caracteres) con su namespace explícito.
//!
//! Los extremos por los que pasa la lógica de paginación
//! ([`next_start`], [`should_continue_paging`]) y el estado persistido
//! ([`SyncState`]) son puros/inyectables y están testeados sin red.

use std::fmt;
use std::fs;
use std::path::Path;
use std::time::Duration;

use dioxus::prelude::{ReadableExt, Signal};
use serde::{Deserialize, Serialize};
use tokio::time::{sleep, Instant};

use crate::lcu::models::SummonerInfo;
use crate::riot_api::client::{RANKED_MATCH_TYPE, RANKED_SOLO_QUEUE};
use crate::riot_api::key::api_key_from_env;
use crate::riot_api::{normalize_platform, RiotApi, RiotApiError};
use crate::stats::storage::{data_dir, JsonlStatsStore, StatsStore};

/// Tope de partidas nuevas descargadas por corrida de sync.
///
/// 300 ≈ 3 páginas de 100. Con el paceo defensivo del cliente (150 ms por
/// llamada) son ~3 requests de historial + 300 de detalle ≈ 45 s de
/// descarga y unas 303 llamadas en ~1 minuto: roza el techo de 100
/// req/2 min de una dev key personal, pero el backoff ante `429` del
/// cliente lo absorbe; ir más alto solo acumularía rate limit para el
/// próximo sync. El dataset crece entre sesiones, así que el tope es por
/// corrida, no global.
pub const MAX_SYNC_MATCHES: usize = 300;

/// Tamaño de página del endpoint de historial (máximo que acepta Match-V5).
pub const PAGE_SIZE: i64 = 100;

/// Variable de entorno con el platform id explícito (precedencia sobre el
/// LCU). Útil para testear/sincronizar sin cliente de LoL abierto.
pub const PLATFORM_ENV: &str = "RIOT_PLATFORM";

/// Ventana de espera de las fuentes al arrancar la tarea (platform vía LCU,
/// puuid): cubre el arranque del cliente de League (parche + login) sin
/// colgar la tarea indefinidamente cuando no hay cliente.
const SOURCE_WAIT: Duration = Duration::from_secs(90);

/// Poll de la señal de platform durante la ventana de espera.
const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// Poll del invocador en el modo "una vez por puuid" (barato: solo compara
/// el valor de la señal).
const PUUID_WATCH_INTERVAL: Duration = Duration::from_secs(5);

/// Errores de detalle consecutivos antes de cortar el sync (una partida
/// puntual con 404/500 no aborta nada; un patrón de errores sí).
const MAX_CONSECUTIVE_DETAIL_ERRORS: u32 = 5;

/// Intentos de resolución del puuid de la API (Account-V1) antes de
/// deshabilitar el sync por esta sesión: un fallo de red transitorio se
/// reintenta en el próximo poll, un fallo persistente no debe martillar la
/// API cada [`PUUID_WATCH_INTERVAL`] para siempre.
const MAX_PUUID_RESOLUTION_ATTEMPTS: u32 = 3;

/// Nombre del archivo de estado del sync dentro del directorio de datos.
const SYNC_STATE_FILE_NAME: &str = "sync_state.json";

/// Estado persistido del sync (trazabilidad entre sesiones: el log de
/// arranque puede reportar contra qué cuenta se sincronizó la última vez).
///
/// NOTA de namespace: `last_synced_puuid` guarda el **puuid unificado de la
/// cuenta Riot** (namespace Account-V1, el que Match-V5 acepta), NO el puuid
/// de juego que reporta el LCU. Archivos escritos por versiones viejas de la
/// app pueden contener un puuid de juego: no pasa nada — el guard del sync
/// vive en memoria y arranca en `None` en cada corrida, así que el primer
/// sync de la sesión re-resuelve el puuid de la API y re-escribe el archivo
/// con el namespace correcto. El cambio de valor nunca dispara lógica: el
/// archivo es solo trazabilidad.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SyncState {
    /// PUUID de la última cuenta sincronizada (namespace Account-V1, ver la
    /// nota de la estructura).
    pub last_synced_puuid: Option<String>,
}

/// Riot ID (`gameName#tagLine`) del invocador actual, tal como lo reporta el
/// LCU en `/lol-summoner/v1/current-summoner`. Es la entrada para resolver
/// el puuid de la API con [`RiotApi::account_puuid_by_riot_id`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RiotId {
    /// Parte del Riot ID antes del `#` (puede contener espacios/acentos).
    pub game_name: String,
    /// Parte después del `#` (ej. `"LAS"`); nunca incluye el `#`.
    pub tag_line: String,
}

impl fmt::Display for RiotId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}#{}", self.game_name, self.tag_line)
    }
}

/// Lógica pura: ¿qué Riot ID usar para resolver el puuid de la API?
///
/// `Some` solo si `gameName` y `tagLine` están presentes y no vacíos (con
/// trim: un payload con espacios de más no rompe la resolución). Cuentas
/// legacy o payloads parciales (uno de los dos `None`, vacío o solo
/// espacios) → `None`: sin Riot ID no se puede resolver el puuid de la API.
#[must_use]
pub fn riot_id_of(info: &SummonerInfo) -> Option<RiotId> {
    let game_name = info.game_name.as_deref().map_or("", str::trim);
    let tag_line = info.tag_line.as_deref().map_or("", str::trim);
    if game_name.is_empty() || tag_line.is_empty() {
        return None;
    }
    Some(RiotId {
        game_name: game_name.to_string(),
        tag_line: tag_line.to_string(),
    })
}

/// Puuid acortado para logs: `…` + últimos 4 caracteres (ej. `…zcpA`).
/// Los puuids son identificadores públicos pero no aportan nada completo en
/// consola; si el valor es más corto que el umbral se muestra entero (nunca
/// se paniquea por slicing con UTF-8: se corta por `char`s).
#[must_use]
pub fn puuid_resumido(puuid: &str) -> String {
    let chars: Vec<char> = puuid.chars().collect();
    if chars.len() <= 8 {
        return puuid.to_string();
    }
    let tail: String = chars[chars.len() - 4..].iter().collect();
    format!("…{tail}")
}

/// Siguiente offset de paginación: avanza por lo realmente devuelto por la
/// API. En la última página corta avanzaría "de menos", pero el corte por
/// página sin ids nuevos (o vacía) ocurre antes de re-pedir ese tramo.
#[must_use]
pub fn next_start(start: i64, page_len: usize) -> i64 {
    start.saturating_add(i64::try_from(page_len).unwrap_or(i64::MAX / 2))
}

/// ¿Vale la pena seguir paginando después de esta página? Corta por:
///
/// - página **vacía**: se recorrió todo el historial;
/// - página **sin ids nuevos**: todo ese tramo ya está en el dataset (no
///   re-descargar detalles conocidos);
/// - **tope** [`MAX_SYNC_MATCHES`] alcanzado en esta corrida.
#[must_use]
pub fn should_continue_paging(
    page_len: usize,
    fresh_in_page: usize,
    downloaded_this_run: usize,
) -> bool {
    page_len > 0 && fresh_in_page > 0 && downloaded_this_run < MAX_SYNC_MATCHES
}

/// Lee el estado del sync. Archivo ausente o corrupto → default (con log):
/// el estado es trazabilidad, nunca algo que deba tumbar el sync.
#[must_use]
pub fn load_sync_state(path: &Path) -> SyncState {
    let Ok(contents) = fs::read_to_string(path) else {
        return SyncState::default();
    };
    match serde_json::from_str(&contents) {
        Ok(state) => state,
        Err(error) => {
            eprintln!(
                "klipy [stats]: {} ilegible, se ignora ({error})",
                path.display()
            );
            SyncState::default()
        }
    }
}

/// Persiste el estado del sync (best-effort: un fallo de IO se loguea y el
/// sync ya terminó igual).
fn save_sync_state(path: &Path, state: &SyncState) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let json = match serde_json::to_string_pretty(state) {
        Ok(json) => json,
        Err(error) => {
            eprintln!("klipy [stats]: no se serializó el estado del sync: {error}");
            return;
        }
    };
    if let Err(error) = fs::write(path, json) {
        eprintln!(
            "klipy [stats]: no se pudo escribir {}: {error}",
            path.display()
        );
    }
}

/// Tarea de vida de la sincronización del dataset. Ver la doc del módulo
/// para el flujo completo.
pub async fn run_dataset_sync(
    summoner: Signal<Option<SummonerInfo>>,
    platform: Signal<Option<String>>,
    store: JsonlStatsStore,
) {
    // Sin key: un solo log y la tarea muere limpia, sin esperas ni spam.
    // (Chequeo temprano para no esperar el platform en vano; `from_env`
    // vuelve a validar más abajo, que es el camino canónico del spec.)
    if api_key_from_env().is_none() {
        eprintln!("klipy [stats]: dataset sync disabled: set RIOT_API_KEY");
        return;
    }

    // Platform: env primero, después la señal del LCU.
    let Some(platform_id) = wait_for_platform(&platform).await else {
        eprintln!(
            "klipy [stats]: dataset sync disabled: sin platform id (set {PLATFORM_ENV} o abrí el cliente de LoL)"
        );
        return;
    };

    let mut api = match RiotApi::from_env(&platform_id) {
        Ok(api) => api,
        Err(RiotApiError::ApiKeyMissing) => {
            eprintln!("klipy [stats]: dataset sync disabled: set RIOT_API_KEY");
            return;
        }
        Err(error) => {
            eprintln!("klipy [stats]: dataset sync disabled: {error}");
            return;
        }
    };

    // Trazabilidad: contra qué cuenta se sincronizó la última vez. El
    // namespace del valor guardado (Account-V1 desde esta versión; puuid de
    // juego en archivos viejos) está documentado en [`SyncState`]: el guard
    // real vive en memoria (`synced`), así que el primer sync de la sesión
    // siempre corre y re-escribe el archivo con el namespace correcto.
    let state_path = data_dir().join(SYNC_STATE_FILE_NAME);
    if let Some(prev) = load_sync_state(&state_path).last_synced_puuid {
        eprintln!(
            "klipy [stats]: último sync registrado para el puuid {}",
            puuid_resumido(&prev)
        );
    }

    // Modo "una vez por Riot ID": el guard en memoria es el Riot ID (qué
    // cuenta es), pero lo que se usa para paginar y se persiste en
    // [`SyncState`] es el **puuid de la API** (namespace Account-V1), que es
    // distinto del puuid de juego del LCU — usar el viejo da HTTP 400 en
    // Match-V5. Los reconectos del LCU re-publican el mismo invocador
    // (patrón espejo) y no disparan nada.
    let mut store = store;
    let mut synced: Option<RiotId> = None;
    let mut resolution_failures: u32 = 0;
    loop {
        // Snapshot corto: el guard de la señal no cruza ningún await.
        let snapshot: Option<SummonerInfo> = (*summoner.read()).clone();

        if let Some(riot_id) = snapshot.as_ref().and_then(riot_id_of) {
            if synced.as_ref() != Some(&riot_id) {
                match api
                    .account_puuid_by_riot_id(&riot_id.game_name, &riot_id.tag_line)
                    .await
                {
                    Ok(api_puuid) => {
                        resolution_failures = 0;
                        eprintln!(
                            "klipy [stats]: puuid de la API resuelto para {riot_id}: api (account-v1) {}",
                            puuid_resumido(&api_puuid)
                        );
                        sync_puuid(&mut api, &mut store, &api_puuid).await;
                        save_sync_state(
                            &state_path,
                            &SyncState {
                                // Namespace Account-V1 (ver doc de SyncState).
                                last_synced_puuid: Some(api_puuid),
                            },
                        );
                        synced = Some(riot_id);
                    }
                    // Riot ID desconocido para la API: no hay reintento que
                    // arregle un 404; disable limpio con log explícito.
                    Err(RiotApiError::Http(404)) => {
                        eprintln!(
                            "klipy [stats]: dataset sync disabled: la Riot API no conoce el Riot ID {riot_id} (404); verificá gameName#tagLine en el cliente"
                        );
                        return;
                    }
                    Err(error) => {
                        resolution_failures += 1;
                        eprintln!(
                            "klipy [stats]: no se pudo resolver el puuid de la API para {riot_id} (intento {resolution_failures}/{MAX_PUUID_RESOLUTION_ATTEMPTS}): {error}"
                        );
                        if resolution_failures >= MAX_PUUID_RESOLUTION_ATTEMPTS {
                            eprintln!(
                                "klipy [stats]: dataset sync disabled: {MAX_PUUID_RESOLUTION_ATTEMPTS} fallos resolviendo el puuid de la API"
                            );
                            return;
                        }
                    }
                }
            }
        } else if snapshot.is_some() {
            // Hay invocador pero no hay Riot ID utilizable (cuenta legacy o
            // payload parcial): sin Riot ID no se puede resolver el puuid de
            // la API, así que no hay nada que sincronizar.
            eprintln!(
                "klipy [stats]: dataset sync disabled: sin Riot ID (gameName/tagLine) no se puede resolver el puuid de la API"
            );
            return;
        }
        // snapshot None = LCU aún sin conectar: se espera en el poll de
        // abajo, como siempre.

        sleep(PUUID_WATCH_INTERVAL).await;
    }
}

/// Espera (acotada a [`SOURCE_WAIT`]) el platform id: [`PLATFORM_ENV`]
/// primero —normalizado y validado—, después la señal del LCU. `None` si
/// ninguna fuente dio un platform válido dentro de la ventana.
async fn wait_for_platform(platform: &Signal<Option<String>>) -> Option<String> {
    if let Some(from_env) = platform_from_env() {
        return Some(from_env);
    }

    let deadline = Instant::now() + SOURCE_WAIT;
    loop {
        // Snapshot corto: el guard no cruza el await del sleep.
        let snapshot: Option<String> = (*platform.read()).clone();
        if let Some(platform_id) = snapshot {
            return Some(platform_id);
        }
        if Instant::now() >= deadline {
            return None;
        }
        sleep(POLL_INTERVAL).await;
    }
}

/// Lee [`PLATFORM_ENV`] del entorno y lo valida. Un valor inválido se
/// loguea y se descarta (cae al LCU).
fn platform_from_env() -> Option<String> {
    let raw = std::env::var(PLATFORM_ENV).ok()?;
    match normalize_platform(&raw) {
        Some(platform) => Some(platform),
        None => {
            eprintln!(
                "klipy [stats]: {PLATFORM_ENV}={raw:?} no es un platform id conocido; se ignora"
            );
            None
        }
    }
}

/// Recorre el historial ranked de `puuid` (namespace **Account-V1**: el
/// puuid unificado de la cuenta Riot que devuelve
/// [`RiotApi::account_puuid_by_riot_id`]) y baja los detalles nuevos al
/// store, con append incremental (ver doc del módulo). Al terminar —por
/// historia completa, tope o errores— loguea el resumen de la corrida. El
/// puuid se loguea truncado ([`puuid_resumido`]).
async fn sync_puuid(api: &mut RiotApi, store: &mut JsonlStatsStore, puuid: &str) {
    eprintln!(
        "klipy [stats]: sync del historial ranked para el puuid api (account-v1) {} ({} partidas ya en el dataset, routing {})",
        puuid_resumido(puuid),
        store.len(),
        api.routing()
    );

    let mut start: i64 = 0;
    let mut downloaded: usize = 0;
    let mut consecutive_errors: u32 = 0;

    loop {
        let page = match api
            .match_ids(
                puuid,
                start,
                PAGE_SIZE,
                RANKED_SOLO_QUEUE,
                Some(RANKED_MATCH_TYPE),
            )
            .await
        {
            Ok(page) => page,
            Err(error) => {
                eprintln!(
                    "klipy [stats]: el historial falló en start={start}: {error}; se conservan las {downloaded} partidas nuevas de esta corrida"
                );
                break;
            }
        };

        let fresh: Vec<String> = page
            .iter()
            .filter(|match_id| !store.contains(match_id.as_str()))
            .cloned()
            .collect();
        if !should_continue_paging(page.len(), fresh.len(), downloaded) {
            break;
        }

        for match_id in &fresh {
            if downloaded >= MAX_SYNC_MATCHES {
                break;
            }
            match api.match_detail(match_id).await {
                Ok(detail) => {
                    consecutive_errors = 0;
                    // Append inmediato: si la app muere acá, lo bajado
                    // queda. `added == 0` = fallo de IO (ya logueado por el
                    // store): no cuenta como descargado y el próximo sync
                    // lo reintenta.
                    downloaded += store.append_matches(std::slice::from_ref(&detail));
                }
                Err(error) => {
                    consecutive_errors += 1;
                    eprintln!("klipy [stats]: el detalle de {match_id} falló: {error}");
                    if consecutive_errors >= MAX_CONSECUTIVE_DETAIL_ERRORS {
                        eprintln!(
                            "klipy [stats]: {MAX_CONSECUTIVE_DETAIL_ERRORS} errores consecutivos; corto el sync con {downloaded} partidas nuevas"
                        );
                        break;
                    }
                }
            }
        }

        if downloaded >= MAX_SYNC_MATCHES || consecutive_errors >= MAX_CONSECUTIVE_DETAIL_ERRORS {
            break;
        }
        start = next_start(start, page.len());
    }

    eprintln!(
        "klipy [stats]: sync terminado: {downloaded} partidas nuevas, {} en el dataset",
        store.len()
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::storage::temp_data_dir;

    /// Constantes del spec: tope 300 por corrida y páginas de 100.
    #[test]
    fn constantes_del_spec() {
        assert_eq!(MAX_SYNC_MATCHES, 300);
        assert_eq!(PAGE_SIZE, 100);
        assert_eq!(PLATFORM_ENV, "RIOT_PLATFORM");
        // Reintentos de resolución del puuid de la API antes de rendirse.
        assert_eq!(MAX_PUUID_RESOLUTION_ATTEMPTS, 3);
    }

    /// Lógica pura de "qué Riot ID usar": con ambos componentes → `Some`;
    /// falta uno, está vacío o solo espacios → `None` (cuenta legacy /
    /// payload parcial: sin Riot ID no se puede resolver el puuid de la API).
    #[test]
    fn riot_id_de_summoner_con_ambos_componentes_o_nada() {
        let base = SummonerInfo {
            puuid: Some(String::from("puuid-de-juego")),
            game_name: Some(String::from("PKT MidKing")),
            tag_line: Some(String::from("Akali")),
            ..SummonerInfo::default()
        };
        assert_eq!(
            riot_id_of(&base),
            Some(RiotId {
                game_name: String::from("PKT MidKing"),
                tag_line: String::from("Akali"),
            })
        );

        // Con espacios de más se recortan (el LCU varía el padding).
        let con_espacios = SummonerInfo {
            game_name: Some(String::from("  Klipy  ")),
            tag_line: Some(String::from(" LAS ")),
            ..base.clone()
        };
        assert_eq!(
            riot_id_of(&con_espacios),
            Some(RiotId {
                game_name: String::from("Klipy"),
                tag_line: String::from("LAS"),
            })
        );

        // Falta tagLine (cuenta legacy).
        let sin_tag = SummonerInfo {
            tag_line: None,
            ..base.clone()
        };
        assert_eq!(riot_id_of(&sin_tag), None);

        // tagLine vacía o solo espacios cuenta como ausente.
        let tag_vacia = SummonerInfo {
            tag_line: Some(String::from("   ")),
            ..base.clone()
        };
        assert_eq!(riot_id_of(&tag_vacia), None);

        // Falta gameName.
        let sin_game_name = SummonerInfo {
            game_name: None,
            ..base
        };
        assert_eq!(riot_id_of(&sin_game_name), None);
    }

    /// El puuid se reporta truncado en logs: `…` + últimos 4; valores cortos
    /// o vacíos se muestran enteros (sin pánicos de slicing).
    #[test]
    fn puuid_resumido_muestra_solo_la_cola() {
        assert_eq!(
            puuid_resumido("puuid-api-terminado-en-zcpA"),
            "…zcpA"
        );
        // Límite: 8 chars o menos va completo.
        assert_eq!(puuid_resumido("12345678"), "12345678");
        assert_eq!(puuid_resumido("abc"), "abc");
        assert_eq!(puuid_resumido(""), "");
        // No paniquea con no-ASCII (corte por chars, no por bytes).
        assert_eq!(puuid_resumido("áéíóú-ññzcpA"), "…zcpA");
    }

    /// El Riot ID se muestra como `gameName#tagLine` (formato canónico).
    #[test]
    fn riot_id_se_muestra_como_game_name_tag_line() {
        let riot_id = RiotId {
            game_name: String::from("PKT MidKing"),
            tag_line: String::from("Akali"),
        };
        assert_eq!(riot_id.to_string(), "PKT MidKing#Akali");
    }

    /// El offset avanza por lo realmente devuelto y no desborda.
    #[test]
    fn next_start_avanza_por_la_pagina_real() {
        assert_eq!(next_start(0, 100), 100);
        assert_eq!(next_start(100, 100), 200);
        assert_eq!(next_start(250, 50), 300); // última página corta
        assert_eq!(next_start(0, 0), 0);
        assert_eq!(next_start(i64::MAX - 1, 100), i64::MAX); // saturating
    }

    /// Corte de paginación: página vacía, página toda conocida o tope
    /// alcanzado detienen el loop; con ids nuevos y tope sin alcanzar,
    /// sigue.
    #[test]
    fn paginacion_corta_por_vacia_conocida_o_tope() {
        // Historia completa (página vacía).
        assert!(!should_continue_paging(0, 0, 0));

        // Todo el tramo ya está en el dataset (re-sync barato).
        assert!(!should_continue_paging(100, 0, 0));

        // Tope alcanzado en la corrida.
        assert!(!should_continue_paging(100, 100, MAX_SYNC_MATCHES));
        assert!(!should_continue_paging(100, 3, MAX_SYNC_MATCHES));

        // Casos de continuación.
        assert!(should_continue_paging(100, 100, 0));
        assert!(should_continue_paging(100, 1, 0)); // solapamiento parcial
        assert!(should_continue_paging(100, 100, MAX_SYNC_MATCHES - 1));
    }

    /// La corrida nunca pasa del tope: 300 descargadas en páginas de 100
    /// corta justo después de la tercera página.
    #[test]
    fn simulacion_de_paginacion_hasta_el_tope() {
        let mut start = 0_i64;
        let mut downloaded = 0_usize;
        let mut pages_fetched = 0_u32;

        loop {
            let page_len = 100; // simulado: siempre hay historia
            let fresh = page_len; // simulado: nada conocido
            pages_fetched += 1;
            if !should_continue_paging(page_len, fresh, downloaded) {
                break;
            }
            downloaded = (downloaded + fresh).min(MAX_SYNC_MATCHES);
            start = next_start(start, page_len);
            assert!(pages_fetched < 10, "no puede irse de la pista");
        }

        assert_eq!(downloaded, MAX_SYNC_MATCHES);
        assert_eq!(start, 300);
        assert_eq!(pages_fetched, 4); // 3 con descarga + 1 que corta
    }

    /// Estado del sync: roundtrip en temp dir, archivo corrupto → default y
    /// default serializa con el campo esperado.
    #[test]
    fn sync_state_roundtrip_y_tolerancia() {
        let dir = temp_data_dir("sync-state");
        let path = dir.join("sync_state.json");

        // Archivo ausente → default.
        assert_eq!(load_sync_state(&path), SyncState::default());

        // Guardar y releer (namespace actual: puuid api Account-V1).
        let state = SyncState {
            last_synced_puuid: Some(String::from("puuid-api-…zcpA")),
        };
        save_sync_state(&path, &state);
        assert_eq!(load_sync_state(&path), state);

        // Corrupto → default (con log), nunca panic.
        fs::write(&path, "{ no es json").expect("escribe basura");
        assert_eq!(load_sync_state(&path), SyncState::default());

        let _ = fs::remove_dir_all(dir);
    }

    /// Un archivo viejo con el puuid de JUEGO (namespace viejo, formato UUID)
    /// se carga sin errores: el valor es solo trazabilidad y el primer sync
    /// de la sesión lo re-escribe con el puuid de la API (Account-V1).
    #[test]
    fn sync_state_de_version_vieja_con_puuid_de_juego_carga_igual() {
        let dir = temp_data_dir("sync-state-viejo");
        let path = dir.join("sync_state.json");
        fs::write(
            &path,
            r#"{"lastSyncedPuuid":"f7e2b22b-d677-558c-95af-495e9807ada3"}"#,
        )
        .expect("siembra el archivo viejo");

        let state = load_sync_state(&path);
        assert_eq!(
            state.last_synced_puuid.as_deref(),
            Some("f7e2b22b-d677-558c-95af-495e9807ada3")
        );

        // Y el formato camelCase se mantiene al re-escribirlo.
        let json = serde_json::to_string(&state).expect("serializa");
        assert_eq!(
            json,
            r#"{"lastSyncedPuuid":"f7e2b22b-d677-558c-95af-495e9807ada3"}"#
        );

        let _ = fs::remove_dir_all(dir);
    }

    /// Formato estable del estado (camelCase, como el resto del dataset).
    #[test]
    fn sync_state_serializa_camel_case() {
        let json = serde_json::to_string(&SyncState {
            last_synced_puuid: Some(String::from("p")),
        })
        .expect("serializa");

        assert_eq!(json, r#"{"lastSyncedPuuid":"p"}"#);
    }
}
