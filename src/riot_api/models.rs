//! Modelos serde de Match-V5 (`/lol/match/v5/matches/...`).
//!
//! Subconjunto mínimo para el dataset de runas (FASE 8a/8b): solo
//! `metadata.matchId`, `info.gameVersion`, `info.participants` (con `perks`
//! completos) e `info.teams` (bans por equipo). Todo campo que no declaremos
//! se ignora al deserializar, y la tolerancia es total:
//!
//! - `#[serde(default)]` a nivel struct: cualquier campo faltante toma su
//!   default, nunca invalida el payload completo.
//! - `teamPosition` se mapea con el mismo [`Position`] del champ select y el
//!   deserializador tolerante ya existente (`""`, `"NONE"`, `"FILL"` o un
//!   código desconocido → `None`): los modos raros sin posiciones no rompen
//!   el parseo.
//! - `perks` es `Option<Perks>`: hay participantes sin runas (modos de
//!   evento, partidas viejas) y deben sobrevivir.
//! - `championId` negativo (quirk conocido de Match-V5: Riot lo reporta en
//!   negativo cuando el jugador usó una skin) se normaliza con `abs()` en la
//!   deserialización, así el id queda listo para cruzar contra los catálogos
//!   de Data Dragon.
//!
//! Los helpers de [`Perks`] (`primary_style`, `sub_style`, `keystone_perk`,
//! `selection_perk_ids`) son puros y son la API que la fase 8b va a usar
//! para el agregado de runas.

use serde::{Deserialize, Serialize};

use crate::lcu::models::{Position, deserialize_optional_position};

/// `description` del estilo principal de runas en el payload de Match-V5.
const PRIMARY_PATH: &str = "primaryPath";
/// `description` del estilo secundario de runas en el payload de Match-V5.
const SUB_PATH: &str = "subPath";

/// Normaliza `championId` de Match-V5: Riot lo reporta negativo cuando el
/// jugador usó una skin; el id canónico del campeón es el valor absoluto.
/// `wrapping_abs` en vez de `abs()` para no paniquear ante el caso degenerado
/// `i64::MIN` (imposible en la práctica, pero el parseo nunca debe fallar por
/// un número raro).
fn deserialize_normalized_champion_id<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = i64::deserialize(deserializer)?;
    Ok(raw.wrapping_abs())
}

/// Metadatos de la partida. Solo nos interesa el id (`"LA1_1234567890"`),
/// que es la clave con la que la fase 8b va a deduplicar el dataset.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct MatchMetadata {
    /// Id global de la partida (ej. `"LA1_1234567890"`).
    pub match_id: String,
}

/// Bloque `info` de la partida: versión del juego, participantes y equipos.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct MatchInfo {
    /// Versión del juego con la que se jugó (ej. `"16.7.623.1234"`), útil
    /// para descartar partidas de parches viejos al agregar runas.
    pub game_version: String,
    /// Los 10 participantes (a veces menos en modos raros).
    pub participants: Vec<Participant>,
    /// Un elemento por equipo, con los bans de ese equipo.
    pub teams: Vec<TeamInfo>,
}

/// Partida terminada, respuesta de `/lol/match/v5/matches/{matchId}`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct MatchDetail {
    pub metadata: MatchMetadata,
    pub info: MatchInfo,
}

