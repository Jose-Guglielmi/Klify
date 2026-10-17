//! Modelos de dominio del perfil del invocador (FASE 11a-2).
//!
//! Todos los datos salen del LCU (sin gastar la API key de Riot):
//!
//! - [`RankedInfo`]: entrada de ranked solo-queue de
//!   `/lol-ranked/v1/ranked-stats/{puuid}` (`queueMap`).
//! - [`MasteryEntry`]: una entrada de
//!   `/lol-champion-mastery/v1/local-player/champion-mastery`.
//! - [`MatchSummary`]: una partida del historial
//!   `/lol-match-history/v1/products/lol/current-summoner/matches`, ya
//!   aplanada a los datos del participante propio (KDA/CS/win).
//! - [`ProfileSummary`]: el paquete que consume la UI (11b): rank + top
//!   champs por maestría (enriquecidos con games/wins del historial) +
//!   últimas partidas + agregado de temporada.
//! - [`LcuMatchDetail`]: detalle completo de una partida
//!   (`/lol-match-history/v1/games/{gameId}`) con los 10 participantes,
//!   on-demand para el modal; [`LcuMatchDetail::teams_split`] los parte en
//!   (mi equipo, equipo enemigo).
//!
//! Convención serde del proyecto: camelCase, defaults en todo campo y
//! `null`/ausente → valor neutro, nunca error de parsing.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Ranked
// ---------------------------------------------------------------------------

/// Entrada de ranked del invocador para una cola (solo la solo-queue se
/// consulta hoy; ver [`crate::profile::lcu_fetch`]).
///
/// Los `Option` son tolerancia, no semántica: un payload real siempre trae
/// los campos, pero una cuenta sin ranked llega con `tier: ""` y
/// `division: "NA"` (verificado en vivo), que el fetcher normaliza a `None`
/// para que la UI muestre "sin ranked" en vez de un rank vacío.
///
/// También sirve como DTO de la entrada del `queueMap`: `queue` se mapea al
/// `queueType` del payload (renames y defaults en todo), así el fetcher
/// deserializa directo y solo aplica la normalización de vacíos.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RankedInfo {
    /// Tipo de cola del payload (ej. `"RANKED_SOLO_5x5"`).
    #[serde(rename = "queueType", default)]
    pub queue: String,
    /// Liga (ej. `"PLATINUM"`); `None` = sin ranked / provisional.
    #[serde(default)]
    pub tier: Option<String>,
    /// División (ej. `"II"`); `None` = sin ranked / provisional.
    #[serde(default)]
    pub division: Option<String>,
    /// Puntos de liga.
    #[serde(default)]
    pub league_points: Option<i64>,
    /// Victorias de la temporada.
    #[serde(default)]
    pub wins: Option<i64>,
    /// Derrotas de la temporada.
    #[serde(default)]
    pub losses: Option<i64>,
}

impl RankedInfo {
    /// Partidas jugadas en la cola (wins + losses; `None` cuenta 0).
    #[must_use]
    pub fn games(&self) -> i64 {
        self.wins.unwrap_or(0) + self.losses.unwrap_or(0)
    }

    /// Winrate de la temporada (0..1). `None` si no hay partidas: mejor
    /// que inventar 0.5.
    #[must_use]
    pub fn winrate(&self) -> Option<f64> {
        let games = self.games();
        (games > 0).then(|| self.wins.unwrap_or(0) as f64 / games as f64)
    }
}

// ---------------------------------------------------------------------------
// Maestría
// ---------------------------------------------------------------------------

/// Una entrada de maestría de campeón (subconjunto tolerante del payload
/// real, que también trae `lastPlayTime`, `highestGrade`, milestones...).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MasteryEntry {
    /// Numeric id del campeón (Data Dragon / LCU).
    #[serde(default)]
    pub champion_id: i64,
    /// Nivel de maestría (1..).
    #[serde(default)]
    pub mastery_level: i64,
    /// Puntos acumulados de maestría.
    #[serde(default)]
    pub mastery_points: i64,
}

