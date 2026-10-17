//! Adapter de U.GG (fallback de FASE 9).
//!
//! Endpoint de overview (formato verificado contra league-lean
//! `lib/sources/ugg.js`):
//!
//! `GET https://stats2.u.gg/lol/1.5/overview/{patch_con_guion_bajo}/
//!    ranked_solo_5x5/{champion_id}/1.5.0.json`
//!
//! - `patch_con_guion_bajo`: `"16.20"` → `"16_20"`.
//! - Constantes de versión (`UGG_STATS_VERSION = "1.5"`,
//!   `UGG_GAME_MODE = "ranked_solo_5x5"`, `UGG_OVERVIEW_VERSION = "1.5.0"`)
//!   cambian por parche: si el endpoint empieza a dar 404, se actualizan
//!   acá (mismo mantenimiento que hace league-lean en `config.js`).
//!
//! La respuesta es un blob anidado `data[server][tier][position]`:
//!
//! - `server` = [`UGG_SERVER_ID`] (12, todas las regiones) y `tier` =
//!   [`UGG_TIER_ID`] (10, plat+) como claves numéricas en texto.
//! - `position` numérico: jungle=1, support=2, bottom=3, top=4, middle=5,
//!   none=6. Cada bucket es una lista de filas; la fila `[0]` es la página
//!   más jugada de esa posición.
//! - `pd[0]` (la fila de perks) = `[games, wins, primaryStyleId,
//!   subStyleId, [perk entries...], ...]`; cada entry puede ser `id` plano
//!   o `[id, ...]` (se toma el primero). `pd[8][2]` = shards como strings
//!   (`"5008"`).
//!
//! Si la posición pedida no tiene datos, se usa la posición con más juegos
//! (escaneando 1..=5 con `games = fila[0]`, igual que league-lean; la
//! etiqueta de la página lo indica). U.GG entrega UNA página (su
//! recomendada plat+).
//!
//! **NOTA verificada 2026-10 en vivo**: `stats2.u.gg` responde **403**
//! (challenge de Cloudflare, "Just a moment...") a clientes no-navegador,
//! con o sin User-Agent de Chrome. El fallback queda implementado con el
//! formato de la referencia por si U.GG lo reabre (o cambia de endpoint),
//! y ante el 403 el orquestador reporta el error limpio en [`super`].

use serde_json::Value;

use super::{keystone_of, RunePageStats, SourceError, SourceKind};
use crate::data_dragon::runes::RuneCatalog;
use crate::lcu::models::Position;

/// Base del CDN de estadísticas de U.GG.
pub const UGG_BASE: &str = "https://stats2.u.gg/lol";

/// Versión mayor del formato de stats (cambia por parche; ver doc del
/// módulo).
pub const UGG_STATS_VERSION: &str = "1.5";

/// Modo de juego de la cola consultada (ranked solo/duo).
pub const UGG_GAME_MODE: &str = "ranked_solo_5x5";

/// Versión menor del formato de overview (cambia por parche).
pub const UGG_OVERVIEW_VERSION: &str = "1.5.0";

/// Bucket de servidor: todas las regiones.
pub const UGG_SERVER_ID: i64 = 12;

/// Bucket de tier: platinum+.
pub const UGG_TIER_ID: i64 = 10;

/// Segmento de parche para la URL: `"16.20"` → `"16_20"`.
#[must_use]
pub fn patch_segment(patch: &str) -> String {
    patch.replace('.', "_")
}

/// URL del overview para un campeón y parche.
#[must_use]
pub fn url_for(champion_id: i64, patch: &str) -> String {
    format!(
        "{UGG_BASE}/{UGG_STATS_VERSION}/overview/{}/{UGG_GAME_MODE}/{champion_id}/{UGG_OVERVIEW_VERSION}.json",
        patch_segment(patch)
    )
}

/// Código numérico de posición de U.GG (`None` → 6 "none").
#[must_use]
pub fn position_code(position: Option<Position>) -> i64 {
    match position {
        Some(Position::Top) => 4,
        Some(Position::Jungle) => 1,
        Some(Position::Middle) => 5,
        Some(Position::Bottom) => 3,
        Some(Position::Utility) => 2,
        None => 6,
    }
}

/// Inverso parcial de [`position_code`], para etiquetar el fallback.
fn position_from_code(code: i64) -> Option<Position> {
    match code {
        4 => Some(Position::Top),
        1 => Some(Position::Jungle),
        5 => Some(Position::Middle),
        3 => Some(Position::Bottom),
        2 => Some(Position::Utility),
        _ => None,
    }
}

