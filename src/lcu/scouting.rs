//! Scouting on-demand de un jugador del draft (estilo Porofessor): dado el
//! `summonerId` numérico que el champ select reporta por miembro, devuelve
//! nombre (Riot ID), nivel, icono de perfil y rank solo-queue de la
//! temporada.
//!
//! Rutas (mismo cliente REST que el resto de la app):
//!
//! - Invocador: `GET /lol-summoner/v1/summoners/{summonerId}` → mismo shape
//!   que `current-summoner`, se parsea directo al [`SummonerInfo`] ya
//!   existente (verificado en `lcu::models` contra el cliente de LA2).
//! - Ranked: `GET /lol-ranked/v1/ranked-stats/{puuid}` → `queueMap`; solo
//!   se lee la key `RANKED_SOLO_5x5` con la misma normalización de
//!   `tier`/`division` (`""`/`NA`/`NONE` → sin rank) que `resolve_solo_queue`
//!   en [`crate::profile::lcu_fetch`]. Ese fetcher es privado al módulo de
//!   perfil, así que acá va la llamada mínima local con el criterio copiado
//!   (fuente citada en cada helper).
//!
//! Errores como `String` legible para la UI (rioplatense, sin ruido
//! técnico); el detalle técnico va al log. Nunca entra en pánico: un
//! invocador sin datos de ranked degrada a `rank: None` ("SIN RANK") y un
//! ranked ilegible no bloquea el resto del scout.

use std::collections::HashMap;

use irelia::requests::Error as IreliaError;
use serde::Deserialize;

use crate::lcu::models::SummonerInfo;
use crate::profile::client_handle::LcuRestClient;
use crate::profile::models::RankedInfo;

/// Endpoint del invocador por id numérico: la ruta lleva el id **directo en
/// el path** (misma respuesta que `current-summoner`, pero de cualquier
/// jugador, no solo el local).
const SUMMONER_URI_TEMPLATE: &str = "/lol-summoner/v1/summoners";
/// Endpoint de ranked-stats por puuid (misma ruta que usa el perfil).
const RANKED_URI_TEMPLATE: &str = "/lol-ranked/v1/ranked-stats";
/// Key del `queueMap` que se lee: solo la solo-queue clásica. El queueMap
/// también trae `JADE_RANKED_SOLO_5x5` (cola nueva 2026), `RANKED_FLEX_SR`,
/// `RANKED_TFT`, etc.: se ignoran (ver doc de [`crate::profile::lcu_fetch`]).
const SOLO_QUEUE_KEY: &str = "RANKED_SOLO_5x5";

// ---------------------------------------------------------------------------
// Modelos de salida
// ---------------------------------------------------------------------------

/// Resumen ranked soloq de un invocador (datos de `/lol-ranked`).
///
/// A diferencia de [`RankedInfo`] (tolerante, con `Option` para todo), acá
/// los campos ya vienen normalizados: el fetcher solo construye un
/// `RankResumen` cuando hay tier y division reales; `None` en el scout es
/// "unranked / sin datos".
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RankResumen {
    /// Liga tal como la nombra el LCU (ej. `"GOLD"`); la traducción al
    /// español la hace [`etiqueta_rank`] al mostrar.
    pub tier: String,
    /// División (ej. `"II"`).
    pub division: String,
    /// Victorias de la temporada.
    pub wins: u32,
    /// Derrotas de la temporada.
    pub losses: u32,
}

impl RankResumen {
    /// Partidas jugadas en la cola (wins + losses).
    #[must_use]
    pub fn games(&self) -> u32 {
        self.wins.saturating_add(self.losses)
    }

    /// Winrate en porcentaje 0-100. Sin partidas → `0.0` (caso borde: el
    /// fetcher no llega acá con una cuenta sin ranked, porque tier/division
    /// vacíos se descartan antes).
    #[must_use]
    pub fn winrate(&self) -> f64 {
        let games = f64::from(self.games());
        if games > 0.0 {
            f64::from(self.wins) / games * 100.0
        } else {
            0.0
        }
    }
}

