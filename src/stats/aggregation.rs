//! Agregación del dataset de partidas a filas de estadísticas de runas.
//!
//! [`aggregate`] consume los [`MatchDetail`] del store (JSONL) y produce un
//! [`Vec<RuneStatRow>`]: una fila por combinación única de
//! `campeón + posición + parche + configuración de runas` (keystone, estilos,
//! ids de runas y runas de stats), sumando `games` y `wins` de todos los
//! participantes que compartan configuración.
//!
//! Se usan **todos** los participantes con perks (los 10 de cada partida), no
//! solo el invocador local: es la forma estándar de ampliar muestra en
//! datasets propios (10× partidas por pase) y el champ select compara
//! configuraciones de cualquier jugador, no solo las tuyas. Los participantes
//! sin `perks` (modos raros, partidas viejas) se ignoran: no aportan
//! configuración de runas.
//!
//! `champion_id` ya llega normalizado desde la fase 8a (Match-V5 lo reporta
//! negativo cuando hubo skin; el modelo hace `abs()` al parsear), así que la
//! agregación no lo toca.
//!
//! La salida es **determinista**: el orden de las filas es el orden de sus
//! campos de configuración (ver el comparador en [`aggregate`]), independiente
//! del orden en que lleguen las partidas o del hashing del agrupamiento.
//!
//! Funciones puras, sin IO ni señales: todo testeado con fixtures inline.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::lcu::models::Position;
use crate::riot_api::models::{MatchDetail, Perks};

/// Identidad de configuración de una fila: todos los campos que definen la
/// celda del dataset, excluidos los acumuladores `games`/`wins`.
///
/// Dos participantes caen en la misma fila si y solo si sus
/// [`RuneStatKey`] son iguales. Deriva `Hash`/`Eq` para usarlo como clave del
/// agrupamiento (y de cualquier mapa futuro del scoring).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuneStatKey {
    /// Id canónico del campeón (ya normalizado positivo).
    pub champion_id: i64,
    /// Posición asignada por el matchmaking (`None` = colas sin rol).
    pub position: Option<Position>,
    /// Parche (`major.minor` de `gameVersion`, ej. `"16.7"`).
    pub patch: String,
    /// Keystone elegida (primera selección del `primaryPath`).
    pub keystone_id: Option<i64>,
    /// Id del árbol de runas principal.
    pub primary_style: Option<i64>,
    /// Id del árbol de runas secundario.
    pub sub_style: Option<i64>,
    /// Todos los ids de runas elegidos: `primaryPath` primero (keystone
    /// incluida), `subPath` después — el orden determinista del helper de la
    /// fase 8a.
    pub perk_ids: Vec<i64>,
    /// Runa de stats de ataque (`statPerks.offense`).
    pub stat_offense: Option<i64>,
    /// Runa de stats flexible (`statPerks.flex`).
    pub stat_flex: Option<i64>,
    /// Runa de stats de defensa (`statPerks.defense`).
    pub stat_defense: Option<i64>,
}

/// Una fila del dataset: una configuración de runas para un campeón en una
/// posición y parche, con sus partidas y victorias acumuladas.
///
/// Deriva `Serialize`/`Deserialize` para poder cachear el agregado a futuro.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuneStatRow {
    /// Id canónico del campeón.
    pub champion_id: i64,
    /// Posición asignada (`None` = cualquier/cónsola sin rol).
    pub position: Option<Position>,
    /// Parche (`"16.7"`).
    pub patch: String,
    /// Keystone elegida.
    pub keystone_id: Option<i64>,
    /// Árbol principal.
    pub primary_style: Option<i64>,
    /// Árbol secundario.
    pub sub_style: Option<i64>,
    /// Ids de runas elegidos (primary primero).
    pub perk_ids: Vec<i64>,
    /// Runa de stats de ataque.
    pub stat_offense: Option<i64>,
    /// Runa de stats flexible.
    pub stat_flex: Option<i64>,
    /// Runa de stats de defensa.
    pub stat_defense: Option<i64>,
    /// Partidas acumuladas de esta configuración.
    pub games: u32,
    /// Victorias acumuladas (≤ `games` salvo dato corrupto).
    pub wins: u32,
}

