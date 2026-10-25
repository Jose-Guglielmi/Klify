//! Acciones LCU para **aplicar** las recomendaciones al cliente real: crear
//! la página de runas recomendada (`/lol-perks/v1/pages`) y cargar el set de
//! ítems (`/lol-item-sets/v1/item-sets/{champion}/sets`) durante el champ
//! select. Es la contraparte de escritura del recomendador (FASE 9/10):
//! aquellas fases leen fuentes públicas y publican candidatos; este módulo
//! materializa la elección en el perfil del invocador.
//!
//! Nombre canónico: toda página/set propio se identifica con el título
//! [`nombre_klify`] (`"Klify · {campeón}"`). Ese título es la llave del
//! inventario ([`consultar_inventario_runas`]) y del dedupe de sets
//! ([`cargar_item_set`]).
//!
//! **Respuestas vacías**: irelia 0.11 NO trae el tipo `IgnoreAny` (solo lo
//! nombra en un doc-comment de una versión posterior). Como el request client
//! valida el código HTTP **antes** de decodificar el cuerpo, un error
//! `Error::Json`/`Error::RmpDecode` significa "2xx con cuerpo no parseable"
//! (típicamente 204 No Content) — es decir, la operación **sí** se hizo.
//! [`respuesta_ignorable`] captura ese caso y lo trata como éxito en todas
//! las escrituras.
//!
//! **Hilos**: a diferencia de `connect_rest` (que escanea procesos con
//! `sysinfo` y por eso corre en `spawn_blocking`), los `get`/`post`/`delete`
//! de irelia son async puros sobre hyper: van directo en la tarea del
//! llamador, sin `spawn_blocking`.
//!
//! Errores: todos devuelven `Result<_, String>` con mensajes en español
//! rioplatense, listos para mostrar en la UI. El cliente REST se toma del
//! handle global ([`client_handle`], publicado por el loop de perfil); sin
//! cliente → `"Cliente no conectado"`.

use irelia::requests::Error;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::profile::client_handle::client_handle;
use crate::stats::source::items::{ItemBuild, ItemStat};
use crate::stats::source::RunePageStats;

/// Endpoint REST de las páginas de runas (maestrías) del invocador.
const PAGES_URI: &str = "/lol-perks/v1/pages";
/// Endpoint REST raíz de los sets de ítems (listado/colección por
/// invocador, y el post por campeón que quedó como fallback).
/// Rutas verificadas en vivo (2026-10-23, LCU LA2):
/// - `GET  {ITEM_SETS_URI}/{summonerId}/sets` → colección del invocador.
/// - `PUT  {ITEM_SETS_URI}/{summonerId}/sets` → reemplaza la colección.
/// - `POST {ITEM_SETS_URI}/{cid}/sets` → agrega un set (queda con uid vacío).
/// - `GET  {ITEM_SETS_URI}` → **404 "Invalid URI format"**: no existe un
///   listado global; por eso el dedupe va por summonerId.
const ITEM_SETS_URI: &str = "/lol-item-sets/v1/item-sets";
/// Id del mapa asociado a los sets de ítems: 11 = Grieta del Invocador.
const MAPA_GRIETA: i64 = 11;
/// Shards de stats por defecto del LCU (adaptativa, adaptativa, armadura):
/// el relleno cuando la fuente trae menos de 3 (la creación exige 9 ids).
const SHARDS_DEFAULT: [i64; 3] = [5008, 5008, 5001];

// ---------------------------------------------------------------------------
// DTOs (tolerantes: payload parcial → default, nunca error de parsing).
// ---------------------------------------------------------------------------

/// Resumen de una página de runas del inventario del invocador
/// (`GET {PAGES_URI}`). Solo los campos que hacen a la acción: el resto del
/// payload real (`selectedPerkIds`, `isActive`, ...) se ignora.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LcuPageSummary {
    /// Id numérico de la página (lo que pide el DELETE).
    #[serde(default)]
    pub id: i64,
    /// Nombre visible de la página.
    #[serde(default)]
    pub name: String,
    /// `isDeletable` REAL del payload del LCU (verificado en vivo
    /// 2026-10-23: la respuesta trae `isDeletable`, NO `isReadOnly` —
    /// por eso las presets del juego antes pasaban como borrables y el
    /// DELETE respondía 403). Sin la clave, conservador: `false` = no se
    /// ofrece para borrar.
    #[serde(default, rename = "isDeletable")]
    pub is_deletable: bool,
}

/// Inventario de páginas de runas del invocador, ya interpretado: cuáles hay
/// y si la nuestra existe. El límite real de creación lo impone el servidor
/// (varía por cuenta: rechaza con 400 "Max pages reached", verificado en
/// vivo 2026-10-23) — no se calcula acá; el fallo por límite se detecta en
/// [`crear_pagina_runas`] (`ResultadoCreacion::SinLugar`).
#[derive(Clone, Debug, PartialEq)]
pub struct InventarioRunas {
    /// Páginas actuales del invocador (las de lectura del GET).
    pub pages: Vec<LcuPageSummary>,
    /// Id de nuestra página canónica ([`nombre_klify`]) si ya existe.
    pub nuestra: Option<i64>,
}