// ---------------------------------------------------------------------------
// Historial
// ---------------------------------------------------------------------------

/// Partida reciente del invocador, aplanada a los datos del participante
/// propio (el endpoint `current-summoner` trae un único participante: el
/// jugador local; ver [`crate::profile::lcu_fetch`]).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchSummary {
    /// Id de la partida (para pedir el detalle on-demand).
    #[serde(default)]
    pub game_id: i64,
    /// Campeón usado por el invocador.
    #[serde(default)]
    pub champion_id: i64,
    /// `stats.win` del payload.
    #[serde(default)]
    pub win: bool,
    #[serde(default)]
    pub kills: i32,
    #[serde(default)]
    pub deaths: i32,
    #[serde(default)]
    pub assists: i32,
    /// Duración en segundos (`gameDuration`).
    #[serde(default)]
    pub duration_secs: i64,
    /// Id de cola (`queueId`: 400 normal, 420 ranked solo, ...).
    #[serde(default)]
    pub queue_id: i64,
    /// Timestamp de creación en ms (`gameCreation`).
    #[serde(default)]
    pub creation_ms: i64,
    /// Farm total: `totalMinionsKilled + neutralMinionsKilled`.
    #[serde(default)]
    pub cs: i32,
}

impl MatchSummary {
    /// KDA de la partida: `(kills + assists) / max(deaths, 1)`. Con 0
    /// muertes se divide por 1 (convención habitual: un perfect sigue
    /// acotado por kills+assists).
    #[must_use]
    pub fn kda(&self) -> f64 {
        (f64::from(self.kills) + f64::from(self.assists)) / f64::from(self.deaths.max(1))
    }
}

// ---------------------------------------------------------------------------
// Perfil agregado
// ---------------------------------------------------------------------------

/// Campeón destacado del perfil: entrada de maestría (top por puntos)
/// enriquecida con partidas/victorias contadas sobre el historial traído.
///
/// **Limitación documentada**: `games`/`wins` solo cuentan las últimas
/// `RECENT_MATCH_COUNT` partidas (20), no la temporada completa; un campeón
/// con maestría alta pero sin partidas recientes muestra `games: 0`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TopChamp {
    #[serde(default)]
    pub champion_id: i64,
    #[serde(default)]
    pub mastery_level: i64,
    #[serde(default)]
    pub mastery_points: i64,
    /// Partidas de ese campeón dentro del historial traído (0 si no jugó
    /// ninguna de las últimas N).
    #[serde(default)]
    pub games: i32,
    /// Victorias de ese campeón dentro del historial traído.
    #[serde(default)]
    pub wins: i32,
}

/// Agregado del historial traído ("últimas N partidas", N =
/// `crate::profile::lcu_fetch::RECENT_MATCH_COUNT` = 20): NO es la
/// temporada completa del servidor, solo lo que devuelve el primer pedido
/// de historial del LCU.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeasonStats {
    /// Partidas del historial traído.
    #[serde(default)]
    pub games: i32,
    /// Victorias dentro de esas partidas.
    #[serde(default)]
    pub wins: i32,
    /// KDA promedio de esas partidas (`MatchSummary::kda` por partida);
    /// `0.0` si no hay partidas.
    #[serde(default)]
    pub avg_kda: f64,
}

impl SeasonStats {
    /// Agrega el historial dado. Puro y testeado: no toca red ni señales.
    #[must_use]
    pub fn from_history(history: &[MatchSummary]) -> Self {
        let games = history.len() as i32;
        let wins = history.iter().filter(|game| game.win).count() as i32;
        let avg_kda = if games > 0 {
            history.iter().map(MatchSummary::kda).sum::<f64>() / f64::from(games)
        } else {
            0.0
        };
        Self {
            games,
            wins,
            avg_kda,
        }
    }
}

