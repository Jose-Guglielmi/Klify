//! Fetchers LCU del perfil del invocador (FASE 11a-2), con el cliente REST
//! de irelia y las rutas **verificadas en vivo** contra un cliente de LA2
//! (2026-10, build 16.20):
//!
//! - Rank: `GET /lol-ranked/v1/ranked-stats/{puuid}` → `{ queueMap: {
//!   "RANKED_SOLO_5x5": {...}, ... } }`. La cuenta de prueba llega sin
//!   ranked (`tier: ""`, `division: "NA"`, `isProvisional: true`); el
//!   queueMap también trae `JADE_RANKED_SOLO_5x5` (cola nueva de la
//!   temporada 2026), `RANKED_FLEX_SR`, `RANKED_PREMADE_5x5`, `RANKED_TFT`
//!   y `RANKED_TFT_DOUBLE_UP`: solo se lee la key solo-queue clásica y si
//!   no está se loguean las keys disponibles.
//! - Maestría: `GET /lol-champion-mastery/v1/local-player/champion-mastery`
//!   (array de ~137 entradas). NOTA: los endpoints clásicos
//!   `/lol-champion-mastery/v1/champion-masteries*` responden 404
//!   "Invalid URI format" en los clientes actuales; esta es la ruta que el
//!   binario expone (confirmada en los strings del cliente y probada 200).
//! - Historial: `GET /lol-match-history/v1/products/lol/current-summoner/
//!   matches?begIndex=0&endIndex=19` → `{ accountId, games: { gameCount,
//!   games: [...] } }`. Cada game trae `participants` con UN único
//!   participante (el propio) más `participantIdentities` con el puuid: la
//!   selección del participante hace match por puuid y cae al único
//!   participante como fallback.
//! - Detalle: `GET /lol-match-history/v1/games/{gameId}` → mismo shape de
//!   game pero con los 10 participantes (los endpoints
//!   `.../matches/{gameId}` y `/lol-match-history/v1/products/lol/{puuid}/
//!   matches/{gameId}` responden 404 en el cliente actual).
//!
//! `build_profile_summary` es pura y testeada: une rank + top maestría +
//! historial en el [`ProfileSummary`] que publica el loop.

use std::cmp::Reverse;
use std::collections::HashMap;

use irelia::requests::{Error as IreliaError, RequestClientType};
use irelia::rest::LcuClient;
use serde::Deserialize;

use crate::lcu::models::SummonerInfo;
use crate::profile::models::{
    LcuMatchDetail, MasteryEntry, MatchSummary, ProfileSummary, RankedInfo, SeasonStats, TopChamp,
};

/// Cantidad de partidas recientes que se piden al historial.
pub const RECENT_MATCH_COUNT: i64 = 20;

/// Key del queueMap de ranked que se usa para el rank del perfil.
const SOLO_QUEUE_KEY: &str = "RANKED_SOLO_5x5";

/// Endpoint del invocador actual (misma fuente que `lcu::client`): da el
/// puuid que el rank y el historial necesitan.
const SUMMONER_URI: &str = "/lol-summoner/v1/current-summoner";
/// Endpoint de ranked-stats por puuid.
const RANKED_URI_TEMPLATE: &str = "/lol-ranked/v1/ranked-stats";
/// Endpoint de maestría del jugador local (ver doc del módulo).
const MASTERY_URI: &str = "/lol-champion-mastery/v1/local-player/champion-mastery";
/// Endpoint de historial del invocador actual (hasta 20 partidas).
const HISTORY_URI_TEMPLATE: &str =
    "/lol-match-history/v1/products/lol/current-summoner/matches?begIndex=0&endIndex=19";
/// Endpoint de detalle de partida (ver doc del módulo).
const MATCH_DETAIL_URI_TEMPLATE: &str = "/lol-match-history/v1/games";

// ---------------------------------------------------------------------------
// DTOs de transporte (subconjunto tolerante de cada payload)
// ---------------------------------------------------------------------------

/// `queueMap` de `/lol-ranked/v1/ranked-stats/{puuid}`: objeto keyed por
/// tipo de cola. El resto del payload (`highestRankedEntry`, splits, ...)
/// se ignora. Las entradas se modelan directo con [`RankedInfo`] (el
/// renames/defaults del modelo cubren el payload).
#[derive(Debug, Default, Deserialize)]
struct RankedStatsDto {
    #[serde(rename = "queueMap", default)]
    queue_map: HashMap<String, RankedInfo>,
}

