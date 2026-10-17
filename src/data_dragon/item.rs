//! Catálogo de ítems de Data Dragon (FASE 11b-2b).
//!
//! Endpoint (verificado en vivo 2026-10-17 contra el parche `16.20.1`):
//!
//! `GET https://ddragon.leagueoflegends.com/cdn/{version}/data/{DATA_LOCALE}/item.json`
//!
//! La respuesta es `{ type, version, basic, data: { "<id>": {...} }, groups,
//! tree }` con ~870 ítems indexados por id-string. De cada ítem solo nos
//! interesan `name` y una descripción corta para la tab ITEMS del champ
//! select.
//!
//! **Fuente de la descripción** (verificada en vivo): el payload 16.20.1 NO
//! trae campo `blurb` (fue removido de la CDN); el corto útil es
//! `plaintext` (frase limpia, sin HTML) y solo lo traen 172/870 ítems. El
//! resto solo tiene `description`, que es HTML de tooltip con stats. La
//! cadena de fallback es `plaintext` → `blurb` (defensiva, por si una
//! versión la restaura) → `description` crudo: la limpieza de HTML la hace
//! la UI con [`crate::ui::format::strip_html`] al renderizar (mismo
//! criterio que `RuneCatalog`, que guarda `short_desc` sin limpiar).
//!
//! **Cache en memoria** por versión (global `Mutex`, mismo patrón que
//! `champion_detail`): el JSON completo pesa ~200 KB y la UI lo consulta en
//! cada champ select; una descarga por versión por corrida basta.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

use super::DataDragonError;
use crate::data_dragon::models::{CDN_BASE_URL, DATA_LOCALE};

/// URL del JSON de ítems para una versión.
#[must_use]
pub fn item_json_url(version: &str) -> String {
    format!("{CDN_BASE_URL}/{version}/data/{DATA_LOCALE}/item.json")
}

/// Entrada del catálogo: lo mínimo que muestran las tabs ITEMS y COUNTERS.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemInfo {
    /// Nombre visible (ej. `"Zhonya's Hourglass"`).
    #[serde(default)]
    pub name: String,
    /// Descripción corta según la cadena `plaintext` → `blurb` →
    /// `description` (ver la doc del módulo). Cruda: la UI limpia el HTML.
    #[serde(default)]
    pub blurb: String,
}

/// Catálogo completo de ítems para una versión de Data Dragon.
///
/// Pensado para vivir en una señal `Signal<Option<Arc<ItemCatalog>>>`
/// (viaja en `Arc` porque pesa ~200 KB y la UI lo clona por render).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemCatalog {
    /// Versión de Data Dragon de la que proviene (ej. `"16.20.1"`).
    pub version: String,
    /// Índice por id numérico (la clave del JSON parseada).
    pub by_id: HashMap<i64, ItemInfo>,
}

impl ItemCatalog {
    /// Entrada del catálogo para un id, si existe.
    #[must_use]
    pub fn get(&self, item_id: i64) -> Option<&ItemInfo> {
        self.by_id.get(&item_id)
    }
}

/// Nombre visible de un ítem: catálogo primero, `#id` como degradación
/// (puro, testeado). También cae a `#id` una entrada con nombre vacío.
#[must_use]
pub fn item_name(catalog: Option<&ItemCatalog>, item_id: i64) -> String {
    catalog
        .and_then(|catalog| catalog.get(item_id))
        .map(|info| info.name.clone())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| format!("#{item_id}"))
}

// ---------------------------------------------------------------------------
// Parseo del payload.
// ---------------------------------------------------------------------------

/// Respuesta cruda de `item.json` (subconjunto tolerante).
#[derive(Debug, Default, Deserialize)]
struct ItemJsonResponse {
    #[serde(default)]
    data: HashMap<String, serde_json::Value>,
}