/// Payload de creación de una página de runas (`POST {PAGES_URI}`), con los
/// campos que el LCU espera del tipo `lol_perks_page`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LcuRunePage {
    /// Nombre de la página: siempre el título canónico.
    #[serde(default)]
    pub name: String,
    /// Marcarla como página activa del invocador.
    #[serde(default)]
    pub current: bool,
    /// 9 ids en orden: 6 runas (keystone primero) + 3 shards de stats.
    #[serde(default)]
    pub selected_perk_ids: Vec<i64>,
    /// Id del árbol de maestrías principal.
    #[serde(default)]
    pub primary_style_id: i64,
    /// Id del árbol de maestrías secundario.
    #[serde(default)]
    pub sub_style_id: i64,
}

/// Ítem dentro de un bloque de un set. El `id` viaja **como string** (así lo
/// exige el LCU) y `count` es la cantidad a comprar.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LcuItemSetItem {
    /// Cantidad del ítem (siempre 1 en nuestras sugerencias).
    #[serde(default = "uno")]
    pub count: i64,
    /// Id del ítem como string (ej. `"3006"`).
    #[serde(default)]
    pub id: String,
}

/// Valor por defecto de [`LcuItemSetItem::count`] (serde).
fn uno() -> i64 {
    1
}

/// Bloque de compra de un set de ítems (ej. "Arranque", "Botas").
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LcuItemSetBlock {
    /// Sin lógica de ocultamiento por hechizo de invocador.
    #[serde(default)]
    pub hide_if_summoner_spell: bool,
    /// Ítems del bloque, en orden de compra.
    #[serde(default)]
    pub items: Vec<LcuItemSetItem>,
    /// Sin lógica de mostrar por hechizo de invocador.
    #[serde(default)]
    pub show_if_summoner_spell: String,
    /// Etiqueta del bloque visible en el cliente.
    #[serde(rename = "type")]
    pub block_type: String,
}

/// Payload de un set de ítems (`POST {ITEM_SETS_URI}/{champion}/sets`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LcuItemSet {
    /// Campeones asociados: solo el elegido.
    #[serde(default)]
    pub associated_champions: Vec<i64>,
    /// Mapas asociados: solo la Grieta del Invocador ([`MAPA_GRIETA`]).
    #[serde(default)]
    pub associated_maps: Vec<i64>,
    /// Bloques: Arranque, Botas, Núcleo y Situacional.
    #[serde(default)]
    pub blocks: Vec<LcuItemSetBlock>,
    /// Título del set: siempre el título canónico.
    #[serde(default)]
    pub title: String,
    /// Tipo de set: `custom` (creado por el usuario, editable/borrable).
    #[serde(rename = "type")]
    pub set_type: String,
}

// ---------------------------------------------------------------------------
// Helpers puros (testeables sin red).
// ---------------------------------------------------------------------------

/// Título canónico de nuestras páginas y sets: `"Klify · {campeón}"`. Es la
/// llave que el inventario usa para encontrar lo propio y el dedupe para
/// reemplazar lo viejo.
#[must_use]
pub fn nombre_klify(champ_nombre: &str) -> String {
    format!("Klify · {champ_nombre}")
}

/// Payload de la página de runas a partir de la página recomendada
/// normalizada: `selectedPerkIds = perk_ids ++ stat_shards` (9 ids, keystone
/// primero — `perk_ids` ya viene en orden de fila desde las fuentes), más los
/// dos árboles, la marca de activa y el título canónico. Si la fuente trae
/// menos de 3 shards, se completan con [`SHARDS_DEFAULT`]: el LCU exige
/// exactamente 9 ids y un faltante es rechazo seguro (400).
#[must_use]
pub fn payload_pagina_runas(page: &RunePageStats, champ_nombre: &str) -> LcuRunePage {
    let stat_shards: Vec<i64> = if page.stat_shards.len() < 3 {
        let mut completos = page.stat_shards.clone();
        completos.extend_from_slice(&SHARDS_DEFAULT[page.stat_shards.len()..]);
        completos
    } else {
        page.stat_shards.clone()
    };

    let mut selected_perk_ids = Vec::with_capacity(page.perk_ids.len() + stat_shards.len());
    selected_perk_ids.extend_from_slice(&page.perk_ids);
    selected_perk_ids.extend_from_slice(&stat_shards);

    LcuRunePage {
        name: nombre_klify(champ_nombre),
        current: true,
        selected_perk_ids,
        primary_style_id: page.primary_style_id,
        sub_style_id: page.sub_style_id,
    }
}

/// Resultado de intentar crear la página de runas en el cliente.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultadoCreacion {
    /// La página quedó creada en el cliente.
    Creada,
    /// El cliente rechazó la creación porque el inventario de páginas está
    /// lleno (400 "Max pages reached"): corresponde abrir el modal de
    /// elección para que el usuario borre una página.
    SinLugar,
}

/// Chequeos de forma del payload ANTES del POST (puro): el LCU rechaza con
/// 400 las páginas incompletas y, como su respuesta no incluye el motivo
/// por irelia, un payload malformado sería indistinguible del inventario
/// lleno. Fallar acá con mensaje claro evita ese diagnóstico a ciegas.
fn validar_pagina(page: &RunePageStats) -> Result<(), String> {
    if page.perk_ids.len() != 6 {
        return Err(format!(
            "la página recomendada vino incompleta del dataset ({} runas en vez de 6)",
            page.perk_ids.len()
        ));
    }
    if page.primary_style_id == 0 || page.sub_style_id == 0 {
        return Err(String::from("la página recomendada vino con el árbol maestro incompleto (estilo en 0)"));
    }
    Ok(())
}

