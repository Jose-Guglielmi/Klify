//! Counters de Lolalytics (FASE 11a-1, pieza 2).
//!
//! Endpoint (mismo mega-endpoint y common params que `ep=rune`, con
//! `ep=counter`; formato verificado contra league-lean
//! `lib/sources/lolalytics.js` y contra la respuesta real del parche 16.20):
//!
//! `GET https://a1.lolalytics.com/mega/?ep=counter&v=1&patch={patch}&c={slug}
//!    &tier=platinum_plus&queue=ranked&region=all[&lane={lane}]`
//!
//! Recorte REAL (verificado en vivo 2026-10-17 con
//! `c=veigar&lane=middle&patch=16.20`):
//!
//! ```json
//! {
//!   "stats": { "cid": 45, "lane": "middle", "analysed": 34745768,
//!              "avgWr": 51.2, "br": 1.78, "pr": "4.15", "wr": "52.02", ... },
//!   "counters": [
//!     { "cid": 42, "vsWr": 68.02, "n": 172, "d1": 13.19, ... },
//!     { "cid": 106, "vsWr": 62.76, "n": 333, ... }
//!   ],
//!   "response": { ... }
//! }
//! ```
//!
//! - `vsWr` es el winrate de **MI** campeón CONTRA ese `cid` (verificado por
//!   league-lean: vsWr alto = matchup que me sale bien; vsWr bajo = counter
//!   real contra mí). `n` son las partidas del matchup.
//! - Ojo con los tipos: `stats.wr` y `stats.pr` llegan como **strings**
//!   (`"52.02"`) mientras `br`/`analysed`/`vsWr`/`n` llegan numéricos. El
//!   deserializador es tolerante a ambos.
//!
//! Normalización: se filtran matchups con `n <` [`MIN_COUNTER_GAMES`] (200,
//! mismo umbral que league-lean) y se ordena por `vsWr` descendente, así que
//! `counters[0..k]` son los mejores matchups y el final los peores.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

use super::{mega_url, SourceError};
use crate::lcu::models::Position;

/// Mínimo de partidas por matchup para considerarlo señal (mismo umbral
/// `COUNTER_MIN_GAMES` de league-lean).
pub const MIN_COUNTER_GAMES: i64 = 200;

/// URL de `ep=counter` para un campeón/posición/parche.
#[must_use]
pub fn counter_url(slug: &str, position: Option<Position>, patch: &str) -> String {
    mega_url("counter", slug, position, patch)
}

/// Número tolerante a f64 opcional: JSON number o string decimal
/// (`"52.02"`, el formato real de `stats.wr`/`stats.pr`).
fn de_f64_flexible<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    let parsed = match value {
        serde_json::Value::Number(number) => number.as_f64(),
        serde_json::Value::String(text) => text.trim().parse::<f64>().ok(),
        _ => None,
    };
    Ok(parsed)
}

/// Un matchup contra un campeón, normalizado.
///
/// Deriva `Serialize`/`Deserialize` (camelCase) para las cachés futuras,
/// igual que [`crate::stats::source::RunePageStats`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CounterEntry {
    /// Id numérico del campeón rival (`cid` del payload).
    pub champion_id: i64,
    /// Winrate de MI campeón contra ese rival, en porcentaje.
    pub vs_winrate: f64,
    /// Partidas del matchup (`n` del payload).
    pub games: i64,
}

/// Resumen de counters de un campeón/posición/parche.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CounterSummary {
    /// Winrate global de MI campeón en esa muestra (`stats.wr`); puede
    /// faltar en un payload capado.
    pub my_wr: Option<f64>,
    /// Pickrate global de MI campeón en porcentaje (`stats.pr`, string en
    /// el payload real). FASE 11b-2a: alimenta el HUD del hero del champ
    /// select; puede faltar en un payload capado.
    #[serde(default)]
    pub pickrate: Option<f64>,
    /// Partidas analizadas del pool (`stats.analysed`).
    pub analysed: Option<i64>,
    /// Matchups con `games >= MIN_COUNTER_GAMES`, ordenados por `vs_winrate`
    /// descendente (mejores primero).
    pub counters: Vec<CounterEntry>,
}

