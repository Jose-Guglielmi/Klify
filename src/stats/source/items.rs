//! Build de ítems de Lolalytics (FASE 11a-1, pieza 3).
//!
//! Endpoint (mismo mega-endpoint y common params, con `ep=build-itemset`;
//! formato verificado contra league-lean `lib/sources/lolalytics.js` y contra
//! la respuesta real del parche 16.20):
//!
//! `GET https://a1.lolalytics.com/mega/?ep=build-itemset&v=1&patch={patch}
//!    &c={slug}&tier=platinum_plus&queue=ranked&region=all[&lane={lane}]`
//!
//! Recorte REAL (verificado en vivo 2026-10-17 con `c=veigar&lane=middle`):
//!
//! ```json
//! { "itemSets": {
//!     "itemSet1":    [["6657", 3784, 1169], ["6655", 4206, 1328], ...],
//!     "itemSet2":    [["6657_4629", 82, 39], ...],
//!     "itemSet3":    [["6657_4629_3089", 63, 35], ...],
//!     "itemSet4":    [["6657_4629_3089_3157", 9, 4], ...],
//!     "itemSet5":    [["6657_3137_3089_2504_3742", 2, 1],
//!                     ["6655_4646_3089_4645_3157", 775, 443], ...],
//!     "itemBootSet1": [["6657", 842, 192], ["3047", 14, 3], ...],
//!     "response": { ... } } }
//! ```
//!
//! Cada entrada es `["id" | "id1_id2_...", picks, wins]` (picks en [1], wins
//! en [2]; winrate = wins/picks·100). Los ids compuestos describen builds de
//! N ítems: se **expanden** a ids individuales sumando picks/wins (mismo
//! criterio que league-lean).
//!
//! **Ojo, hallazgo de la verificación en vivo**: los arrays llegan SIN
//! ordenar por picks (el `itemSet5[0]` real de Veigar middle tenía 2 picks
//! mientras el máximo era 775). League-lean asume `itemSet5[0]` = build más
//! jugado, pero en el payload actual eso es falso: acá se ordena por picks
//! descendente antes de elegir core y situational (documentado en
//! [`parse_item_build`]).
//!
//! "Sugerido vs comp" ([`suggest_vs_comp`]): heurística de **cruce** entre
//! una tabla constante de enemigos→ítem ([`ANTI_ENEMY_ITEMS`]) y los ítems
//! que el campeón ya juega (core/situational). NO son stats per-matchup:
//! solo se sugiere un ítem anti-X si ese ítem ya es estadísticamente normal
//! en el build del campeón. Sin match → lista vacía y la UI muestra el
//! build normal. Cero datos fabricados.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

use super::{mega_url, SourceError};
use crate::data_dragon::models::item_icon_url;
use crate::lcu::models::Position;

/// URL de `ep=build-itemset` para un campeón/posición/parche.
#[must_use]
pub fn itemset_url(slug: &str, position: Option<Position>, patch: &str) -> String {
    mega_url("build-itemset", slug, position, patch)
}

/// Ids de botas de Grieta del Invocador (cola ranked). `itemBootSet1` trae
/// "ítems jugados junto a botas", no solo botas (el set real de Veigar trae
/// 6657/6655/3003 ahí dentro), así que hace falta este filtro. Tabla de
/// league-lean `lib/data/boots.js` (verificada contra CommunityDragon);
/// refrescar por parche si Riot agrega una variante nueva.
pub const BOOT_IDS: &[i64] = &[
    // Tier-1 / clásicas
    3005, // Ghostcrawlers
    3006, // Berserker's Greaves
    3008, // Gluttonous Greaves
    3009, // Boots of Swiftness
    3020, // Sorcerer's Shoes
    3047, // Plated Steelcaps
    3111, // Mercury's Treads
    3117, // Mobility Boots
    3158, // Ionian Boots of Lucidity
    // Variantes mejoradas de la temporada 15
    3010, // Symbiotic Soles
    3013, // Synchronized Souls
    3168, // Immortal Path
    3170, // Swiftmarch
    3171, // Crimson Lucidity
    3173, // Chainlaced Crushers
    3174, // Armored Advance
    3175, // Spellslinger's Shoes
    3176, // Forever Forward
];

