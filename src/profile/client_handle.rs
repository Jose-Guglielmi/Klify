//! Handle global al cliente REST del LCU (FASE 11b-1).
//!
//! El cliente vive dentro de [`crate::profile::run_profile_loop`], pero la
//! UI necesita acceso on-demand: el modal de detalle de partida llama
//! [`crate::profile::lcu_fetch::fetch_match_detail`] cuando el usuario hace
//! click en una fila del historial.
//!
//! **Decisión de diseño**: `irelia::rest::LcuClient` es `Clone`
//! (`impl<T: RequestClientTrait + Clone> Clone for LcuClient<T>`; el
//! `RequestClientType` de la feature `__hyper` es un `hyper::Client`
//! clonable y barato — comparte el pool interno). Por eso la solución
//! preferente del spec aplica: un global `RwLock<Option<LcuClient>>` con
//! `set_client_handle()` (el loop lo publica al conectar y lo limpia al
//! desconectar) y [`client_handle()`] (clona el cliente para usarlo en una
//! tarea del modal). No hace falta el canal mpsc de reserva.
//!
//! El lock es tolerante: un lock envenenado degrada a `None` (la UI muestra
//! "Cliente no conectado") en vez de propagar el pánico.

use std::sync::RwLock;

use irelia::requests::RequestClientType;
use irelia::rest::LcuClient;

/// Cliente REST del LCU con el request client por defecto de irelia (el
/// mismo tipo que usan todos los fetchers del perfil).
pub type LcuRestClient = LcuClient<RequestClientType>;

/// Handle global: `Some` mientras el loop de perfil tenga cliente vivo.
static CLIENT_HANDLE: RwLock<Option<LcuRestClient>> = RwLock::new(None);

/// Publica (o limpia) el handle al cliente. Lo llama [`run_profile_loop`]:
/// `Some` al conectar, `None` al detectar caída.
pub fn set_client_handle(client: Option<LcuRestClient>) {
    if let Ok(mut guard) = CLIENT_HANDLE.write() {
        *guard = client;
    }
}

/// Clona el handle vigente, si hay cliente conectado. `None` → la UI
/// muestra "Cliente no conectado" en vez de intentar el fetch.
#[must_use]
pub fn client_handle() -> Option<LcuRestClient> {
    CLIENT_HANDLE.read().ok().and_then(|guard| guard.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sin cliente publicado → `None` (el modal muestra su estado "sin
    /// cliente"). No se puede construir un `LcuClient` en tests sin LCU
    /// real, así que se cubre solo el caso vacío del global (el de `Some`
    /// es un passthrough de `Option` sin lógica propia).
    #[test]
    fn sin_handle_publicado_devuelve_none() {
        set_client_handle(None);
        assert!(client_handle().is_none());
    }
}