impl CounterSummary {
    /// Los `n` mejores matchups para mí (ya vienen ordenados: prefijo).
    #[must_use]
    pub fn strong_against(&self, n: usize) -> &[CounterEntry] {
        let end = n.min(self.counters.len());
        &self.counters[..end]
    }

    /// Los `n` peores matchups para mí, peor primero (cola invertida).
    #[must_use]
    pub fn weak_against(&self, n: usize) -> Vec<CounterEntry> {
        let start = self.counters.len().saturating_sub(n);
        self.counters[start..].iter().rev().cloned().collect()
    }
}

/// Busca el matchup contra `champion_id` en una lista de counters ya
/// normalizada. Útil para el panel "VS ENEMIGOS EN VIVO": ¿cómo me trata
/// ESTE enemigo?
///
/// `None` si el matchup no está (lo filtró [`MIN_COUNTER_GAMES`], el rival
/// no tiene partidas o simplemente no aparece).
#[must_use]
pub fn wr_vs(counters: &[CounterEntry], champion_id: i64) -> Option<&CounterEntry> {
    counters
        .iter()
        .find(|entry| entry.champion_id == champion_id)
}

/// Respuesta cruda de `ep=counter` (subconjunto tolerante).
#[derive(Debug, Default, Deserialize)]
struct LolalyticsCounterResponse {
    #[serde(default)]
    stats: Option<StatsDto>,
    #[serde(default)]
    counters: Vec<CounterDto>,
}

#[derive(Debug, Default, Deserialize)]
struct StatsDto {
    /// Winrate global de MI campeón; string en el payload real.
    #[serde(default, deserialize_with = "de_f64_flexible")]
    wr: Option<f64>,
    /// Pickrate global de MI campeón; string en el payload real.
    #[serde(default, deserialize_with = "de_f64_flexible")]
    pr: Option<f64>,
    /// Partidas analizadas del pool.
    #[serde(default)]
    analysed: Option<i64>,
}

#[derive(Debug, Default, Deserialize)]
struct CounterDto {
    #[serde(default)]
    cid: i64,
    /// `vsWr` en el payload (winrate de MI champ contra `cid`).
    #[serde(rename = "vsWr", default)]
    vs_wr: f64,
    #[serde(default)]
    n: i64,
}

/// Parseo puro de la respuesta (testeado con el recorte real de 16.20):
/// filtra por [`MIN_COUNTER_GAMES`], ordena por `vsWr` desc y expone los
/// stats globales. Un payload sin `stats` ni `counters` es error (respuesta
/// vacía o capada); con solo uno de los dos, se tolera.
pub(crate) fn parse_response(value: serde_json::Value) -> Result<CounterSummary, SourceError> {
    let response: LolalyticsCounterResponse = serde_json::from_value(value)
        .map_err(|error| SourceError::Lolalytics(format!("payload no parseable: {error}")))?;

    if response.stats.is_none() && response.counters.is_empty() {
        return Err(SourceError::Lolalytics(String::from(
            "respuesta sin stats ni counters",
        )));
    }

    let mut counters: Vec<CounterEntry> = response
        .counters
        .into_iter()
        .filter(|dto| dto.n >= MIN_COUNTER_GAMES)
        .map(|dto| CounterEntry {
            champion_id: dto.cid,
            vs_winrate: dto.vs_wr,
            games: dto.n,
        })
        .collect();
    // Orden estable por vsWr desc: los mejores matchups primero. `total_cmp`
    // es seguro ante NaN (a diferencia de `partial_cmp` + unwrap).
    counters.sort_by(|a, b| b.vs_winrate.total_cmp(&a.vs_winrate));

    let StatsDto { wr, pr, analysed } = response.stats.unwrap_or_default();
    Ok(CounterSummary {
        my_wr: wr,
        pickrate: pr,
        analysed,
        counters,
    })
}