/// Payload del set de ítems a partir del build recomendado: 4 bloques en
/// orden (Arranque, Botas, Núcleo, Situacional), cada ítem con `count: 1` e
/// id como string, asociado solo al campeón y a la Grieta.
#[must_use]
pub fn payload_item_set(build: &ItemBuild, champion_id: i64, champ_nombre: &str) -> LcuItemSet {
    fn bloque(block_type: &str, stats: &[ItemStat]) -> LcuItemSetBlock {
        LcuItemSetBlock {
            hide_if_summoner_spell: false,
            items: stats
                .iter()
                .map(|stat| LcuItemSetItem {
                    count: 1,
                    id: stat.item_id.to_string(),
                })
                .collect(),
            show_if_summoner_spell: String::new(),
            block_type: block_type.to_string(),
        }
    }

    LcuItemSet {
        associated_champions: vec![champion_id],
        associated_maps: vec![MAPA_GRIETA],
        blocks: vec![
            bloque("Arranque", &build.starters),
            bloque("Botas", &build.boots),
            bloque("Núcleo", &build.core),
            bloque("Situacional", &build.situational),
        ],
        title: nombre_klify(champ_nombre),
        set_type: String::from("custom"),
    }
}

/// ¿El error de irelia corresponde a una operación que **sí** se hizo?
/// `true` solo para errores de decodificación (`Json`/`RmpDecode`), que el
/// cliente reporta tras validar un 2xx con cuerpo no parseable (ej. 204
/// vacío) — el equivalente del `IgnoreAny` que esta versión no trae. Un
/// `Error::Request(status)` (4xx/5xx) o de transporte significa que la
/// operación NO se hizo.
///
/// # Invariante (vale SOLO con el transporte `__hyper`)
///
/// El invariante "`Error::Json`/`Error::RmpDecode` ⇒ ya pasó el chequeo de
/// status" depende del transporte con que irelia 0.11 compila: con `__hyper`
/// el cliente valida el código HTTP **antes** de decodificar el cuerpo, así
/// que un error de decodificación siempre llega tras un 2xx. La variante
/// `__reqwest` **no** chequea el status antes de decodificar: con ese
/// feature set, un cuerpo roto sobre un 4xx/5xx llegaría como
/// `Error::Json` y este helper lo clasificaría como éxito de una operación
/// que falló.
///
/// ⚠️ Si el feature set de irelia cambia (o se actualiza el crate a una
/// versión con otro transporte por defecto), **revisar este helper y su
/// test** antes de volver a confiar en la clasificación.
fn respuesta_ignorable(error: &Error) -> bool {
    matches!(error, Error::Json(_) | Error::RmpDecode(_))
}

/// Inventario interpretado a partir del listado crudo del GET (puro, sin
/// cliente): busca la página con el título canónico. El límite real lo
/// impone el servidor: se detecta al intentar crear
/// ([`ResultadoCreacion::SinLugar`]).
#[must_use]
fn inventario_desde_pages(pages: Vec<LcuPageSummary>, champ_nombre: &str) -> InventarioRunas {
    let titulo = nombre_klify(champ_nombre);
    let nuestra = pages
        .iter()
        .find(|page| page.name == titulo)
        .map(|page| page.id);
    InventarioRunas { pages, nuestra }
}

// ---------------------------------------------------------------------------
// Acciones (async, con el handle global del cliente REST).
// ---------------------------------------------------------------------------

/// summonerId numérico del invocador actual (`GET /lol-summoner/v1/
/// current-summoner`): lo necesita el listado de sets de ítems (la ruta es
/// `/lol-item-sets/v1/item-sets/{summonerId}/sets`; sin summonerId no hay
/// colección). `summonerId` puede llegar como número o string (verificado
/// en models.rs): se parsea tolerante.
async fn summoner_id_actual() -> Result<i64, String> {
    const SUMMONER_URI: &str = "/lol-summoner/v1/current-summoner";

    #[derive(Debug, Deserialize)]
    struct RefSummoner {
        #[serde(rename = "summonerId", default)]
        summoner_id: Option<Value>,
    }

    let client = client_handle().ok_or_else(|| String::from("Cliente no conectado"))?;
    let info: RefSummoner = client
        .get(SUMMONER_URI)
        .await
        .map_err(|error| format!("no se pudo leer el invocador actual del cliente: {error}"))?;

    info.summoner_id
        .as_ref()
        .and_then(|valor| {
            valor
                .as_i64()
                .or_else(|| valor.as_str().and_then(|texto| texto.parse::<i64>().ok()))
        })
        .ok_or_else(|| String::from("el cliente no reportó summonerId para el invocador actual"))
}

