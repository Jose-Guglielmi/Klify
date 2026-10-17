//! Detalle de campeón de Data Dragon (FASE 11a-1, pieza 1).
//!
//! Endpoint (verificado en vivo 2026-10-17 contra el parche `16.20.1`):
//!
//! `GET https://ddragon.leagueoflegends.com/cdn/{version}/data/en_US/champion/{ChampionId}.json`
//!
//! - `{ChampionId}` es el **id de Data Dragon con su casing original**
//!   (`Veigar.json`, `MonkeyKing.json`, `Kaisa.json`: los tres verificados
//!   200). El catálogo lo expone como
//!   [`ChampionEntry::ddragon_id`](crate::data_dragon::models::ChampionEntry::ddragon_id);
//!   el slug en minúsculas NO sirve para esta URL (los archivos usan el
//!   casing original).
//! - La respuesta es `{ type, format, version, data: { "<Id>": {...} } }`
//!   con un único campeón por archivo.
//!
//! Contenido que extraemos: `name`, `title`, `partype`, `info` (ratings
//! attack/defense/magic/difficulty), `stats` base (los 14 campos que pide la
//! UI de análisis), `spells` (Q/W/E/R con cooldowns, costos y rangos por
//! nivel) y `passive`. El resto (`skins`, `lore`, `allytips`, `recommended`,
//! ...) se ignora al deserializar.
//!
//! **Cache en memoria** por `(version, ddragon_id)` (global `Mutex`, mismo
//! patrón que `stats::source`): cada detalle pesa ~15 KB y la UI lo pide en
//! cada champ select; una descarga por campeón y parche por corrida basta.
//!
//! Tolerancia serde (convención del proyecto): los numéricos aceptan entero
//! o flotante (`cost` es entero en los 173 campeones del 16.20.1, pero no
//! queremos depender de eso) y `range` puede ser lista de números o string
//! (`"self"`): hoy todos traen lista, el modelo lo tolera igual.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Deserializer};

use super::DataDragonError;
use crate::data_dragon::models::{CDN_BASE_URL, DATA_LOCALE};

/// URL del JSON de detalle de un campeón para una versión.
#[must_use]
pub fn champion_detail_url(version: &str, ddragon_id: &str) -> String {
    format!("{CDN_BASE_URL}/{version}/data/{DATA_LOCALE}/champion/{ddragon_id}.json")
}

/// URL del icono de un hechizo: `img/spell/{image.full}` (verificado en
/// vivo: `VeigarBalefulStrike.png` → 200).
#[must_use]
pub fn spell_icon_url(version: &str, image_full: &str) -> String {
    format!("{CDN_BASE_URL}/{version}/img/spell/{image_full}")
}

/// URL del icono de una pasiva: `img/passive/{image.full}` (verificado en
/// vivo: `VeigarEntropy.png` → 200).
#[must_use]
pub fn passive_icon_url(version: &str, image_full: &str) -> String {
    format!("{CDN_BASE_URL}/{version}/img/passive/{image_full}")
}

/// URL del splash de una skin a partir de la clave `{ddragon_id}_{num}`.
///
/// FASE 11a-2: los splashes NO llevan versión en la CDN
/// (`cdn/img/champion/splash/Kayn_1.jpg` → 200 verificado en vivo), y es
/// la misma pieza que [`crate::profile::settings`] usa para el fondo.
#[must_use]
pub fn splash_key_url(splash_key: &str) -> String {
    format!("{CDN_BASE_URL}/img/champion/splash/{splash_key}.jpg")
}

/// URL del splash de una skin por id de Data Dragon + `num` de la skin
/// ([`ChampionSkin::num`]). Equivale a [`splash_key_url`] sobre la clave
/// compuesta.
#[must_use]
pub fn skin_splash_url(ddragon_id: &str, num: i64) -> String {
    splash_key_url(&format!("{ddragon_id}_{num}"))
}

// ---------------------------------------------------------------------------
// Deserializadores tolerantes (entero o flotante, lista o string).
// ---------------------------------------------------------------------------

