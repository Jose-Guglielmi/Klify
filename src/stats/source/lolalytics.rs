//! Adapter de Lolalytics (fuente primaria de FASE 9).
//!
//! Endpoint (`ep=rune`, formato verificado contra league-lean
//! `lib/sources/lolalytics.js` y contra la respuesta real del parche 16.20):
//!
//! `GET https://a1.lolalytics.com/mega/?ep=rune&v=1&patch={patch}&c={slug}
//!    &tier=platinum_plus&queue=ranked&region=all[&lane={lane}]`
//!
//! - `patch` es `major.minor` (ej. `"16.20"`); `slug` es el id de Data
//!   Dragon en minúsculas ([`crate::data_dragon::models::champion_slug`]);
//!   `lane` ∈ {top, jungle, middle, bottom, support} y **se omite** sin
//!   posición asignada. Tier fijo `platinum_plus` (mismo default que
//!   league-lean; pendiente hacerlo configurable si el spec lo pide).
//!
//! La respuesta trae `summary.runes.pick` (página más jugada) y
//! `summary.runes.win` (página de mayor winrate; puede repetir a `pick`),
//! cada una con `wr` (%), `n` (partidas), `set` (`pri`/`sec`/`mod`: ids de
//! runas y shards) y `page` (`pri`/`sec`: índices 0..4 del árbol). El resto
//! del payload (`header`, `avgWinRate`, `analysed`, `nav`, `response`, ...)
//! se ignora.
//!
//! Normalización: `wins` se deriva como `round(n * wr / 100)` (el endpoint
//! no trae victorias); los árboles se resuelven con el catálogo de runas y
//! como fallback el índice de página (ver [`super::resolve_style_id`]). Si
//! `win` y `pick` describen la misma configuración (mismos estilos, runas y
//! shards), se conserva una sola página.

use serde::Deserialize;

use super::{keystone_of, resolve_style_id, RunePageStats, SourceError, SourceKind};
use crate::data_dragon::runes::RuneCatalog;
use crate::lcu::models::Position;

/// Base del mega-endpoint de Lolalytics.
pub const LOLALYTICS_BASE: &str = "https://a1.lolalytics.com/mega/";

/// Tier de la consulta (estadísticamente relevante y el default de
/// league-lean).
pub const DEFAULT_TIER: &str = "platinum_plus";

/// URL de `ep=rune` para un campeón/posición/parche. Sin posición se omite
/// el parámetro `lane` (no se manda `lane=none`).
#[must_use]
pub fn url_for(slug: &str, position: Option<Position>, patch: &str) -> String {
    super::mega_url("rune", slug, position, patch)
}

/// Respuesta de `ep=rune` (subconjunto; el resto del payload se ignora).
#[derive(Debug, Default, Deserialize)]
struct LolalyticsResponse {
    /// `summary.runes`; ausente → error claro (respuesta vacía o capada).
    #[serde(default)]
    summary: Option<SummaryDto>,
}

#[derive(Debug, Default, Deserialize)]
struct SummaryDto {
    #[serde(default)]
    runes: Option<RunesSummaryDto>,
}

#[derive(Debug, Default, Deserialize)]
struct RunesSummaryDto {
    #[serde(default)]
    pick: Option<RunePageDto>,
    #[serde(default)]
    win: Option<RunePageDto>,
}

/// Una página de `summary.runes.{pick,win}`.
///
/// `wr`/`n` se tipan `f64` para tolerar enteros y flotantes del payload;
/// los ids llegan como enteros.
#[derive(Debug, Default, Deserialize)]
struct RunePageDto {
    /// Winrate en porcentaje (ej. `51.06`).
    #[serde(default)]
    wr: f64,
    /// Partidas de la muestra (ej. `559936`).
    #[serde(default)]
    n: f64,
    /// Ids de la página.
    #[serde(default)]
    set: RuneSetDto,
    /// Índices de árbol (0..4) de la página.
    #[serde(default)]
    page: PageIndexDto,
}

#[derive(Debug, Default, Deserialize)]
struct RuneSetDto {
    /// Runas del árbol principal (keystone primero).
    #[serde(default)]
    pri: Vec<i64>,
    /// Runas del árbol secundario.
    #[serde(default)]
    sec: Vec<i64>,
    /// Shards (runas de stats). `"mod"` es keyword en Rust → rename.
    #[serde(rename = "mod", default)]
    mods: Vec<i64>,
}

#[derive(Debug, Default, Deserialize)]
struct PageIndexDto {
    #[serde(default)]
    pri: usize,
    #[serde(default)]
    sec: usize,
}