/// Todo lo que Klify sabe de un jugador del draft, listo para el panel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScoutInfo {
    /// Id numérico del invocador (el mismo que se pidió).
    pub summoner_id: i64,
    /// `gameName` del Riot ID, o `"Invocador"` si el payload no lo trajo
    /// (o vino vacío).
    pub name: String,
    /// Parte del Riot ID después del `#` (ej. `"Akali"`); `None` en cuentas
    /// legacy sin Riot ID.
    pub tag_line: Option<String>,
    /// Nivel de invocador (`0` si el payload no lo trae, sesiones cargando).
    pub level: i64,
    /// Id del icono de perfil (para la URL de Data Dragon); `0` si falta.
    pub profile_icon_id: i64,
    /// PUUID de la cuenta (vacío si el payload no lo trajo: en ese caso no
    /// hay ranked que pedir).
    pub puuid: String,
    /// Ranked solo-queue de la temporada; `None` = unranked / sin datos.
    pub rank: Option<RankResumen>,
}

// ---------------------------------------------------------------------------
// DTOs de transporte (mismo subconjunto tolerante que el perfil)
// ---------------------------------------------------------------------------

/// `queueMap` de `/lol-ranked/v1/ranked-stats/{puuid}`: objeto keyed por
/// tipo de cola; el resto del payload (`highestRankedEntry`, splits, ...) se
/// ignora. Criterio copiado de `RankedStatsDto` en
/// [`crate::profile::lcu_fetch`] (privado allá): las entradas se modelan con
/// [`RankedInfo`], cuyos renames/defaults cubren el payload real.
#[derive(Debug, Default, Deserialize)]
struct RankedStatsDto {
    #[serde(rename = "queueMap", default)]
    queue_map: HashMap<String, RankedInfo>,
}

// ---------------------------------------------------------------------------
// Helpers puros (testeables sin red) — criterio citado del perfil
// ---------------------------------------------------------------------------

/// Normaliza un valor de rank del LCU (`tier`/`division`): `""`, `"NA"` y
/// `"NONE"` significan "sin dato" y pasan a `None` (cuenta sin ranked o
/// provisional). Copia exacta de `clean_rank_value` en
/// [`crate::profile::lcu_fetch`] (privado allá), con la misma fuente: la
/// cuenta de prueba de LA2 llega con `tier: ""` / `division: "NA"`.
#[must_use]
fn clean_rank_value(raw: Option<String>) -> Option<String> {
    let trimmed = raw?.trim().to_string();
    match trimmed.to_ascii_uppercase().as_str() {
        "" | "NA" | "NONE" => None,
        _ => Some(trimmed),
    }
}

/// Conteo `i64` del payload a `u32` sin pánico: `None`/negativo → `0`,
/// fuera de rango → saturado en `u32::MAX` (los conteos reales nunca se
/// acercan; es pura defensa contra payloads raros).
#[must_use]
fn conteo_u32(raw: Option<i64>) -> u32 {
    u32::try_from(raw.unwrap_or(0).max(0)).unwrap_or(u32::MAX)
}

/// Convierte una entrada solo-queue del queueMap (ya parseada a
/// [`RankedInfo`]) al [`RankResumen`] del panel.
///
/// Criterio de `resolve_solo_queue` ([`crate::profile::lcu_fetch`]): tier o
/// division normalizados a `None` (`""`/`NA`/`NONE`) → sin rank; acá, como
/// [`RankResumen`] no lleva `Option`, la entrada se descarta entera (la UI
/// muestra "SIN RANK").
#[must_use]
fn rank_resumen_desde(entry: &RankedInfo) -> Option<RankResumen> {
    let tier = clean_rank_value(entry.tier.clone())?;
    let division = clean_rank_value(entry.division.clone())?;
    Some(RankResumen {
        tier,
        division,
        wins: conteo_u32(entry.wins),
        losses: conteo_u32(entry.losses),
    })
}