/// Una entrada del array de maestría (subconjunto).
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MasteryDto {
    #[serde(default)]
    champion_id: i64,
    #[serde(default)]
    champion_level: i64,
    #[serde(default)]
    champion_points: i64,
}

/// Envoltorio del historial: `{ accountId, games: { gameCount, games } }`.
#[derive(Debug, Default, Deserialize)]
struct HistoryResponseDto {
    #[serde(default)]
    games: HistoryGamesDto,
}

/// Contenido de `games` (`gameCount` y fechas del payload se ignoran).
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HistoryGamesDto {
    #[serde(default)]
    games: Vec<HistoryGameDto>,
}

/// Un game del historial o del detalle: mismo shape en ambos endpoints.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HistoryGameDto {
    #[serde(default)]
    game_id: i64,
    #[serde(default)]
    game_duration: i64,
    #[serde(default)]
    queue_id: i64,
    #[serde(default)]
    game_creation: i64,
    #[serde(default)]
    participants: Vec<HistoryParticipantDto>,
    #[serde(default)]
    participant_identities: Vec<HistoryIdentityDto>,
}

/// Participante de un game (subconjunto: champ + stats propios).
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HistoryParticipantDto {
    #[serde(default)]
    participant_id: i64,
    #[serde(default)]
    champion_id: i64,
    #[serde(default)]
    stats: HistoryStatsDto,
}

/// Subconjunto de `stats` que alimenta [`MatchSummary`].
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HistoryStatsDto {
    #[serde(default)]
    win: bool,
    #[serde(default)]
    kills: i32,
    #[serde(default)]
    deaths: i32,
    #[serde(default)]
    assists: i32,
    #[serde(default)]
    total_minions_killed: i32,
    #[serde(default)]
    neutral_minions_killed: i32,
}

/// Identidad de participante (para resolver el puuid → participantId).
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HistoryIdentityDto {
    #[serde(default)]
    participant_id: i64,
    #[serde(default)]
    player: HistoryPlayerDto,
}

/// Subconjunto de `player` dentro de la identidad.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HistoryPlayerDto {
    #[serde(default)]
    puuid: Option<String>,
}

// ---------------------------------------------------------------------------
// Conversiones puras (DTO → modelo de dominio), testeadas con fixtures
// ---------------------------------------------------------------------------

/// Normaliza un valor de rank del LCU (`tier`/`division`): `""`, `"NA"` y
/// `"NONE"` significan "sin dato" y pasan a `None` (cuenta sin ranked o
/// provisional).
#[must_use]
fn clean_rank_value(raw: Option<String>) -> Option<String> {
    let trimmed = raw?.trim().to_string();
    match trimmed.to_ascii_uppercase().as_str() {
        "" | "NA" | "NONE" => None,
        _ => Some(trimmed),
    }
}

/// Resuelve la entrada solo-queue del queueMap, ya normalizada (`tier`/
/// `division` vacíos → `None`; `queue` de respaldo = la key del mapa).
/// Si la key [`SOLO_QUEUE_KEY`] no está (payload distinto al esperado),
/// loguea las keys disponibles y devuelve `None` (la UI muestra "sin
/// ranked").
#[must_use]
fn resolve_solo_queue(stats: &RankedStatsDto) -> Option<RankedInfo> {
    match stats.queue_map.get(SOLO_QUEUE_KEY) {
        Some(entry) => {
            let mut ranked = entry.clone();
            if ranked.queue.trim().is_empty() {
                ranked.queue = SOLO_QUEUE_KEY.to_string();
            }
            ranked.tier = clean_rank_value(ranked.tier);
            ranked.division = clean_rank_value(ranked.division);
            Some(ranked)
        }
        None => {
            let mut keys: Vec<&str> = stats.queue_map.keys().map(String::as_str).collect();
            keys.sort_unstable();
            eprintln!(
                "klipy [profile]: aviso: ranked-stats sin la key {SOLO_QUEUE_KEY:?}; keys: {keys:?}"
            );
            None
        }
    }
}