// ---------------------------------------------------------------------------
// Cache en memoria: una consulta por (slug, posición, parche) por corrida.
// ---------------------------------------------------------------------------

/// Clave de cache de counters.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CounterKey {
    /// Slug del campeón (id ddragon en minúsculas).
    pub slug: String,
    /// Posición pedida (`None` = sin rol).
    pub position: Option<Position>,
    /// Parche consultado (`major.minor`).
    pub patch: String,
}

/// Mapa de cache en memoria.
pub type CounterCache = HashMap<CounterKey, CounterSummary>;

/// Cache global de la app (una entrada por consulta exitosa por corrida).
fn counters_cache() -> &'static Mutex<CounterCache> {
    static COUNTERS_CACHE: OnceLock<Mutex<CounterCache>> = OnceLock::new();
    COUNTERS_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Lookup puro sobre un mapa de cache (testeable sin el global).
#[must_use]
pub fn lookup_counters(cache: &CounterCache, key: &CounterKey) -> Option<CounterSummary> {
    cache.get(key).cloned()
}

/// Inserción pura sobre un mapa de cache (testeable sin el global).
pub fn insert_counters(cache: &mut CounterCache, key: CounterKey, summary: CounterSummary) {
    cache.insert(key, summary);
}

/// Lookup sobre la cache global. Cache envenenada → miss y re-consulta
/// (mismo criterio que la cache de páginas de runas).
fn cached_lookup(key: &CounterKey) -> Option<CounterSummary> {
    let cache = counters_cache().lock().ok()?;
    lookup_counters(&cache, key)
}

/// Inserción sobre la cache global (best-effort: un lock envenenado solo
/// pierde esta entrada).
fn cached_store(key: CounterKey, summary: CounterSummary) {
    if let Ok(mut cache) = counters_cache().lock() {
        insert_counters(&mut cache, key, summary);
    }
}

// ---------------------------------------------------------------------------
// Fetch.
// ---------------------------------------------------------------------------

/// Descarga el resumen de counters de `slug + posición + parche`: matchups
/// filtrados por [`MIN_COUNTER_GAMES`] y ordenados por vsWr desc. Sin
/// cache: usar [`fetch_counters_cached`] desde la app.
pub async fn fetch_counters(
    client: &reqwest::Client,
    slug: &str,
    position: Option<Position>,
    patch: &str,
) -> Result<CounterSummary, SourceError> {
    let url = counter_url(slug, position, patch);
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

    parse_response(value)
}