impl RuneStatRow {
    /// La identidad de configuración de esta fila (todos los campos menos
    /// `games`/`wins`).
    #[must_use]
    pub fn key(&self) -> RuneStatKey {
        RuneStatKey {
            champion_id: self.champion_id,
            position: self.position,
            patch: self.patch.clone(),
            keystone_id: self.keystone_id,
            primary_style: self.primary_style,
            sub_style: self.sub_style,
            perk_ids: self.perk_ids.clone(),
            stat_offense: self.stat_offense,
            stat_flex: self.stat_flex,
            stat_defense: self.stat_defense,
        }
    }
}

/// Prefijo `major.minor` de una versión de juego: `"16.7.623.1234"` →
/// `"16.7"`, `"16.7"` → `"16.7"`, `"16"` → `"16"`. Vacío si no hay un primer
/// componente utilizable. Puro y testeado.
///
/// Lógica gemela al `patch_prefix` privado de `data_dragon` (que elige la
/// versión de la CDN): acá solo interesa etiquetar la fila con el parche.
#[must_use]
pub fn patch_of(game_version: &str) -> String {
    let mut components = game_version.split('.');
    let Some(major) = components.next().filter(|major| !major.is_empty()) else {
        return String::new();
    };
    let minor = components.next().unwrap_or_default();
    if minor.is_empty() {
        major.to_string()
    } else {
        format!("{major}.{minor}")
    }
}

/// Rango de orden para posiciones: `None` (sin rol) primero, después el orden
/// canónico Top/Jungle/Middle/Bottom/Utility. Solo sirve para el orden
/// determinista de la salida, no es un ranking de juego.
fn position_rank(position: Option<Position>) -> u8 {
    match position {
        None => 0,
        Some(Position::Top) => 1,
        Some(Position::Jungle) => 2,
        Some(Position::Middle) => 3,
        Some(Position::Bottom) => 4,
        Some(Position::Utility) => 5,
    }
}

/// Agrega las partidas a filas de estadísticas, ordenadas de forma
/// determinista por configuración (`champion_id`, posición, parche y resto de
/// la configuración). Los participantes sin perks se ignoran.
#[must_use]
pub fn aggregate(matches: &[MatchDetail]) -> Vec<RuneStatRow> {
    let mut groups: HashMap<RuneStatKey, (u32, u32)> = HashMap::new();

    for detail in matches {
        let patch = patch_of(&detail.info.game_version);
        for participant in &detail.info.participants {
            // Sin runas no hay configuración que agregar (modos raros,
            // partidas viejas).
            let Some(perks) = participant.perks.as_ref() else {
                continue;
            };

            let key = rune_stat_key(
                participant.champion_id,
                participant.team_position,
                &patch,
                perks,
            );
            let entry = groups.entry(key).or_insert((0, 0));
            entry.0 += 1;
            if participant.win {
                entry.1 += 1;
            }
        }
    }

    let mut rows: Vec<RuneStatRow> = groups
        .into_iter()
        .map(|(key, (games, wins))| RuneStatRow {
            champion_id: key.champion_id,
            position: key.position,
            patch: key.patch,
            keystone_id: key.keystone_id,
            primary_style: key.primary_style,
            sub_style: key.sub_style,
            perk_ids: key.perk_ids,
            stat_offense: key.stat_offense,
            stat_flex: key.stat_flex,
            stat_defense: key.stat_defense,
            games,
            wins,
        })
        .collect();

    // Orden determinista por configuración completa. `Position` no deriva
    // `Ord`, así que la posición entra por su rango; el resto de los campos
    // compara con el orden natural de `i64`/`String`/`Vec<i64>`.
    rows.sort_by(|a, b| {
        a.champion_id
            .cmp(&b.champion_id)
            .then_with(|| position_rank(a.position).cmp(&position_rank(b.position)))
            .then_with(|| a.patch.cmp(&b.patch))
            .then_with(|| a.perk_ids.cmp(&b.perk_ids))
            .then_with(|| a.keystone_id.cmp(&b.keystone_id))
            .then_with(|| a.primary_style.cmp(&b.primary_style))
            .then_with(|| a.sub_style.cmp(&b.sub_style))
            .then_with(|| a.stat_offense.cmp(&b.stat_offense))
            .then_with(|| a.stat_flex.cmp(&b.stat_flex))
            .then_with(|| a.stat_defense.cmp(&b.stat_defense))
    });
    rows
}