/// Uuid con formato canónico (`8-4-4-4-12` hex) para el uid del set propio
/// en PUT de la colección: el listado exige uid no-vacío para poder dedupar
/// después (los sets creados por POST quedan con uid vacío, verificado en
/// vivo). No es un uuid v4 criptográfico: basta unicidad y formato.
#[must_use]
fn uid_aleatorio(cid: i64, titulo: &str) -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hash, Hasher};
    use std::time::{SystemTime, UNIX_EPOCH};

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|intervalo| intervalo.as_nanos())
        .unwrap_or_default();

    let mut completo = String::new();
    for ronda in 0..2_u64 {
        let mut hasher = RandomState::new().build_hasher();
        nanos.hash(&mut hasher);
        cid.hash(&mut hasher);
        titulo.hash(&mut hasher);
        ronda.hash(&mut hasher);
        completo.push_str(&format!("{:016x}", hasher.finish()));
    }
    format!(
        "{}-{}-{}-{}-{}",
        &completo[0..8],
        &completo[8..12],
        &completo[12..16],
        &completo[16..20],
        &completo[20..32]
    )
}

/// Lee el inventario de páginas de runas del invocador y lo interpreta:
/// [`InventarioRunas::nuestra`] trae el id de nuestra página si ya existe.
/// El límite de páginas no se calcula acá: si el servidor rechaza la crea-
/// ción por "Max pages reached", [`crear_pagina_runas`] lo reporta como
/// [`ResultadoCreacion::SinLugar`].
///
/// Errores → `Err` con mensaje en español (incluye `"Cliente no conectado"`
/// si no hay cliente REST publicado).
pub async fn consultar_inventario_runas(champ_nombre: &str) -> Result<InventarioRunas, String> {
    let client = client_handle().ok_or_else(|| String::from("Cliente no conectado"))?;

    let pages: Vec<LcuPageSummary> = match client.get(PAGES_URI).await {
        Ok(pages) => pages,
        Err(error) => {
            eprintln!("klipy [acciones]: no se pudieron leer las páginas de runas: {error}");
            return Err(format!(
                "no se pudieron leer las páginas de runas del cliente: {error}"
            ));
        }
    };

    Ok(inventario_desde_pages(pages, champ_nombre))
}

/// Borra una página de runas por id (`DELETE {PAGES_URI}/{id}`). Un 2xx con
/// cuerpo no parseable (204 vacío) cuenta como éxito
/// ([`respuesta_ignorable`]).
pub async fn borrar_pagina_runas(id: i64) -> Result<(), String> {
    let client = client_handle().ok_or_else(|| String::from("Cliente no conectado"))?;
    let uri = format!("{PAGES_URI}/{id}");

    match client.delete::<Value>(uri).await {
        Ok(_) => {}
        Err(error) if respuesta_ignorable(&error) => {
            eprintln!("klipy [acciones]: respuesta 2xx con cuerpo no parseable ({error})");
        }
        Err(Error::Request(status)) if status == reqwest::StatusCode::FORBIDDEN => {
            eprintln!(
                "klipy [acciones]: la página {id} no es borrable (preset del juego): 403"
            );
            return Err(String::from(
                "esa página es un preset del juego y no se puede borrar",
            ));
        }
        Err(error) => {
            eprintln!(
                "klipy [acciones]: no se pudo borrar la página de runas {id}: {error}"
            );
            return Err(format!(
                "no se pudo borrar la página de runas (id {id}): {error}"
            ));
        }
    }

    eprintln!("klipy [acciones]: página de runas {id} borrada");
    Ok(())
}

/// Crea la página de runas recomendada con el payload de
/// [`payload_pagina_runas`] (`POST {PAGES_URI}`).
///
/// Devuelve [`ResultadoCreacion::SinLugar`] cuando el servidor rechaza la
/// creación por inventario lleno (400, verificado en vivo: el header de la
/// respuesta ya validó la forma del payload, que [`validar_pagina`]); en ese
/// caso el llamador abre el modal de elección. NO gestiona el límite antes:
/// eso asume que el servidor lo dice.
pub async fn crear_pagina_runas(
    page: &RunePageStats,
    champ_nombre: &str,
) -> Result<ResultadoCreacion, String> {
    let client = client_handle().ok_or_else(|| String::from("Cliente no conectado"))?;
    validar_pagina(page)?;
    let payload = payload_pagina_runas(page, champ_nombre);

    match client.post::<LcuRunePage, Value>(PAGES_URI, payload).await {
        Ok(_) => {}
        Err(error) if respuesta_ignorable(&error) => {
            eprintln!("klipy [acciones]: respuesta 2xx con cuerpo no parseable ({error})");
        }
        Err(Error::Request(status)) if status == reqwest::StatusCode::BAD_REQUEST => {
            eprintln!(
                "klipy [acciones]: payload rechazado (POST {PAGES_URI}): {}",
                serde_json::to_string_pretty(&payload_pagina_runas(page, champ_nombre))
                    .unwrap_or_default()
            );
            eprintln!(
                "klipy [acciones]: inventario de páginas de {champ_nombre} lleno (400 max pages): hay que liberar un hueco"
            );
            return Ok(ResultadoCreacion::SinLugar);
        }
        Err(error) => {
            eprintln!(
                "klipy [acciones]: no se pudo crear la página de runas de {champ_nombre}: {error}"
            );
            return Err(format!(
                "no se pudo crear la página de runas de {champ_nombre}: {error}"
            ));
        }
    }

    eprintln!(
        "klipy [acciones]: página '{}' creada para {champ_nombre}",
        nombre_klify(champ_nombre)
    );
    Ok(ResultadoCreacion::Creada)
}

