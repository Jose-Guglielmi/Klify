//! Acceso a Data Dragon (estáticos públicos de Riot: campeones, runas,
//! imágenes, versiones).
//!
//! La API no requiere autenticación. El flujo de carga ([`run_catalog_loader`]
//! y [`run_runes_loader`], tareas de por vida de la app, una por catálogo):
//!
//! 1. `GET /api/versions.json` → lista descendente de versiones; la primera
//!    es la más reciente.
//! 2. Se elige la **versión objetivo** con [`choose_version`]: si el cliente
//!    de LoL está conectado manda su parche (`client_version`, los dos
//!    primeros componentes de `/lol-patch/v1/game-version` — ver la nota de
//!    [`crate::lcu::client`]: el endpoint clásico `/lol-game-data/v1/version`
//!    ya no existe en los clientes actuales —, ej. `"16.20"` de
//!    `"16.20.8248524+..."`); si no hay cliente, la más reciente de la lista.
//! 3. Si el catálogo en la señal ya está en esa versión, no se descarga nada.
//! 4. Si no, `GET /cdn/{versión}/data/en_US/champion.json` (o
//!    `runesReforged.json`) y se publica en la señal.
//! 5. Se duerme [`RETRY_INTERVAL`] y se repite: las tareas **nunca terminan**,
//!    así reaccionan a un cliente que aparece tarde o cambia de parche
//!    (re-descarga en el ciclo siguiente, ≤ [`RETRY_INTERVAL`]).
//!
//! Si algo falla (sin internet, CDN caída) se loguea y se reintenta en el
//! próximo ciclo; mientras tanto la app funciona con placeholders (campeones)
//! o sin runas, y el loop LCU toma los catálogos apenas estén.
//!
//! **Convención de niveles de log** (sin crates de logging: la consola de
//! `dx serve` puede etiquetar cualquier stderr como ERROR, el nivel real
//! vive en el texto): los fallos de un ciclo de reintento son **informativos**
//! y se marcan `aviso: todavía sin ...` — se esperan (arranque sin red,
//! catálogo tardío) y la tarea los recupera sola. Los que no llevan `aviso`
//! (falla al crear el cliente HTTP) sí son errores reales.
//!
//! Ambas llamadas de cada ciclo reutilizan un único `reqwest::Client` por
//! intento (connection pooling y timeouts acotados). El sondeo de versión
//! también detecta que Riot publicó un parche nuevo en la CDN sin que el
//! cliente cambie: el catálogo se actualiza al siguiente ciclo.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use dioxus::prelude::{ReadableExt, Signal, WritableExt};
use tokio::time::sleep;

use crate::data_dragon::item::{ItemCatalog, load_item_catalog};
use crate::data_dragon::models::{
    ChampionCatalog, ChampionListResponse, champion_json_url,
};
use crate::data_dragon::runes::{RuneCatalog, RuneTreeDto, runes_reforged_url};

pub mod champion_detail;
pub mod item;
pub mod models;
pub mod runes;

/// URL del listado de versiones (array de strings descendente, la primera es
/// la más reciente).
const VERSIONS_URL: &str = "https://ddragon.leagueoflegends.com/api/versions.json";
/// Pausa entre ciclos de los loaders (reintentos y re-chequeo de versión).
pub const RETRY_INTERVAL: Duration = Duration::from_secs(30);
/// Límite para establecer conexión con la CDN.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Límite total por request (versions.json, champion.json o runesReforged.json).
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

/// Errores de la descarga de catálogos. Solo se loguean: la app sigue con
/// placeholders hasta el próximo reintento.
#[derive(Debug)]
pub enum DataDragonError {
    /// Error de transporte/HTTP de `reqwest`.
    Http(reqwest::Error),
    /// `versions.json` no trajo ninguna versión utilizable.
    EmptyVersions,
    /// La respuesta llegó bien pero no trajo el dato pedido (payload capado
    /// o id inexistente). FASE 11a: detalle por-campeón.
    MissingData(String),
}

impl fmt::Display for DataDragonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Http(error) => write!(f, "error de HTTP contra Data Dragon: {error}"),
            Self::EmptyVersions => write!(f, "versions.json no trajo versiones"),
            Self::MissingData(what) => write!(f, "respuesta de Data Dragon sin {what}"),
        }
    }
}

impl std::error::Error for DataDragonError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Http(error) => Some(error),
            Self::EmptyVersions | Self::MissingData(_) => None,
        }
    }
}

impl From<reqwest::Error> for DataDragonError {
    fn from(error: reqwest::Error) -> Self {
        Self::Http(error)
    }
}