/// Un participante de la partida: lo mínimo para el dataset de runas.
///
/// `perks` y `team_position` son `Option` porque hay modos/épocas sin runas
/// ni posiciones asignadas; un participante sin esos campos se parsea igual
/// y queda marcado como ausente en vez de romper la partida entera.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Participant {
    /// PUUID del jugador (`None` en payloads viejos/anónimos).
    pub puuid: Option<String>,
    /// Id del campeón, normalizado con `abs()` (ver
    /// [`deserialize_normalized_champion_id`]): Match-V5 lo reporta negativo
    /// cuando el jugador usó una skin.
    #[serde(deserialize_with = "deserialize_normalized_champion_id")]
    pub champion_id: i64,
    /// Nombre interno del campeón (ej. `"Ahri"`); `None`/vacío en partidas
    /// viejas.
    pub champion_name: Option<String>,
    /// `100` (equipo azul) o `200` (rojo).
    pub team_id: i64,
    /// `true` si el equipo del participante ganó.
    pub win: bool,
    /// Posición asignada por el matchmaking (`TOP`, `JUNGLE`, `MIDDLE`,
    /// `BOTTOM`, `UTILITY`), con el mismo mapeo tolerante del champ select:
    /// `""`, `NONE` u otro código desconocido → `None`.
    #[serde(deserialize_with = "deserialize_optional_position")]
    pub team_position: Option<Position>,
    /// Runas del participante; `None` si el payload no las trae.
    pub perks: Option<Perks>,
}

/// Runas de un participante: runas de stats + los dos estilos (principal y
/// secundario) con sus selecciones.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Perks {
    /// Runas de stats (fila 3 del editor de runas); `None` si el payload no
    /// las trae.
    pub stat_perks: Option<StatPerks>,
    /// Los dos estilos (`primaryPath` y `subPath`), en el orden del payload.
    pub styles: Vec<PerkStyle>,
}

/// Runas de stats del participante (`statPerks`): los tres slots de la
/// tercera fila, ids `5001..5003` y `5007..5008` según el shard.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct StatPerks {
    pub offense: i64,
    pub flex: i64,
    pub defense: i64,
}

/// Un estilo de runas del participante (árbol completo de keystone+runas).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PerkStyle {
    /// `"primaryPath"` (estilo principal) o `"subPath"` (secundario).
    pub description: String,
    /// Id del árbol de runas (mismos ids de `runesReforged.json` de Data
    /// Dragon: `8000` Precisión, `8100` Dominación, `8200` Hechicería,
    /// `8300` Inspiración, `8400` Valor).
    pub style: i64,
    /// Selecciones del estilo: 4 en el principal (keystone + 3), 2 en el
    /// secundario.
    pub selections: Vec<PerkSelection>,
}

/// Una runa elegida dentro de un estilo, con las variables de la partida.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PerkSelection {
    /// Id de la runa (mismos ids de `runesReforged.json`).
    pub perk: i64,
    /// Valor de la variable 1 de la runa en esta partida (daño hecho, etc.);
    /// el dataset lo guarda pero el scoring puede ignorarlo.
    pub var1: i64,
    /// Valor de la variable 2.
    pub var2: i64,
    /// Valor de la variable 3.
    pub var3: i64,
}

/// Bloque de un equipo de la partida: por ahora solo los bans (útiles a
/// futuro para el recomendador); `win` de equipo es redundante con
/// `Participant::win` pero cuesta nada y valida el parseo.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TeamInfo {
    /// `100` (equipo azul) o `200` (rojo).
    pub team_id: i64,
    /// `true` si el equipo ganó.
    pub win: bool,
    /// Bans del equipo; `None` si el payload no los trae (o trae `null`):
    /// algunas épocas/modos no tuvieron bans.
    pub bans: Option<Vec<TeamBan>>,
}

/// Un ban de equipo: solo el id del campeón banneado.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TeamBan {
    /// Id del campeón banneado (`-1` = ban no usado, se conserva crudo).
    pub champion_id: i64,
}

impl Perks {
    /// Estilo cuyo `description` coincide exactamente con el código del
    /// payload (`"primaryPath"` / `"subPath"`).
    fn style_by_path(&self, description: &str) -> Option<&PerkStyle> {
        self.styles
            .iter()
            .find(|style| style.description == description)
    }

    /// Id del árbol de runas principal (keystone incluido), si vino.
    #[must_use]
    pub fn primary_style(&self) -> Option<i64> {
        self.style_by_path(PRIMARY_PATH).map(|style| style.style)
    }

    /// Id del árbol de runas secundario, si vino.
    #[must_use]
    pub fn sub_style(&self) -> Option<i64> {
        self.style_by_path(SUB_PATH).map(|style| style.style)
    }