/// ¿Es un id de bota?
#[must_use]
pub fn is_boot(item_id: i64) -> bool {
    BOOT_IDS.contains(&item_id)
}

/// Stats de un ítem dentro del build. `winrate` = `wins/games·100` (con
/// guard de división por cero).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemStat {
    /// Id numérico del ítem (la clave del icono en la CDN).
    pub item_id: i64,
    /// Partidas (picks) de la muestra.
    pub games: u64,
    /// Victorias de la muestra.
    pub wins: u64,
    /// Winrate en porcentaje (0-100).
    pub winrate: f64,
}

impl ItemStat {
    /// URL del icono del ítem para una versión (verificado en vivo:
    /// `img/item/3157.png` → 200).
    #[must_use]
    pub fn icon_url(&self, version: &str) -> String {
        item_icon_url(version, self.item_id)
    }
}

/// ItemStat a partir de picks/wins crudos.
fn item_stat(item_id: i64, games: u64, wins: u64) -> ItemStat {
    let winrate = if games > 0 {
        wins as f64 / games as f64 * 100.0
    } else {
        0.0
    };
    ItemStat { item_id, games, wins, winrate }
}

/// Build de ítems normalizado.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemBuild {
    /// Arranques más jugados (de `itemSet1`, top 3 por picks, compuestos
    /// expandidos).
    pub starters: Vec<ItemStat>,
    /// Botas con stats (de `itemBootSet1` filtrado por [`BOOT_IDS`], top 3
    /// por picks).
    pub boots: Vec<ItemStat>,
    /// Build completo de 5 ítems más jugado (`itemSet5` ordenado por picks,
    /// primero), cada ítem con los juegos/victorias del build completo, en
    /// orden de compra según el string compuesto.
    pub core: Vec<ItemStat>,
    /// Swap-ins de builds alternativos (`itemSet5` siguientes 7 builds por
    /// picks), excluyendo botas y core, ponderados por picks, top 6.
    pub situational: Vec<ItemStat>,
}

impl ItemBuild {
    /// Busca los stats de un ítem en situational y core (el cruce de
    /// [`suggest_vs_comp`]). Primero situational: es donde viviría un swap.
    #[must_use]
    pub fn find_item(&self, item_id: i64) -> Option<ItemStat> {
        self.situational
            .iter()
            .chain(self.core.iter())
            .find(|stat| stat.item_id == item_id)
            .copied()
    }
}

// ---------------------------------------------------------------------------
// Tabla anti-enemigo (heurística de cruce, NO stats per-matchup).
// ---------------------------------------------------------------------------

/// Reglas declarativas de la sugerencia vs comp. Hoy solo acota cuántas
/// sugerencias devolver como máximo (con `max_suggestions == 0` la feature
/// queda deshabilitada); la relevancia del ítem la impone el cruce con el
/// build del propio campeón.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompRules {
    /// Máximo de sugerencias a devolver.
    pub max_suggestions: usize,
}

impl Default for CompRules {
    fn default() -> Self {
        Self { max_suggestions: 3 }
    }
}