/// Texto de descripción de un ítem según la cadena de la doc del módulo
/// (puro, testeado). Campos ausentes o vacíos se saltan; vacío total
/// (`""`) si el ítem no trae ninguno.
fn choose_blurb(dto: &serde_json::Value) -> String {
    let field = |key: &str| {
        dto.get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(String::from)
    };
    field("plaintext")
        .or_else(|| field("blurb"))
        .or_else(|| field("description"))
        .unwrap_or_default()
}

/// Nombre crudo de un ítem dentro de su DTO (vacío si no trae).
fn raw_name(dto: &serde_json::Value) -> String {
    dto.get("name")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// Parseo puro del payload (testeado con un recorte real de 16.20.1):
/// indexa `data` por id numérico; las entradas con clave no numérica se
/// ignoran en lugar de invalidar el catálogo. Sin `data` o con `data`
/// vacía → error, así el loader NO cachea un catálogo inútil y reintenta
/// en el próximo ciclo.
pub(crate) fn parse_item_catalog(
    version: String,
    value: serde_json::Value,
) -> Result<ItemCatalog, DataDragonError> {
    // No-objeto (ej. `[]`) con el `#[serde(default)]` de `data` pasaría
    // silenciosamente como "data vacía"; el diagnóstico correcto es
    // "no parseable" (contrato del test `payloads_sin_data`).
    if !value.is_object() {
        return Err(DataDragonError::MissingData(String::from(
            "item.json no parseable: se esperaba un objeto",
        )));
    }
    let response: ItemJsonResponse = serde_json::from_value(value).map_err(|error| {
        DataDragonError::MissingData(format!("item.json no parseable: {error}"))
    })?;
    if response.data.is_empty() {
        return Err(DataDragonError::MissingData(String::from(
            "item.json con data",
        )));
    }

    let by_id = response
        .data
        .iter()
        .filter_map(|(key, dto)| {
            let item_id = key.parse::<i64>().ok()?;
            Some((
                item_id,
                ItemInfo {
                    name: raw_name(dto),
                    blurb: choose_blurb(dto),
                },
            ))
        })
        .collect();

    Ok(ItemCatalog { version, by_id })
}

// ---------------------------------------------------------------------------
// Cache en memoria: una descarga por versión por corrida de la app.
// ---------------------------------------------------------------------------

/// Mapa de cache en memoria (versión → catálogo).
pub type ItemCatalogCache = HashMap<String, ItemCatalog>;

/// Cache global de la app (una entrada por descarga exitosa por corrida).
fn catalog_cache() -> &'static Mutex<ItemCatalogCache> {
    static ITEM_CATALOG_CACHE: OnceLock<Mutex<ItemCatalogCache>> = OnceLock::new();
    ITEM_CATALOG_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Lookup puro sobre un mapa de cache (testeable sin el global).
#[must_use]
pub fn lookup_catalog(cache: &ItemCatalogCache, version: &str) -> Option<ItemCatalog> {
    cache.get(version).cloned()
}

/// Inserción pura sobre un mapa de cache (testeable sin el global).
pub fn insert_catalog(cache: &mut ItemCatalogCache, version: &str, catalog: ItemCatalog) {
    cache.insert(version.to_string(), catalog);
}

/// Lookup sobre la cache global. Cache envenenada → miss y re-descarga
/// (mismo criterio que la cache de detalles de campeón).
fn cached_lookup(version: &str) -> Option<ItemCatalog> {
    let cache = catalog_cache().lock().ok()?;
    lookup_catalog(&cache, version)
}

/// Inserción sobre la cache global (best-effort: un lock envenenado solo
/// pierde esta entrada).
fn cached_store(version: &str, catalog: &ItemCatalog) {
    if let Ok(mut cache) = catalog_cache().lock() {
        insert_catalog(&mut cache, version, catalog.clone());
    }
}

// ---------------------------------------------------------------------------
// Carga.
// ---------------------------------------------------------------------------

/// Descarga (o sirve de cache) el catálogo de ítems de una versión. El
/// loader de la app ([`super::run_item_loader`]) la llama con la versión
/// pineada al parche del cliente.
pub async fn load_item_catalog(
    client: &reqwest::Client,
    version: &str,
) -> Result<ItemCatalog, DataDragonError> {
    if let Some(catalog) = cached_lookup(version) {
        return Ok(catalog);
    }

    let url = item_json_url(version);
    let value = client
        .get(&url)
        .send()
        .await?
        .error_for_status()?
        .json::<serde_json::Value>()
        .await?;

    let catalog = parse_item_catalog(version.to_string(), value)?;
    cached_store(version, &catalog);
    Ok(catalog)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Recorte REAL de la respuesta de 16.20.1 (verificado en vivo con
    /// `curl https://ddragon.leagueoflegends.com/cdn/16.20.1/data/{DATA_LOCALE}/item.json`).
    /// Ojo: NO hay campo `blurb`; 3157 trae `plaintext` y 4645 no (solo
    /// `description` con HTML). `roto` tiene clave no numérica.
    fn fixture() -> serde_json::Value {
        serde_json::json!({
            "type": "item",
            "version": "16.20.1",
            "basic": {},
            "data": {
                "3157": {
                    "name": "Zhonya's Hourglass",
                    "description": "<mainText><stats><attention>105</attention> Ability Power<br><attention>50</attention> Armor</stats><br><br><active>Time Stop</active><br>Enter <keyword>Stasis</keyword> for 2.5 seconds.</mainText>",
                    "plaintext": "Activate to become invincible but unable to take actions",
                    "colloq": "Zhonyas;Hourglass;ZH",
                    "gold": { "base": 3000, "total": 3000, "sell": 2100, "purchasable": true },
                    "tags": ["Ability Power", "Active"],
                    "maps": { "11": true }
                },
                "4645": {
                    "name": "Shadowflame",
                    "description": "<mainText><stats><attention>110</attention> Ability Power<br><attention>15</attention> Magic Penetration</stats><br><br><passive>Cinderbloom</passive><br><magicDamage>Magic</magicDamage> damage is increased up to 20%.</mainText>",
                    "plaintext": "",
                    "gold": { "base": 2900, "total": 2900 }
                },
                "3020": {
                    "name": "Sorcerer's Shoes",
                    "description": "<mainText><stats><attention>12</attention> Magic Penetration<br><attention>45</attention> Move Speed</stats><br><br></mainText>",
                    "plaintext": "Enhances Move Speed and magic damage"
                },
                "roto": { "name": "Roto", "plaintext": "clave sin id numérico" }
            },
            "groups": [],
            "tree": []
        })
    }

    #[test]
    fn url_de_items_tiene_el_formato_de_la_cdn() {
        assert_eq!(
            item_json_url("16.20.1"),
            format!("https://ddragon.leagueoflegends.com/cdn/16.20.1/data/{DATA_LOCALE}/item.json")
        );
    }

    /// Fixture real: indexa por id numérico, ignora la clave no numérica y
    /// elige `plaintext` como descripción corta cuando está.
    #[test]
    fn parsea_el_fixture_real_indexando_por_id() {
        let catalog = parse_item_catalog(String::from("16.20.1"), fixture()).expect("parsea");

        assert_eq!(catalog.version, "16.20.1");
        assert_eq!(catalog.by_id.len(), 3, "la clave no numérica se ignora");

        let zhonyas = catalog.get(3157).expect("Zhonya's");
        assert_eq!(zhonyas.name, "Zhonya's Hourglass");
        assert_eq!(
            zhonyas.blurb,
            "Activate to become invincible but unable to take actions"
        );

        // Sin `plaintext` útil → cae a `description` crudo (la UI limpia).
        let shadowflame = catalog.get(4645).expect("Shadowflame");
        assert_eq!(shadowflame.name, "Shadowflame");
        assert!(shadowflame.blurb.starts_with("<mainText>"));
        assert!(shadowflame.blurb.contains("Cinderbloom"));

        let botas = catalog.get(3020).expect("Sorcerer's Shoes");
        assert_eq!(botas.blurb, "Enhances Move Speed and magic damage");

        // Item desconocido → None, y `item_name` degrada a #id.
        assert!(catalog.get(9999).is_none());
        assert_eq!(item_name(Some(&catalog), 9999), "#9999");
        assert_eq!(item_name(Some(&catalog), 3157), "Zhonya's Hourglass");
        assert_eq!(item_name(None, 3157), "#3157");
    }

    /// Cadena de fallback de la descripción: `plaintext` gana sobre
    /// `blurb`, y `blurb` sobre `description`; vacíos se saltan y un ítem
    /// sin descripción queda vacío (sin inventar texto).
    #[test]
    fn choose_blurb_prefiere_plaintext_luego_blurb_luego_description() {
        let con_todo = serde_json::json!({
            "name": "X",
            "plaintext": "corta y limpia",
            "blurb": "media",
            "description": "<mainText>larga con HTML</mainText>"
        });
        assert_eq!(choose_blurb(&con_todo), "corta y limpia");

        let sin_plaintext = serde_json::json!({
            "name": "Y",
            "blurb": "media",
            "description": "<mainText>larga</mainText>"
        });
        assert_eq!(choose_blurb(&sin_plaintext), "media");

        let solo_description = serde_json::json!({
            "name": "Z",
            "plaintext": "   ",
            "description": "<mainText>larga</mainText>"
        });
        assert_eq!(choose_blurb(&solo_description), "<mainText>larga</mainText>");

        // Ítem sin descripción (payload mínimo).
        let vacio = serde_json::json!({ "name": "W" });
        assert_eq!(choose_blurb(&vacio), "");

        // Nombre presente pasa tal cual; nombre ausente → vacío (la UI
        // cae a #id).
        assert_eq!(raw_name(&vacio), "W");
        let sin_nombre = serde_json::json!({ "plaintext": "algo" });
        assert_eq!(raw_name(&sin_nombre), "");
        let mut catalogo = ItemCatalog::default();
        catalogo.by_id.insert(1, ItemInfo::default());
        assert_eq!(item_name(Some(&catalogo), 1), "#1");
    }

    /// Sin `data` o con `data` vacía → error claro (el loader reintenta y
    /// no cachea un catálogo vacío).
    #[test]
    fn payloads_sin_data_fallan_claro() {
        let sin_data = parse_item_catalog(String::from("16.20.1"), serde_json::json!({}))
            .expect_err("sin data");
        assert!(sin_data.to_string().contains("item.json"));

        let data_vacia = parse_item_catalog(
            String::from("16.20.1"),
            serde_json::json!({ "data": {}, "groups": [] }),
        )
        .expect_err("data vacía");
        assert!(data_vacia.to_string().contains("item.json"));

        // Payload no-objeto también cae en MissingData (no paniquea).
        let no_objeto = parse_item_catalog(String::from("16.20.1"), serde_json::json!([]))
            .expect_err("no objeto");
        assert!(no_objeto.to_string().contains("no parseable"));
    }

    /// Cache pura: miss → put → hit por versión.
    #[test]
    fn cache_pura_guarda_y_pisa_por_version() {
        let mut cache = ItemCatalogCache::new();
        assert!(lookup_catalog(&cache, "16.20.1").is_none());

        let mut by_id = HashMap::new();
        by_id.insert(
            3157,
            ItemInfo {
                name: String::from("Zhonya's Hourglass"),
                blurb: String::from("stasis"),
            },
        );
        let catalog = ItemCatalog {
            version: String::from("16.20.1"),
            by_id,
        };
        insert_catalog(&mut cache, "16.20.1", catalog.clone());
        assert_eq!(lookup_catalog(&cache, "16.20.1"), Some(catalog));

        // Otra versión es otra entrada.
        assert!(lookup_catalog(&cache, "16.19.1").is_none());
    }
}