    /// Keystone del participante: la primera selección del `primaryPath`.
    #[must_use]
    pub fn keystone_perk(&self) -> Option<i64> {
        self.style_by_path(PRIMARY_PATH)?
            .selections
            .first()
            .map(|selection| selection.perk)
    }

    /// Todos los ids de runas elegidos, primaryPath primero y subPath
    /// después (orden de selección dentro de cada estilo, sin deduplicar).
    /// Los estilos con `description` desconocida se ignoran: el orden queda
    /// determinista aunque el payload invierta los estilos.
    #[must_use]
    pub fn selection_perk_ids(&self) -> Vec<i64> {
        [PRIMARY_PATH, SUB_PATH]
            .iter()
            .filter_map(|path| self.style_by_path(path))
            .flat_map(|style| style.selections.iter().map(|s| s.perk))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Partida realista (fase 8a): 3 participantes —uno con perks completos
    /// y `championId` negativo por skin, otro sin `teamPosition`, y uno de
    /// modos raros sin perks ni `teamPosition`— más teams con bans.
    const MATCH_FIXTURE: &str = r#"{
        "metadata": {
            "dataVersion": "2.0",
            "matchId": "LA1_1234567890",
            "participants": ["puuid-a", "puuid-b", "puuid-c"]
        },
        "info": {
            "gameId": 1234567890,
            "gameDuration": 1802,
            "gameMode": "CLASSIC",
            "gameVersion": "16.7.623.1234",
            "participants": [
                {
                    "puuid": "puuid-a",
                    "championId": -103,
                    "championName": "Ahri",
                    "teamId": 100,
                    "win": true,
                    "teamPosition": "MIDDLE",
                    "perks": {
                        "statPerks": {"defense": 5002, "flex": 5008, "offense": 5005},
                        "styles": [
                            {
                                "description": "primaryPath",
                                "style": 8100,
                                "selections": [
                                    {"perk": 8112, "var1": 12345, "var2": 2, "var3": 0},
                                    {"perk": 8139, "var1": 5000, "var2": 0, "var3": 0},
                                    {"perk": 8143, "var1": 444, "var2": 0, "var3": 0},
                                    {"perk": 8137, "var1": 11, "var2": 22, "var3": 33}
                                ]
                            },
                            {
                                "description": "subPath",
                                "style": 8200,
                                "selections": [
                                    {"perk": 8210, "var1": 0, "var2": 0, "var3": 0},
                                    {"perk": 8234, "var1": 9, "var2": 0, "var3": 0}
                                ]
                            }
                        ]
                    }
                },
                {
                    "puuid": "puuid-b",
                    "championId": 412,
                    "championName": "Thresh",
                    "teamId": 200,
                    "win": false,
                    "teamPosition": "",
                    "perks": {
                        "statPerks": {"defense": 5001, "flex": 5003, "offense": 5007},
                        "styles": []
                    }
                },
                {
                    "puuid": "puuid-c",
                    "championId": 22,
                    "championName": "",
                    "teamId": 100,
                    "win": true
                }
            ],
            "teams": [
                {
                    "teamId": 100,
                    "win": true,
                    "bans": [{"championId": 86}, {"championId": 157}, {"championId": -1}]
                },
                {
                    "teamId": 200,
                    "win": false,
                    "bans": null
                }
            ]
        }
    }"#;

    #[test]
    fn parsea_la_partida_completa() {
        let detail: MatchDetail =
            serde_json::from_str(MATCH_FIXTURE).expect("fixture válido");

        assert_eq!(detail.metadata.match_id, "LA1_1234567890");
        assert_eq!(detail.info.game_version, "16.7.623.1234");
        assert_eq!(detail.info.participants.len(), 3);
        assert_eq!(detail.info.teams.len(), 2);
    }