/// Tabla CONSTANTE enemigo → ítem anti, con razón etiquetada para la UI.
///
/// Es una heurística de **identidad de enemigos conocidos** (no tenemos
/// roles ni tipo de daño por campeón en runtime): por cada enemigo presente
/// en la partida se busca un ítem clásico contra su patrón de juego, y SOLO
/// se sugiere si el campeón elegido ya juega ese ítem (cruce con
/// core/situational). Los pares son de los conocidos y estables del juego
/// (stasis contra burst/ult canalizado, limpiar CC largo, mitigar crítico...).
pub const ANTI_ENEMY_ITEMS: &[(i64, i64, &str)] = &[
    // Stasis contra assassins y ults canalizadas (ítems AP).
    (238, 3157, "Zhonya's: stasis esquiva el burst de Zed"),
    (105, 3157, "Zhonya's: stasis esquiva su combo"),
    (55, 3157, "Zhonya's: stasis cancela su ult"),
    (141, 3157, "Zhonya's: stasis esquiva su burst"),
    (9, 3157, "Zhonya's: stasis corta su canalización"),
    // Spell shield contra engage y CC largo.
    (238, 3814, "Edge of Night: bloquea su engage"),
    (84, 3102, "Banshee's: el escudo esquiva su engage"),
    (32, 3140, "QSS: limpia el aturdimiento de su ult"),
    (25, 3140, "QSS: limpia su snare"),
    (111, 3140, "QSS: limpia su aturdimiento"),
    (89, 3140, "QSS: limpia su cadena de CC"),
    (412, 3140, "QSS: limpia su enganche"),
    (555, 3140, "QSS: limpia su aturdimiento"),
    // Mitigación física contra crítico / velocidad de ataque.
    (157, 3143, "Randuin's: mitiga su daño por crítico"),
    (777, 3143, "Randuin's: mitiga su daño por crítico"),
    (23, 3143, "Randuin's: mitiga su daño por crítico"),
    (11, 3110, "Frozen Heart: baja su velocidad de ataque"),
    // Anticuración contra sustain fuerte.
    (19, 3075, "Thornmail: corta su curación"),
    (36, 3075, "Thornmail: corta su curación"),
];

/// Razón etiquetada de la tabla para `(ítem, enemigos)`, lista para la UI.
/// La primera fila que matchee gana (la tabla no repite ítem por enemigo).
#[must_use]
pub fn suggestion_reason(item_id: i64, enemies: &[i64]) -> Option<&'static str> {
    ANTI_ENEMY_ITEMS
        .iter()
        .find(|&&(enemy, anti_item, _)| {
            anti_item == item_id && enemies.contains(&enemy)
        })
        .map(|&(_, _, reason)| reason)
}