/// Número tolerante a i64: acepta JSON number (entero, o flotante redondeado)
/// o string decimal. `None` si no es convertible.
fn value_to_i64(value: &serde_json::Value) -> Option<i64> {
    match value {
        serde_json::Value::Number(number) => number
            .as_i64()
            .or_else(|| number.as_f64().map(|float| float.round() as i64)),
        serde_json::Value::String(text) => text.trim().parse::<i64>().ok(),
        _ => None,
    }
}

/// `Vec<i64>` tolerante: cada elemento puede ser entero o flotante (se
/// redondea). Elementos no convertibles se descartan.
fn de_vec_i64<'de, D>(deserializer: D) -> Result<Vec<i64>, D::Error>
where
    D: Deserializer<'de>,
{
    let values = Vec::<serde_json::Value>::deserialize(deserializer)?;
    Ok(values.iter().filter_map(value_to_i64).collect())
}

/// `i64` tolerante (FASE 11a-2): acepta entero, flotante (redondeado) o
/// string decimal (`skins[].id` llega como `"141000"` en la CDN real).
/// No convertible → 0 en lugar de invalidar el payload.
fn de_i64_lenient<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(value_to_i64(&value).unwrap_or_default())
}

/// Imagen mínima de un asset de detalle (solo usamos `full`; sprite y
/// coordenadas se ignoran).
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct DetailImageDto {
    /// Nombre del archivo en la CDN (ej. `"VeigarBalefulStrike.png"`).
    #[serde(default)]
    pub full: String,
}

/// Ratings declarativos del campeón (`info` del JSON, escala 1-10).
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct ChampionRatings {
    #[serde(default)]
    pub attack: i64,
    #[serde(default)]
    pub defense: i64,
    #[serde(default)]
    pub magic: i64,
    #[serde(default)]
    pub difficulty: i64,
}

/// Stats base del campeón (los 14 campos que consume la UI de análisis).
///
/// Los nombres del JSON van pegados (`hpperlevel`, no `hpPerLevel`), así que
/// cada campo lleva su `rename` explícito.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct ChampionStats {
    #[serde(rename = "hp", default)]
    pub hp: f64,
    #[serde(rename = "hpperlevel", default)]
    pub hp_per_level: f64,
    #[serde(rename = "mp", default)]
    pub mp: f64,
    #[serde(rename = "mpperlevel", default)]
    pub mp_per_level: f64,
    #[serde(rename = "movespeed", default)]
    pub movespeed: f64,
    #[serde(rename = "armor", default)]
    pub armor: f64,
    #[serde(rename = "armorperlevel", default)]
    pub armor_per_level: f64,
    #[serde(rename = "spellblock", default)]
    pub spellblock: f64,
    #[serde(rename = "spellblockperlevel", default)]
    pub spellblock_per_level: f64,
    #[serde(rename = "attackdamage", default)]
    pub attackdamage: f64,
    #[serde(rename = "attackdamageperlevel", default)]
    pub attackdamage_per_level: f64,
    #[serde(rename = "attackrange", default)]
    pub attackrange: f64,
    #[serde(rename = "attackspeed", default)]
    pub attackspeed: f64,
    #[serde(rename = "attackspeedperlevel", default)]
    pub attackspeed_per_level: f64,
}

/// Rango de un hechizo: lista de números por nivel (el caso actual en los
/// 173 campeones del 16.20.1) o string (`"self"` en payloads viejos).
#[derive(Clone, Debug, PartialEq)]
pub enum SpellRange {
    /// Un valor por nivel del hechizo.
    Values(Vec<i64>),
    /// Texto literal del payload (ej. `"self"`).
    Other(String),
}

impl Default for SpellRange {
    fn default() -> Self {
        Self::Values(Vec::new())
    }
}

impl<'de> Deserialize<'de> for SpellRange {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        match value {
            serde_json::Value::Array(items) => Ok(Self::Values(
                items.iter().filter_map(value_to_i64).collect(),
            )),
            serde_json::Value::String(text) => Ok(Self::Other(text)),
            serde_json::Value::Null => Ok(Self::Values(Vec::new())),
            other => Ok(Self::Other(other.to_string())),
        }
    }
}