/// Carga el set de ítems recomendado en el cliente.
///
/// Camino principal (verificado en vivo 2026-10-23): la colección del
/// invocador — `GET {ITEM_SETS_URI}/{summonerId}/sets` — con dedupe REAL:
/// se filtra cualquier set previo con el título canónico y este campeón y
/// se agrega el nuestro con uid nuevo; el resultado se escribe con
/// `PUT {...}/{summonerId}/sets` (201). Los sets creados así traen uid, lo
/// que permite deduparlos en cargas futuras (los creados por POST quedan
/// con uid vacío).
///
/// Fallback best-effort: si el summonerId, la lectura de la colección o el
/// PUT falla, se cae al `POST {ITEM_SETS_URI}/{cid}/sets` (204, sin uid y
/// sin dedupe — puede dejar duplicados, tolerable) y se avisa por log.
///
/// Si el build no trae NINGÚN ítem, queda la limpieza: se elimina nuestro
/// set previo si existe y no se agrega nada.
pub async fn cargar_item_set(
    build: &ItemBuild,
    champion_id: i64,
    champ_nombre: &str,
) -> Result<(), String> {
    let client = client_handle().ok_or_else(|| String::from("Cliente no conectado"))?;
    let payload = payload_item_set(build, champion_id, champ_nombre);
    let titulo = nombre_klify(champ_nombre);
    let hay_items = !(build.starters.is_empty()
        && build.boots.is_empty()
        && build.core.is_empty()
        && build.situational.is_empty());

    // 1. Camino principal: colección por summonerId (GET + PUT).
    match summoner_id_actual().await {
        Ok(summoner_id) => {
            let uri = format!("{ITEM_SETS_URI}/{summoner_id}/sets");
            match client.get::<Value>(&uri).await {
                Ok(mut coleccion) => {
                    if coleccion.get("itemSets").and_then(Value::as_array).is_some() {
                        if let Some(item_sets) = coleccion.get_mut("itemSets").and_then(Value::as_array_mut) {
                            item_sets.retain(|item| {
                                let es_titulo = item.get("title").and_then(Value::as_str) == Some(titulo.as_str());
                                let tiene_cid = item
                                    .get("associatedChampions")
                                    .and_then(Value::as_array)
                                    .is_some_and(|lista| lista.contains(&Value::from(champion_id)));
                                !(es_titulo && tiene_cid)
                            });
                            if hay_items {
                                item_sets.push(nuestro_set_value(
                                    build,
                                    champion_id,
                                    &titulo,
                                    uid_aleatorio(champion_id, &titulo),
                                ));
                            }
                        }

                        match client.put::<Value, Value>(&uri, coleccion).await {
                            Ok(_) => {
                                eprintln!(
                                    "klipy [acciones]: set de ítems '{}' {} para {champ_nombre} ({champion_id}) vía colección",
                                    titulo,
                                    if hay_items { "cargado/actualizado" } else { "eliminado" }
                                );
                                return Ok(());
                            }
                            Err(error) if respuesta_ignorable(&error) => {
                                eprintln!(
                                    "klipy [acciones]: respuesta 2xx con cuerpo no parseable ({error})"
                                );
                                return Ok(());
                            }
                            Err(error) => {
                                eprintln!(
                                    "klipy [acciones]: aviso: falló el PUT de la colección ({error}); cae al POST por campeón"
                                );
                            }
                        }
                    } else {
                        eprintln!(
                            "klipy [acciones]: aviso: la colección de sets no trae 'itemSets' lista; cae al POST por campeón"
                        );
                    }
                }
                Err(error) => {
                    eprintln!(
                        "klipy [acciones]: aviso: no se pudo leer la colección de sets ({error}); cae al POST por campeón"
                    );
                }
            }
        }
        Err(error) => {
            eprintln!(
                "klipy [acciones]: aviso: no hay summonerId ({error}); cae al POST por campeón"
            );
        }
    }

    // 2. Fallback: el POST por campeón (siempre verificado 204).
    let uri = format!("{ITEM_SETS_URI}/{champion_id}/sets");
    match client.post::<LcuItemSet, Value>(uri, payload).await {
        Ok(_) => {}
        Err(error) if respuesta_ignorable(&error) => {
            eprintln!("klipy [acciones]: respuesta 2xx con cuerpo no parseable ({error})");
        }
        Err(error) => {
            eprintln!(
                "klipy [acciones]: payload rechazado (POST {ITEM_SETS_URI}/{champion_id}/sets): {}",
                serde_json::to_string_pretty(&payload_item_set(build, champion_id, champ_nombre))
                    .unwrap_or_default()
            );
            eprintln!(
                "klipy [acciones]: no se pudo cargar el set de ítems de {champ_nombre}: {error}"
            );
            return Err(format!(
                "no se pudo cargar el set de ítems de {champ_nombre}: {error}"
            ));
        }
    }

    eprintln!(
        "klipy [acciones]: set de ítems '{titulo}' cargado para {champ_nombre} ({champion_id}) vía POST"
    );
    Ok(())
}