/// Igual que [`fetch_counters`] pero consultando primero la cache en
/// memoria: una consulta de red por `(slug, posición, parche)` por corrida.
pub async fn fetch_counters_cached(
    client: &reqwest::Client,
    slug: &str,
    position: Option<Position>,
    patch: &str,
) -> Result<CounterSummary, SourceError> {
    let key = CounterKey {
        slug: slug.to_string(),
        position,
        patch: patch.to_string(),
    };

    if let Some(summary) = cached_lookup(&key) {
        eprintln!(
            "klipy [counters]: cache hit para {slug} ({patch}): {} matchup(s)",
            summary.counters.len()
        );
        return Ok(summary);
    }

    let summary = fetch_counters(client, slug, position, patch).await?;
    eprintln!(
        "klipy [counters]: {} matchup(s) (≥ {MIN_COUNTER_GAMES} juegos) para {slug} ({patch})",
        summary.counters.len()
    );
    cached_store(key, summary.clone());
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Recorte REAL de la respuesta de 16.20 (verificado en vivo con
    /// `curl 'https://a1.lolalytics.com/mega/?ep=counter&v=1&patch=16.20&c=veigar&tier=platinum_plus&queue=ranked&region=all&lane=middle'`).
    /// Ojo: `wr`/`pr` llegan como strings; el primer matchup del payload
    /// real (cid 42 con n=172) queda filtrado por el umbral.
    fn fixture_veigar_middle() -> serde_json::Value {
        serde_json::json!({
            "stats": {
                "defaultLane": "middle", "cid": 45, "lane": "middle",
                "vsLane": "middle", "analysed": 34745768, "avgWr": 51.2,
                "br": 1.78, "pr": "4.15", "wr": "52.02",
                "lanes": { "top": 3.2, "jungle": 0, "middle": 66.4, "bottom": 20.3, "support": 10 },
                "counters": { "strong": [42, 106, 81], "weak": [23, 29, 5] }
            },
            "counters": [
                { "cid": 42, "vsWr": 68.02, "n": 172, "d1": 13.19, "d2": 10, "allWr": 54.83, "defaultLane": "bottom" },
                { "cid": 106, "vsWr": 62.76, "n": 333, "d1": 8.83, "d2": 5.63, "allWr": 53.93, "defaultLane": "top" },
                { "cid": 81, "vsWr": 61.03, "n": 390, "d1": 4.98, "d2": 1.8, "allWr": 56.05, "defaultLane": "bottom" },
                { "cid": 13, "vsWr": 60.15, "n": 1428, "d1": 6.77, "d2": 3.56, "allWr": 53.38, "defaultLane": "middle" },
                { "cid": 101, "vsWr": 46.84, "n": 3930, "d1": -0.47, "d2": -3.69, "allWr": 47.31, "defaultLane": "middle" },
                { "cid": 23, "vsWr": 43.81, "n": 210, "d1": -3.43, "d2": -6.65, "allWr": 47.24, "defaultLane": "top" }
            ],
            "response": { "valid": true, "duration": "61" }
        })
    }

    #[test]
    fn url_de_counter_usa_los_common_params() {
        assert_eq!(
            counter_url("veigar", Some(Position::Middle), "16.20"),
            "https://a1.lolalytics.com/mega/?ep=counter&v=1&patch=16.20&c=veigar&tier=platinum_plus&queue=ranked&region=all&lane=middle"
        );
        assert!(
            counter_url("ahri", None, "16.20").ends_with("region=all"),
            "sin posición no hay parámetro lane"
        );
    }

    /// Fixture real: filtra n < 200, ordena por vsWr desc y expone el wr
    /// global tolerando el string `"52.02"`.
    #[test]
    fn parsea_el_fixture_real_filtrando_y_ordenando() {
        let summary = parse_response(fixture_veigar_middle()).expect("parsea");

        // El matchup con n=172 (cid 42) queda fuera por el umbral.
        assert_eq!(summary.counters.len(), 5);
        assert!(wr_vs(&summary.counters, 42).is_none());

        // Orden por vsWr descendente.
        let ids: Vec<i64> = summary
            .counters
            .iter()
            .map(|entry| entry.champion_id)
            .collect();
        assert_eq!(ids, vec![106, 81, 13, 101, 23]);

        let top = &summary.counters[0];
        assert_eq!(top.champion_id, 106);
        assert!((top.vs_winrate - 62.76).abs() < f64::EPSILON);
        assert_eq!(top.games, 333);

        // stats.wr tolerado como string → f64; `pr` ídem (FASE 11b-2a).
        assert_eq!(summary.my_wr, Some(52.02));
        assert_eq!(summary.pickrate, Some(4.15));
        assert_eq!(summary.analysed, Some(34_745_768));
    }

    /// Los helpers strong/weak recortan la lista ya ordenada: strong toma el
    /// prefijo (mejores) y weak la cola invertida (peor primero).
    #[test]
    fn strong_y_weak_recortan_la_lista_ordenada() {
        let summary = parse_response(fixture_veigar_middle()).expect("parsea");

        let strong: Vec<i64> = summary
            .strong_against(2)
            .iter()
            .map(|entry| entry.champion_id)
            .collect();
        assert_eq!(strong, vec![106, 81]);

        let weak: Vec<i64> = summary
            .weak_against(2)
            .iter()
            .map(|entry| entry.champion_id)
            .collect();
        // Peor primero: 23 (43.81) antes que 101 (46.84).
        assert_eq!(weak, vec![23, 101]);

        // Pedir más de lo que hay no panea: devuelve todo lo disponible.
        assert_eq!(summary.strong_against(99).len(), 5);
        assert_eq!(summary.weak_against(99).len(), 5);
        assert_eq!(CounterSummary::default().strong_against(3), &[] as &[CounterEntry]);
        assert!(CounterSummary::default().weak_against(3).is_empty());
    }

    /// `wr_vs` encuentra por id del rival en listas crudas u ordenadas.
    #[test]
    fn wr_vs_busca_por_champion_id() {
        let summary = parse_response(fixture_veigar_middle()).expect("parsea");

        let entry = wr_vs(&summary.counters, 13).expect(" matchup contra 13");
        assert_eq!(entry.games, 1428);
        assert!((entry.vs_winrate - 60.15).abs() < f64::EPSILON);

        assert!(wr_vs(&summary.counters, 999).is_none());
        assert!(wr_vs(&[], 13).is_none());
    }

    /// Payloads capados: sin stats ni counters → error claro; sin counters
    /// pero con stats → summary vacío válido; `wr` numérico también se
    /// tolera.
    #[test]
    fn respuestas_capadas_se_toleran_o_fallan_claro() {
        let error = parse_response(serde_json::json!({ "response": { "valid": true } }))
            .expect_err("sin datos");
        assert!(error.to_string().contains("sin stats ni counters"));

        let solo_stats = parse_response(serde_json::json!({ "stats": { "wr": 51.4 } }))
            .expect("stats solos son válidos");
        assert_eq!(solo_stats.my_wr, Some(51.4));
        assert!(solo_stats.counters.is_empty());

        let solo_counters = parse_response(serde_json::json!({
            "counters": [ { "cid": 8, "vsWr": 55.0, "n": 500 }]
        }))
        .expect("counters solos son válidos");
        assert_eq!(solo_counters.my_wr, None);
        assert_eq!(solo_counters.counters.len(), 1);
    }

    /// Todos los matchups por debajo del umbral → lista vacía (no error).
    #[test]
    fn sin_matchups_que_pasen_el_umbral_da_lista_vacia() {
        let summary = parse_response(serde_json::json!({
            "stats": { "wr": "50.0", "analysed": 1000 },
            "counters": [
                { "cid": 8, "vsWr": 55.0, "n": 199 },
                { "cid": 9, "vsWr": 45.0, "n": 0 }
            ]
        }))
        .expect("parsea");

        assert!(summary.counters.is_empty());
        assert_eq!(summary.my_wr, Some(50.0));
    }

    /// Cache pura: miss → put → hit, y la clave distingue slug/posición/parche.
    #[test]
    fn cache_pura_guarda_y_pisa_por_clave() {
        let mut cache = CounterCache::new();
        let key = CounterKey {
            slug: String::from("veigar"),
            position: Some(Position::Middle),
            patch: String::from("16.20"),
        };
        assert!(lookup_counters(&cache, &key).is_none());

        let summary = CounterSummary {
            my_wr: Some(52.02),
            pickrate: Some(4.15),
            analysed: Some(34_745_768),
            counters: vec![CounterEntry {
                champion_id: 106,
                vs_winrate: 62.76,
                games: 333,
            }],
        };
        insert_counters(&mut cache, key.clone(), summary.clone());
        assert_eq!(lookup_counters(&cache, &key), Some(summary));

        let otra_pos = CounterKey {
            position: Some(Position::Top),
            ..key.clone()
        };
        assert!(lookup_counters(&cache, &otra_pos).is_none());
    }
}