/// Convierte una página cruda a la forma normalizada. Partidas y victorias
/// redondeadas a enteros (≥ 0); `wr` se conserva tal cual llega.
fn normalize(dto: &RunePageDto, label: &str, runes: Option<&RuneCatalog>) -> RunePageStats {
    let games = dto.n.round().max(0.0) as u64;
    let wins = (dto.n * dto.wr / 100.0).round().max(0.0) as u64;

    let mut perk_ids = dto.set.pri.clone();
    perk_ids.extend(dto.set.sec.iter().copied());

    RunePageStats {
        source: SourceKind::Lolalytics,
        label: label.to_string(),
        games,
        wins,
        winrate_pct: dto.wr,
        primary_style_id: resolve_style_id(&dto.set.pri, dto.page.pri, runes),
        sub_style_id: resolve_style_id(&dto.set.sec, dto.page.sec, runes),
        keystone_id: keystone_of(&dto.set.pri, runes),
        perk_ids,
        stat_shards: dto.set.mods.clone(),
        estilo: None,
        sintetizada: false,
    }
}

/// Parseo puro de la respuesta (testeado con el recorte real de 16.20).
/// Error claro si `summary.runes.pick` falta o no trae runas principales.
pub(crate) fn parse_response(
    value: serde_json::Value,
    runes: Option<&RuneCatalog>,
) -> Result<Vec<RunePageStats>, SourceError> {
    let response: LolalyticsResponse = serde_json::from_value(value)
        .map_err(|error| SourceError::Lolalytics(format!("payload no parseable: {error}")))?;

    let summary = response
        .summary
        .and_then(|summary| summary.runes)
        .ok_or_else(|| {
            SourceError::Lolalytics(String::from("respuesta sin summary.runes"))
        })?;
    let pick = summary.pick.ok_or_else(|| {
        SourceError::Lolalytics(String::from("respuesta sin summary.runes.pick"))
    })?;
    if pick.set.pri.is_empty() {
        return Err(SourceError::Lolalytics(String::from(
            "summary.runes.pick sin runas principales",
        )));
    }

    let mut pages = vec![normalize(&pick, "Lolalytics · más jugada", runes)];

    // `win` puede ser la misma página que `pick` (mismo set): en ese caso
    // una sola entrada. La comparación es por configuración (estilos, runas
    // y shards), no por métricas: misma página con otro wr no aporta nada.
    if let Some(win) = summary.win {
        let win_page = normalize(&win, "Lolalytics · mejor winrate", runes);
        if win_page.config_key() != pages[0].config_key() {
            pages.push(win_page);
        }
    }

    Ok(pages)
}