/// Perfil completo del invocador que consume la UI (FASE 11b).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileSummary {
    /// Ranked solo-queue de la temporada (`None` = cuenta sin ranked o
    /// fetch fallido).
    #[serde(default)]
    pub rank: Option<RankedInfo>,
    /// Top 3 campeones por puntos de maestría, enriquecidos con el
    /// historial (ver [`TopChamp`]).
    #[serde(default)]
    pub top_champs: Vec<TopChamp>,
    /// Últimas partidas (hasta `RECENT_MATCH_COUNT`), más reciente primero
    /// (el orden en que el LCU las devuelve).
    #[serde(default)]
    pub recent: Vec<MatchSummary>,
    /// Agregado del historial traído.
    #[serde(default)]
    pub season: SeasonStats,
}

// ---------------------------------------------------------------------------
// Detalle de partida (on-demand para el modal de la UI)
// ---------------------------------------------------------------------------

/// Un participante del detalle de partida, ya aplanado (los stats anidados
/// del payload —`stats.win`, `stats.kills`, ...— se suben a campos planos
/// deserializando vía un DTO crudo privado; la serialización sale plana).
///
/// FASE 11b-1: además de KDA/CS, el modal de detalle muestra oro, daño a
/// campeones, build (`item0..item5`) y runas (`perk0..perk5`). Todos esos
/// campos son **opcionales/tolerantes**: un payload capado (partida en
/// curso, recorte del LCU) deserializa igual y la UI muestra lo que haya.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", from = "TeamPlayerRaw")]
pub struct TeamPlayer {
    /// `participantId` del payload: es la clave que linkea con
    /// [`ParticipantIdentity`] para saber quién es quién.
    #[serde(default)]
    pub participant_id: i64,
    #[serde(default)]
    pub champion_id: i64,
    /// 100 (azul) / 200 (rojo).
    #[serde(default)]
    pub team_id: i64,
    /// `stats.win`.
    #[serde(default)]
    pub win: bool,
    #[serde(default)]
    pub kills: i32,
    #[serde(default)]
    pub deaths: i32,
    #[serde(default)]
    pub assists: i32,
    /// `stats.totalMinionsKilled + stats.neutralMinionsKilled`.
    #[serde(default)]
    pub cs: i32,
    /// `stats.goldEarned` (oro total de la partida); `None` si el payload
    /// no lo trae.
    #[serde(default)]
    pub gold_earned: Option<i64>,
    /// `stats.totalDamageDealtToChampions`; `None` si el payload no lo
    /// trae.
    #[serde(default)]
    pub damage_to_champions: Option<i64>,
    /// Build final: `stats.item0..item5`; `0` = casillero vacío.
    #[serde(default)]
    pub items: [i64; 6],
    /// Runas equipadas: `stats.perk0..perk5` con ids `> 0`, en orden de
    /// ranura (sin shards ni ids de estilo).
    #[serde(default)]
    pub rune_ids: Vec<i64>,
}

impl TeamPlayer {
    /// KDA del participante: `(kills + assists) / max(deaths, 1)` — misma
    /// convención que [`MatchSummary::kda`].
    #[must_use]
    pub fn kda(&self) -> f64 {
        (f64::from(self.kills) + f64::from(self.assists)) / f64::from(self.deaths.max(1))
    }
}

/// DTO crudo de un participante (estructura anidada real del payload).
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TeamPlayerRaw {
    #[serde(default)]
    participant_id: i64,
    #[serde(default)]
    champion_id: i64,
    #[serde(default)]
    team_id: i64,
    #[serde(default)]
    stats: TeamPlayerStatsRaw,
}

/// Subconjunto de `stats` que la UI necesita. Los campos de build/runas y
/// las métricas de oro/daño son `Option`: el LCU los trae en partidas
/// completas, pero un payload capado no debe invalidar el parsing.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TeamPlayerStatsRaw {
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
    #[serde(default)]
    gold_earned: Option<i64>,
    #[serde(default)]
    total_damage_dealt_to_champions: Option<i64>,
    #[serde(default)]
    item0: Option<i64>,
    #[serde(default)]
    item1: Option<i64>,
    #[serde(default)]
    item2: Option<i64>,
    #[serde(default)]
    item3: Option<i64>,
    #[serde(default)]
    item4: Option<i64>,
    #[serde(default)]
    item5: Option<i64>,
    #[serde(default)]
    perk0: Option<i64>,
    #[serde(default)]
    perk1: Option<i64>,
    #[serde(default)]
    perk2: Option<i64>,
    #[serde(default)]
    perk3: Option<i64>,
    #[serde(default)]
    perk4: Option<i64>,
    #[serde(default)]
    perk5: Option<i64>,
}