/// Sugerencias "vs comp": ítems anti-enemigo de [`ANTI_ENEMY_ITEMS`] que el
/// campeón **ya juega** (cruce con `build.situational` y `build.core`),
/// para los enemigos presentes en `enemies`. Sin match → vacío (la UI
/// muestra el build normal).
///
/// Documentación honesta: es una heurística de cruce — no hay stats
/// per-matchup detrás, solo "contra X, tu build ya tiene Y que le responde".
pub fn suggest_vs_comp(build: &ItemBuild, enemies: &[i64], rules: &CompRules) -> Vec<ItemStat> {
    let mut out: Vec<ItemStat> = Vec::new();
    for &enemy in enemies {
        for &(table_enemy, item_id, _) in ANTI_ENEMY_ITEMS {
            if table_enemy != enemy || out.len() >= rules.max_suggestions {
                continue;
            }
            let Some(stat) = build.find_item(item_id) else {
                continue;
            };
            if out.iter().any(|s| s.item_id == item_id) {
                continue;
            }
            out.push(stat);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Parseo del payload.
// ---------------------------------------------------------------------------

/// Número entero tolerante de un `Value` (int o float redondeado).
fn as_u64(value: Option<&serde_json::Value>) -> Option<u64> {
    match value? {
        serde_json::Value::Number(number) => number
            .as_u64()
            .or_else(|| number.as_f64().map(|float| float.round().max(0.0) as u64)),
        serde_json::Value::String(text) => text.trim().parse::<u64>().ok(),
        _ => None,
    }
}

/// Expande una entrada `["id" | "id1_id2_...", picks, wins]` a ids
/// individuales con sus picks/wins. Entradas malformadas (sin string de id)
/// → `None`.
fn expand_entry(entry: &serde_json::Value) -> Option<(Vec<i64>, u64, u64)> {
    let array = entry.as_array()?;
    let id_text = array.first()?.as_str()?;
    let ids: Vec<i64> = id_text
        .split('_')
        .filter_map(|part| part.trim().parse::<i64>().ok())
        .collect();
    if ids.is_empty() {
        return None;
    }
    let picks = as_u64(array.get(1)).unwrap_or(0);
    let wins = as_u64(array.get(2)).unwrap_or(0);
    Some((ids, picks, wins))
}

/// Expande y **agrega** un set completo: los ids compuestos suman sus
/// picks/wins por id individual.
fn aggregate_set(entries: &[serde_json::Value]) -> HashMap<i64, (u64, u64)> {
    let mut agg: HashMap<i64, (u64, u64)> = HashMap::new();
    for entry in entries {
        let Some((ids, picks, wins)) = expand_entry(entry) else {
            continue;
        };
        for id in ids {
            let slot = agg.entry(id).or_insert((0, 0));
            slot.0 += picks;
            slot.1 += wins;
        }
    }
    agg
}

/// Convierte un agregado en `Vec<ItemStat>` ordenado por picks desc (y por
/// id para desempatar determinista).
fn aggregate_to_stats(agg: &HashMap<i64, (u64, u64)>) -> Vec<ItemStat> {
    let mut stats: Vec<ItemStat> = agg
        .iter()
        .map(|(&item_id, &(games, wins))| item_stat(item_id, games, wins))
        .collect();
    stats.sort_by(|a, b| {
        b.games
            .cmp(&a.games)
            .then_with(|| a.item_id.cmp(&b.item_id))
    });
    stats
}

/// Set crudo del payload: `itemSets.itemSetN` → array de entradas.
fn raw_set<'a>(value: &'a serde_json::Value, name: &str) -> Option<&'a Vec<serde_json::Value>> {
    value
        .get("itemSets")
        .and_then(|sets| sets.get(name))
        .and_then(serde_json::Value::as_array)
}

/// Parseo puro del payload (testeado con el recorte real de 16.20).
///
/// 1. `starters`: `itemSet1` agregado (compuestos expandidos), top 3 por
///    picks.
/// 2. `boots`: `itemBootSet1` agregado y filtrado por [`BOOT_IDS`], top 3
///    por picks (el set trae leyendas mezcladas, ver la doc del módulo).
/// 3. `core`: `itemSet5` **ordenado por picks desc** — el payload llega sin
///    ordenar (ver la doc del módulo); el build más jugado aporta los ids en
///    orden de compra, cada uno con los juegos/victorias del build completo.
/// 4. `situational`: los siguientes 7 builds de `itemSet5` por picks,
///    expandidos y agregados, excluyendo botas y los ids del core,
///    ponderados por picks, top 6.
///
/// Payload sin `itemSets` → error claro. Sets individuales ausentes →
/// vectores vacíos (tolerante).
pub(crate) fn parse_item_build(value: serde_json::Value) -> Result<ItemBuild, SourceError> {
    let value = &value;
    if value.get("itemSets").is_none() {
        return Err(SourceError::Lolalytics(String::from(
            "respuesta sin itemSets",
        )));
    }

    let starters = {
        let agg = aggregate_set(raw_set(value, "itemSet1").map_or(&[][..], Vec::as_slice));
        aggregate_to_stats(&agg).into_iter().take(3).collect()
    };

    let boots = {
        let agg = aggregate_set(raw_set(value, "itemBootSet1").map_or(&[][..], Vec::as_slice));
        aggregate_to_stats(&agg)
            .into_iter()
            .filter(|stat| is_boot(stat.item_id))
            .take(3)
            .collect()
    };

    // itemSet5 ordenado por picks desc: primero el build más jugado (core),
    // después los alternativos (pool de situational).
    let mut builds: Vec<(Vec<i64>, u64, u64)> = raw_set(value, "itemSet5")
        .map_or(&[][..], Vec::as_slice)
        .iter()
        .filter_map(expand_entry)
        .collect();
    builds.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

    let core = builds
        .first()
        .map_or(Vec::new(), |(ids, picks, wins)| {
            ids.iter().map(|&id| item_stat(id, *picks, *wins)).collect()
        });
    let core_ids: Vec<i64> = core.iter().map(|stat| stat.item_id).collect();

    let mut situational_agg: HashMap<i64, (u64, u64)> = HashMap::new();
    for (ids, picks, wins) in builds.iter().skip(1).take(7) {
        for &id in ids {
            if is_boot(id) || core_ids.contains(&id) {
                continue;
            }
            let slot = situational_agg.entry(id).or_insert((0, 0));
            slot.0 += *picks;
            slot.1 += *wins;
        }
    }
    let situational: Vec<ItemStat> = aggregate_to_stats(&situational_agg)
        .into_iter()
        .take(6)
        .collect();

    Ok(ItemBuild {
        starters,
        boots,
        core,
        situational,
    })
}

// ---------------------------------------------------------------------------
// Cache en memoria: una consulta por (slug, posición, parche) por corrida.
// ---------------------------------------------------------------------------

/// Clave de cache de builds de ítems.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ItemKey {
    /// Slug del campeón (id ddragon en minúsculas).
    pub slug: String,
    /// Posición pedida (`None` = sin rol).
    pub position: Option<Position>,
    /// Parche consultado (`major.minor`).
    pub patch: String,
}