    /// Quirk de Match-V5: `championId` llega negativo (skins) y se normaliza
    /// con abs; el resto de los campos sobrevive intacto.
    #[test]
    fn champion_id_negativo_por_skin_se_normaliza_con_abs() {
        let detail: MatchDetail =
            serde_json::from_str(MATCH_FIXTURE).expect("fixture válido");

        let ahri = &detail.info.participants[0];
        assert_eq!(ahri.champion_id, 103);
        assert_eq!(ahri.champion_name.as_deref(), Some("Ahri"));
        assert_eq!(ahri.team_id, 100);
        assert!(ahri.win);
        assert_eq!(ahri.team_position, Some(Position::Middle));
        assert_eq!(ahri.puuid.as_deref(), Some("puuid-a"));
    }

    /// Posiciones tolerantes: `""` (colas sin rol) → `None`, y la ausencia
    /// del campo (modos raros) también → `None`, sin romper el participante.
    #[test]
    fn team_position_tolerante_a_codigos_y_ausencias() {
        let detail: MatchDetail =
            serde_json::from_str(MATCH_FIXTURE).expect("fixture válido");

        let thresh = &detail.info.participants[1];
        assert_eq!(thresh.team_position, None);
        assert_eq!(thresh.champion_id, 412);
        assert!(!thresh.win);

        let sin_perks = &detail.info.participants[2];
        assert_eq!(sin_perks.team_position, None);
        assert_eq!(sin_perks.perks, None);
    }

    /// Helpers de perks sobre el participante con runas completas.
    #[test]
    fn helpers_de_perks_extraen_primario_secundario_y_keystone() {
        let detail: MatchDetail =
            serde_json::from_str(MATCH_FIXTURE).expect("fixture válido");
        let perks = detail.info.participants[0]
            .perks
            .as_ref()
            .expect("Ahri tiene perks");

        assert_eq!(perks.primary_style(), Some(8100));
        assert_eq!(perks.sub_style(), Some(8200));
        assert_eq!(perks.keystone_perk(), Some(8112));
        assert_eq!(
            perks.selection_perk_ids(),
            vec![8112, 8139, 8143, 8137, 8210, 8234]
        );
        assert_eq!(
            perks.stat_perks,
            Some(StatPerks {
                offense: 5005,
                flex: 5008,
                defense: 5002,
            })
        );
    }

    /// El orden de `selection_perk_ids` es determinista aunque el payload
    /// invierta los estilos (subPath antes de primaryPath) y aunque venga un
    /// estilo con `description` desconocida: primaryPath primero, subPath
    /// después, desconocidos ignorados. El keystone siempre sale del
    /// primaryPath aunque no sea el primer elemento de `styles`.
    #[test]
    fn selection_perk_ids_es_determinista_con_estilos_invertidos() {
        let perks = Perks {
            styles: vec![
                PerkStyle {
                    description: String::from("subPath"),
                    style: 8200,
                    selections: vec![PerkSelection {
                        perk: 8210,
                        ..PerkSelection::default()
                    }],
                },
                PerkStyle {
                    description: String::from("unknownPath"),
                    style: 8300,
                    selections: vec![PerkSelection {
                        perk: 9999,
                        ..PerkSelection::default()
                    }],
                },
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
            ],
            ..Perks::default()
        };

        assert_eq!(perks.primary_style(), Some(8100));
        assert_eq!(perks.sub_style(), Some(8200));
        assert_eq!(perks.keystone_perk(), Some(8112));
        // Ni el estilo invertido ni el desconocido alteran el orden.
        assert_eq!(perks.selection_perk_ids(), vec![8112, 8139, 8210]);
    }

    /// Un `primaryPath` presente pero sin selecciones (payload recortado):
    /// el estilo se reporta (`Some`) pero el keystone es `None`, sin índices
    /// fuera de rango ni pánico.
    #[test]
    fn primary_sin_selecciones_da_keystone_none() {
        let perks = Perks {
            styles: vec![PerkStyle {
                description: String::from("primaryPath"),
                style: 8400,
                selections: vec![],
            }],
            ..Perks::default()
        };

        assert_eq!(perks.primary_style(), Some(8400));
        assert_eq!(perks.keystone_perk(), None);
        assert!(perks.selection_perk_ids().is_empty());
    }