/// Participante propio dentro de un game del historial/detalle:
///
/// 1. Match por puuid vía `participantIdentities` → `participantId`.
/// 2. Fallback: si el game trae un único participante, ese (es la forma
///    real del endpoint `current-summoner`).
/// 3. Si no hay forma de identificarlo → `None` (la partida se descarta).
#[must_use]
fn my_participant<'a>(
    game: &'a HistoryGameDto,
    my_puuid: Option<&str>,
) -> Option<&'a HistoryParticipantDto> {
    if let Some(puuid) = my_puuid {
        let my_id = game
            .participant_identities
            .iter()
            .find(|identity| identity.player.puuid.as_deref() == Some(puuid))
            .map(|identity| identity.participant_id);
        if let Some(participant) = my_id.and_then(|id| {
            game.participants
                .iter()
                .find(|participant| participant.participant_id == id)
        }) {
            return Some(participant);
        }
    }

    if game.participants.len() == 1 {
        return game.participants.first();
    }
    None
}

/// Convierte un game del historial a [`MatchSummary`] usando los datos del
/// participante propio. `None` si el game no es interpretable (sin
/// participante propio).
#[must_use]
fn match_summary_from_game(
    game: &HistoryGameDto,
    my_puuid: Option<&str>,
) -> Option<MatchSummary> {
    let participant = my_participant(game, my_puuid)?;
    Some(MatchSummary {
        game_id: game.game_id,
        champion_id: participant.champion_id,
        win: participant.stats.win,
        kills: participant.stats.kills,
        deaths: participant.stats.deaths,
        assists: participant.stats.assists,
        duration_secs: game.game_duration,
        queue_id: game.queue_id,
        creation_ms: game.game_creation,
        cs: participant.stats.total_minions_killed + participant.stats.neutral_minions_killed,
    })
}

/// Convierte la respuesta completa del historial a la lista de
/// [`MatchSummary`], descartando (con aviso) los games sin participante
/// propio interpretable.
#[must_use]
fn match_summaries_from_history(
    response: &HistoryResponseDto,
    my_puuid: Option<&str>,
) -> Vec<MatchSummary> {
    response
        .games
        .games
        .iter()
        .filter_map(|game| {
            match_summary_from_game(game, my_puuid).or_else(|| {
                eprintln!(
                    "klipy [profile]: aviso: partida {} del historial sin participante propio interpretable; se descarta",
                    game.game_id
                );
                None
            })
        })
        .collect()
}

/// Top 3 de maestría por puntos (empates: orden estable del payload, que
/// ya viene ordenado por el LCU).
#[must_use]
fn top_mastery_entries(mastery: &[MasteryEntry]) -> Vec<MasteryEntry> {
    let mut sorted: Vec<MasteryEntry> = mastery.to_vec();
    sorted.sort_by_key(|entry| Reverse(entry.mastery_points));
    sorted.truncate(3);
    sorted
}

/// Une rank + maestría + historial en el [`ProfileSummary`] de la UI.
///
/// Pura y testeada (sin IO ni señales):
///
/// - `top_champs`: top 3 de [`MasteryEntry`] por puntos, enriquecidos con
///   games/wins contados sobre `history` (0 si el campeón no aparece en las
///   últimas N partidas — ver [`TopChamp`]).
/// - `recent`: el historial tal cual (ya aplanado a [`MatchSummary`]).
/// - `season`: agregado del historial ([`SeasonStats::from_history`]).
#[must_use]
pub fn build_profile_summary(
    ranked: Option<RankedInfo>,
    mastery: &[MasteryEntry],
    history: &[MatchSummary],
) -> ProfileSummary {
    let top_champs = top_mastery_entries(mastery)
        .into_iter()
        .map(|entry| {
            let mine: Vec<&MatchSummary> = history
                .iter()
                .filter(|game| game.champion_id == entry.champion_id)
                .collect();
            TopChamp {
                champion_id: entry.champion_id,
                mastery_level: entry.mastery_level,
                mastery_points: entry.mastery_points,
                games: mine.len() as i32,
                wins: mine.iter().filter(|game| game.win).count() as i32,
            }
        })
        .collect();

    ProfileSummary {
        rank: ranked,
        top_champs,
        recent: history.to_vec(),
        season: SeasonStats::from_history(history),
    }
}

// ---------------------------------------------------------------------------
// Fetchers async (con el cliente REST de irelia)
// ---------------------------------------------------------------------------