/// Número entero tolerante: JSON number (int o float redondeado) o string
/// decimal (formato real de los shards de U.GG).
fn as_i64(value: &Value) -> Option<i64> {
    match value {
        Value::Number(number) => number
            .as_i64()
            .or_else(|| number.as_f64().map(|float| float.round() as i64)),
        Value::String(text) => text.trim().parse::<i64>().ok(),
        _ => None,
    }
}

/// Ídem [`as_i64`] para conteos no negativos.
fn as_u64(value: &Value) -> Option<u64> {
    match value {
        Value::Number(number) => number
            .as_u64()
            .or_else(|| number.as_f64().map(|float| float.round().max(0.0) as u64)),
        Value::String(text) => text.trim().parse::<u64>().ok(),
        _ => None,
    }
}

/// Entry de runa: `id` plano o `[id, ...]` (tomar el primero).
fn perk_entry_id(entry: &Value) -> Option<i64> {
    match entry {
        Value::Array(parts) => parts.first().and_then(as_i64),
        _ => as_i64(entry),
    }
}

/// Bucket `data[server][tier]` del blob; falta → error claro.
fn tier_bucket(value: &Value) -> Result<&Value, SourceError> {
    value
        .get("data")
        .and_then(|data| data.get(UGG_SERVER_ID.to_string()))
        .and_then(|server| server.get(UGG_TIER_ID.to_string()))
        .ok_or_else(|| {
            SourceError::Ugg(format!(
                "blob sin data[{UGG_SERVER_ID}][{UGG_TIER_ID}]"
            ))
        })
}

/// ¿Es una fila utilizable? (array no vacío).
fn row_is_usable(row: &Value) -> bool {
    row.as_array().is_some_and(|rows| !rows.is_empty())
}

/// Fila con más juegos entre las posiciones 1..=5 (league-lean no escanea
/// la posición 6 "none" para el fallback). `(código, fila)`.
fn most_played_row(tier: &Value) -> Result<(i64, &Value), SourceError> {
    let mut best: Option<(u64, i64, &Value)> = None;
    for code in 1..=5i64 {
        let Some(row) = tier
            .get(code.to_string())
            .and_then(|bucket| bucket.get(0))
            .filter(|row| row_is_usable(row))
        else {
            continue;
        };
        let Some(games) = row.get(0).and_then(as_u64) else {
            continue;
        };
        if best.is_none_or(|(best_games, _, _)| games > best_games) {
            best = Some((games, code, row));
        }
    }
    best.map(|(_, code, row)| (code, row)).ok_or_else(|| {
        SourceError::Ugg(String::from("ninguna posición tiene datos de runas"))
    })
}

/// Parsea la fila de perks (`[games, wins, primaryStyleId, subStyleId,
/// [entries...], ...]`) a la forma normalizada. Shards ausentes → vacío.
fn parse_row(row: &Value, label: &str, runes: Option<&RuneCatalog>) -> Result<RunePageStats, SourceError> {
    let missing = |what: &str| {
        SourceError::Ugg(format!("fila de runas malformada: falta {what}"))
    };

    let games = row.get(0).and_then(as_u64).ok_or_else(|| missing("games"))?;
    let wins = row.get(1).and_then(as_u64).unwrap_or(0);
    let primary_style_id = row
        .get(2)
        .and_then(as_i64)
        .ok_or_else(|| missing("primaryStyleId"))?;
    let sub_style_id = row
        .get(3)
        .and_then(as_i64)
        .ok_or_else(|| missing("subStyleId"))?;
    let perk_entries = row
        .get(4)
        .and_then(Value::as_array)
        .ok_or_else(|| missing("lista de runas"))?;
    let perk_ids: Vec<i64> = perk_entries.iter().filter_map(perk_entry_id).collect();
    if perk_ids.is_empty() {
        return Err(SourceError::Ugg(String::from(
            "fila de runas sin runas utilizables",
        )));
    }

    let stat_shards: Vec<i64> = row
        .get(8)
        .and_then(|block| block.get(2))
        .and_then(Value::as_array)
        .map(|shards| shards.iter().filter_map(as_i64).collect())
        .unwrap_or_default();

    // Layout u.gg: las primeras 4 runas son del árbol principal, el resto
    // del secundario. La keystone vive en las primeras.
    let primary_perks = &perk_ids[..perk_ids.len().min(4)];

    let winrate_pct = if games > 0 {
        wins as f64 / games as f64 * 100.0
    } else {
        0.0
    };

    Ok(RunePageStats {
        source: SourceKind::Ugg,
        label: label.to_string(),
        games,
        wins,
        winrate_pct,
        primary_style_id,
        sub_style_id,
        keystone_id: keystone_of(primary_perks, runes),
        perk_ids,
        stat_shards,
    })
}