    /// Participante con `perks.styles: []` (modos raros): los helpers son
    /// `None`/vacío en vez de romper.
    #[test]
    fn perks_vacios_producen_helpers_none() {
        let detail: MatchDetail =
            serde_json::from_str(MATCH_FIXTURE).expect("fixture válido");
        let perks = detail.info.participants[1]
            .perks
            .as_ref()
            .expect("Thresh tiene perks aunque sin estilos");

        assert_eq!(perks.primary_style(), None);
        assert_eq!(perks.sub_style(), None);
        assert_eq!(perks.keystone_perk(), None);
        assert!(perks.selection_perk_ids().is_empty());
    }

    /// Bans de equipo: lista → `Some`, `null` → `None`. El `-1` (ban sin
    /// usar) se conserva crudo.
    #[test]
    fn bans_tolerantes_a_null() {
        let detail: MatchDetail =
            serde_json::from_str(MATCH_FIXTURE).expect("fixture válido");
        let teams = &detail.info.teams;

        assert_eq!(
            teams[0].bans,
            Some(vec![
                TeamBan { champion_id: 86 },
                TeamBan { champion_id: 157 },
                TeamBan { champion_id: -1 },
            ])
        );
        assert!(teams[0].win);
        assert_eq!(teams[1].bans, None);
        assert!(!teams[1].win);
    }

    /// Payload mínimo (partida recortada, metadata/teams ausentes, perks
    /// `null`): nada rompe y todo cae a defaults.
    #[test]
    fn payload_minimo_cae_a_defaults() {
        let detail: MatchDetail = serde_json::from_value(json!({
            "metadata": {"matchId": "LA1_1"},
            "info": {
                "participants": [
                    {"puuid": null, "perks": null, "championName": ""}
                ]
            }
        }))
        .expect("payload mínimo parsea");

        assert_eq!(detail.metadata.match_id, "LA1_1");
        assert_eq!(detail.info.game_version, "");
        assert_eq!(detail.info.teams, vec![]);
        assert_eq!(detail.info.participants[0].perks, None);
        assert_eq!(detail.info.participants[0].puuid, None);
        assert_eq!(detail.info.participants[0].champion_id, 0);
        assert_eq!(detail.info.participants[0].team_id, 0);
        assert!(!detail.info.participants[0].win);
    }

    /// Un objeto totalmente vacío también es tolerado (defaults en todos los
    /// niveles, gracias a `#[serde(default)]` de contenedor).
    #[test]
    fn objeto_vacio_parsea_a_default() {
        let detail: MatchDetail = serde_json::from_str("{}").expect("vacio parsea");
        assert_eq!(detail, MatchDetail::default());
    }

    /// El dataset (fase 8b) persiste en JSONL: las partidas deben serializar
    /// y deserializar idénticas (roundtrip), incluyendo el championId ya
    /// normalizado (positivo).
    #[test]
    fn match_detail_hace_roundtrip_serde() {
        let detail: MatchDetail =
            serde_json::from_str(MATCH_FIXTURE).expect("fixture válido");

        let json = serde_json::to_string(&detail).expect("serializa");
        let back: MatchDetail = serde_json::from_str(&json).expect("deserializa");

        assert_eq!(back, detail);
    }

    /// `championId` con valor positivo no se toca, y un hipotético
    /// `i64::MIN` no paniquea el parseo (quirk del `abs` nativo).
    #[test]
    fn normalizacion_de_champion_id_en_extremos() {
        let participant: Participant = serde_json::from_value(json!({
            "championId": 22
        }))
        .expect("positivo parsea");
        assert_eq!(participant.champion_id, 22);

        let participant: Participant = serde_json::from_value(json!({
            "championId": -412
        }))
        .expect("negativo parsea");
        assert_eq!(participant.champion_id, 412);

        let participant: Participant = serde_json::from_value(json!({
            "championId": i64::MIN
        }))
        .expect("i64::MIN no paniquea");
        assert_eq!(participant.champion_id, i64::MIN); // wrapping_abs lo deja
    }
}