/// Versión de Data Dragon a usar, dada la lista de versiones disponibles y la
/// versión del juego que reporta el cliente LCU (ej. `"16.7.623.1234"`).
///
/// Regla:
/// - Sin `client_game_version` (o sin prefijo utilizable) → la primera de la
///   lista (la más reciente).
/// - Con ella → sus dos primeros componentes (`"16.7"`) y la **primera**
///   versión de la lista cuyo primer componente coincida y cuyo segundo
///   empiece igual (`"16.7.1"` sí, `"16.70.1"` no: la comparación respeta el
///   borde de componente). La lista se recorre en el orden dado, así que con
///   listas no ordenadas gana el primer match posicional.
/// - Sin match → cae a la primera de la lista (latest).
/// - Lista vacía → string vacío (los loaders lo tratan como
///   [`DataDragonError::EmptyVersions`]).
#[must_use]
pub fn choose_version(available: &[String], client_game_version: Option<&str>) -> String {
    let Some(latest) = available.first() else {
        return String::new();
    };

    let Some(prefix) = client_game_version.and_then(patch_prefix) else {
        return latest.clone();
    };

    available
        .iter()
        .find(|candidate| matches_patch_prefix(candidate, &prefix))
        .cloned()
        .unwrap_or_else(|| latest.clone())
}

/// Prefijo de parche de una versión de juego: sus dos primeros componentes
/// (`"16.7.623.1234"` → `"16.7"`; `"16.7"` → `"16.7"`; `"16"` → `"16"`).
/// `None` si no hay un primer componente utilizable (string vacío).
fn patch_prefix(version: &str) -> Option<String> {
    let mut components = version.split('.');
    let major = components.next()?;
    if major.is_empty() {
        return None;
    }
    let minor = components.next().unwrap_or_default();
    Some(if minor.is_empty() {
        major.to_string()
    } else {
        format!("{major}.{minor}")
    })
}

/// `candidate` empieza con `prefix` respetando el borde de componente: el
/// carácter siguiente al prefijo debe ser `.` o no existir (`"16.7"` matchea
/// `"16.7.1"` y `"16.7"`, pero no `"16.70.1"`).
fn matches_patch_prefix(candidate: &str, prefix: &str) -> bool {
    candidate.starts_with(prefix)
        && matches!(candidate.as_bytes().get(prefix.len()), None | Some(b'.'))
}

/// Descarga `versions.json`.
async fn fetch_versions(client: &reqwest::Client) -> Result<Vec<String>, DataDragonError> {
    Ok(client
        .get(VERSIONS_URL)
        .send()
        .await?
        .error_for_status()?
        .json::<Vec<String>>()
        .await?)
}

/// Versión de Data Dragon objetivo para este ciclo: el parche del cliente si
/// hay, si no el más reciente. Error si `versions.json` no trajo nada.
async fn resolve_target_version(
    client: &reqwest::Client,
    client_game_version: Option<&str>,
) -> Result<String, DataDragonError> {
    let versions = fetch_versions(client).await?;
    let version = choose_version(&versions, client_game_version);
    if version.is_empty() {
        return Err(DataDragonError::EmptyVersions);
    }
    Ok(version)
}

/// Descarga el catálogo de campeones para una versión concreta.
pub async fn load_champion_catalog(
    client: &reqwest::Client,
    version: &str,
) -> Result<ChampionCatalog, DataDragonError> {
    let response = client
        .get(champion_json_url(version))
        .send()
        .await?
        .error_for_status()?
        .json::<ChampionListResponse>()
        .await?;

    Ok(ChampionCatalog::build(version.to_string(), response))
}

/// Descarga el catálogo de runas (`runesReforged.json`) para una versión
/// concreta.
pub async fn load_runes_catalog(
    client: &reqwest::Client,
    version: &str,
) -> Result<RuneCatalog, DataDragonError> {
    let trees = client
        .get(runes_reforged_url(version))
        .send()
        .await?
        .error_for_status()?
        .json::<Vec<RuneTreeDto>>()
        .await?;

    Ok(RuneCatalog::build(version.to_string(), trees))
}

/// Cliente HTTP con timeouts acotados: sin ellos un request colgado dejaría
/// la app en placeholders para siempre. Público para que la UI haga cargas
/// on-demand propias (picker de splash del perfil, FASE 11b-1).
pub fn http_client() -> Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .build()
}

/// Versión más reciente publicada en `versions.json` (para cargas
/// on-demand sin cliente conectado; los loaders usan
/// [`resolve_target_version`] porque además pinean al parche del cliente).
pub async fn latest_version(client: &reqwest::Client) -> Result<String, DataDragonError> {
    fetch_versions(client)
        .await?
        .into_iter()
        .next()
        .ok_or(DataDragonError::EmptyVersions)
}

