//! Soporte **en partida** (FASE 12): Live Client Data API del propio juego
//! (`https://127.0.0.1:2999/liveclientdata/...`, HTTPS con cert self-signed
//! de Riot, sin autenticación).
//!
//! Diseño: este módulo SOLO consulta y resuelve — publica
//! `Option<LiveGame>` por un canal `watch` y **no toca señales de UI**.
//! Es [`crate::lcu::client::run_lcu_loop`] quien **espeja** la partida al
//! mismo estado que alimenta champ select ([`AppState::InChampSelect`] +
//! [`MatchContext`]), con su propia máquina de estados como árbitro: así
//! hero, tabs, counters y modo VS funcionan en partida sin duplicar nada.
//!
//! Datos: `activePlayer` (mi champ) + `allPlayers` (equipos con
//! `championName`, `team` ORDER/CHAOS, `position`). Los nombres se
//! resuelven contra el catálogo (`ddragon_id`, fallback slug): sin
//! resolución para MI champ no hay análisis posible → `None`.

use std::time::Duration;

use dioxus::prelude::*;
use serde::Deserialize;
use tokio::sync::watch;

use crate::data_dragon::models::{ChampionCatalog, ChampionEntry};
use crate::lcu::models::{MatchContext, Position, TeamMember};

/// Puerto fijo de la Live Client Data API (solo sirve el proceso del juego
/// mientras hay partida en curso).
const LIVE_PORT: u16 = 2999;

/// Cadencia de sondeo: en partida los datos cambian lento (nuestro consumo
/// es el roster, no los scores); 5 s es suficiente y es local.
const POLL_INTERVAL: Duration = Duration::from_secs(5);

/// Timeout cortos: si el juego no contesta, la partida no está lista.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(4);

/// Partida en curso resuelta contra el catálogo (pura, testeada).
#[derive(Clone, Debug, PartialEq)]
pub struct LiveGame {
    /// Modo (`"CLASSIC"`, `"ARAM"`, `"PRACTICETOOL"`, …).
    pub game_mode: String,
    /// Segundos de juego (`gameTime`).
    pub game_time_secs: f64,
    /// Campeón local resuelto (id numérico del catálogo).
    pub my_champion_id: i64,
    /// Nombre visible de mi campeón (catálogo; fallback `championName`).
    pub my_champion_name: String,
    /// Posición reportada por la Live API, si la cola la trae.
    pub my_position: Option<Position>,
    /// Aliados resueltos (excluyéndome): `(champion_id, position)`.
    pub allies: Vec<(i64, Option<Position>)>,
    /// Enemigos resueltos (bots incluidos): `(champion_id, position)`.
    pub enemies: Vec<(i64, Option<Position>)>,
}

impl LiveGame {
    /// Contexto sintético para reusar TODA la UI de champ select: `cell_id`
    /// sintéticos (0 = yo, aliados desde 1, enemigos desde 100) que la UI
    /// usa solo como clave de fila.
    #[must_use]
    pub fn match_context(&self) -> MatchContext {
        MatchContext {
            my_cell_id: Some(0),
            my_champion_id: Some(self.my_champion_id),
            my_position: self.my_position,
            allies: self
                .allies
                .iter()
                .enumerate()
                .map(|(index, (champion_id, position))| TeamMember {
                    cell_id: 1 + index as i64,
                    champion_id: Some(*champion_id),
                    position: *position,
                })
                .collect(),
            enemies: self
                .enemies
                .iter()
                .enumerate()
                .map(|(index, (champion_id, position))| TeamMember {
                    cell_id: 100 + index as i64,
                    champion_id: Some(*champion_id),
                    position: *position,
                })
                .collect(),
        }
    }
}