/// Arma la clave de configuración de un participante a partir de sus perks
/// (helpers puros de la fase 8a).
fn rune_stat_key(
    champion_id: i64,
    position: Option<Position>,
    patch: &str,
    perks: &Perks,
) -> RuneStatKey {
    RuneStatKey {
        champion_id,
        position,
        patch: patch.to_string(),
        keystone_id: perks.keystone_perk(),
        primary_style: perks.primary_style(),
        sub_style: perks.sub_style(),
        perk_ids: perks.selection_perk_ids(),
        stat_offense: perks.stat_perks.as_ref().map(|stats| stats.offense),
        stat_flex: perks.stat_perks.as_ref().map(|stats| stats.flex),
        stat_defense: perks.stat_perks.as_ref().map(|stats| stats.defense),
    }
}

/// Filas del dataset para una consulta de champ select: campeón + parche,
/// con `position = Some(p)` para restringir a esa posición o `None` para
/// cualquier posición (el dataset por posición es chico al arrancar; el
/// scoring puede preferir el pool combinado).
#[must_use]
pub fn rows_for<'a>(
    rows: &'a [RuneStatRow],
    champion_id: i64,
    position: Option<Position>,
    patch: &str,
) -> Vec<&'a RuneStatRow> {
    rows.iter()
        .filter(|row| {
            row.champion_id == champion_id
                && row.patch == patch
                && position.is_none_or(|wanted| row.position == Some(wanted))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::riot_api::models::{
        MatchInfo, MatchMetadata, Participant, PerkSelection, PerkStyle, StatPerks,
    };
    use serde_json::json;

    /// Configuración de runas "completa" de prueba (estilo del fixture real
    /// de la fase 8a: Electrocute + 3, Hechicería + 2).
    fn perks_ahri() -> Perks {
        Perks {
            stat_perks: Some(StatPerks {
                offense: 5005,
                flex: 5008,
                defense: 5002,
            }),
            styles: vec![
                PerkStyle {
                    description: String::from("primaryPath"),
                    style: 8100,
                    selections: vec![
                        PerkSelection {
                            perk: 8112,
                            ..PerkSelection::default()
                        },
                        PerkSelection {
                            perk: 8139,
                            ..PerkSelection::default()
                        },
                    ],
                },
                PerkStyle {
                    description: String::from("subPath"),
                    style: 8200,
                    selections: vec![PerkSelection {
                        perk: 8210,
                        ..PerkSelection::default()
                    }],
                },
            ],
        }
    }

    /// Participante de prueba.
    fn participant(
        champion_id: i64,
        position: Option<Position>,
        win: bool,
        perks: Option<Perks>,
    ) -> Participant {
        Participant {
            puuid: None,
            champion_id,
            champion_name: None,
            team_id: 100,
            win,
            team_position: position,
            perks,
        }
    }

    /// Partida de prueba con la versión dada.
    fn match_of(version: &str, participants: Vec<Participant>) -> MatchDetail {
        MatchDetail {
            metadata: MatchMetadata {
                match_id: String::from("LA1_1"),
            },
            info: MatchInfo {
                game_version: version.to_string(),
                participants,
                teams: Vec::new(),
            },
        }
    }

    #[test]
    fn patch_of_extrae_el_prefijo_major_minor() {
        assert_eq!(patch_of("16.7.623.1234"), "16.7");
        assert_eq!(patch_of("16.7"), "16.7");
        assert_eq!(patch_of("16"), "16");
        assert_eq!(patch_of(""), "");
        assert_eq!(patch_of(".7"), "");
    }

    /// Agregación nominal: participantes con la misma configuración caen en
    /// la misma fila (games/wins suman); el participante sin perks se
    /// ignora; el champion_id negativo por skin ya llega normalizado.
    #[test]
    fn agrega_participantes_con_la_misma_configuracion() {
        let perks = perks_ahri();

        let m1 = match_of(
            "16.7.623.1234",
            vec![
                // Ahri MIDDLE con perks, win. El id ya llega normalizado
                // (positivo) desde la fase 8a: `aggregate` no lo toca (el
                // quirk del -103 con skin se prueba en el fixture JSON de
                // más abajo, que pasa por la deserialización real).
                participant(103, Some(Position::Middle), true, Some(perks.clone())),
                // Thresh sin posición (cola sin rol), loss.
                participant(412, None, false, Some(Perks::default())),
                // Sin perks: ignorado por completo.
                participant(22, Some(Position::Top), true, None),
            ],
        );
        let m2 = match_of(
            "16.7.999.1",
            vec![
                // Misma config de Ahri MIDDLE, win otra vez.
                participant(103, Some(Position::Middle), true, Some(perks)),
                // Ahri BOT con otra keystone: fila distinta aunque el
                // campeón sea el mismo.
                participant(
                    103,
                    Some(Position::Bottom),
                    false,
                    Some({
                        let mut other = Perks::default();
                        other.styles.push(PerkStyle {
                            description: String::from("primaryPath"),
                            style: 8100,
                            selections: vec![PerkSelection {
                                perk: 8008,
                                ..PerkSelection::default()
                            }],
                        });
                        other
                    }),
                ),
            ],
        );

        let rows = aggregate(&[m1, m2]);

        // 3 filas: Ahri MIDDLE, Ahri BOT y Thresh (None).
        assert_eq!(rows.len(), 3);

        let ahri_mid = &rows[0];
        assert_eq!(ahri_mid.champion_id, 103);
        assert_eq!(ahri_mid.position, Some(Position::Middle));
        assert_eq!(ahri_mid.patch, "16.7");
        assert_eq!(ahri_mid.keystone_id, Some(8112));
        assert_eq!(ahri_mid.primary_style, Some(8100));
        assert_eq!(ahri_mid.sub_style, Some(8200));
        assert_eq!(ahri_mid.perk_ids, vec![8112, 8139, 8210]);
        assert_eq!(ahri_mid.stat_offense, Some(5005));
        assert_eq!(ahri_mid.stat_flex, Some(5008));
        assert_eq!(ahri_mid.stat_defense, Some(5002));
        assert_eq!(ahri_mid.games, 2);
        assert_eq!(ahri_mid.wins, 2);

        // Ahri BOT: 1 game, 0 wins (la fila 2 del orden por champion_id +
        // posición).
        let ahri_bot = &rows[1];
        assert_eq!(ahri_bot.champion_id, 103);
        assert_eq!(ahri_bot.position, Some(Position::Bottom));
        assert_eq!(ahri_bot.keystone_id, Some(8008));
        assert_eq!(ahri_bot.games, 1);
        assert_eq!(ahri_bot.wins, 0);

        // La fila de Thresh: perks sin estilos → todo None, games 1, wins 0.
        let thresh = &rows[2];
        assert_eq!(thresh.champion_id, 412);
        assert_eq!(thresh.position, None);
        assert_eq!(thresh.keystone_id, None);
        assert!(thresh.perk_ids.is_empty());
        assert_eq!(thresh.games, 1);
        assert_eq!(thresh.wins, 0);
    }

    /// El orden de la salida es determinista e independiente del orden de
    /// entrada: agregar [A, B] produce lo mismo que [B, A].
    #[test]
    fn la_salida_es_determinista_ante_orden_de_entrada_distinto() {
        let perks = perks_ahri();
        let a = match_of(
            "16.7.1",
            vec![participant(
                103,
                Some(Position::Middle),
                true,
                Some(perks.clone()),
            )],
        );
        let b = match_of(
            "16.8.1",
            vec![participant(22, Some(Position::Top), false, Some(perks))],
        );

        assert_eq!(aggregate(&[a.clone(), b.clone()]), aggregate(&[b, a]));
    }

    /// Distinto parche = fila distinta, aunque la configuración sea idéntica.
    #[test]
    fn parches_distintos_no_se_mezclan() {
        let perks = perks_ahri();
        let vieja = match_of(
            "16.6.1",
            vec![participant(
                103,
                Some(Position::Middle),
                true,
                Some(perks.clone()),
            )],
        );
        let nueva = match_of(
            "16.7.1",
            vec![participant(103, Some(Position::Middle), false, Some(perks))],
        );

        let rows = aggregate(&[vieja, nueva]);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].patch, "16.6");
        assert_eq!(rows[1].patch, "16.7");
    }

    /// `rows_for`: filtra por campeón+parche; `position None` = cualquier
    /// posición, `Some` = solo esa.
    #[test]
    fn rows_for_filtra_por_campeon_parche_y_posicion() {
        let rows = vec![
            RuneStatRow {
                champion_id: 103,
                position: Some(Position::Middle),
                patch: String::from("16.7"),
                games: 5,
                wins: 3,
                ..RuneStatRow::default_config()
            },
            RuneStatRow {
                champion_id: 103,
                position: Some(Position::Bottom),
                patch: String::from("16.7"),
                games: 2,
                wins: 1,
                ..RuneStatRow::default_config()
            },
            RuneStatRow {
                champion_id: 103,
                position: Some(Position::Middle),
                patch: String::from("16.6"),
                games: 9,
                wins: 4,
                ..RuneStatRow::default_config()
            },
            RuneStatRow {
                champion_id: 412,
                position: Some(Position::Middle),
                patch: String::from("16.7"),
                games: 7,
                wins: 2,
                ..RuneStatRow::default_config()
            },
        ];

        // Cualquier posición: ambas filas de Ahri 16.7.
        let any = rows_for(&rows, 103, None, "16.7");
        assert_eq!(any.len(), 2);
        assert_eq!(any[0].position, Some(Position::Middle));
        assert_eq!(any[1].position, Some(Position::Bottom));

        // Posición puntual.
        let mid = rows_for(&rows, 103, Some(Position::Middle), "16.7");
        assert_eq!(mid.len(), 1);
        assert_eq!(mid[0].games, 5);

        // Parche o campeón distinto → vacío.
        assert!(rows_for(&rows, 103, None, "16.9").is_empty());
        assert!(rows_for(&rows, 22, None, "16.7").is_empty());
    }

    /// `key()` excluye games/wins: dos filas con distinto conteo comparten
    /// clave si su configuración es igual.
    #[test]
    fn key_ignora_games_y_wins() {
        let base = RuneStatRow {
            champion_id: 103,
            position: Some(Position::Middle),
            patch: String::from("16.7"),
            games: 5,
            wins: 3,
            ..RuneStatRow::default_config()
        };
        let mut other = base.clone();
        other.games += 1;
        other.wins += 1;

        assert_eq!(base.key(), other.key());
        assert_ne!(base, other);
    }

    /// `RuneStatRow` (y su clave) hace roundtrip serde, y `Position`
    /// serializa como el código UPPERCASE del LCU.
    #[test]
    fn rune_stat_row_hace_roundtrip_serde() {
        let row = RuneStatRow {
            champion_id: 103,
            position: Some(Position::Utility),
            patch: String::from("16.7"),
            keystone_id: Some(8112),
            primary_style: Some(8100),
            sub_style: Some(8200),
            perk_ids: vec![8112, 8139, 8210],
            stat_offense: Some(5005),
            stat_flex: Some(5008),
            stat_defense: Some(5002),
            games: 12,
            wins: 7,
        };

        let json = serde_json::to_string(&row).expect("serializa");
        // Formato camelCase estable (caché a futuro).
        assert!(json.contains("\"championId\":103"));
        assert!(json.contains("\"keystoneId\":8112"));
        assert!(json.contains("\"position\":\"UTILITY\""));

        let back: RuneStatRow = serde_json::from_str(&json).expect("deserializa");
        assert_eq!(back, row);
        assert_eq!(back.key(), row.key());
    }

    /// La agregación consume el fixture JSON real de la fase 8a (con
    /// championId negativo y participante sin perks) y produce filas
    /// correctas.
    #[test]
    fn agrega_el_fixture_json_de_la_fase_8a() {
        let detail: MatchDetail = serde_json::from_value(json!({
            "metadata": {"matchId": "LA1_123"},
            "info": {
                "gameVersion": "16.7.623.1234",
                "participants": [
                    {
                        "puuid": "p1",
                        "championId": -103,
                        "teamId": 100,
                        "win": true,
                        "teamPosition": "MIDDLE",
                        "perks": {
                            "statPerks": {"offense": 5005, "flex": 5008, "defense": 5002},
                            "styles": [
                                {"description": "primaryPath", "style": 8100, "selections": [
                                    {"perk": 8112}, {"perk": 8139}
                                ]},
                                {"description": "subPath", "style": 8200, "selections": [
                                    {"perk": 8210}
                                ]}
                            ]
                        }
                    },
                    {"puuid": "p2", "championId": 22, "teamId": 100, "win": true}
                ]
            }
        }))
        .expect("fixture parsea");

        let rows = aggregate(&[detail]);
        assert_eq!(rows.len(), 1, "el sin perks no genera fila");
        assert_eq!(rows[0].champion_id, 103);
        assert_eq!(rows[0].games, 1);
        assert_eq!(rows[0].wins, 1);
    }

    impl RuneStatRow {
        /// Config vacía para armar filas de prueba legibles.
        fn default_config() -> Self {
            Self {
                champion_id: 0,
                position: None,
                patch: String::new(),
                keystone_id: None,
                primary_style: None,
                sub_style: None,
                perk_ids: Vec::new(),
                stat_offense: None,
                stat_flex: None,
                stat_defense: None,
                games: 0,
                wins: 0,
            }
        }
    }
}