/// GET del invocador actual: `None` si el endpoint falla (se loguea; el
/// loop simplemente no actualiza en este ciclo).
async fn fetch_current_summoner(client: &LcuClient<RequestClientType>) -> Option<SummonerInfo> {
    match client.get::<SummonerInfo>(SUMMONER_URI).await {
        Ok(info) => Some(info),
        Err(error) => {
            eprintln!("klipy [profile]: no se pudo leer el invocador actual: {error}");
            None
        }
    }
}

/// GET de ranked-stats del puuid dado. 404 (cuenta sin datos de ranked en
/// este servidor) o payload sin solo-queue → `None` sin ruido; otros
/// errores se loguean.
async fn fetch_ranked(
    client: &LcuClient<RequestClientType>,
    puuid: &str,
) -> Option<RankedInfo> {
    let uri = format!("{RANKED_URI_TEMPLATE}/{puuid}");
    match client.get::<RankedStatsDto>(&uri).await {
        Ok(stats) => resolve_solo_queue(&stats),
        Err(IreliaError::Request(status)) if status.as_u16() == 404 => None,
        Err(error) => {
            eprintln!("klipy [profile]: no se pudo leer el ranked del invocador: {error}");
            None
        }
    }
}

/// GET de la maestría completa y recorte al top 3 por puntos. Fallo o
/// 404 → lista vacía (perfil sin top champs; no es fatal).
async fn fetch_top_mastery(client: &LcuClient<RequestClientType>) -> Vec<MasteryEntry> {
    let entries = match client.get::<Vec<MasteryDto>>(MASTERY_URI).await {
        Ok(entries) => entries,
        Err(IreliaError::Request(status)) if status.as_u16() == 404 => Vec::new(),
        Err(error) => {
            eprintln!("klipy [profile]: no se pudo leer la maestría: {error}");
            Vec::new()
        }
    };

    let mastery: Vec<MasteryEntry> = entries
        .into_iter()
        .map(|dto| MasteryEntry {
            champion_id: dto.champion_id,
            mastery_level: dto.champion_level,
            mastery_points: dto.champion_points,
        })
        .collect();
    top_mastery_entries(&mastery)
}

/// GET del historial reciente del invocador actual, aplanado a
/// [`MatchSummary`] del participante propio. Fallo → lista vacía.
async fn fetch_recent_history(
    client: &LcuClient<RequestClientType>,
    my_puuid: Option<&str>,
) -> Vec<MatchSummary> {
    let response = match client.get::<HistoryResponseDto>(HISTORY_URI_TEMPLATE).await {
        Ok(response) => response,
        Err(IreliaError::Request(status)) if status.as_u16() == 404 => {
            // Cuenta sin partidas (o sin historial visible): lista vacía.
            return Vec::new();
        }
        Err(error) => {
            eprintln!("klipy [profile]: no se pudo leer el historial: {error}");
            return Vec::new();
        }
    };

    let count = response.games.games.len();
    let summaries = match_summaries_from_history(&response, my_puuid);
    if summaries.len() != count {
        eprintln!(
            "klipy [profile]: aviso: el historial trajo {count} partidas y {} se descartaron",
            count - summaries.len()
        );
    }
    summaries
}

/// GET on-demand del detalle completo de una partida (para el modal de la
/// UI 11b). No vive en señales. El error de irelia se propaga tal cual
/// (el caller decide qué mostrar).
pub async fn fetch_match_detail(
    client: &LcuClient<RequestClientType>,
    game_id: i64,
) -> Result<LcuMatchDetail, IreliaError> {
    let uri = format!("{MATCH_DETAIL_URI_TEMPLATE}/{game_id}");
    client.get::<LcuMatchDetail>(&uri).await
}