/// Parseo puro del blob de overview (testeado sin red): fila de la posición
/// pedida, o la de la posición más jugada si aquella no tiene datos.
pub(crate) fn parse_overview(
    value: &Value,
    wanted: Option<Position>,
    runes: Option<&RuneCatalog>,
) -> Result<Vec<RunePageStats>, SourceError> {
    let tier = tier_bucket(value)?;

    let requested = tier
        .get(position_code(wanted).to_string())
        .and_then(|bucket| bucket.get(0))
        .filter(|row| row_is_usable(row));
    if let Some(row) = requested {
        return Ok(vec![parse_row(row, "U.GG · plat+ recomendada", runes)?]);
    }

    let (code, row) = most_played_row(tier)?;
    let label = match position_from_code(code) {
        Some(position) => format!(
            "U.GG · plat+ (posición más jugada: {})",
            super::lane_of(Some(position)).unwrap_or("sin rol")
        ),
        None => String::from("U.GG · plat+ (posición más jugada)"),
    };
    Ok(vec![parse_row(row, &label, runes)?])
}

/// Descarga la página recomendada de U.GG para `champion_id + posición +
/// parche`. Siempre devuelve 0 o 1 página (U.GG expone una sola).
pub async fn fetch_rune_page(
    client: &reqwest::Client,
    champion_id: i64,
    position: Option<Position>,
    patch: &str,
    runes: Option<&RuneCatalog>,
) -> Result<Vec<RunePageStats>, SourceError> {
    let url = url_for(champion_id, patch);
    let value = client
        .get(&url)
        .send()
        .await
        .and_then(|response| response.error_for_status())
        .map_err(|error| SourceError::Ugg(format!("GET {url} falló: {error}")))?
        .json::<Value>()
        .await
        .map_err(|error| SourceError::Ugg(format!("respuesta de {url} no es JSON: {error}")))?;

    parse_overview(&value, position, runes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_dragon::runes::{RuneDef, RuneTree};
    use serde_json::json;
    use std::collections::HashMap;

    /// Blob con dos posiciones: top ("4") con 1000 juegos y jungle ("1")
    /// con 5000. La fila mezcla entries planas y arrays (formato real de
    /// u.gg) y trae shards como strings en `fila[8][2]`.
    fn blob() -> Value {
        json!({
            "data": {
                "12": {
                    "10": {
                        "4": [
                            [1000, 520, 8000, 8200,
                             [8005, [8009], 9103, 8014, 8345, [8304]],
                             0, 0, 0,
                             ["ofensa", "flex", ["5005", "5008", "5001"]]]
                        ],
                        "1": [
                            [5000, 2600, 8100, 8000,
                             [8112, 8139, 8126, 8100, 8347, 8304],
                             0, 0, 0,
                             ["ofensa", "flex", ["5008", "5008", "5002"]]]
                        ]
                    }
                }
            }
        })
    }

    /// Catálogo mínimo: Press the Attack (8005) keystone de Precisión.
    fn catalogo() -> RuneCatalog {
        let mut by_id = HashMap::new();
        by_id.insert(
            8005_i64,
            RuneDef {
                id: 8005,
                key: String::from("PressTheAttack"),
                name: String::from("Press the Attack"),
                icon_path: String::new(),
                tree_id: 8000,
                slot_index: 0,
                is_keystone: true,
                short_desc: String::new(),
            },
        );
        RuneCatalog {
            version: String::from("16.20.1"),
            trees: vec![RuneTree::default()],
            by_id,
        }
    }

    #[test]
    fn url_y_segmento_de_parche() {
        assert_eq!(patch_segment("16.20"), "16_20");
        assert_eq!(patch_segment("9.9"), "9_9");
        assert_eq!(
            url_for(498, "16.20"),
            "https://stats2.u.gg/lol/1.5/overview/16_20/ranked_solo_5x5/498/1.5.0.json"
        );
    }

    /// Mapeo de posiciones (códigos del spec/league-lean).
    #[test]
    fn codigos_de_posicion_de_ugg() {
        assert_eq!(position_code(Some(Position::Top)), 4);
        assert_eq!(position_code(Some(Position::Jungle)), 1);
        assert_eq!(position_code(Some(Position::Middle)), 5);
        assert_eq!(position_code(Some(Position::Bottom)), 3);
        assert_eq!(position_code(Some(Position::Utility)), 2);
        assert_eq!(position_code(None), 6);
    }

    /// Posición pedida con datos: una página con winrate derivado
    /// (wins/games·100), entries mixtas aplanadas y shards parseados de
    /// strings. Keystone por convención (primera runa del primario).
    #[test]
    fn parsea_la_posicion_pedida_con_entries_mixtas_y_shards() {
        let pages = parse_overview(&blob(), Some(Position::Top), None).expect("parsea");

        assert_eq!(pages.len(), 1);
        let page = &pages[0];
        assert_eq!(page.source, SourceKind::Ugg);
        assert_eq!(page.label, "U.GG · plat+ recomendada");
        assert_eq!(page.games, 1000);
        assert_eq!(page.wins, 520);
        assert!((page.winrate_pct - 52.0).abs() < 1e-9);
        // Los estilos vienen directos de la fila (no hay índice que resolver).
        assert_eq!(page.primary_style_id, 8000);
        assert_eq!(page.sub_style_id, 8200);
        assert_eq!(
            page.perk_ids,
            vec![8005, 8009, 9103, 8014, 8345, 8304]
        );
        assert_eq!(page.stat_shards, vec![5005, 5008, 5001]);
        // Sin catálogo: primera runa del primario.
        assert_eq!(page.keystone_id, Some(8005));
    }

    /// Con catálogo, la keystone se resuelve contra `is_keystone`.
    #[test]
    fn keystone_resuelve_con_el_catalogo() {
        let pages = parse_overview(&blob(), Some(Position::Top), Some(&catalogo()))
            .expect("parsea");
        assert_eq!(pages[0].keystone_id, Some(8005));
    }

    /// Posición pedida sin datos (Utility=2 no existe) → la posición con
    /// más juegos (jungle, 5000), con etiqueta de fallback.
    #[test]
    fn posicion_vacia_cae_a_la_mas_jugada() {
        let pages = parse_overview(&blob(), Some(Position::Utility), None).expect("parsea");

        assert_eq!(pages.len(), 1);
        let page = &pages[0];
        assert_eq!(page.games, 5000);
        assert_eq!(page.wins, 2600);
        assert!((page.winrate_pct - 52.0).abs() < 1e-9);
        assert_eq!(page.primary_style_id, 8100);
        assert_eq!(page.sub_style_id, 8000);
        assert_eq!(page.keystone_id, Some(8112));
        assert_eq!(
            page.label,
            "U.GG · plat+ (posición más jugada: jungle)"
        );
    }

    /// Sin posición pedida el código es 6 ("none"); si ese bucket no existe
    /// también cae al más jugado.
    #[test]
    fn sin_posicion_cae_al_mas_jugado_cuando_none_no_tiene_datos() {
        let pages = parse_overview(&blob(), None, None).expect("parsea");
        assert_eq!(pages[0].games, 5000);
    }

    /// Fila sin shards (menos de 9 elementos) → shards vacíos, sin error.
    #[test]
    fn fila_sin_shards_es_tolerada() {
        let value = json!({
            "data": { "12": { "10": { "4": [ [100, 51, 8000, 8200, [8005]] ] } } }
        });

        let pages = parse_overview(&value, Some(Position::Top), None).expect("parsea");
        assert!(pages[0].stat_shards.is_empty());
        assert_eq!(pages[0].games, 100);
        assert_eq!(pages[0].wins, 51);
    }

    /// Blob sin bucket de tier, o con todas las posiciones vacías → error
    /// claro de U.GG.
    #[test]
    fn blobs_vacios_dan_error_claro() {
        let error = parse_overview(&json!({ "data": {} }), Some(Position::Top), None)
            .expect_err("sin tier");
        assert!(error.to_string().contains("data[12][10]"));

        let error = parse_overview(
            &json!({ "data": { "12": { "10": { "4": [] } } } }),
            Some(Position::Top),
            None,
        )
        .expect_err("todas vacías");
        assert!(error.to_string().contains("ninguna posición"));
    }
}