/// Nuestro set de ítems en la forma de colección (útil para el PUT): campos
/// obligatorios del esquema del listado (verificado con los sets de
/// Porofessor en vivo): `map`/`mode`/`startedFrom`/`sortrank`/
/// `preferredItemSlots`/`uid` + título, tipo, campeón, mapa y bloques. Los
/// bloques SIN ítems se descartan (un bloque vacío es un rechazo seguro).
#[must_use]
fn nuestro_set_value(build: &ItemBuild, champion_id: i64, titulo: &str, uid: String) -> Value {
    fn bloque(block_type: &str, stats: &[ItemStat]) -> Value {
        serde_json::json!({
            "type": block_type,
            "items": stats
                .iter()
                .map(|stat| serde_json::json!({"id": stat.item_id.to_string(), "count": 1}))
                .collect::<Vec<Value>>(),
        })
    }

    let bloques = [
        ("Arranque", &build.starters),
        ("Botas", &build.boots),
        ("Núcleo", &build.core),
        ("Situacional", &build.situational),
    ]
    .into_iter()
    .filter(|(_, stats)| !stats.is_empty())
    .map(|(nombre, stats)| bloque(nombre, stats))
    .collect::<Vec<Value>>();

    serde_json::json!({
        "title": titulo,
        "type": "custom",
        "map": "any",
        "mode": "any",
        "startedFrom": "blank",
        "sortrank": 0,
        "preferredItemSlots": [],
        "associatedChampions": [champion_id],
        "associatedMaps": [MAPA_GRIETA],
        "blocks": bloques,
        "uid": uid,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::stats::source::SourceKind;

    /// Página recomendada de prueba: keystone 8008 primero en `perk_ids`
    /// (mismo orden de fila que reportan las fuentes) y 3 shards.
    fn pagina_de_prueba() -> RunePageStats {
        RunePageStats {
            source: SourceKind::Lolalytics,
            label: String::from("Lolalytics · más jugada"),
            games: 559_936,
            wins: 285_903,
            winrate_pct: 51.06,
            primary_style_id: 8000,
            sub_style_id: 8300,
            perk_ids: vec![8008, 8009, 9103, 8014, 8345, 8304],
            stat_shards: vec![5005, 5008, 5001],
            keystone_id: Some(8008),
            estilo: None,
            sintetizada: false,
        }
    }

    /// Ítem con stats de prueba (solo importa el id para el payload).
    fn item_de_prueba(item_id: i64) -> ItemStat {
        ItemStat {
            item_id,
            games: 100,
            wins: 52,
            winrate: 52.0,
        }
    }

    /// Build con las cuatro secciones pobladas.
    fn build_de_prueba() -> ItemBuild {
        ItemBuild {
            starters: vec![
                item_de_prueba(1055),
                item_de_prueba(2003),
                item_de_prueba(3340),
            ],
            boots: vec![item_de_prueba(3006)],
            core: vec![
                item_de_prueba(6672),
                item_de_prueba(3031),
                item_de_prueba(3072),
                item_de_prueba(6673),
                item_de_prueba(3046),
            ],
            situational: vec![item_de_prueba(3153), item_de_prueba(3124)],
        }
    }

    /// El nombre canónico del proyecto, con y sin apóstrofe en el champ.
    #[test]
    fn nombre_klify_arma_el_nombre_canonico() {
        assert_eq!(nombre_klify("Ahri"), "Klify · Ahri");
        assert_eq!(nombre_klify("Kai'Sa"), "Klify · Kai'Sa");
        assert_eq!(nombre_klify(""), "Klify · ");
    }

    /// `payload_pagina_runas`: 9 ids seleccionados, keystone primero y los
    /// shards al final, en el orden de la página; título canónico, activa y
    /// ambos árboles.
    #[test]
    fn payload_pagina_runas_genera_nueve_perks_keystone_primero() {
        let payload = payload_pagina_runas(&pagina_de_prueba(), "Ahri");

        assert_eq!(payload.name, "Klify · Ahri");
        assert!(payload.current);
        assert_eq!(payload.primary_style_id, 8000);
        assert_eq!(payload.sub_style_id, 8300);

        assert_eq!(payload.selected_perk_ids.len(), 9);
        // Keystone primero: la primera runa del árbol principal.
        assert_eq!(payload.selected_perk_ids[0], 8008);
        // Las 6 runas de la página, en orden, seguidas por los 3 shards.
        assert_eq!(
            payload.selected_perk_ids,
            vec![8008, 8009, 9103, 8014, 8345, 8304, 5005, 5008, 5001]
        );
        assert_eq!(&payload.selected_perk_ids[6..], &[5005, 5008, 5001]);
    }

    /// La página serializa con las claves camelCase que espera el LCU.
    #[test]
    fn payload_pagina_runas_serializa_camel_case() {
        let payload = payload_pagina_runas(&pagina_de_prueba(), "Ahri");
        let texto = serde_json::to_string(&payload).expect("serializa");

        assert!(texto.contains("\"name\":\"Klify · Ahri\""));
        assert!(texto.contains("\"current\":true"));
        assert!(texto.contains("\"selectedPerkIds\":[8008,8009,9103,8014,8345,8304,5005,5008,5001]"));
        assert!(texto.contains("\"primaryStyleId\":8000"));
        assert!(texto.contains("\"subStyleId\":8300"));
    }

    /// `payload_item_set`: 4 bloques en orden fijo, ids como string con
    /// `count: 1`, asociado solo al campeón y a la Grieta (mapa 11).
    #[test]
    fn payload_item_set_arma_cuatro_bloques_con_ids_como_string() {
        let payload = payload_item_set(&build_de_prueba(), 22, "Jinx");

        assert_eq!(payload.title, "Klify · Jinx");
        assert_eq!(payload.set_type, "custom");
        assert_eq!(payload.associated_champions, vec![22]);
        assert_eq!(payload.associated_maps, vec![MAPA_GRIETA]);

        let tipos: Vec<&str> = payload
            .blocks
            .iter()
            .map(|bloque| bloque.block_type.as_str())
            .collect();
        assert_eq!(tipos, ["Arranque", "Botas", "Núcleo", "Situacional"]);

        for bloque in &payload.blocks {
            assert!(!bloque.hide_if_summoner_spell);
            assert_eq!(bloque.show_if_summoner_spell, "");
            for item in &bloque.items {
                assert_eq!(item.count, 1);
                // El id SIEMPRE viaja como string numérico, como exige el LCU.
                assert!(
                    item.id.parse::<i64>().is_ok(),
                    "id no numérico: {}",
                    item.id
                );
            }
        }

        // Cantidades por bloque de la fixture y orden de compra intacto.
        assert_eq!(payload.blocks[0].items.len(), 3);
        assert_eq!(payload.blocks[1].items.len(), 1);
        assert_eq!(payload.blocks[2].items.len(), 5);
        assert_eq!(payload.blocks[3].items.len(), 2);
        assert_eq!(payload.blocks[0].items[0].id, "1055");
    }

    /// El set serializa con las claves camelCase del LCU, incluidos los
    /// `type` (de set y de bloque) y los ítems `{count, id}`.
    #[test]
    fn payload_item_set_serializa_camel_case() {
        let payload = payload_item_set(&build_de_prueba(), 22, "Jinx");
        let texto = serde_json::to_string(&payload).expect("serializa");

        assert!(texto.contains("\"title\":\"Klify · Jinx\""));
        assert!(texto.contains("\"type\":\"custom\""));
        assert!(texto.contains("\"associatedChampions\":[22]"));
        assert!(texto.contains("\"associatedMaps\":[11]"));
        assert!(texto.contains("\"type\":\"Arranque\""));
        assert!(texto.contains("\"hideIfSummonerSpell\":false"));
        assert!(texto.contains("\"showIfSummonerSpell\":\"\""));
        // El ítem de la bota de la fixture: id string, count 1.
        assert!(texto.contains("\"count\":1,\"id\":\"3006\""));
    }

    /// `respuesta_ignorable`: error de decodificación tras 2xx = éxito
    /// (cuerpo vacío/204); error de estado (404, y también un 5xx como el
    /// 502) = la operación NO se hizo. (`Error::Request` se construye con el
    /// `StatusCode` re-exportado por reqwest, mismo tipo
    /// `http::StatusCode` de irelia.)
    #[test]
    fn respuesta_ignorable_clasifica_decode_vs_estado() {
        let error_json = serde_json::from_str::<Value>("{").expect_err("JSON inválido");
        assert!(respuesta_ignorable(&Error::Json(error_json)));

        let status = Error::Request(reqwest::StatusCode::NOT_FOUND);
        assert!(!respuesta_ignorable(&status));

        // Un 5xx (502 Bad Gateway) tampoco es ignorable: la operación falló.
        let servidor = Error::Request(reqwest::StatusCode::BAD_GATEWAY);
        assert!(!respuesta_ignorable(&servidor));
    }

    /// `inventario_desde_pages`: encuentra nuestra página por título
    /// canónico; el límite ya NO se calcula acá (lo impone el servidor).
    #[test]
    fn inventario_marca_nuestra() {
        let titulo = nombre_klify("Ahri");

        // Con página propia: `nuestra` trae el id, pase lo que pase con la
        // cantidad (el límite real lo dice el servidor).
        let propias = vec![
            LcuPageSummary {
                id: 1,
                name: String::from("Otra página"),
                is_deletable: true,
            },
            LcuPageSummary {
                id: 2,
                name: titulo.clone(),
                is_deletable: true,
            },
        ];
        let inventario = inventario_desde_pages(propias.clone(), "Ahri");
        assert_eq!(inventario.nuestra, Some(2));
        assert_eq!(inventario.pages, propias);

        // Sin la nuestra: `nuestra` None, sin importar cuántas páginas haya
        // (4 en esta cuenta ya era lleno para el LCU, 2026-10-23).
        let llenas: Vec<LcuPageSummary> = (0..4)
            .map(|i| LcuPageSummary {
                id: i as i64,
                name: format!("página {i}"),
                is_deletable: true,
            })
            .collect();
        let inventario = inventario_desde_pages(llenas, "Ahri");
        assert_eq!(inventario.nuestra, None);
    }

    /// `payload_pagina_runas` completa los shards faltantes con
    /// [`SHARDS_DEFAULT`]: la creación exige exactamente 9 ids y una página
    /// con menos es rechazo seguro del LCU.
    #[test]
    fn payload_pagina_runas_rellena_shards_faltantes() {
        let mut page = pagina_de_prueba();
        page.stat_shards = vec![5005]; // faltan 2
        let payload = payload_pagina_runas(&page, "Akali");

        assert_eq!(
            payload.selected_perk_ids,
            vec![8008, 8009, 9103, 8014, 8345, 8304, 5005, 5008, 5001]
        );
    }

    /// `validar_pagina` rechaza ANTES del POST los payloads malformados:
    /// runas != 6 (el LCU respondería 400 sin explicar por qué) y árboles
    /// en 0.
    #[test]
    fn validar_pagina_rechaza_las_malformadas() {
        let ok = pagina_de_prueba();
        assert_eq!(validar_pagina(&ok), Ok(()));

        let mut corta = ok.clone();
        corta.perk_ids = vec![8008, 8009, 9103];
        assert!(validar_pagina(&corta).is_err());

        let mut sin_arbol = ok;
        sin_arbol.primary_style_id = 0;
        assert!(validar_pagina(&sin_arbol).is_err());
    }

    /// `uid_aleatorio` formato uuid canónico (8-4-4-4-12 hex) y distinto
    /// entre llamadas (semilla de reloj + hasher aleatorio del proceso).
    #[test]
    fn uid_aleatorio_formato_y_unicidad() {
        let a = uid_aleatorio(84, "Klify · Akali");
        let b = uid_aleatorio(84, "Klify · Akali");
        assert_ne!(a, b);
        assert_eq!(a.len(), 36);
        for (idx, ch) in a.chars().enumerate() {
            if [8, 13, 18, 23].contains(&idx) {
                assert_eq!(ch, '-', "posición {idx} debe ser guion");
            } else {
                assert!(
                    ch.is_ascii_hexdigit(),
                    "posición {idx} debe ser hex: {ch}"
                );
            }
        }
    }

    /// `nuestro_set_value`: forma de colección del PUT (uid, campos
    /// obligatorios) y bloques vacíos descartados.
    #[test]
    fn nuestro_set_value_arma_la_forma_de_coleccion() {
        let mut build = build_de_prueba();
        build.core.clear(); // un bloque vacío no viaja
        let uid = uid_aleatorio(84, "Klify · Akali");
        let set = nuestro_set_value(&build, 84, "Klify · Akali", uid);

        assert_eq!(set.get("title").and_then(Value::as_str), Some("Klify · Akali"));
        assert_eq!(set.get("type").and_then(Value::as_str), Some("custom"));
        assert_eq!(set.get("map").and_then(Value::as_str), Some("any"));
        assert_eq!(set.get("mode").and_then(Value::as_str), Some("any"));
        assert_eq!(
            set.get("startedFrom").and_then(Value::as_str),
            Some("blank")
        );
        assert_eq!(set.get("sortrank").and_then(Value::as_i64), Some(0));
        assert!(
            set.get("uid")
                .and_then(Value::as_str)
                .is_some_and(|uid| uid.len() == 36 && !uid.is_empty()),
            "uid con formato uuid"
        );
        assert_eq!(
            set.get("associatedChampions").and_then(Value::as_array).map(Vec::len),
            Some(1)
        );
        assert_eq!(
            set.get("associatedMaps"),
            Some(&json!([11]))
        );

        let tipos: Vec<&str> = set
            .get("blocks")
            .and_then(Value::as_array)
            .map(|bloques| {
                bloques
                    .iter()
                    .filter_map(|b| b.get("type").and_then(Value::as_str))
                    .collect()
            })
            .unwrap_or_default();
        assert_eq!(tipos, vec!["Arranque", "Botas", "Situacional"]);
    }

    /// `LcuPageSummary` tolerante: payload completo (con campos extra
    /// ignorados) y parcial (`isDeletable`/`name` ausentes → default
    /// conservador: no borrable).
    #[test]
    fn lcu_page_summary_tolera_payloads_parciales() {
        let completo: LcuPageSummary = serde_json::from_value(json!({
            "id": 7,
            "name": "Klify · Ahri",
            "isDeletable": false,
            "current": false,
            "selectedPerkIds": [8008]
        }))
        .expect("completo parsea");
        assert_eq!(
            completo,
            LcuPageSummary {
                id: 7,
                name: String::from("Klify · Ahri"),
                is_deletable: false,
            }
        );

        let parcial: LcuPageSummary = serde_json::from_value(json!({"id": 3})).expect("parcial parsea");
        assert_eq!(parcial.id, 3);
        assert_eq!(parcial.name, "");
        assert!(!parcial.is_deletable);
    }

    /// Roundtrip serde de `LcuItemSet`: un payload serializado vuelve a
    /// parsear idéntico (los DTOs son tolerantes pero conservan su forma).
    #[test]
    fn payload_item_set_hace_roundtrip_serde() {
        let payload = payload_item_set(&build_de_prueba(), 22, "Jinx");
        let texto = serde_json::to_string(&payload).expect("serializa");
        let de_vuelta: LcuItemSet = serde_json::from_str(&texto).expect("deserializa");

        assert_eq!(de_vuelta, payload);
    }
}