/// Tarea de por vida que mantiene el catálogo de campeones en la señal,
/// pineado al parche del cliente LCU (o al más reciente si no hay cliente).
/// Reintenta cada [`RETRY_INTERVAL`] ante fallos y re-descarga cuando la
/// versión objetivo cambia; nunca retorna.
pub async fn run_catalog_loader(
    mut catalog: Signal<Option<ChampionCatalog>>,
    client_version: Signal<Option<String>>,
) {
    loop {
        let client = match http_client() {
            Ok(client) => client,
            Err(error) => {
                eprintln!("klipy [data_dragon]: no se pudo crear el cliente HTTP: {error}");
                sleep(RETRY_INTERVAL).await;
                continue;
            }
        };

        // Snapshot de la versión del juego del cliente antes de cualquier
        // `await`: el guard vive solo en esta expresión.
        let pinned: Option<String> = (*client_version.read()).clone();

        match resolve_target_version(&client, pinned.as_deref()).await {
            Ok(target) => {
                // ¿Ya está el catálogo en la versión objetivo? Evita
                // re-descargar en cada ciclo si nada cambió.
                let up_to_date = catalog
                    .read()
                    .as_ref()
                    .is_some_and(|data| data.version == target);
                if up_to_date {
                    sleep(RETRY_INTERVAL).await;
                    continue;
                }

                match load_champion_catalog(&client, &target).await {
                    Ok(data) => {
                        eprintln!(
                            "klipy [data_dragon]: catálogo v{} cargado ({} campeones)",
                            data.version,
                            data.by_id.len()
                        );
                        catalog.set(Some(data));
                    }
                    Err(error) => {
                        // Aviso informativo (no error): la tarea reintenta
                        // sola en el próximo ciclo, ver la convención de
                        // niveles en la doc del módulo.
                        eprintln!(
                            "klipy [data_dragon]: aviso: todavía sin catálogo ({error}); reintentando en {:?}",
                            RETRY_INTERVAL
                        );
                    }
                }
            }
            Err(error) => {
                eprintln!(
                    "klipy [data_dragon]: aviso: todavía sin versión objetivo ({error}); reintentando en {:?}",
                    RETRY_INTERVAL
                );
            }
        }

        sleep(RETRY_INTERVAL).await;
    }
}

/// Tarea de por vida que mantiene el catálogo de runas en la señal, con la
/// misma estrategia de pineado y reintentos que [`run_catalog_loader`].
pub async fn run_runes_loader(
    mut runes: Signal<Option<RuneCatalog>>,
    client_version: Signal<Option<String>>,
) {
    loop {
        let client = match http_client() {
            Ok(client) => client,
            Err(error) => {
                eprintln!("klipy [data_dragon]: no se pudo crear el cliente HTTP: {error}");
                sleep(RETRY_INTERVAL).await;
                continue;
            }
        };

        let pinned: Option<String> = (*client_version.read()).clone();

        match resolve_target_version(&client, pinned.as_deref()).await {
            Ok(target) => {
                let up_to_date = runes
                    .read()
                    .as_ref()
                    .is_some_and(|data| data.version == target);
                if up_to_date {
                    sleep(RETRY_INTERVAL).await;
                    continue;
                }

                match load_runes_catalog(&client, &target).await {
                    Ok(data) => {
                        eprintln!(
                            "klipy [data_dragon]: runas v{} cargadas ({} runas, {} árboles)",
                            data.version,
                            data.by_id.len(),
                            data.trees.len()
                        );
                        runes.set(Some(data));
                    }
                    Err(error) => {
                        // Aviso informativo (no error): la tarea reintenta
                        // sola en el próximo ciclo, ver la convención de
                        // niveles en la doc del módulo.
                        eprintln!(
                            "klipy [data_dragon]: aviso: todavía sin runas ({error}); reintentando en {:?}",
                            RETRY_INTERVAL
                        );
                    }
                }
            }
            Err(error) => {
                eprintln!(
                    "klipy [data_dragon]: aviso: todavía sin versión objetivo ({error}); reintentando en {:?}",
                    RETRY_INTERVAL
                );
            }
        }

        sleep(RETRY_INTERVAL).await;
    }
}