/// Mapa de cache en memoria.
pub type ItemCache = HashMap<ItemKey, ItemBuild>;

/// Cache global de la app (una entrada por consulta exitosa por corrida).
fn items_cache() -> &'static Mutex<ItemCache> {
    static ITEMS_CACHE: OnceLock<Mutex<ItemCache>> = OnceLock::new();
    ITEMS_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Lookup puro sobre un mapa de cache (testeable sin el global).
#[must_use]
pub fn lookup_items(cache: &ItemCache, key: &ItemKey) -> Option<ItemBuild> {
    cache.get(key).cloned()
}

/// Inserción pura sobre un mapa de cache (testeable sin el global).
pub fn insert_items(cache: &mut ItemCache, key: ItemKey, build: ItemBuild) {
    cache.insert(key, build);
}

/// Lookup sobre la cache global (cache envenenada → miss, mismo criterio
/// que las otras caches de la app).
fn cached_lookup(key: &ItemKey) -> Option<ItemBuild> {
    let cache = items_cache().lock().ok()?;
    lookup_items(&cache, key)
}

/// Inserción sobre la cache global (best-effort).
fn cached_store(key: ItemKey, build: ItemBuild) {
    if let Ok(mut cache) = items_cache().lock() {
        insert_items(&mut cache, key, build);
    }
}

// ---------------------------------------------------------------------------
// Fetch.
// ---------------------------------------------------------------------------

/// Descarga el build de ítems de `slug + posición + parche`. Sin cache:
/// usar [`fetch_item_build_cached`] desde la app.
pub async fn fetch_item_build(
    client: &reqwest::Client,
    slug: &str,
    position: Option<Position>,
    patch: &str,
) -> Result<ItemBuild, SourceError> {
    let url = itemset_url(slug, position, patch);
    let value = client
        .get(&url)
        .send()
        .await
        .and_then(|response| response.error_for_status())
        .map_err(|error| SourceError::Lolalytics(format!("GET {url} falló: {error}")))?
        .json::<serde_json::Value>()
        .await
        .map_err(|error| {
            SourceError::Lolalytics(format!("respuesta de {url} no es JSON: {error}"))
        })?;

    parse_item_build(value)
}