/// Resuelve la entrada solo-queue del queueMap completo. Si la key
/// [`SOLO_QUEUE_KEY`] no está (payload distinto al esperado), loguea las
/// keys disponibles y devuelve `None` — mismo comportamiento que
/// `resolve_solo_queue` en [`crate::profile::lcu_fetch`].
#[must_use]
fn resumen_solo_queue(stats: &RankedStatsDto) -> Option<RankResumen> {
    match stats.queue_map.get(SOLO_QUEUE_KEY) {
        Some(entry) => rank_resumen_desde(entry),
        None => {
            let mut keys: Vec<&str> = stats.queue_map.keys().map(String::as_str).collect();
            keys.sort_unstable();
            eprintln!(
                "klipy [scouting]: aviso: ranked-stats sin la key {SOLO_QUEUE_KEY:?}; keys: {keys:?}"
            );
            None
        }
    }
}

/// Traducción de tiers del LCU (inglés) al español para el panel. Un tier
/// desconocido viaja en mayúsculas, tal cual llegó (nunca vacío).
#[must_use]
fn tier_en_espanol(tier: &str) -> String {
    match tier.trim().to_ascii_uppercase().as_str() {
        "IRON" => String::from("HIERRO"),
        "BRONZE" => String::from("BRONCE"),
        "SILVER" => String::from("PLATA"),
        "GOLD" => String::from("ORO"),
        "PLATINUM" => String::from("PLATINO"),
        "EMERALD" => String::from("ESMERALDA"),
        "DIAMOND" => String::from("DIAMANTE"),
        "MASTER" => String::from("MAESTRO"),
        "GRANDMASTER" => String::from("GRANMAESTRO"),
        "CHALLENGER" => String::from("RETADOR"),
        _ => tier.trim().to_ascii_uppercase(),
    }
}

/// Mapeo puro de [`SummonerInfo`] (payload crudo) a [`ScoutInfo`]: nombre
/// con fallback `"Invocador"` (ausente o vacío), defaults en `0` para
/// nivel/icono y puuid vacío si falta. El `summoner_id` viaja del parámetro
/// (es el id que se pidió, no el que el payload repita).
#[must_use]
fn scout_desde_summoner(
    summoner_id: i64,
    summoner: SummonerInfo,
    rank: Option<RankResumen>,
) -> ScoutInfo {
    let SummonerInfo {
        puuid,
        game_name,
        tag_line,
        summoner_level,
        profile_icon_id,
        ..
    } = summoner;

    let name = game_name
        .map(|nombre| nombre.trim().to_string())
        .filter(|nombre| !nombre.is_empty())
        .unwrap_or_else(|| String::from("Invocador"));

    ScoutInfo {
        summoner_id,
        name,
        tag_line,
        level: summoner_level.unwrap_or(0),
        profile_icon_id: profile_icon_id.unwrap_or(0),
        puuid: puuid.unwrap_or_default(),
        rank,
    }
}

// ---------------------------------------------------------------------------
// Etiquetas para la UI (puras, testeadas)
// ---------------------------------------------------------------------------

/// Etiqueta de rank para el panel: `"ORO II · 54% · 120 PARTIDAS"`
/// (tier traducido al español, WR redondeado); `"SIN RANK"` si no hay
/// datos. Pura y testeada.
#[must_use]
pub fn etiqueta_rank(rank: Option<&RankResumen>) -> String {
    let Some(rank) = rank else {
        return String::from("SIN RANK");
    };

    let tier = tier_en_espanol(&rank.tier);
    let division = rank.division.trim().to_ascii_uppercase();
    let wr = rank.winrate().round() as u64;
    let games = rank.games();

    if division.is_empty() {
        format!("{tier} · {wr}% · {games} PARTIDAS")
    } else {
        format!("{tier} {division} · {wr}% · {games} PARTIDAS")
    }
}

/// Clase de color del WR para el panel: `"jugador-wr--verde"` con WR
/// mayor a 50, `"jugador-wr--rojo"` menor a 50, `""` en 50 justo o sin
/// rank. Pura y testeada.
#[must_use]
pub fn clase_wr_rank(rank: Option<&RankResumen>) -> &'static str {
    let Some(rank) = rank else {
        return "";
    };
    let wr = rank.winrate();
    if wr > 50.0 {
        "jugador-wr--verde"
    } else if wr < 50.0 {
        "jugador-wr--rojo"
    } else {
        ""
    }
}