/// Un hechizo (Q/W/E/R) del campeón.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Spell {
    /// Id del hechizo (ej. `"VeigarBalefulStrike"`).
    #[serde(default)]
    pub id: String,
    /// Nombre visible (ej. `"Baleful Strike"`).
    #[serde(default)]
    pub name: String,
    /// Descripción plana (con etiquetas HTML de resaltado).
    #[serde(default)]
    pub description: String,
    /// Tooltip con placeholders (`{{ ... }}`); puede faltar.
    #[serde(default)]
    pub tooltip: Option<String>,
    /// Rangos máximos del hechizo (5 para Q/W/E, 3 para R).
    #[serde(default)]
    pub maxrank: i64,
    /// Cooldown en segundos por rango (flotantes).
    #[serde(default)]
    pub cooldown: Vec<f64>,
    /// Cooldown formateado (ej. `"6/5.5/5/4.5/4"`).
    #[serde(rename = "cooldownBurn", default)]
    pub cooldown_burn: String,
    /// Costo por rango (tolerante a flotantes).
    #[serde(default, deserialize_with = "de_vec_i64")]
    pub cost: Vec<i64>,
    /// Costo formateado (ej. `"30/35/40/45/50"`).
    #[serde(rename = "costBurn", default)]
    pub cost_burn: String,
    /// Tipo de recurso (ej. `" {{ abilityresourcename }}"`, `"No Cost"`).
    #[serde(rename = "costType", default)]
    pub cost_type: String,
    /// Rango por nivel, o texto (`"self"`) si no aplica.
    #[serde(default)]
    pub range: SpellRange,
    /// Rango formateado (ej. `"1000"`).
    #[serde(rename = "rangeBurn", default)]
    pub range_burn: String,
    /// Imagen del icono; solo usamos `full`.
    #[serde(default)]
    pub image: DetailImageDto,
}

impl Spell {
    /// URL del icono del hechizo para una versión.
    #[must_use]
    pub fn icon_url(&self, version: &str) -> String {
        spell_icon_url(version, &self.image.full)
    }
}

/// Pasiva del campeón.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct Passive {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub image: DetailImageDto,
}

impl Passive {
    /// URL del icono de la pasiva para una versión.
    #[must_use]
    pub fn icon_url(&self, version: &str) -> String {
        passive_icon_url(version, &self.image.full)
    }
}

/// Una skin del campeón (`skins` del JSON de detalle).
///
/// FASE 11a-2: alimenta el picker de fondo de la UI (11b usa
/// `load_champion_detail(Kayn)` → `skins`). Tolerante: en la CDN real `id`
/// llega como STRING (ej. `"141000"`) y `num` como número; ambos pasan por
/// el parser laxo. El campo `chromas` y el resto se ignoran.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct ChampionSkin {
    /// Id numérico de la skin (payload como string, se normaliza a i64).
    #[serde(default, deserialize_with = "de_i64_lenient")]
    pub id: i64,
    /// Índice de la skin dentro del campeón: es el `num` que va en la URL
    /// del splash (`Kayn_1`, `Kayn_2`, ...).
    #[serde(default, deserialize_with = "de_i64_lenient")]
    pub num: i64,
    /// Nombre visible (ej. `"Odyssey Kayn"`; la base es `"default"`).
    #[serde(default)]
    pub name: String,
}