/// Igual que [`fetch_item_build`] pero consultando primero la cache en
/// memoria: una consulta de red por `(slug, posición, parche)` por corrida.
pub async fn fetch_item_build_cached(
    client: &reqwest::Client,
    slug: &str,
    position: Option<Position>,
    patch: &str,
) -> Result<ItemBuild, SourceError> {
    let key = ItemKey {
        slug: slug.to_string(),
        position,
        patch: patch.to_string(),
    };

    if let Some(build) = cached_lookup(&key) {
        eprintln!(
            "klipy [items]: cache hit para {slug} ({patch}): {} core + {} situational",
            build.core.len(),
            build.situational.len()
        );
        return Ok(build);
    }

    let build = fetch_item_build(client, slug, position, patch).await?;
    eprintln!(
        "klipy [items]: build para {slug} ({patch}): {} arranques, {} botas, core de {} ítems, {} situational",
        build.starters.len(),
        build.boots.len(),
        build.core.len(),
        build.situational.len()
    );
    cached_store(key, build.clone());
    Ok(build)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Recorte REAL de la respuesta de 16.20 (verificado en vivo con
    /// `curl 'https://a1.lolalytics.com/mega/?ep=build-itemset&v=1&patch=16.20&c=veigar&tier=platinum_plus&queue=ranked&region=all&lane=middle'`).
    /// Tal cual llega: SIN ordenar por picks (itemSet5[0] real tenía 2
    /// picks; el máximo, 775). itemBootSet1 trae leyendas mezcladas.
    fn fixture_veigar_middle() -> serde_json::Value {
        serde_json::json!({
            "itemSets": {
                "itemSet1": [
                    ["6657", 3784, 1169], ["3003", 1112, 315], ["3152", 139, 46],
                    ["6655", 4206, 1328], ["2065", 9, 3]
                ],
                "itemBootSet1": [
                    ["6657", 842, 192], ["6655", 767, 179], ["3152", 23, 4],
                    ["3119", 4, 2], ["3089", 12, 3], ["3003", 254, 54],
                    ["3047", 14, 3], ["3009", 15, 3], ["3020", 9, 4]
                ],
                "itemSet5": [
                    ["6657_3137_3089_2504_3742", 2, 1],
                    ["3152_3137_3089_3003_3157", 3, 2],
                    ["6655_4646_3089_3157_3102", 120, 67],
                    ["6657_4629_3089_3157_3135", 4, 3],
                    ["6657_3003_4629_3089_3137", 3, 2],
                    ["6655_4645_3089_3041_4646", 3, 2],
                    ["6655_3089_3135_3102_4645", 9, 7],
                    ["6655_3089_3102_3135_4645", 4, 2],
                    ["6655_4646_3089_3157_3137", 34, 20],
                    ["6657_3003_3089_3135_3157", 235, 124],
                    ["6655_4646_3089_4645_3157", 775, 443],
                    ["6657_3003_3089_4645_3157", 385, 219],
                    ["6657_3003_3089_3157_3135", 301, 169]
                ],
                "itemBootSet2": [["6657_3020", 1621, 563], ["6657_3111", 247, 80]]
            },
            "response": { "valid": true }
        })
    }

    #[test]
    fn url_de_itemset_usa_los_common_params() {
        assert_eq!(
            itemset_url("veigar", Some(Position::Middle), "16.20"),
            "https://a1.lolalytics.com/mega/?ep=build-itemset&v=1&patch=16.20&c=veigar&tier=platinum_plus&queue=ranked&region=all&lane=middle"
        );
    }

    /// Expansión de ids compuestos: cada parte parsea a id individual.
    #[test]
    fn expand_entry_separa_ids_compuestos() {
        let entry = serde_json::json!(["6655_4646_3089_4645_3157", 775, 443]);
        let (ids, picks, wins) = expand_entry(&entry).expect("entrada válida");
        assert_eq!(ids, vec![6655, 4646, 3089, 4645, 3157]);
        assert_eq!(picks, 775);
        assert_eq!(wins, 443);

        // Id simple y tolerancia numérica (picks como float).
        let simple = serde_json::json!(["6657", 3784.0, 1169]);
        let (ids, picks, wins) = expand_entry(&simple).expect("entrada simple");
        assert_eq!(ids, vec![6657]);
        assert_eq!(picks, 3784);
        assert_eq!(wins, 1169);

        // Malformada: sin string de id o con id no numérico → None / ids vacíos.
        assert!(expand_entry(&serde_json::json!([42, 1, 2])).is_none());
        assert!(
            expand_entry(&serde_json::json!(["x_y", 1, 2])).is_none(),
            "sin ids numéricos no hay entrada"
        );
    }

    /// Fixture real: starters top 3 por picks, botas filtradas por BOOT_IDS,
    /// core = build más jugado EN ORDEN DE COMPRA (pese al desorden crudo),
    /// situational sin botas ni core, top 6 por picks.
    #[test]
    fn parsea_el_fixture_real_completo() {
        let build = parse_item_build(fixture_veigar_middle()).expect("parsea");

        // Starters: 6655 (4206) > 6657 (3784) > 3003 (1112). Pese a que el
        // payload lista 6657 primero.
        let starters: Vec<i64> = build
            .starters
            .iter()
            .map(|stat| stat.item_id)
            .collect();
        assert_eq!(starters, vec![6655, 6657, 3003]);
        assert!((build.starters[0].winrate - 1328.0 / 4206.0 * 100.0).abs() < 1e-9);

        // Botas: del itemBootSet1 real solo 3009 (15), 3047 (14) y 3020 (9)
        // son botas; 6657/6655/3003/3089/3119 se filtran por BOOT_IDS.
        let boots: Vec<i64> = build.boots.iter().map(|stat| stat.item_id).collect();
        assert_eq!(boots, vec![3009, 3047, 3020]);

        // Core: el build de 775 picks, en orden de compra, con los juegos
        // del build completo.
        let core: Vec<i64> = build.core.iter().map(|stat| stat.item_id).collect();
        assert_eq!(core, vec![6655, 4646, 3089, 4645, 3157]);
        assert_eq!(build.core[0].games, 775);
        assert_eq!(build.core[0].wins, 443);
        assert!((build.core[0].winrate - 443.0 / 775.0 * 100.0).abs() < 1e-9);

        // Situational: pool = los 7 builds siguientes al core por picks
        // (385, 301, 235, 120, 34, 9, 4). Ojo: hay dos builds empatados en
        // 4 picks; el desempate determinista por ids lexicográfico deja en
        // el pool a 6655_3089_3102_3135_4645 y deja fuera a 6657_4629_3089_
        // 3157_3135. Agregando por id (sin botas ni core):
        //   6657 (385+301+235=921), 3003 (385+301+235=921),
        //   3135 (301+235+9+4=549), 3102 (120+9+4=133), 3137 (34).
        // 6657 y 3003 empatan en picks → desempate por id: 3003 primero.
        let situational: Vec<i64> = build
            .situational
            .iter()
            .map(|stat| stat.item_id)
            .collect();
        assert_eq!(situational, vec![3003, 6657, 3135, 3102, 3137]);
        let zhonyas = build.find_item(3157).expect("Zhonya's en el core");
        assert_eq!(zhonyas.games, 775);
    }

    /// Botas en builds alternativos no cuelan en situational; sin itemSet5
    /// el build sale vacío sin error; sin `itemSets` → error claro.
    #[test]
    fn filtra_botas_toler_sets_ausentes_y_falla_claro() {
        // Bota (3020) en un build alternativo → excluida del situational.
        let build = parse_item_build(serde_json::json!({
            "itemSets": {
                "itemSet5": [
                    ["6655_3089_3157", 100, 55],
                    ["6657_3020", 50, 25]
                ]
            }
        }))
        .expect("parsea");
        assert_eq!(build.core.len(), 3);
        assert!(build
            .situational
            .iter()
            .all(|stat| !is_boot(stat.item_id)));
        assert_eq!(build.situational, vec![item_stat(6657, 50, 25)]);

        // itemSet5 ausente → build vacío (tolerante).
        let vacio = parse_item_build(serde_json::json!({ "itemSets": {} }))
            .expect("itemSets vacío es tolerado");
        assert!(vacio.core.is_empty());
        assert!(vacio.situational.is_empty());

        // Sin itemSets → error claro.
        let error = parse_item_build(serde_json::json!({ "response": {} }))
            .expect_err("sin itemSets");
        assert!(error.to_string().contains("sin itemSets"));
    }

    /// `suggest_vs_comp`: solo sugiere ítems de la tabla contra enemigos
    /// presentes Y que el build ya juega; respeta el tope de reglas; sin
    /// match → vacío.
    #[test]
    fn suggest_vs_comp_cruza_tabla_con_el_build() {
        let build = parse_item_build(fixture_veigar_middle()).expect("parsea");

        // Veigar juega Zhonya's (3157, en el core) → contra Zed (238) sale
        // la sugerencia con los stats del core. Contra Yi (11, Frozen Heart)
        // no juega 3110 → sin sugerencia.
        let suggestions = suggest_vs_comp(&build, &[238], &CompRules::default());
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].item_id, 3157);
        assert_eq!(suggestions[0].games, 775);
        assert_eq!(
            suggestion_reason(3157, &[238]),
            Some("Zhonya's: stasis esquiva el burst de Zed")
        );

        assert!(suggest_vs_comp(&build, &[11], &CompRules::default()).is_empty());
        assert!(suggest_vs_comp(&build, &[], &CompRules::default()).is_empty());

        // Dos enemigos con match → dos sugerencias, tope de reglas respetado.
        let dos = suggest_vs_comp(&build, &[238, 84], &CompRules::default());
        assert_eq!(dos.len(), 2);
        assert_eq!(dos[0].item_id, 3157);
        assert_eq!(dos[1].item_id, 3102); // Banshee's, en situational (120 picks)

        let cap = suggest_vs_comp(&build, &[238, 84], &CompRules { max_suggestions: 1 });
        assert_eq!(cap.len(), 1);

        // Reglas en cero → feature deshabilitada.
        assert!(suggest_vs_comp(&build, &[238], &CompRules { max_suggestions: 0 }).is_empty());

        // Enemigo fuera de la tabla → vacío.
        assert!(suggest_vs_comp(&build, &[45], &CompRules::default()).is_empty());
        assert_eq!(suggestion_reason(3157, &[45]), None);
    }

    /// Cache pura: miss → put → hit por clave.
    #[test]
    fn cache_pura_guarda_y_pisa_por_clave() {
        let mut cache = ItemCache::new();
        let key = ItemKey {
            slug: String::from("veigar"),
            position: Some(Position::Middle),
            patch: String::from("16.20"),
        };
        assert!(lookup_items(&cache, &key).is_none());

        let build = ItemBuild {
            core: vec![item_stat(3157, 775, 443)],
            ..ItemBuild::default()
        };
        insert_items(&mut cache, key.clone(), build.clone());
        assert_eq!(lookup_items(&cache, &key), Some(build));

        let otro_parche = ItemKey {
            patch: String::from("16.19"),
            ..key
        };
        assert!(lookup_items(&cache, &otro_parche).is_none());
    }

    /// Icono de ítem: la URL es la de la CDN con el id numérico.
    #[test]
    fn icon_url_de_item_stat_apunta_a_la_cdn() {
        let stat = item_stat(3157, 775, 443);
        assert_eq!(
            stat.icon_url("16.20.1"),
            "https://ddragon.leagueoflegends.com/cdn/16.20.1/img/item/3157.png"
        );
    }

    /// `is_boot` sobre la tabla real: clásicas sí, leyendas no.
    #[test]
    fn is_boot_distingue_botas_de_leyendas() {
        for id in [3006, 3020, 3047, 3111, 3158, 3176] {
            assert!(is_boot(id), "{id} es bota");
        }
        for id in [6655, 6657, 3089, 3157, 3003, 3119] {
            assert!(!is_boot(id), "{id} NO es bota");
        }
    }
}