// ---------------------------------------------------------------------------
// Fetchers async (con el cliente REST global del perfil)
// ---------------------------------------------------------------------------

/// GET del invocador por id numérico. 404 → el invocador no existe en este
/// servidor (mensaje para la UI); otros errores → mensaje corto, con el
/// detalle técnico al log.
async fn fetch_summoner(client: &LcuRestClient, summoner_id: i64) -> Result<SummonerInfo, String> {
    let uri = format!("{SUMMONER_URI_TEMPLATE}/{summoner_id}");
    match client.get::<SummonerInfo>(&uri).await {
        Ok(summoner) => Ok(summoner),
        Err(IreliaError::Request(status)) if status.as_u16() == 404 => {
            Err(String::from("no encontré ese invocador"))
        }
        Err(error) => {
            eprintln!("klipy [scouting]: no se pudo leer el invocador {summoner_id}: {error}");
            Err(String::from("no pude leer el invocador desde el cliente"))
        }
    }
}

/// GET del ranked solo-queue del puuid dado. 404 (cuenta sin datos de
/// ranked en este servidor), payload sin solo-queue o error de red → `None`
/// sin ruido (solo log): el scout muestra "SIN RANK" y no bloquea el resto.
/// Criterio idéntico a `fetch_ranked` en [`crate::profile::lcu_fetch`]
/// (privado allá, por eso la llamada mínima local).
async fn fetch_rank_resumen(client: &LcuRestClient, puuid: &str) -> Option<RankResumen> {
    let uri = format!("{RANKED_URI_TEMPLATE}/{puuid}");
    match client.get::<RankedStatsDto>(&uri).await {
        Ok(stats) => resumen_solo_queue(&stats),
        Err(IreliaError::Request(status)) if status.as_u16() == 404 => None,
        Err(error) => {
            eprintln!("klipy [scouting]: no se pudo leer el ranked de {puuid}: {error}");
            None
        }
    }
}