/// Tarea de por vida que mantiene el catálogo de ítems en la señal (FASE
/// 11b-2b), con la misma estrategia de pineado y reintentos que
/// [`run_catalog_loader`]. El catálogo viaja en `Arc` porque el JSON
/// completo pesa ~200 KB y la UI lo clona en cada render de las tabs
/// ITEMS/COUNTERS del champ select.
pub async fn run_item_loader(
    mut items: Signal<Option<Arc<ItemCatalog>>>,
    client_version: Signal<Option<String>>,
) {
    loop {
        let client = match http_client() {
            Ok(client) => client,
            Err(error) => {
                eprintln!("klipy [data_dragon]: no se pudo crear el cliente HTTP: {error}");
                sleep(RETRY_INTERVAL).await;
                continue;
            }
        };

        let pinned: Option<String> = (*client_version.read()).clone();

        match resolve_target_version(&client, pinned.as_deref()).await {
            Ok(target) => {
                let up_to_date = items
                    .read()
                    .as_ref()
                    .is_some_and(|data| data.version == target);
                if up_to_date {
                    sleep(RETRY_INTERVAL).await;
                    continue;
                }

                match load_item_catalog(&client, &target).await {
                    Ok(data) => {
                        eprintln!(
                            "klipy [data_dragon]: catálogo de ítems v{} cargado ({} ítems)",
                            data.version,
                            data.by_id.len()
                        );
                        items.set(Some(Arc::new(data)));
                    }
                    Err(error) => {
                        // Aviso informativo (no error): la tarea reintenta
                        // sola en el próximo ciclo, ver la convención de
                        // niveles en la doc del módulo.
                        eprintln!(
                            "klipy [data_dragon]: aviso: todavía sin ítems ({error}); reintentando en {:?}",
                            RETRY_INTERVAL
                        );
                    }
                }
            }
            Err(error) => {
                eprintln!(
                    "klipy [data_dragon]: aviso: todavía sin versión objetivo ({error}); reintentando en {:?}",
                    RETRY_INTERVAL
                );
            }
        }

        sleep(RETRY_INTERVAL).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn versiones(lista: &[&str]) -> Vec<String> {
        lista.iter().map(|v| (*v).to_string()).collect()
    }

    /// Cliente cuyo parche coincide con la entrada más reciente de la lista.
    #[test]
    fn match_exacto_contra_la_primera() {
        let available = versiones(&["16.8.1", "16.7.1", "16.6.1"]);

        assert_eq!(
            choose_version(&available, Some("16.8.623.1234")),
            "16.8.1"
        );
    }

    /// El caso nominal: el juego trae un build (`16.7.623.1234`) y ddragon
    /// solo tiene el parche (`16.7.1`); se elige por prefijo de parche.
    #[test]
    fn match_por_prefijo_de_parche() {
        let available = versiones(&["16.8.1", "16.7.1", "16.6.1"]);

        assert_eq!(
            choose_version(&available, Some("16.7.623.1234")),
            "16.7.1"
        );
    }

    /// Sin ninguna versión que matchee el parche del cliente (instalado más
    /// nuevo o más viejo que todo lo que conoce la CDN) → la más reciente.
    #[test]
    fn sin_match_cae_al_latest() {
        let available = versiones(&["16.8.1", "16.7.1"]);

        assert_eq!(choose_version(&available, Some("9.9.483.1")), "16.8.1");
        assert_eq!(choose_version(&available, Some("99.0.1.1")), "16.8.1");
    }

    /// Sin cliente (`None`) o con una versión sin componentes utilizables →
    /// la más reciente.
    #[test]
    fn sin_version_de_cliente_devuelve_latest() {
        let available = versiones(&["16.8.1", "16.7.1"]);

        assert_eq!(choose_version(&available, None), "16.8.1");
        assert_eq!(choose_version(&available, Some("")), "16.8.1");
    }

    /// La regla no asume lista ordenada: se elige el primer match en el
    /// orden de la lista; sin match, cae a la primera entrada (latest).
    #[test]
    fn lista_desordenada_elige_el_primer_match_posicional() {
        let available = versiones(&["15.19.1", "15.21.1", "15.20.1", "15.7.1"]);

        assert_eq!(
            choose_version(&available, Some("15.20.500.1")),
            "15.20.1"
        );
        // Sin match → la primera de la lista, sea cual sea su posición.
        assert_eq!(choose_version(&available, Some("15.9.1.1")), "15.19.1");
    }

    /// El borde de componente evita falsos positivos: `16.70.1` NO es del
    /// parche `16.7`.
    #[test]
    fn prefijo_respeta_el_borde_de_componente() {
        let available = versiones(&["16.70.1", "16.7.1", "16.6.1"]);

        assert_eq!(
            choose_version(&available, Some("16.7.623.1234")),
            "16.7.1"
        );
    }

    /// Clientes con menos de dos componentes: `"16"` pide el parche `16`,
    /// y una lista vacía produce un string vacío (error para el loader).
    #[test]
    fn casos_borde_de_version_y_de_lista() {
        let available = versiones(&["16.8.1", "15.21.1"]);
        assert_eq!(choose_version(&available, Some("16.1.1.1")), "16.8.1");

        assert_eq!(choose_version(&[], Some("16.7.1.1")), "");
        assert_eq!(choose_version(&[], None), "");
    }
}