/// Descarga las páginas de runas de Lolalytics para `slug + posición +
/// parche`: la más jugada y, si difiere en configuración, la de mejor
/// winrate. Tolerante a campos ausentes; estricto con `summary.runes.pick`.
pub async fn fetch_rune_pages(
    client: &reqwest::Client,
    slug: &str,
    position: Option<Position>,
    patch: &str,
    runes: Option<&RuneCatalog>,
) -> Result<Vec<RunePageStats>, SourceError> {
    let url = url_for(slug, position, patch);
    let value = client
        .get(&url)
        .send()
        .await
        .and_then(|response| response.error_for_status())
        .map_err(|error| SourceError::Lolalytics(format!("GET {url} falló: {error}")))?
        .json::<serde_json::Value>()
        .await
        .map_err(|error| SourceError::Lolalytics(format!("respuesta de {url} no es JSON: {error}")))?;

    parse_response(value, runes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lcu::models::Position;
    use serde_json::json;

    /// Recorte REAL de la respuesta de 16.20 (verificado en vivo con
    /// `curl 'https://a1.lolalytics.com/mega/?ep=rune&v=1&patch=16.20&c=kaisa&tier=platinum_plus&queue=ranked&region=all&lane=bottom'`).
    /// Pick y win traen la misma muestra (wr/n) pero sets distintos.
    fn fixture_kaisa_bottom() -> serde_json::Value {
        json!({
            "header": { "n": 602909, "defaultLane": "bottom", "lane": "bottom" },
            "summary": {
                "runes": {
                    "pick": {
                        "wr": 51.06, "n": 559936,
                        "page": { "pri": 0, "sec": 4 },
                        "set": {
                            "pri": [8008, 8009, 9103, 8014],
                            "sec": [8345, 8304],
                            "mod": [5005, 5008, 5001]
                        }
                    },
                    "win": {
                        "wr": 51.06, "n": 559936,
                        "page": { "pri": 0, "sec": 4 },
                        "set": {
                            "pri": [8008, 9111, 9104, 8017],
                            "sec": [8321, 8313],
                            "mod": [5005, 5008, 5011]
                        }
                    }
                }
            },
            "analysed": 33449615,
            "avgWinRate": 51.19,
            "response": { "valid": true, "duration": "58" }
        })
    }

    #[test]
    fn url_incluye_lane_y_omite_sin_posicion() {
        assert_eq!(
            url_for("kaisa", Some(Position::Bottom), "16.20"),
            "https://a1.lolalytics.com/mega/?ep=rune&v=1&patch=16.20&c=kaisa&tier=platinum_plus&queue=ranked&region=all&lane=bottom"
        );
        // UTILITY → support (mapeo Lolalytics).
        assert!(url_for("ahri", Some(Position::Utility), "16.20").contains("&lane=support"));
        // Sin posición: sin parámetro lane.
        assert_eq!(
            url_for("drmundo", None, "16.20"),
            "https://a1.lolalytics.com/mega/?ep=rune&v=1&patch=16.20&c=drmundo&tier=platinum_plus&queue=ranked&region=all"
        );
    }

    /// Fixture real: pick + win con sets distintos → dos páginas, con
    /// winrate derivado (wins = round(n·wr/100)) y perks/shards intactos.
    #[test]
    fn parsea_pick_y_win_distintos_del_fixture_real() {
        let pages = parse_response(fixture_kaisa_bottom(), None).expect("parsea");

        assert_eq!(pages.len(), 2);

        let pick = &pages[0];
        assert_eq!(pick.source, SourceKind::Lolalytics);
        assert_eq!(pick.games, 559_936);
        assert_eq!(pick.wins, 285_903); // round(559936 · 51.06 / 100)
        assert!((pick.winrate_pct - 51.06).abs() < f64::EPSILON);
        assert_eq!(
            pick.perk_ids,
            vec![8008, 8009, 9103, 8014, 8345, 8304] // pri + sec
        );
        assert_eq!(pick.stat_shards, vec![5005, 5008, 5001]);
        // Sin catálogo: estilos por índice (0 → 8000, 4 → 8400) y keystone
        // por convención (primera runa del primario).
        assert_eq!(pick.primary_style_id, 8000);
        assert_eq!(pick.sub_style_id, 8400);
        assert_eq!(pick.keystone_id, Some(8008));

        let win = &pages[1];
        assert_eq!(win.games, 559_936);
        assert_eq!(
            win.perk_ids,
            vec![8008, 9111, 9104, 8017, 8321, 8313]
        );
        assert_eq!(win.stat_shards, vec![5005, 5008, 5011]);
        assert_ne!(win, pick);
    }

    /// Solo `pick` (sin `win`) → una sola página.
    #[test]
    fn parsea_pick_solo() {
        let value = json!({
            "summary": { "runes": { "pick": {
                "wr": 52.5, "n": 1000,
                "page": { "pri": 1, "sec": 2 },
                "set": { "pri": [8112, 8139], "sec": [8210], "mod": [5008, 5008, 5002] }
            } } }
        });

        let pages = parse_response(value, None).expect("parsea");
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].games, 1000);
        assert_eq!(pages[0].wins, 525); // round(1000 · 52.5 / 100)
        assert_eq!(pages[0].primary_style_id, 8100); // índice 1 sin catálogo
        assert_eq!(pages[0].sub_style_id, 8200); // índice 2 sin catálogo
        assert_eq!(pages[0].perk_ids, vec![8112, 8139, 8210]);
        assert_eq!(pages[0].stat_shards, vec![5008, 5008, 5002]);
    }

    /// `win` con la misma configuración que `pick` → una sola página (la de
    /// pick, con su etiqueta).
    #[test]
    fn win_igual_a_pick_se_deduplica() {
        let value = json!({
            "summary": { "runes": {
                "pick": {
                    "wr": 50.0, "n": 100,
                    "page": { "pri": 0, "sec": 1 },
                    "set": { "pri": [8005], "sec": [8112], "mod": [5005] }
                },
                "win": {
                    "wr": 51.0, "n": 90,
                    "page": { "pri": 0, "sec": 1 },
                    "set": { "pri": [8005], "sec": [8112], "mod": [5005] }
                }
            } }
        });

        let pages = parse_response(value, None).expect("parsea");
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].label, "Lolalytics · más jugada");
        assert_eq!(pages[0].winrate_pct, 50.0);
    }

    /// Sin `summary.runes` (respuesta capada) → error claro; también con
    /// `pick` sin runas principales.
    #[test]
    fn respuestas_vacias_dan_error_claro() {
        let error = parse_response(json!({ "analysed": 1 }), None)
            .expect_err("sin summary.runes");
        assert!(error.to_string().contains("summary.runes"));

        let error = parse_response(
            json!({ "summary": { "runes": { "pick": { "wr": 50.0, "n": 10, "set": {} } } } }),
            None,
        )
        .expect_err("pick sin pri");
        assert!(error.to_string().contains("runas principales"));
    }

    /// Tolerancia: página sin `page` (índices) ni `mod` (shards) parsea con
    /// defaults; `wr`/`n` ausentes cuentan como 0.
    #[test]
    fn pagina_minimalista_parsea_con_defaults() {
        let value = json!({
            "summary": { "runes": { "pick": { "set": { "pri": [8005], "sec": [8112] } } } }
        });

        let pages = parse_response(value, None).expect("parsea");
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].games, 0);
        assert_eq!(pages[0].wins, 0);
        assert_eq!(pages[0].winrate_pct, 0.0);
        assert_eq!(pages[0].primary_style_id, 8000); // índice default 0
        assert_eq!(pages[0].sub_style_id, 8000); // índice default 0
        assert!(pages[0].stat_shards.is_empty());
    }
}