/// Resuelve un miembro del draft: summoner por id → puuid → ranked soloq.
///
/// Un id inválido (≤ 0) o un 404 del summoner → `Err("no encontré ese
/// invocador")`; otros errores de red → `Err` con mensaje corto. El ranked
/// nunca falla el scout: sin datos (unranked, puuid ausente o error) viaja
/// `rank: None`.
pub async fn buscar_scout(client: &LcuRestClient, summoner_id: i64) -> Result<ScoutInfo, String> {
    if summoner_id <= 0 {
        return Err(String::from("no encontré ese invocador"));
    }

    let summoner = fetch_summoner(client, summoner_id).await?;
    let puuid = summoner.puuid.clone().unwrap_or_default();
    let rank = if puuid.is_empty() {
        None
    } else {
        fetch_rank_resumen(client, &puuid).await
    };

    Ok(scout_desde_summoner(summoner_id, summoner, rank))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Rank de prueba: ORO II con 65-55 (54.16% → redondea a 54%).
    fn rank_de_prueba(wins: u32, losses: u32) -> RankResumen {
        RankResumen {
            tier: String::from("GOLD"),
            division: String::from("II"),
            wins,
            losses,
        }
    }

    /// Etiqueta con rank: tier traducido, WR redondeado y partidas.
    #[test]
    fn etiqueta_rank_traduce_y_formatea() {
        let rank = rank_de_prueba(65, 55);
        assert_eq!(rank.games(), 120);
        assert!((rank.winrate() - 65.0 / 120.0 * 100.0).abs() < 1e-9);
        assert_eq!(etiqueta_rank(Some(&rank)), "ORO II · 54% · 120 PARTIDAS");
    }

    /// Sin rank → "SIN RANK".
    #[test]
    fn etiqueta_rank_sin_rank_da_sin_rank() {
        assert_eq!(etiqueta_rank(None), "SIN RANK");
    }

    /// Rank con cero partidas (borde): WR 0% pero etiqueta válida, sin NaN
    /// ni pánico.
    #[test]
    fn etiqueta_rank_con_cero_partidas_muestra_cero() {
        let rank = rank_de_prueba(0, 0);
        assert_eq!(rank.games(), 0);
        assert_eq!(rank.winrate(), 0.0);
        assert_eq!(etiqueta_rank(Some(&rank)), "ORO II · 0% · 0 PARTIDAS");
    }

    /// Tier desconocido (payload futuro o mod): viaja en mayúsculas, tal
    /// cual llegó, y la etiqueta no se rompe.
    #[test]
    fn etiqueta_rank_con_tier_desconocido_pasa_en_mayusculas() {
        let mut rank = rank_de_prueba(10, 10);
        rank.tier = String::from("wood");
        assert_eq!(etiqueta_rank(Some(&rank)), "WOOD II · 50% · 20 PARTIDAS");
    }

    /// Clase de color según WR: verde arriba de 50, rojo abajo, vacío en 50
    /// justo y sin rank.
    #[test]
    fn clase_wr_rank_segun_el_winrate() {
        assert_eq!(clase_wr_rank(Some(&rank_de_prueba(60, 40))), "jugador-wr--verde");
        assert_eq!(clase_wr_rank(Some(&rank_de_prueba(30, 40))), "jugador-wr--rojo");
        assert_eq!(clase_wr_rank(Some(&rank_de_prueba(5, 5))), "");
        assert_eq!(clase_wr_rank(None), "");
    }

    /// Payload REAL de ranked-stats de una cuenta sin ranked (recorte del
    /// cliente de LA2, mismo fixture que el perfil): tier `""` / division
    /// `"NA"` → `None`, la UI muestra "SIN RANK".
    #[test]
    fn resumen_solo_queue_con_cuenta_sin_rank_da_none() {
        let stats: RankedStatsDto = serde_json::from_value(json!({
            "queueMap": {
                "JADE_RANKED_SOLO_5x5": { "queueType": "JADE_RANKED_SOLO_5x5", "tier": "", "division": "NA" },
                "RANKED_SOLO_5x5": {
                    "queueType": "RANKED_SOLO_5x5",
                    "tier": "",
                    "division": "NA",
                    "isProvisional": true,
                    "leaguePoints": 0,
                    "losses": 0,
                    "wins": 0
                }
            }
        }))
        .expect("payload real parsea");

        assert!(resumen_solo_queue(&stats).is_none());
    }

    /// Cuenta rankeada (sintético con la misma forma del payload real):
    /// GOLD I 30-20 → resumen con games 50 y WR 60%, y la etiqueta traduce
    /// a "ORO I".
    #[test]
    fn resumen_solo_queue_parsea_un_rank_valido() {
        let stats: RankedStatsDto = serde_json::from_value(json!({
            "queueMap": {
                "RANKED_SOLO_5x5": {
                    "queueType": "RANKED_SOLO_5x5",
                    "tier": "GOLD",
                    "division": "I",
                    "leaguePoints": 33,
                    "wins": 30,
                    "losses": 20
                }
            }
        }))
        .expect("payload parsea");

        let resumen = resumen_solo_queue(&stats).expect("hay solo-queue");
        assert_eq!(resumen.tier, "GOLD");
        assert_eq!(resumen.division, "I");
        assert_eq!(resumen.wins, 30);
        assert_eq!(resumen.losses, 20);
        assert_eq!(resumen.games(), 50);
        assert_eq!(resumen.winrate(), 60.0);
        assert_eq!(etiqueta_rank(Some(&resumen)), "ORO I · 60% · 50 PARTIDAS");
    }

    /// Payload sin la key solo-queue (solo TFT, por ejemplo) → `None` (y
    /// las keys disponibles van al log).
    #[test]
    fn resumen_solo_queue_sin_la_key_da_none() {
        let stats: RankedStatsDto = serde_json::from_value(json!({
            "queueMap": { "RANKED_TFT": { "queueType": "RANKED_TFT", "tier": "GOLD", "division": "I" } }
        }))
        .expect("payload parsea");

        assert!(resumen_solo_queue(&stats).is_none());
    }

    /// Entrada con tier `"NONE"` (otro valor "sin dato" del criterio del
    /// perfil) → `None` igual.
    #[test]
    fn resumen_solo_queue_con_tier_none_da_none() {
        let stats: RankedStatsDto = serde_json::from_value(json!({
            "queueMap": { "RANKED_SOLO_5x5": { "tier": "NONE", "division": "III", "wins": 1, "losses": 1 } }
        }))
        .expect("payload parsea");

        assert!(resumen_solo_queue(&stats).is_none());
    }

    /// La normalización `""`/`NA`/`NONE` → `None`, criterio copiado de
    /// `clean_rank_value` del perfil.
    #[test]
    fn clean_rank_value_normaliza_los_valores_sin_dato() {
        assert_eq!(clean_rank_value(None), None);
        assert_eq!(clean_rank_value(Some(String::new())), None);
        assert_eq!(clean_rank_value(Some(String::from("NA"))), None);
        assert_eq!(clean_rank_value(Some(String::from("NONE"))), None);
        assert_eq!(clean_rank_value(Some(String::from("none"))), None);
        assert_eq!(clean_rank_value(Some(String::from("  "))), None);
        assert_eq!(
            clean_rank_value(Some(String::from("PLATINUM"))),
            Some(String::from("PLATINUM"))
        );
    }

    /// Conteo tolerante: `None`/negativo → 0, fuera de rango → saturado,
    /// nunca pánico.
    #[test]
    fn conteo_u32_satura_sin_panico() {
        assert_eq!(conteo_u32(None), 0);
        assert_eq!(conteo_u32(Some(-5)), 0);
        assert_eq!(conteo_u32(Some(30)), 30);
        assert_eq!(conteo_u32(Some(i64::from(u32::MAX) + 100)), u32::MAX);
    }

    /// Mapeo `SummonerInfo` → `ScoutInfo`: payload REAL (recorte del cliente
    /// de LA2, ver `lcu::models`) mapea completo; y sin nombre (o vacío)
    /// cae al fallback "Invocador" con defaults en 0.
    #[test]
    fn scout_desde_summoner_mapea_y_cae_al_fallback() {
        let real: SummonerInfo = serde_json::from_value(json!({
            "accountId": 2440484679085024i64,
            "gameName": "PKT MidKing",
            "profileIconId": 6601,
            "puuid": "f7e2b22b-d677-558c-95af-495e9807ada3",
            "summonerId": 23727787,
            "summonerLevel": 391,
            "tagLine": "Akali"
        }))
        .expect("payload real parsea");

        let scout = scout_desde_summoner(23_727_787, real, Some(rank_de_prueba(65, 55)));
        assert_eq!(scout.summoner_id, 23_727_787);
        assert_eq!(scout.name, "PKT MidKing");
        assert_eq!(scout.tag_line.as_deref(), Some("Akali"));
        assert_eq!(scout.level, 391);
        assert_eq!(scout.profile_icon_id, 6601);
        assert_eq!(scout.puuid, "f7e2b22b-d677-558c-95af-495e9807ada3");
        assert_eq!(scout.rank, Some(rank_de_prueba(65, 55)));

        // Payload mínimo: nombre en blanco → "Invocador"; sin level/icono
        // → 0; sin puuid → vacío; sin rank → None.
        let anonimo: SummonerInfo =
            serde_json::from_value(json!({ "gameName": "   ", "puuid": null }))
                .expect("payload mínimo parsea");
        let scout = scout_desde_summoner(1, anonimo, None);
        assert_eq!(scout.name, "Invocador");
        assert_eq!(scout.tag_line, None);
        assert_eq!(scout.level, 0);
        assert_eq!(scout.profile_icon_id, 0);
        assert_eq!(scout.puuid, "");
        assert_eq!(scout.rank, None);
    }
}