/// Ciclo completo de fetch del perfil: summoner → rank + maestría +
/// historial → [`build_profile_summary`].
///
/// `None` solo si hasta el invocador falló (cliente muerto o login en
/// curso): en ese caso el loop conserva el último perfil válido en la
/// señal y reintenta en el próximo ciclo. Los fetch individuales que
/// fallan degradan el perfil (rank `None`, listas vacías) pero no lo
/// bloquean.
pub(crate) async fn fetch_profile_summary(
    client: &LcuClient<RequestClientType>,
) -> Option<ProfileSummary> {
    let summoner = fetch_current_summoner(client).await?;
    let puuid = summoner.puuid.as_deref();

    let ranked = match puuid {
        Some(puuid) if !puuid.is_empty() => fetch_ranked(client, puuid).await,
        _ => None,
    };
    let mastery = fetch_top_mastery(client).await;
    let history = fetch_recent_history(client, puuid).await;

    Some(build_profile_summary(ranked, &mastery, &history))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Puuid real del cliente de LA2 usado para verificar en vivo.
    const PUUID: &str = "f7e2b22b-d677-558c-95af-495e9807ada3";

    /// ranked-stats real (cuenta sin ranked) → `None` de tier/division y
    /// `Some(RankedInfo)` de la entrada.
    #[test]
    fn ranked_sin_rank_se_normaliza() {
        let dto: RankedStatsDto = serde_json::from_value(json!({
            "currentSeasonSplitPoints": 0,
            "queueMap": {
                "JADE_RANKED_SOLO_5x5": { "queueType": "JADE_RANKED_SOLO_5x5", "tier": "", "division": "NA" },
                "RANKED_SOLO_5x5": {
                    "queueType": "RANKED_SOLO_5x5",
                    "tier": "",
                    "division": "NA",
                    "isProvisional": true,
                    "leaguePoints": 0,
                    "losses": 0,
                    "warnings": null,
                    "wins": 0
                }
            }
        }))
        .expect("payload real parsea");

        let ranked = resolve_solo_queue(&dto).expect("la key solo-queue está");

        assert_eq!(ranked.queue, "RANKED_SOLO_5x5");
        assert_eq!(ranked.tier, None);
        assert_eq!(ranked.division, None);
        assert_eq!(ranked.league_points, Some(0));
    }

    /// Cuenta rankeada (sintético con la misma forma real: tier y division
    /// con valores): se conservan tal cual.
    #[test]
    fn ranked_con_datos_se_conserva() {
        let dto: RankedStatsDto = serde_json::from_value(json!({
            "queueMap": {
                "RANKED_SOLO_5x5": {
                    "queueType": "RANKED_SOLO_5x5",
                    "tier": "PLATINUM",
                    "division": "II",
                    "leaguePoints": 75,
                    "wins": 30,
                    "losses": 20
                }
            }
        }))
        .expect("payload parsea");

        let ranked = resolve_solo_queue(&dto).expect("la key está");

        assert_eq!(ranked.tier.as_deref(), Some("PLATINUM"));
        assert_eq!(ranked.division.as_deref(), Some("II"));
        assert_eq!(ranked.winrate(), Some(0.6));
    }

    /// Entrada del queueMap sin `queueType` (payload capado): el fetcher
    /// respalda el nombre de la cola con la key del mapa, así el modelo
    /// nunca viaja con `queue` vacío.
    #[test]
    fn ranked_sin_queue_type_usa_la_key_del_mapa() {
        let dto: RankedStatsDto = serde_json::from_value(json!({
            "queueMap": {
                "RANKED_SOLO_5x5": {
                    "tier": "GOLD",
                    "division": "I",
                    "leaguePoints": 33,
                    "wins": 4,
                    "losses": 2
                }
            }
        }))
        .expect("payload parsea");

        let ranked = resolve_solo_queue(&dto).expect("la key está");

        assert_eq!(ranked.queue, "RANKED_SOLO_5x5");
        assert_eq!(ranked.tier.as_deref(), Some("GOLD"));
        assert_eq!(ranked.division.as_deref(), Some("I"));
        assert_eq!(ranked.games(), 6);
        assert!((ranked.winrate().expect("con games") - 4.0 / 6.0).abs() < 1e-9);
    }

    /// Payload sin la key esperada → `None` (y las keys se loguean).
    #[test]
    fn ranked_sin_la_key_solo_queue_da_none() {
        let dto: RankedStatsDto = serde_json::from_value(json!({
            "queueMap": { "RANKED_TFT": { "queueType": "RANKED_TFT", "tier": "GOLD", "division": "I" } }
        }))
        .expect("payload parsea");

        assert!(resolve_solo_queue(&dto).is_none());
    }

    /// La normalización `""`/`NA`/`NONE` → `None` de tier y division.
    #[test]
    fn clean_rank_value_normaliza_los_valores_sin_dato() {
        assert_eq!(clean_rank_value(None), None);
        assert_eq!(clean_rank_value(Some(String::from(""))), None);
        assert_eq!(clean_rank_value(Some(String::from("NA"))), None);
        assert_eq!(clean_rank_value(Some(String::from("NONE"))), None);
        assert_eq!(clean_rank_value(Some(String::from("none"))), None);
        assert_eq!(clean_rank_value(Some(String::from("  "))), None);
        assert_eq!(
            clean_rank_value(Some(String::from("PLATINUM"))),
            Some(String::from("PLATINUM"))
        );
    }

    /// Recorte REAL de `/lol-champion-mastery/v1/local-player/champion-
    /// mastery` (2026-10): los primeros 3 elementos de las 137 entradas
    /// (campos extra como `lastPlayTime`/`milestoneGrades` se ignoran).
    #[test]
    fn maestria_parsea_el_payload_real() {
        let entries: Vec<MasteryDto> = serde_json::from_value(json!([
            {
                "championId": 84,
                "championLevel": 39,
                "championPoints": 443788,
                "championPointsSinceLastLevel": 49188,
                "championPointsUntilNextLevel": -38188,
                "championSeasonMilestone": 2,
                "highestGrade": "S+",
                "lastPlayTime": 1792185258000i64,
                "milestoneGrades": ["A"],
                "puuid": "f7e2b22b-d677-558c-95af-495e9807ada3",
                "tokensEarned": 1
            },
            {
                "championId": 82,
                "championLevel": 12,
                "championPoints": 150320,
                "highestGrade": "",
                "lastPlayTime": 1773535706000i64,
                "milestoneGrades": [],
                "puuid": "f7e2b22b-d677-558c-95af-495e9807ada3",
                "tokensEarned": 1
            },
            {
                "championId": 166,
                "championLevel": 9,
                "championPoints": 105200,
                "highestGrade": "B+",
                "lastPlayTime": 1791847472000i64,
                "milestoneGrades": ["B+"],
                "puuid": "f7e2b22b-d677-558c-95af-495e9807ada3",
                "tokensEarned": 0
            }
        ]))
        .expect("payload real parsea");

        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].champion_id, 84);
        assert_eq!(entries[0].champion_level, 39);
        assert_eq!(entries[0].champion_points, 443_788);
        assert_eq!(entries[1].champion_id, 82);
        assert_eq!(entries[2].champion_points, 105_200);
    }

    /// Historial con la forma REAL del endpoint `current-summoner`: cada
    /// game trae UN participante (el propio) + identidad con puuid. Se
    /// aplanan a MatchSummary con CS sumado.
    #[test]
    fn historial_real_se_aplana_al_participante_propio() {
        let response: HistoryResponseDto = serde_json::from_value(json!({
            "accountId": 2440484679085024i64,
            "games": {
                "gameBeginDate": "",
                "gameCount": 2,
                "gameEndDate": "",
                "gameIndexBegin": 0,
                "gameIndexEnd": 1,
                "games": [
                    {
                        "endOfGameResult": "GameComplete",
                        "gameCreation": 1792187581712i64,
                        "gameDuration": 1688,
                        "gameId": 1631048155,
                        "gameMode": "CLASSIC",
                        "gameType": "MATCHED_GAME",
                        "participantIdentities": [{
                            "participantId": 1,
                            "player": {
                                "gameName": "PKT MidKing",
                                "puuid": PUUID,
                                "summonerId": 23727787,
                                "tagLine": "Akali"
                            }
                        }],
                        "participants": [{
                            "championId": 10,
                            "participantId": 1,
                            "stats": {
                                "assists": 6, "deaths": 5, "kills": 6,
                                "totalMinionsKilled": 169,
                                "neutralMinionsKilled": 3,
                                "win": true
                            },
                            "teamId": 100
                        }],
                        "queueId": 400
                    },
                    {
                        "gameCreation": 1792185647246i64,
                        "gameDuration": 1481,
                        "gameId": 1631041158,
                        "participantIdentities": [{
                            "participantId": 1,
                            "player": { "puuid": PUUID }
                        }],
                        "participants": [{
                            "championId": 36,
                            "participantId": 1,
                            "stats": {
                                "assists": 9, "deaths": 3, "kills": 2,
                                "totalMinionsKilled": 41,
                                "neutralMinionsKilled": 0,
                                "win": false
                            },
                            "teamId": 100
                        }],
                        "queueId": 400
                    }
                ]
            }
        }))
        .expect("payload real parsea");

        let summaries = match_summaries_from_history(&response, Some(PUUID));

        assert_eq!(summaries.len(), 2);

        assert_eq!(summaries[0].game_id, 1_631_048_155);
        assert_eq!(summaries[0].champion_id, 10);
        assert!(summaries[0].win);
        assert_eq!((summaries[0].kills, summaries[0].deaths, summaries[0].assists), (6, 5, 6));
        assert_eq!(summaries[0].cs, 172);
        assert_eq!(summaries[0].duration_secs, 1_688);
        assert_eq!(summaries[0].queue_id, 400);
        assert_eq!(summaries[0].creation_ms, 1_792_187_581_712);

        assert_eq!(summaries[1].game_id, 1_631_041_158);
        assert_eq!(summaries[1].champion_id, 36);
        assert!(!summaries[1].win);
        assert_eq!(summaries[1].cs, 41);
    }

    /// El match por puuid prioriza la identidad: con VARIOS participantes
    /// (shape del detalle) elige el propio, y sin puuid conocido cae al
    /// único participante solo si es único.
    #[test]
    fn seleccion_del_participante_propio() {
        let game: HistoryGameDto = serde_json::from_value(json!({
            "gameId": 1,
            "participantIdentities": [
                { "participantId": 1, "player": { "puuid": PUUID } },
                { "participantId": 2, "player": { "puuid": "otro" } }
            ],
            "participants": [
                { "participantId": 1, "championId": 10, "stats": { "win": true } },
                { "participantId": 2, "championId": 20, "stats": { "win": false } }
            ]
        }))
        .expect("game parsea");

        // Con puuid: el participante 1 (champion 10).
        let mine = my_participant(&game, Some(PUUID)).expect("participante propio");
        assert_eq!(mine.champion_id, 10);

        // Con puuid que no está y varios participantes: None (no adivina).
        assert!(my_participant(&game, Some("otro-puuid")).is_none());

        // Un único participante sin identidad útil: fallback al único.
        let solo: HistoryGameDto = serde_json::from_value(json!({
            "gameId": 2,
            "participants": [{ "participantId": 7, "championId": 84 }]
        }))
        .expect("game parsea");
        let fallback = my_participant(&solo, None).expect("único participante");
        assert_eq!(fallback.champion_id, 84);

        // Varios participantes y sin puuid: None.
        assert!(my_participant(&game, None).is_none());
    }

    /// Identidad con MI puuid pero cuyo `participantId` no existe entre los
    /// participantes: no se puede resolver por identidad, así que solo se
    /// acepta el fallback del participante único; con varios, None.
    #[test]
    fn my_participant_con_identidad_huerfana_cae_al_fallback_o_none() {
        // Único participante con pid distinto al de la identidad → fallback.
        let solo: HistoryGameDto = serde_json::from_value(json!({
            "gameId": 3,
            "participantIdentities": [
                { "participantId": 9, "player": { "puuid": PUUID } }
            ],
            "participants": [
                { "participantId": 1, "championId": 84, "stats": { "win": true } }
            ]
        }))
        .expect("game parsea");
        let fallback = my_participant(&solo, Some(PUUID)).expect("único participante");
        assert_eq!(fallback.champion_id, 84);

        // Varios participantes y pid huérfano: no hay forma de saber cuál es
        // el propio → None (no adivina).
        let multi: HistoryGameDto = serde_json::from_value(json!({
            "gameId": 4,
            "participantIdentities": [
                { "participantId": 9, "player": { "puuid": PUUID } },
                { "participantId": 1, "player": { "puuid": "otro" } },
                { "participantId": 2, "player": { "puuid": "otro-mas" } }
            ],
            "participants": [
                { "participantId": 1, "championId": 10, "stats": { "win": true } },
                { "participantId": 2, "championId": 20, "stats": { "win": false } }
            ]
        }))
        .expect("game parsea");
        assert!(my_participant(&multi, Some(PUUID)).is_none());
    }

    /// Un game sin participante interpretable se descarta (con aviso) y no
    /// rompe el resto.
    #[test]
    fn historial_con_game_ilegible_descarta_solo_ese_game() {
        let response: HistoryResponseDto = serde_json::from_value(json!({
            "games": { "games": [
                { "gameId": 1, "participants": [] },
                { "gameId": 2, "participants": [
                    { "participantId": 1, "championId": 36, "stats": { "win": false } }
                ] }
            ] }
        }))
        .expect("payload parsea");

        let summaries = match_summaries_from_history(&response, Some(PUUID));
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].game_id, 2);
    }

    /// Maestría desordenada → top 3 por puntos.
    #[test]
    fn top_mastery_ordena_por_puntos() {
        let mastery = vec![
            MasteryEntry { champion_id: 1, mastery_level: 5, mastery_points: 100 },
            MasteryEntry { champion_id: 2, mastery_level: 7, mastery_points: 500_000 },
            MasteryEntry { champion_id: 3, mastery_level: 3, mastery_points: 50 },
            MasteryEntry { champion_id: 4, mastery_level: 4, mastery_points: 300 },
            MasteryEntry { champion_id: 5, mastery_level: 2, mastery_points: 10 },
        ];

        let top = top_mastery_entries(&mastery);

        let ids: Vec<i64> = top.iter().map(|entry| entry.champion_id).collect();
        assert_eq!(ids, vec![2, 4, 1]);
    }

    /// `build_profile_summary` puro: top champs enriquecidos con el
    /// historial, champ sin partidas → games 0, y season agregada.
    #[test]
    fn build_profile_summary_enriquece_top_champs_con_el_historial() {
        let ranked = Some(RankedInfo {
            queue: String::from("RANKED_SOLO_5x5"),
            tier: Some(String::from("PLATINUM")),
            division: Some(String::from("II")),
            league_points: Some(75),
            wins: Some(30),
            losses: Some(20),
        });
        let mastery = vec![
            MasteryEntry { champion_id: 84, mastery_level: 39, mastery_points: 443_788 },
            MasteryEntry { champion_id: 82, mastery_level: 12, mastery_points: 150_320 },
            MasteryEntry { champion_id: 10, mastery_level: 7, mastery_points: 60_000 },
            MasteryEntry { champion_id: 166, mastery_level: 9, mastery_points: 105_200 },
        ];
        // Historial: Kayn (84) ganada, Ahri (10) ganada + perdida, nada de
        // Karma (82) → games 0.
        let history = vec![
            MatchSummary {
                game_id: 1,
                champion_id: 84,
                win: true,
                kills: 10,
                deaths: 2,
                assists: 8,
                duration_secs: 1_800,
                queue_id: 420,
                creation_ms: 1,
                cs: 200,
            },
            MatchSummary {
                game_id: 2,
                champion_id: 10,
                win: true,
                kills: 6,
                deaths: 5,
                assists: 6,
                duration_secs: 1_688,
                queue_id: 400,
                creation_ms: 2,
                cs: 172,
            },
            MatchSummary {
                game_id: 3,
                champion_id: 10,
                win: false,
                kills: 2,
                deaths: 3,
                assists: 9,
                duration_secs: 1_481,
                queue_id: 400,
                creation_ms: 3,
                cs: 41,
            },
        ];

        let summary = build_profile_summary(ranked.clone(), &mastery, &history);

        // Top 3 por puntos: 84, 82, 166 (el 10 queda afuera por puntos).
        let ids: Vec<i64> = summary.top_champs.iter().map(|c| c.champion_id).collect();
        assert_eq!(ids, vec![84, 82, 166]);

        let kayn = &summary.top_champs[0];
        assert_eq!(kayn.games, 1);
        assert_eq!(kayn.wins, 1);
        assert_eq!(kayn.mastery_points, 443_788);

        // Karma: en el top por maestría pero sin partidas recientes.
        let karma = &summary.top_champs[1];
        assert_eq!(karma.games, 0);
        assert_eq!(karma.wins, 0);

        let velkoz = &summary.top_champs[2];
        assert_eq!(velkoz.games, 0);

        // El historial viaja completo y la season lo agrega.
        assert_eq!(summary.recent.len(), 3);
        assert_eq!(summary.season.games, 3);
        assert_eq!(summary.season.wins, 2);
        assert!(summary.season.avg_kda > 0.0);

        assert_eq!(summary.rank, ranked);
    }

    /// Sin ranked, sin maestría y sin historial: perfil válido y vacío
    /// (cuenta nueva), nunca pánico.
    #[test]
    fn build_profile_summary_con_todo_vacio() {
        let summary = build_profile_summary(None, &[], &[]);

        assert_eq!(summary, ProfileSummary::default());
        assert_eq!(summary.season.avg_kda, 0.0);
    }
}