// ---------------------------------------------------------------------------
// DTOs tolerantes del payload `allgamedata`.
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LiveDto {
    /// Verificado EN VIVO (parche 16.20, práctica): la raíz trae
    /// `activePlayer`, `allPlayers`, `events` y `gameData` — el bloque de
    /// stats se llama `gameData` y **no trae `gameState`** (parche actual).
    #[serde(default)]
    game_data: Option<GameDataDto>,
    #[serde(default)]
    active_player: Option<ActivePlayerDto>,
    #[serde(default)]
    all_players: Vec<PlayerDto>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GameDataDto {
    /// Documentado por Riot pero AUSENTE en el payload actual; si una
    /// versión lo restaura se respeta (`!= InProgress` → no hay juego).
    #[serde(default)]
    game_state: String,
    #[serde(default)]
    game_mode: String,
    #[serde(default)]
    game_time: f64,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ActivePlayerDto {
    /// Riot id COMPLETA con tag (ej. `"PKT MidKing#Akali"`), igual que
    /// `allPlayers[].summonerName` (verificado en vivo).
    #[serde(default)]
    summoner_name: String,
    /// Verificado en vivo: `activePlayer` NO trae `championName` — el champ
    /// local se resuelve desde su entrada en `allPlayers`.
    #[serde(default)]
    champion_name: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlayerDto {
    #[serde(default)]
    summoner_name: String,
    #[serde(default)]
    champion_name: String,
    /// `"ORDER"` (azul) o `"CHAOS"` (rojo).
    #[serde(default)]
    team: String,
    /// `"TOP"`, `"JUNGLE"`, `"MIDDLE"`, `"BOTTOM"`, `"UTILITY"` o vacío.
    #[serde(default)]
    position: String,
}

/// Posición de la Live API a [`Position`] (pura, testeada). Vacío o
/// desconocido → `None` (ARAM/práctica no la reportan).
#[must_use]
fn parse_position(raw: &str) -> Option<Position> {
    match raw.trim().to_ascii_uppercase().as_str() {
        "TOP" => Some(Position::Top),
        "JUNGLE" => Some(Position::Jungle),
        "MIDDLE" => Some(Position::Middle),
        "BOTTOM" => Some(Position::Bottom),
        "UTILITY" => Some(Position::Utility),
        _ => None,
    }
}

/// `championName` de la Live API → id numérico del catálogo: match exacto
/// por `ddragon_id` (la Live API usa el id de Data Dragon), fallback slug
/// case-insensitive (pura, testeada).
#[must_use]
fn resolve_champion_id<'a>(
    champion_name: &str,
    catalog: &'a ChampionCatalog,
) -> Option<(i64, &'a ChampionEntry)> {
    if champion_name.is_empty() {
        return None;
    }
    if let Some((id, entry)) = catalog
        .by_id
        .iter()
        .find(|(_, entry)| entry.ddragon_id == champion_name)
    {
        return Some((*id, entry));
    }
    catalog
        .by_id
        .iter()
        .find(|(_, entry)| entry.slug.eq_ignore_ascii_case(champion_name))
        .map(|(id, entry)| (*id, entry))
}

/// Resuelve el DTO crudo a [`LiveGame`] contra el catálogo (puro, testeado):
/// criterio "en partida" = payload válido con MI jugador resoluble (el
/// parche actual no trae `gameState`; si una versión lo restaura y dice otra
/// cosa, se respeta). Jugadores sin resolución se descartan.
#[must_use]
fn resolve_live_game(
    dto: &LiveDto,
    catalog: Option<&ChampionCatalog>,
) -> Option<(LiveGame, usize, usize)> {
    if let Some(game_data) = &dto.game_data {
        if !game_data.game_state.is_empty() && game_data.game_state != "InProgress" {
            return None;
        }
    }
    let catalog = catalog?;
    let active = dto.active_player.as_ref()?;
    let me = dto
        .all_players
        .iter()
        .find(|player| player.summoner_name == active.summoner_name)
        .or_else(|| {
            dto.all_players
                .iter()
                .find(|player| player.champion_name == active.champion_name)
        })?;

    let (my_id, my_entry) = resolve_champion_id(&me.champion_name, catalog)?;
    let my_team = me.team.clone();

    let mut allies = Vec::new();
    let mut enemies = Vec::new();
    let mut skipped = 0usize;
    for player in &dto.all_players {
        if player.summoner_name == me.summoner_name {
            continue;
        }
        let Some((id, _)) = resolve_champion_id(&player.champion_name, catalog) else {
            skipped += 1;
            continue;
        };
        let position = parse_position(&player.position);
        if player.team == my_team {
            allies.push((id, position));
        } else {
            enemies.push((id, position));
        }
    }

    let (game_mode, game_time) = dto
        .game_data
        .as_ref()
        .map(|data| (data.game_mode.clone(), data.game_time))
        .unwrap_or_default();

    Some((
        LiveGame {
            game_mode,
            game_time_secs: game_time,
            my_champion_id: my_id,
            my_champion_name: if my_entry.name.is_empty() {
                me.champion_name.clone()
            } else {
                my_entry.name.clone()
            },
            my_position: parse_position(&me.position),
            allies,
            enemies,
        },
        dto.all_players.len().saturating_sub(1),
        skipped,
    ))
}

/// Cliente HTTP dedicado a la Live API: acepta el cert self-signed de Riot
/// (práctica estándar para 2999, todo local) con timeouts acotados.
fn live_client() -> Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .build()
}

/// GET de `/liveclientdata/allgamedata`. Sin juego (proceso cerrado) la
/// conexión se rechaza; con juego cargando puede dar 404. Ambos → `Err`.
async fn fetch_allgamedata(client: &reqwest::Client) -> Result<LiveDto, String> {
    let url = format!("https://127.0.0.1:{LIVE_PORT}/liveclientdata/allgamedata");
    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|error| format!("sin juego: {error}"))?;
    if !response.status().is_success() {
        return Err(format!("live http {}", response.status()));
    }
    response
        .json::<LiveDto>()
        .await
        .map_err(|error| format!("live no parseable: {error}"))
}