impl From<TeamPlayerRaw> for TeamPlayer {
    fn from(raw: TeamPlayerRaw) -> Self {
        let stats = raw.stats;
        let items = [
            stats.item0.unwrap_or(0),
            stats.item1.unwrap_or(0),
            stats.item2.unwrap_or(0),
            stats.item3.unwrap_or(0),
            stats.item4.unwrap_or(0),
            stats.item5.unwrap_or(0),
        ];
        let rune_ids = [
            stats.perk0,
            stats.perk1,
            stats.perk2,
            stats.perk3,
            stats.perk4,
            stats.perk5,
        ]
        .into_iter()
        .flatten()
        .filter(|perk_id| *perk_id > 0)
        .collect();

        Self {
            participant_id: raw.participant_id,
            champion_id: raw.champion_id,
            team_id: raw.team_id,
            win: stats.win,
            kills: stats.kills,
            deaths: stats.deaths,
            assists: stats.assists,
            cs: stats.total_minions_killed + stats.neutral_minions_killed,
            gold_earned: stats.gold_earned,
            damage_to_champions: stats.total_damage_dealt_to_champions,
            items,
            rune_ids,
        }
    }
}

/// Identidad de un participante: linkea `participantId` con el `puuid`.
/// El `player` real trae muchos más campos (`gameName`, `platformId`, ...)
/// que se ignoran.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParticipantIdentity {
    #[serde(default)]
    pub participant_id: i64,
    #[serde(default)]
    pub player: ParticipantPlayer,
}

/// Subconjunto de `player` dentro de la identidad.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParticipantPlayer {
    /// PUUID del jugador (`None` si el payload no lo trae).
    #[serde(default)]
    pub puuid: Option<String>,
}

/// Detalle completo de una partida (`GET /lol-match-history/v1/games/
/// {gameId}`, verificado en vivo: 10 participantes + identidades + teams).
///
/// Se usa on-demand (modal de detalle), no vive en señales. Tolerante: un
/// payload mínimo parsea con defaults; `teams` del payload (bans,
/// objetivos) no se modela hoy porque la UI no lo muestra.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LcuMatchDetail {
    #[serde(default)]
    pub game_id: i64,
    /// Duración en segundos (`gameDuration` del payload).
    #[serde(rename = "gameDuration", default)]
    pub game_duration_secs: i64,
    #[serde(default)]
    pub queue_id: i64,
    #[serde(default)]
    pub participants: Vec<TeamPlayer>,
    #[serde(default)]
    pub participant_identities: Vec<ParticipantIdentity>,
}