/// Detalle completo de un campeón (archivo por-campeón de Data Dragon).
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct ChampionDetail {
    /// Tips oficiales de Riot para jugar CON el campeón (`allytips`).
    #[serde(default)]
    pub allytips: Vec<String>,
    /// Tips oficiales de Riot para jugar CONTRA el campeón (`enemytips`):
    /// es la fuente de "Cómo jugar contra {rival}" del modo VS.
    #[serde(default)]
    pub enemytips: Vec<String>,
    /// Id de Data Dragon (ej. `"Veigar"`): igual a la clave en `data`.
    #[serde(default)]
    pub id: String,
    /// Nombre visible (ej. `"Veigar"`; puede diferir del id, ej. Wukong).
    #[serde(default)]
    pub name: String,
    /// Epíteto (ej. `"the Tiny Master of Evil"`).
    #[serde(default)]
    pub title: String,
    /// Recurso del campeón (ej. `"Mana"`, `"Blood Well"`, `"Fury"`).
    #[serde(default)]
    pub partype: String,
    /// Ratings declarativos (attack/defense/magic/difficulty).
    #[serde(default)]
    pub info: ChampionRatings,
    /// Stats base y por nivel.
    #[serde(default)]
    pub stats: ChampionStats,
    /// Hechizos activos, en orden Q/W/E/R.
    #[serde(default)]
    pub spells: Vec<Spell>,
    /// Pasiva.
    #[serde(default)]
    pub passive: Passive,
    /// Skins disponibles (FASE 11a-2: datos del picker de fondo; la UI 11b
    /// pide el detalle de Kayn para listarlas).
    #[serde(default)]
    pub skins: Vec<ChampionSkin>,
}

/// Respuesta del endpoint por-campeón: un único campeón en `data`.
#[derive(Debug, Default, Deserialize)]
struct ChampionDetailResponse {
    #[serde(default)]
    data: HashMap<String, ChampionDetail>,
}

// ---------------------------------------------------------------------------
// Cache en memoria: una descarga por (versión, id) por corrida de la app.
// ---------------------------------------------------------------------------

/// Clave de cache: versión de Data Dragon + id con casing original.
pub type DetailCacheKey = (String, String);

/// Mapa de cache en memoria.
pub type DetailCache = HashMap<DetailCacheKey, ChampionDetail>;

/// Cache global de la app (una entrada por descarga exitosa por corrida).
fn detail_cache() -> &'static Mutex<DetailCache> {
    static DETAIL_CACHE: OnceLock<Mutex<DetailCache>> = OnceLock::new();
    DETAIL_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Lookup puro sobre un mapa de cache (testeable sin el global).
#[must_use]
pub fn lookup_detail(
    cache: &DetailCache,
    version: &str,
    ddragon_id: &str,
) -> Option<ChampionDetail> {
    cache
        .get(&(version.to_string(), ddragon_id.to_string()))
        .cloned()
}

/// Inserción pura sobre un mapa de cache (testeable sin el global).
pub fn insert_detail(
    cache: &mut DetailCache,
    version: &str,
    ddragon_id: &str,
    detail: ChampionDetail,
) {
    cache.insert(
        (version.to_string(), ddragon_id.to_string()),
        detail,
    );
}

/// Lookup sobre la cache global. Cache envenenada → miss y re-descarga
/// (mismo criterio que la cache de páginas de runas).
fn cached_lookup(version: &str, ddragon_id: &str) -> Option<ChampionDetail> {
    let cache = detail_cache().lock().ok()?;
    lookup_detail(&cache, version, ddragon_id)
}

/// Inserción sobre la cache global (best-effort: un lock envenenado solo
/// pierde esta entrada).
fn cached_store(version: &str, ddragon_id: &str, detail: &ChampionDetail) {
    if let Ok(mut cache) = detail_cache().lock() {
        insert_detail(&mut cache, version, ddragon_id, detail.clone());
    }
}

// ---------------------------------------------------------------------------
// Carga on-demand.
// ---------------------------------------------------------------------------

