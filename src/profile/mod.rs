//! Perfil del invocador (FASE 11a-2): rank solo-queue, top maestría,
//! historial reciente y settings del fondo — TODO por LCU, sin gastar la
//! API key de Riot.
//!
//! Piezas:
//!
//! - [`models`]: tipos de dominio ([`ProfileSummary`], [`RankedInfo`],
//!   [`MasteryEntry`], [`MatchSummary`], [`LcuMatchDetail`]) tolerantes y
//!   testeables.
//! - [`lcu_fetch`]: fetchers con el cliente REST de irelia sobre las
//!   rutas reales del cliente actual (verificadas en vivo contra un
//!   cliente de LA2 16.20; los endpoints clásicos de maestría/detalle ya
//!   no existen) + `build_profile_summary` pura.
//! - [`settings`]: `settings.json` del fondo de splash, path inyectable,
//!   tolerante a archivo ausente/corrupto.
//!
//! **[`run_profile_loop`]** mantiene la señal `profile:
//! Signal<Option<ProfileSummary>>` con el patrón de espejo local del
//! proyecto (misma mecánica que `summoner`/`platform` en
//! `lcu::client`): una **segunda conexión REST** propia
//! ([`LcuClient::connect`] — válida, el loop principal tiene la suya para
//! websocket + REST) que:
//!
//! 1. Detecta el cliente (reintento cada [`DETECTION_RETRY`]); sin cliente
//!    → `profile` queda en `None` (la UI 11b muestra su pantalla "sin
//!    cliente").
//! 2. Con cliente: fetch completo → `build_profile_summary` → `.set()` solo
//!    si cambió.
//! 3. Re-fetch cada [`PROFILE_REFRESH_INTERVAL`] (5 min) mientras el
//!    cliente siga vivo (health check con `head` cada
//!    [`HEALTH_CHECK_INTERVAL`]).
//! 4. Al caer el cliente → reset a `None` y vuelta a detectar.
//!
//! Un fetch individual que falla degrada el perfil (rank `None`, listas
//! vacías) pero nunca borra el último resumen válido: solo se actualiza la
//! señal con fetchs que llegan hasta el final.

use std::time::Duration;

use dioxus::prelude::{Signal, WritableExt};
use irelia::requests::RequestClientType;
use irelia::rest::LcuClient;
use tokio::time::sleep;

use crate::lcu::events::CHAMP_SELECT_SESSION_URI;
use crate::profile::lcu_fetch::fetch_profile_summary;
use crate::profile::models::ProfileSummary;

pub mod client_handle;
pub mod lcu_fetch;
pub mod models;
pub mod settings;

/// Pausa entre intentos de detección del cliente (igual que el loop LCU).
const DETECTION_RETRY: Duration = Duration::from_millis(2500);
/// Frecuencia del health check REST mientras hay cliente (igual que el
/// loop LCU): cualquier código HTTP cuenta como "cliente vivo".
const HEALTH_CHECK_INTERVAL: Duration = Duration::from_millis(5000);
/// Frecuencia de re-fetch del perfil con cliente vivo (5 min, según spec).
pub const PROFILE_REFRESH_INTERVAL: Duration = Duration::from_secs(300);

/// Conecta el cliente REST de irelia (bloqueante: escaneo de procesos con
/// `sysinfo`) en `spawn_blocking`, igual que `lcu::client::connect_rest`.
async fn connect_rest() -> Option<LcuClient<RequestClientType>> {
    match tokio::task::spawn_blocking(LcuClient::connect).await {
        Ok(Ok(client)) => Some(client),
        // "No hay cliente" es el caso esperado mientras no corre el LoL:
        // no llena la consola.
        Ok(Err(error)) => {
            eprintln!("klipy [profile]: aviso: todavía sin cliente REST ({error})");
            None
        }
        Err(error) => {
            eprintln!("klipy [profile]: tarea de conexión REST falló: {error}");
            None
        }
    }
}

/// Sincroniza el espejo local del perfil con su señal solo si cambió el
/// valor (mismo patrón que `transition`/`set_summoner` en `lcu::client`).
fn set_profile(
    current: &mut Option<ProfileSummary>,
    profile: &mut Signal<Option<ProfileSummary>>,
    next: Option<ProfileSummary>,
) {
    if *current != next {
        *current = next.clone();
        profile.set(next);
    }
}

/// Bucle de vida del perfil. Nunca retorna: vive mientras viva la app,
/// alternando entre detección y sesión según el cliente (ver doc del
/// módulo).
pub async fn run_profile_loop(mut profile: Signal<Option<ProfileSummary>>) {
    // Espejo local: la señal solo cambia cuando el valor realmente cambió.
    let mut current = Option::<ProfileSummary>::None;
    set_profile(&mut current, &mut profile, None);

    loop {
        // 1. Detección: sin cliente → perfil en None (pantalla "sin
        //    cliente" de la UI 11b), handle global limpio (el modal de
        //    detalle muestra "Cliente no conectado") y reintento.
        let Some(client) = connect_rest().await else {
            client_handle::set_client_handle(None);
            set_profile(&mut current, &mut profile, None);
            sleep(DETECTION_RETRY).await;
            continue;
        };
        eprintln!("klipy [profile]: cliente LCU conectado; cargando perfil");

        // Handle global para accesos on-demand desde la UI (modal de
        // detalle de partida). `LcuClient` es Clone barato: comparte el
        // request client interno (ver `client_handle`).
        client_handle::set_client_handle(Some(client.clone()));

        // 2. Sesión: re-fetch cada REFRESH con health check cada HEALTH.
        let mut alive = true;
        while alive {
            // Un ciclo que no llega hasta el final (summoner falló) no
            // toca la señal: el espejo conserva el último perfil válido.
            if let Some(summary) = fetch_profile_summary(&client).await {
                eprintln!(
                    "klipy [profile]: perfil actualizado ({} partidas recientes, {} top champs)",
                    summary.recent.len(),
                    summary.top_champs.len()
                );
                set_profile(&mut current, &mut profile, Some(summary));
            }

            // Espera hasta el próximo re-fetch chequeando salud cada
            // HEALTH_CHECK_INTERVAL. `head` solo falla ante errores de
            // transporte (cliente muerto); cualquier código HTTP indica
            // cliente vivo (mismo criterio que `lcu::client`). El intervalo
            // de refresh es múltiplo exacto del de salud, así que no hay
            // deriva entre espera y re-fetch.
            let mut waited = Duration::ZERO;
            while waited < PROFILE_REFRESH_INTERVAL {
                sleep(HEALTH_CHECK_INTERVAL).await;
                waited += HEALTH_CHECK_INTERVAL;
                if !client.head(CHAMP_SELECT_SESSION_URI).await.is_ok() {
                    alive = false;
                    break;
                }
            }
        }

        // 3. Caída → perfil en None, handle limpio y nueva ronda de
        //    detección.
        eprintln!("klipy [profile]: cliente LCU desconectado; perfil reseteado");
        client_handle::set_client_handle(None);
        set_profile(&mut current, &mut profile, None);
        sleep(DETECTION_RETRY).await;
    }
}