impl LcuMatchDetail {
    /// Parte los participantes en `(mi equipo, equipo enemigo)` resolviendo
    /// el equipo propio por `my_puuid` vía las identidades.
    ///
    /// Si el puuid no figura en la partida (o el detalle no trajo
    /// identidades), devuelve `(vec![], vec![])`: mejor un modal vacío que
    /// adivinar equipos.
    #[must_use]
    pub fn teams_split(&self, my_puuid: &str) -> (Vec<TeamPlayer>, Vec<TeamPlayer>) {
        let my_team = self
            .participant_identities
            .iter()
            .find(|identity| identity.player.puuid.as_deref() == Some(my_puuid))
            .and_then(|identity| {
                self.participants
                    .iter()
                    .find(|player| player.participant_id == identity.participant_id)
                    .map(|player| player.team_id)
            });

        let Some(my_team) = my_team else {
            return (Vec::new(), Vec::new());
        };

        let mine: Vec<TeamPlayer> = self
            .participants
            .iter()
            .filter(|player| player.team_id == my_team)
            .cloned()
            .collect();
        let enemies: Vec<TeamPlayer> = self
            .participants
            .iter()
            .filter(|player| player.team_id != my_team)
            .cloned()
            .collect();
        (mine, enemies)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Recorte REAL de `/lol-ranked/v1/ranked-stats/{puuid}` del cliente de
    /// LA2 (2026-10): cuenta sin ranked → `tier: ""`, `division: "NA"`,
    /// `isProvisional: true`. El resto de las keys del `queueMap`
    /// (`JADE_RANKED_SOLO_5x5`, `RANKED_FLEX_SR`, ...) se ignora.
    fn fixture_ranked_sin_rank() -> serde_json::Value {
        json!({
            "currentSeasonSplitPoints": 0,
            "highestRankedEntry": { "queueType": "JADE_RANKED_SOLO_5x5", "tier": "", "division": "NA" },
            "queueMap": {
                "JADE_RANKED_SOLO_5x5": { "queueType": "JADE_RANKED_SOLO_5x5", "tier": "", "division": "NA" },
                "RANKED_SOLO_5x5": {
                    "climbingIndicatorActive": false,
                    "division": "NA",
                    "highestDivision": "NA",
                    "highestTier": "",
                    "isProvisional": true,
                    "leaguePoints": 0,
                    "losses": 0,
                    "miniSeriesProgress": "",
                    "provisionalGameThreshold": 5,
                    "provisionalGamesRemaining": 5,
                    "queueType": "RANKED_SOLO_5x5",
                    "ratedRating": 0,
                    "ratedTier": "NONE",
                    "tier": "",
                    "warnings": null,
                    "wins": 0
                },
                "RANKED_TFT": { "queueType": "RANKED_TFT", "tier": "", "division": "NA" }
            }
        })
    }

    /// `RankedInfo` se parsea directo de la entrada del queueMap (campos
    /// extra como `isProvisional`/`warnings` se ignoran).
    #[test]
    fn ranked_info_parsea_la_entrada_real_del_queue_map() {
        let entry = fixture_ranked_sin_rank()["queueMap"]["RANKED_SOLO_5x5"].clone();
        let ranked: RankedInfo = serde_json::from_value(entry).expect("entrada real parsea");

        assert_eq!(ranked.queue, "RANKED_SOLO_5x5");
        assert_eq!(ranked.league_points, Some(0));
        assert_eq!(ranked.wins, Some(0));
        assert_eq!(ranked.losses, Some(0));

        // games/winrate sobre una cuenta sin partidas.
        assert_eq!(ranked.games(), 0);
        assert_eq!(ranked.winrate(), None);
    }

    /// Helpers de ranked con datos concretos (sintético con la MISMA forma
    /// del payload real, que en esta cuenta llega sin rank).
    #[test]
    fn games_y_winrate_de_ranked_con_datos() {
        let ranked = RankedInfo {
            queue: String::from("RANKED_SOLO_5x5"),
            tier: Some(String::from("PLATINUM")),
            division: Some(String::from("II")),
            league_points: Some(75),
            wins: Some(30),
            losses: Some(20),
        };

        assert_eq!(ranked.games(), 50);
        assert_eq!(ranked.winrate(), Some(0.6));

        // `None` en wins/losses cuenta 0.
        let vacio = RankedInfo {
            queue: String::from("RANKED_SOLO_5x5"),
            tier: None,
            division: None,
            league_points: None,
            wins: None,
            losses: None,
        };
        assert_eq!(vacio.games(), 0);
        assert_eq!(vacio.winrate(), None);
    }

    /// `MasteryEntry` es el modelo de dominio (el parse del payload crudo
    /// con `championLevel`/`championPoints` vive en `lcu_fetch`, con su
    /// DTO): acá se cubre el roundtrip serde para persistencia futura.
    #[test]
    fn maestria_hace_roundtrip_serde() {
        let entries = vec![
            MasteryEntry {
                champion_id: 84,
                mastery_level: 39,
                mastery_points: 443_788,
            },
            MasteryEntry {
                champion_id: 82,
                mastery_level: 12,
                mastery_points: 150_320,
            },
        ];

        let json = serde_json::to_string(&entries).expect("serializa");
        let back: Vec<MasteryEntry> = serde_json::from_str(&json).expect("deserializa");
        assert_eq!(back, entries);
    }

    /// Partida del historial (recorte REAL del primer game del historial de
    /// `current-summoner`, 2026-10): KDA 6/5/6 con Ahri (id 10), CS 169+3.
    fn match_summary_real() -> MatchSummary {
        MatchSummary {
            game_id: 1_631_048_155,
            champion_id: 10,
            win: true,
            kills: 6,
            deaths: 5,
            assists: 6,
            duration_secs: 1_688,
            queue_id: 400,
            creation_ms: 1_792_187_581_712,
            cs: 172,
        }
    }

    /// KDA: normal, perfect (0 muertes) y borde 0/0/0.
    #[test]
    fn kda_de_match_summary() {
        assert_eq!(match_summary_real().kda(), 2.4); // (6+6)/5

        let perfect = MatchSummary {
            kills: 10,
            deaths: 0,
            assists: 2,
            ..MatchSummary::default()
        };
        assert_eq!(perfect.kda(), 12.0); // (10+2)/1

        let silencio = MatchSummary::default();
        assert_eq!(silencio.kda(), 0.0);
    }

    /// `SeasonStats::from_history`: agregado puro del historial traído.
    #[test]
    fn season_stats_agrega_el_historial() {
        let ganada = match_summary_real();
        let perdida = MatchSummary {
            game_id: 2,
            champion_id: 36,
            win: false,
            kills: 2,
            deaths: 3,
            assists: 9,
            duration_secs: 1_481,
            queue_id: 400,
            creation_ms: 1_792_185_647_246,
            cs: 41,
        };

        let stats = SeasonStats::from_history(&[ganada.clone(), perdida.clone()]);
        assert_eq!(stats.games, 2);
        assert_eq!(stats.wins, 1);
        // (2.4 + (2+9)/3) / 2 = (2.4 + 3.666..) / 2
        assert!((stats.avg_kda - (2.4 + 11.0 / 3.0) / 2.0).abs() < 1e-9);

        // Historial vacío → todo 0 (nunca NaN).
        assert_eq!(
            SeasonStats::from_history(&[]),
            SeasonStats {
                games: 0,
                wins: 0,
                avg_kda: 0.0
            }
        );
    }

    /// Recorte REAL de `/lol-match-history/v1/games/1631048155` (2026-10):
    /// participante propio (pid 1, equipo 100) + un enemigo (pid 6, equipo
    /// 200) + identidades con puuid (la lista completa trae 10 + 10).
    fn fixture_detalle_real() -> serde_json::Value {
        json!({
            "endOfGameResult": "GameComplete",
            "gameCreation": 1792187581712i64,
            "gameDuration": 1688,
            "gameId": 1631048155,
            "gameMode": "CLASSIC",
            "gameType": "MATCHED_GAME",
            "participants": [
                {
                    "championId": 10,
                    "highestAchievedSeasonTier": "",
                    "participantId": 1,
                    "spell1Id": 4,
                    "spell2Id": 12,
                    "stats": {
                        "assists": 6, "deaths": 5, "kills": 6,
                        "totalMinionsKilled": 169, "neutralMinionsKilled": 3,
                        "win": true, "goldEarned": 10240, "champLevel": 17
                    },
                    "teamId": 100,
                    "timeline": { "lane": "JUNGLE", "role": "NONE" }
                },
                {
                    "championId": 106,
                    "participantId": 6,
                    "stats": {
                        "assists": 2, "deaths": 9, "kills": 1,
                        "totalMinionsKilled": 95, "neutralMinionsKilled": 4,
                        "win": false
                    },
                    "teamId": 200
                }
            ],
            "participantIdentities": [
                {
                    "participantId": 1,
                    "player": {
                        "currentPlatformId": "LA2",
                        "gameName": "PKT MidKing",
                        "puuid": "f7e2b22b-d677-558c-95af-495e9807ada3",
                        "summonerId": 23727787,
                        "tagLine": "Akali"
                    }
                },
                {
                    "participantId": 6,
                    "player": {
                        "gameName": "Enemigo",
                        "puuid": "5b371886-1da5-5453-a4ff-1e6a71d81657"
                    }
                }
            ],
            "platformId": "LA2",
            "queueId": 400,
            "teams": [ { "teamId": 100, "win": "Win" }, { "teamId": 200, "win": "Fail" } ]
        })
    }

    /// El detalle real parsea con stats aplanados y CS sumado.
    #[test]
    fn detalle_real_parsea_aplanado() {
        let detail: LcuMatchDetail =
            serde_json::from_value(fixture_detalle_real()).expect("detalle real parsea");

        assert_eq!(detail.game_id, 1_631_048_155);
        assert_eq!(detail.game_duration_secs, 1_688);
        assert_eq!(detail.queue_id, 400);
        assert_eq!(detail.participants.len(), 2);
        assert_eq!(detail.participant_identities.len(), 2);

        let yo = &detail.participants[0];
        assert_eq!(yo.participant_id, 1);
        assert_eq!(yo.champion_id, 10);
        assert_eq!(yo.team_id, 100);
        assert!(yo.win);
        assert_eq!((yo.kills, yo.deaths, yo.assists), (6, 5, 6));
        assert_eq!(yo.cs, 172); // 169 + 3
        // FASE 11b-1: el fixture real trae goldEarned; daño/build/runas
        // ausentes → tolerado como vacío.
        assert_eq!(yo.gold_earned, Some(10_240));
        assert_eq!(yo.damage_to_champions, None);
        assert_eq!(yo.items, [0; 6]);
        assert!(yo.rune_ids.is_empty());

        let enemigo = &detail.participants[1];
        assert_eq!(enemigo.team_id, 200);
        assert!(!enemigo.win);
        assert_eq!(enemigo.cs, 99); // 95 + 4
    }

    /// `teams_split` con el puuid real: mi equipo primero, enemigos después.
    #[test]
    fn teams_split_separa_mi_equipo_del_enemigo() {
        let detail: LcuMatchDetail =
            serde_json::from_value(fixture_detalle_real()).expect("detalle real parsea");

        let (mine, enemies) =
            detail.teams_split("f7e2b22b-d677-558c-95af-495e9807ada3");

        assert_eq!(mine.len(), 1);
        assert_eq!(mine[0].champion_id, 10);
        assert_eq!(enemies.len(), 1);
        assert_eq!(enemies[0].champion_id, 106);
    }

    /// Puuid ausente de la partida → dos equipos vacíos, nunca pánico ni
    /// equipos inventados.
    #[test]
    fn teams_split_con_puuid_ajeno_devuelve_vacio() {
        let detail: LcuMatchDetail =
            serde_json::from_value(fixture_detalle_real()).expect("detalle real parsea");

        let (mine, enemies) = detail.teams_split("puuid-que-no-jugo");
        assert!(mine.is_empty());
        assert!(enemies.is_empty());
    }

    /// Identidad con MI puuid pero cuyo `participantId` no existe entre los
    /// participantes: no hay forma de resolver el equipo propio → dos
    /// equipos vacíos (mejor modal vacío que adivinar), nunca pánico.
    #[test]
    fn teams_split_con_identidad_huerfana_devuelve_vacio() {
        let detail: LcuMatchDetail = serde_json::from_value(json!({
            "gameId": 7,
            "participants": [
                { "participantId": 1, "championId": 10, "teamId": 100, "stats": { "win": true } },
                { "participantId": 2, "championId": 20, "teamId": 200, "stats": { "win": false } }
            ],
            "participantIdentities": [
                { "participantId": 5, "player": { "puuid": "mi-puuid" } },
                { "participantId": 1, "player": { "puuid": "otro" } }
            ]
        }))
        .expect("detalle parsea");

        let (mine, enemies) = detail.teams_split("mi-puuid");
        assert!(mine.is_empty());
        assert!(enemies.is_empty());
    }

    /// Un payload mínimo (partida en curso o payload capado) parsea con
    /// defaults en todo. Los campos numéricos toleran AUSENCIA (no `null`
    /// explícito: el LCU siempre los manda como número; los nullable del
    /// modelo son `Option`).
    #[test]
    fn detalle_tolerante_a_payload_minimo() {
        let detail: LcuMatchDetail =
            serde_json::from_value(json!({})).expect("objeto vacío parsea");
        assert_eq!(detail, LcuMatchDetail::default());

        // Un participante con solo el champ (stats enteros ausentes) también.
        let detail: LcuMatchDetail = serde_json::from_value(json!({
            "gameId": 42,
            "participants": [{ "participantId": 3, "championId": 84, "teamId": 200 }]
        }))
        .expect("payload parcial parsea");

        assert_eq!(detail.game_id, 42);
        let player = &detail.participants[0];
        assert_eq!(player.champion_id, 84);
        assert_eq!(player.team_id, 200);
        assert_eq!(player.cs, 0);
        assert!(!player.win);
    }

    /// FASE 11b-1: build y runas del participante (`item0..item5`,
    /// `perk0..perk5` del stats anidado) se aplanan; los casilleros vacíos
    /// (`0`) se descartan de las runas y quedan como `0` en el build.
    #[test]
    fn detalle_parsea_build_y_runas_del_participante() {
        let mut detail: LcuMatchDetail = serde_json::from_value(json!({
            "gameId": 100,
            "participants": [{
                "participantId": 1,
                "championId": 10,
                "teamId": 100,
                "stats": {
                    "win": true,
                    "goldEarned": 10240,
                    "totalDamageDealtToChampions": 21456,
                    "item0": 3157, "item1": 3020, "item2": 0,
                    "item3": 3165, "item4": 4645, "item5": 3089,
                    "perk0": 8128, "perk1": 8126, "perk2": 8138,
                    "perk3": 9105, "perk4": 8233, "perk5": 0
                }
            }]
        }))
        .expect("payload con build parsea");

        let player = &mut detail.participants[0];
        assert_eq!(player.gold_earned, Some(10_240));
        assert_eq!(player.damage_to_champions, Some(21_456));
        assert_eq!(player.items, [3157, 3020, 0, 3165, 4645, 3089]);
        // El perk 0 (ranura vacía) se descarta.
        assert_eq!(player.rune_ids, vec![8128, 8126, 8138, 9105, 8233]);

        // Ratio KDA con la misma convención que MatchSummary.
        player.kills = 10;
        player.deaths = 0;
        player.assists = 2;
        assert!((player.kda() - 12.0).abs() < 1e-9);
    }

    /// El perfil completo serializa y deserializa idéntico (base para
    /// persistirlo a futuro).
    #[test]
    fn profile_summary_hace_roundtrip_serde() {
        let summary = ProfileSummary {
            rank: Some(RankedInfo {
                queue: String::from("RANKED_SOLO_5x5"),
                tier: Some(String::from("PLATINUM")),
                division: Some(String::from("II")),
                league_points: Some(75),
                wins: Some(30),
                losses: Some(20),
            }),
            top_champs: vec![TopChamp {
                champion_id: 84,
                mastery_level: 39,
                mastery_points: 443_788,
                games: 3,
                wins: 2,
            }],
            recent: vec![match_summary_real()],
            season: SeasonStats::from_history(&[match_summary_real()]),
        };

        let json = serde_json::to_string(&summary).expect("serializa");
        let back: ProfileSummary = serde_json::from_str(&json).expect("deserializa");
        assert_eq!(back, summary);
    }
}