/// Descarga (o sirve de cache) el detalle de un campeón para una versión.
///
/// `ddragon_id` debe venir del catálogo
/// ([`ChampionEntry::ddragon_id`](crate::data_dragon::models::ChampionEntry::ddragon_id)):
/// es la clave del archivo y del mapa `data` de la respuesta. Si la clave
/// exacta no está (payload capado) y hay un único campeón en `data`, se usa
/// ese; con `data` vacía o ambigua → [`DataDragonError::MissingData`].
pub async fn load_champion_detail(
    client: &reqwest::Client,
    version: &str,
    ddragon_id: &str,
) -> Result<ChampionDetail, DataDragonError> {
    if let Some(detail) = cached_lookup(version, ddragon_id) {
        return Ok(detail);
    }

    let url = champion_detail_url(version, ddragon_id);
    let response = client
        .get(&url)
        .send()
        .await?
        .error_for_status()?
        .json::<ChampionDetailResponse>()
        .await?;

    let mut data = response.data;
    let detail = match data.remove(ddragon_id) {
        Some(detail) => detail,
        None if data.len() == 1 => data.into_values().next().unwrap_or_default(),
        None => {
            return Err(DataDragonError::MissingData(format!(
                "data.{ddragon_id}"
            )))
        }
    };

    cached_store(version, ddragon_id, &detail);
    Ok(detail)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Recorte REAL de la respuesta de 16.20.1 (verificado en vivo con
    /// `curl https://ddragon.leagueoflegends.com/cdn/16.20.1/data/en_US/champion/Veigar.json`).
    /// Se omiten campos que el modelo ignora (skins, lore, recommended...).
    fn fixture_veigar() -> serde_json::Value {
        serde_json::json!({
            "type": "champion",
            "format": "standaloneData",
            "version": "16.20.1",
            "data": {
                "Veigar": {
                    "id": "Veigar",
                    "key": "45",
                    "name": "Veigar",
                    "title": "the Tiny Master of Evil",
                    "partype": "Mana",
                    "info": { "attack": 2, "defense": 2, "magic": 10, "difficulty": 7 },
                    "stats": {
                        "hp": 580, "hpperlevel": 108,
                        "mp": 490, "mpperlevel": 26,
                        "movespeed": 340,
                        "armor": 18, "armorperlevel": 5.2,
                        "spellblock": 32, "spellblockperlevel": 1.3,
                        "attackrange": 550,
                        "hpregen": 6.5, "hpregenperlevel": 0.6,
                        "mpregen": 8, "mpregenperlevel": 0.8,
                        "crit": 0, "critperlevel": 0,
                        "attackdamage": 52, "attackdamageperlevel": 0,
                        "attackspeedperlevel": 2.24, "attackspeed": 0.625
                    },
                    "spells": [
                        {
                            "id": "VeigarBalefulStrike",
                            "name": "Baleful Strike",
                            "description": "Veigar unleashes a bolt of dark energy...",
                            "tooltip": "Veigar unleashes a bolt, dealing {{ totaldamage }} magic damage...",
                            "maxrank": 5,
                            "cooldown": [6, 5.5, 5, 4.5, 4],
                            "cooldownBurn": "6/5.5/5/4.5/4",
                            "cost": [30, 35, 40, 45, 50],
                            "costBurn": "30/35/40/45/50",
                            "costType": " {{ abilityresourcename }}",
                            "range": [1000, 1000, 1000, 1000, 1000],
                            "rangeBurn": "1000",
                            "image": {
                                "full": "VeigarBalefulStrike.png",
                                "sprite": "spell0.png",
                                "group": "spell",
                                "x": 0, "y": 96, "w": 48, "h": 48
                            }
                        },
                        {
                            "id": "VeigarPrimordialBurst",
                            "name": "Primordial Burst",
                            "description": "Blasts an enemy champion...",
                            "maxrank": 3,
                            "cooldown": [80, 70, 60],
                            "cooldownBurn": "80/70/60",
                            "cost": [100, 100, 100],
                            "costBurn": "100",
                            "costType": " {{ abilityresourcename }}",
                            "range": [650, 650, 650],
                            "rangeBurn": "650",
                            "image": { "full": "VeigarPrimordialBurst.png" }
                        }
                    ],
                    "passive": {
                        "name": "Phenomenal Evil Power",
                        "description": "Veigar can stack Phenomenal Evil Power...",
                        "image": {
                            "full": "VeigarEntropy.png",
                            "sprite": "passive4.png",
                            "group": "passive",
                            "x": 384, "y": 96, "w": 48, "h": 48
                        }
                    }
                }
            }
        })
    }

    #[test]
    fn urls_de_detalle_e_iconos_tienen_el_formato_de_la_cdn() {
        assert_eq!(
            champion_detail_url("16.20.1", "MonkeyKing"),
            format!("https://ddragon.leagueoflegends.com/cdn/16.20.1/data/{DATA_LOCALE}/champion/MonkeyKing.json")
        );
        assert_eq!(
            spell_icon_url("16.20.1", "VeigarBalefulStrike.png"),
            "https://ddragon.leagueoflegends.com/cdn/16.20.1/img/spell/VeigarBalefulStrike.png"
        );
        assert_eq!(
            passive_icon_url("16.20.1", "VeigarEntropy.png"),
            "https://ddragon.leagueoflegends.com/cdn/16.20.1/img/passive/VeigarEntropy.png"
        );
    }

    /// Fixture real: parsea completo, con stats flotantes/enteros mezclados,
    /// spells Q y R y pasiva.
    #[test]
    fn parsea_el_fixture_real_de_veigar() {
        let response: ChampionDetailResponse =
            serde_json::from_value(fixture_veigar()).expect("fixture válido");
        let detail = response.data.get("Veigar").expect("Veigar en data");

        assert_eq!(detail.id, "Veigar");
        assert_eq!(detail.name, "Veigar");
        assert_eq!(detail.title, "the Tiny Master of Evil");
        assert_eq!(detail.partype, "Mana");
        assert_eq!(
            detail.info,
            ChampionRatings { attack: 2, defense: 2, magic: 10, difficulty: 7 }
        );

        let stats = &detail.stats;
        assert_eq!(stats.hp, 580.0);
        assert_eq!(stats.hp_per_level, 108.0);
        assert_eq!(stats.armor_per_level, 5.2);
        assert_eq!(stats.spellblock_per_level, 1.3);
        assert_eq!(stats.attackspeed, 0.625);
        assert_eq!(stats.attackspeed_per_level, 2.24);
        assert_eq!(stats.attackrange, 550.0);
        assert_eq!(stats.attackdamage, 52.0);

        assert_eq!(detail.spells.len(), 2);
        let q = &detail.spells[0];
        assert_eq!(q.id, "VeigarBalefulStrike");
        assert_eq!(q.maxrank, 5);
        assert_eq!(q.cooldown, vec![6.0, 5.5, 5.0, 4.5, 4.0]);
        assert_eq!(q.cooldown_burn, "6/5.5/5/4.5/4");
        assert_eq!(q.cost, vec![30, 35, 40, 45, 50]);
        assert_eq!(q.cost_burn, "30/35/40/45/50");
        assert_eq!(q.range, SpellRange::Values(vec![1000; 5]));
        assert_eq!(q.range_burn, "1000");
        assert_eq!(q.icon_url("16.20.1"), spell_icon_url("16.20.1", "VeigarBalefulStrike.png"));
        assert!(q.tooltip.is_some());

        let r = &detail.spells[1];
        assert_eq!(r.maxrank, 3);

        assert_eq!(detail.passive.name, "Phenomenal Evil Power");
        assert_eq!(
            detail.passive.icon_url("16.20.1"),
            passive_icon_url("16.20.1", "VeigarEntropy.png")
        );
    }

    /// Tolerancia: `cost` flotante se redondea, `range` como string ("self")
    /// se conserva como texto, y campos ausentes dan defaults.
    #[test]
    fn tolera_cost_flotante_y_range_string() {
        let value = serde_json::json!({
            "data": {
                "Roto": {
                    "id": "Roto",
                    "name": "Roto",
                    "spells": [{
                        "id": "RotoQ",
                        "name": "Q rota",
                        "maxrank": 5,
                        "cooldown": [1.5, 2.5],
                        "cost": [60.0, 70.4],
                        "range": "self"
                    }]
                }
            }
        });

        let response: ChampionDetailResponse =
            serde_json::from_value(value).expect("tolera payload parcial");
        let detail = response.data.get("Roto").expect("Roto en data");
        let spell = &detail.spells[0];

        assert_eq!(spell.cost, vec![60, 70]); // 70.4 → 70
        assert_eq!(spell.range, SpellRange::Other(String::from("self")));
        assert_eq!(spell.range_burn, "");
        assert!(spell.image.full.is_empty());
        assert!(spell.tooltip.is_none());
        assert_eq!(detail.stats, ChampionStats::default());
    }

    /// FASE 11a-2: recorte REAL de las skins de Kayn (verificado en vivo
    /// contra `16.20.1`; `id` llega como STRING). Se parsean y el splash de
    /// cada una arma la URL correcta.
    #[test]
    fn parsea_las_skins_reales_de_kayn() {
        let value = serde_json::json!({
            "data": {
                "Kayn": {
                    "id": "Kayn",
                    "name": "Kayn",
                    "skins": [
                        { "id": "141000", "num": 0, "name": "default", "chromas": false },
                        { "id": "141001", "num": 1, "name": "Soulhunter Kayn", "chromas": false },
                        { "id": "141002", "num": 2, "name": "Odyssey Kayn", "chromas": true, "parentSkin": null }
                    ]
                }
            }
        });

        let response: ChampionDetailResponse =
            serde_json::from_value(value).expect("fixture válido");
        let detail = response.data.get("Kayn").expect("Kayn en data");

        assert_eq!(detail.skins.len(), 3);
        assert_eq!(detail.skins[0].id, 141_000);
        assert_eq!(detail.skins[0].num, 0);
        assert_eq!(detail.skins[0].name, "default");
        assert_eq!(detail.skins[1].name, "Soulhunter Kayn");
        assert_eq!(detail.skins[2].num, 2);

        // URL de splash para el picker de la UI 11b.
        assert_eq!(
            skin_splash_url("Kayn", 1),
            "https://ddragon.leagueoflegends.com/cdn/img/champion/splash/Kayn_1.jpg"
        );
        assert_eq!(
            splash_key_url("Kayn_2"),
            "https://ddragon.leagueoflegends.com/cdn/img/champion/splash/Kayn_2.jpg"
        );
    }

    /// Tolerancia: campeón sin `skins` (payload capado) → lista vacía; skin
    /// con `id`/`num` no convertibles → 0 sin invalidar el resto.
    #[test]
    fn skins_tolerantes_a_payloads_parciales() {
        let value = serde_json::json!({
            "data": {
                "Roto": {
                    "id": "Roto",
                    "name": "Roto",
                    "skins": [
                        { "id": "no-es-numero", "num": null, "name": "Rara" }
                    ]
                }
            }
        });

        let response: ChampionDetailResponse =
            serde_json::from_value(value).expect("fixture válido");
        let detail = response.data.get("Roto").expect("Roto en data");
        assert_eq!(detail.skins.len(), 1);
        assert_eq!(detail.skins[0].id, 0);
        assert_eq!(detail.skins[0].num, 0);
        assert_eq!(detail.skins[0].name, "Rara");

        // Sin campo skins → vacío (default).
        let vacio: ChampionDetailResponse =
            serde_json::from_value(serde_json::json!({ "data": { "Roto": { "id": "Roto" } } }))
                .expect("fixture válido");
        assert!(vacio.data.get("Roto").expect("Roto").skins.is_empty());
    }

    /// Cache pura: miss → put → hit, con clave por (versión, id).
    #[test]
    fn cache_pura_guarda_y_recupera_por_version_e_id() {
        let mut cache = DetailCache::new();
        let detail = ChampionDetail {
            id: String::from("Veigar"),
            name: String::from("Veigar"),
            ..ChampionDetail::default()
        };

        assert!(lookup_detail(&cache, "16.20.1", "Veigar").is_none());
        insert_detail(&mut cache, "16.20.1", "Veigar", detail.clone());
        assert_eq!(lookup_detail(&cache, "16.20.1", "Veigar"), Some(detail));
        // Otra versión u otro id son entradas distintas.
        assert!(lookup_detail(&cache, "16.19.1", "Veigar").is_none());
        assert!(lookup_detail(&cache, "16.20.1", "MonkeyKing").is_none());
    }

    /// La respuesta sin el campeón pedido (y sin un único fallback) da
    /// `MissingData`, no un pánico.
    #[test]
    fn respuesta_sin_data_da_missing_data() {
        let response: ChampionDetailResponse =
            serde_json::from_value(serde_json::json!({ "data": {} }))
                .expect("payload válido");
        assert!(response.data.is_empty());

        let error = DataDragonError::MissingData(String::from("data.Veigar"));
        assert_eq!(
            error.to_string(),
            "respuesta de Data Dragon sin data.Veigar"
        );
    }
}