/// Tarea de por vida: sondea la Live API y publica `Option<LiveGame>` en el
/// canal `watch` (solo notifica cambios reales; el receptor es el loop LCU,
/// árbitro del espejo). Sin catálogo todavía → no publica (`None` no: un
/// `None` real significa "no hay partida").
pub async fn run_live_fetch_loop(
    tx: watch::Sender<Option<LiveGame>>,
    catalog: Signal<Option<ChampionCatalog>>,
) {
    let client = match live_client() {
        Ok(client) => client,
        Err(error) => {
            eprintln!("klipy [live]: cliente http no disponible: {error}");
            return;
        }
    };

    let mut last: Option<LiveGame> = None;
    let mut last_error: Option<String> = None;
    let mut first_poll = true;
    let mut ticker = tokio::time::interval(POLL_INTERVAL);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        ticker.tick().await;
        let catalog_snapshot = catalog.read().clone();

        let resolved = match fetch_allgamedata(&client).await {
            Ok(dto) => {
                if first_poll {
                    eprintln!("klipy [live]: primer sondeo ok (sin verificar partida aún)");
                }
                resolve_live_game(&dto, catalog_snapshot.as_ref())
            }
            Err(error) => {
                // Un error por cambio de mensaje (sin juego = normal y no
                // se loguea más que la primera vez; un cambio SÍ es raro).
                if last_error.as_deref() != Some(error.as_str()) {
                    eprintln!("klipy [live]: sin datos: {error}");
                    last_error = Some(error);
                }
                None
            }
        };
        first_poll = false;

        let next = resolved.map(|(game, total, skipped)| {
            if skipped > 0 {
                eprintln!(
                    "klipy [live]: {skipped} de {total} jugadores sin resolver contra el catalogo"
                );
            }
            game
        });

        if next != last {
            eprintln!(
                "klipy [live]: {}",
                match &next {
                    Some(game) => format!(
                        "partida {} · {} ({} aliados, {} enemigos)",
                        game.game_mode,
                        game.my_champion_name,
                        game.allies.len(),
                        game.enemies.len()
                    ),
                    None => String::from("sin partida (gameState != InProgress o sin resolver)"),
                }
            );
            last = next.clone();
            let _ = tx.send(next);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn catalog() -> ChampionCatalog {
        let mut by_id = HashMap::new();
        for (id, name, ddragon) in
            [(34, "Akali", "Akali"), (64, "Kayn", "Kayn"), (238, "Zed", "Zed")]
        {
            by_id.insert(
                id,
                ChampionEntry {
                    name: String::from(name),
                    image_url: format!("https://example/{ddragon}.png"),
                    slug: String::from(name),
                    ddragon_id: String::from(ddragon),
                },
            );
        }
        ChampionCatalog {
            version: String::from("16.20.1"),
            by_id,
        }
    }

    fn dto(game_state: Option<&str>, my_name: &str) -> LiveDto {
        LiveDto {
            game_data: Some(GameDataDto {
                game_state: game_state.map(String::from).unwrap_or_default(),
                game_mode: String::from("PRACTICETOOL"),
                game_time: 125.5,
            }),
            active_player: Some(ActivePlayerDto {
                // Riot id completa con tag, como el payload real.
                summoner_name: format!("{my_name}#LA2"),
                champion_name: String::new(),
            }),
            all_players: vec![
                PlayerDto {
                    summoner_name: format!("{my_name}#LA2"),
                    champion_name: String::from("Akali"),
                    team: String::from("ORDER"),
                    position: String::from("MIDDLE"),
                },
                PlayerDto {
                    summoner_name: String::from("ally#LA2"),
                    champion_name: String::from("Kayn"),
                    team: String::from("ORDER"),
                    position: String::from("JUNGLE"),
                },
                PlayerDto {
                    summoner_name: String::from("bot1"),
                    champion_name: String::from("Zed"),
                    team: String::from("CHAOS"),
                    position: String::from("MIDDLE"),
                },
                PlayerDto {
                    summoner_name: String::from("ghost"),
                    champion_name: String::from("DesconocidoXX"),
                    team: String::from("CHAOS"),
                    position: String::from("NONE"),
                },
            ],
        }
    }

    /// Resolución nominal: payload válido + mi champ → aliados/enemigos
    /// separados por team, bots incluidos, desconocidos descartados. Sin
    /// `gameState` (payload real del 16.20) también resuelve.
    #[test]
    fn resolve_live_game_separa_equipos_y_descarta_desconocidos() {
        let catalog = catalog();
        let (game, total, skipped) =
            resolve_live_game(&dto(None, "lufort"), Some(&catalog)).expect("resuelve");

        assert_eq!(game.game_mode, "PRACTICETOOL");
        assert_eq!(game.game_time_secs, 125.5);
        assert_eq!(game.my_champion_id, 34);
        assert_eq!(game.my_champion_name, "Akali");
        assert_eq!(game.my_position, Some(Position::Middle));
        assert_eq!(game.allies, vec![(64, Some(Position::Jungle))]);
        assert_eq!(game.enemies, vec![(238, Some(Position::Middle))]);
        assert_eq!(total, 3);
        assert_eq!(skipped, 1, "el campeón desconocido se descarta");
    }

    /// Con `gameState` restaurado por una versión futura, un valor distinto
    /// de InProgress bloquea la resolución.
    #[test]
    fn resolve_live_game_respeta_gamestate_si_existe() {
        let catalog = catalog();

        let en_curso = dto(Some("InProgress"), "lufort");
        assert!(resolve_live_game(&en_curso, Some(&catalog)).is_some());

        let terminada = dto(Some("Ended"), "lufort");
        assert!(resolve_live_game(&terminada, Some(&catalog)).is_none());
    }

    /// Sin catálogo, sin activePlayer o con champ irresolveable → None
    /// (nada que analizar).
    #[test]
    fn resolve_live_game_exige_catalogo_y_mi_champ() {
        let catalog = catalog();

        assert!(resolve_live_game(&dto(None, "x"), None).is_none());

        let mut sin_active = dto(None, "lufort");
        sin_active.active_player = None;
        assert!(resolve_live_game(&sin_active, Some(&catalog)).is_none());

        let mut champ_raro = dto(None, "lufort");
        champ_raro.all_players[0].champion_name = String::from("DesconocidoYY");
        assert!(resolve_live_game(&champ_raro, Some(&catalog)).is_none());
    }

    /// Contexto sintético: yo en cell 0, aliados desde 1, enemigos desde
    /// 100; ids y posiciones intactos.
    #[test]
    fn match_context_sintetico_mapea_cell_ids() {
        let catalog = catalog();
        let (game, _, _) =
            resolve_live_game(&dto(Some("InProgress"), "lufort"), Some(&catalog))
                .expect("resuelve");
        let context = game.match_context();

        assert_eq!(context.my_cell_id, Some(0));
        assert_eq!(context.my_champion_id, Some(34));
        assert_eq!(context.my_position, Some(Position::Middle));
        assert_eq!(context.allies.len(), 1);
        assert_eq!(context.allies[0].cell_id, 1);
        assert_eq!(context.allies[0].champion_id, Some(64));
        assert_eq!(context.enemies.len(), 1);
        assert_eq!(context.enemies[0].cell_id, 100);
        assert_eq!(context.enemies[0].champion_id, Some(238));
    }

    /// Parser de posiciones: las cinco canónicas y el resto a None.
    #[test]
    fn parse_position_cubre_las_cinco_y_descarta_el_resto() {
        assert_eq!(parse_position("TOP"), Some(Position::Top));
        assert_eq!(parse_position("jungle"), Some(Position::Jungle));
        assert_eq!(parse_position(" MIDDLE "), Some(Position::Middle));
        assert_eq!(parse_position("BOTTOM"), Some(Position::Bottom));
        assert_eq!(parse_position("UTILITY"), Some(Position::Utility));
        assert_eq!(parse_position(""), None);
        assert_eq!(parse_position("ARALLEL"), None);
    }

    /// Resolución por slug case-insensitive cuando `championName` difiere
    /// del ddragon_id.
    #[test]
    fn resolve_champion_id_cae_al_slug() {
        let catalog = catalog();
        assert_eq!(resolve_champion_id("Kayn", &catalog).map(|(id, _)| id), Some(64));
        assert_eq!(resolve_champion_id("kayn", &catalog).map(|(id, _)| id), Some(64));
        assert_eq!(resolve_champion_id("kAyn", &catalog).map(|(id, _)| id), Some(64));
        assert_eq!(resolve_champion_id("Noxus", &catalog), None);
        assert_eq!(resolve_champion_id("", &catalog), None);
    }
}
