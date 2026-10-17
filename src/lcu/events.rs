//! Eventos y payloads del champ select de la API LCU.
//!
//! El cliente emite, por WebSocket, eventos con URI
//! [`CHAMP_SELECT_SESSION_URI`] con el formato
//! `[<request_type>, "OnJsonApiEvent_lol-champ-select_v1_session", { data, eventType, uri }]`.
//! `irelia` parsea la tupla y nos entrega `data` como `serde_json::Value`:
//! - `data: null` → la sesión de champ select se cerró (evento `Delete`).
//! - `data: { ... }` → sesión activa, con `localMemberCellId`, `myTeam`,
//!   `enemyTeam` y el cronómetro `timer` (su campo `phase` es la única fase
//!   explícita que trae el payload).
//!
//! El modelo acepta las dos generaciones de nombres de campo que se ven en
//! payloads reales (`localMemberCellId`/`localPlayerCellId`,
//! `enemyTeam`/`theirTeam`) vía aliases serde: el nombre primario es el de
//! esta implementación y el alias cubre la variante histórica.
//!
//! Este módulo contiene el modelo serde del payload (subconjunto extensible:
//! los campos que no declaramos se ignoran) y funciones puras de parsing,
//! cubiertas por tests unitarios.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::lcu::models::{deserialize_optional_position, ChampionInfo, Position};

/// Endpoint REST/WebSocket de la sesión de champ select.
pub const CHAMP_SELECT_SESSION_URI: &str = "/lol-champ-select/v1/session";

/// Fase del champ select según `timer.phase`. Valores reales observados:
/// `""` (transiciones), `PLANNING`, `BAN_PICK`, `FINALIZATION` y
/// `GAME_STARTING`; cualquier otro se mapea a `None` al parsear.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ChampSelectPhase {
    /// Pre-draft: asignación/declaración de posiciones.
    Planning,
    /// Bans y picks.
    BanPick,
    /// Fase final: intercambios y lock-in definitivo.
    Finalization,
    /// El draft terminó y el juego está por arrancar.
    GameStarting,
}

impl ChampSelectPhase {
    /// Interpreta el código del payload. Desconocido o vacío → `None`.
    #[must_use]
    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "PLANNING" => Some(Self::Planning),
            "BAN_PICK" => Some(Self::BanPick),
            "FINALIZATION" => Some(Self::Finalization),
            "GAME_STARTING" => Some(Self::GameStarting),
            _ => None,
        }
    }
}

/// Deserializa la fase del timer tolerante a valores desconocidos (`None`,
/// nunca error).
fn deserialize_optional_phase<'de, D>(deserializer: D) -> Result<Option<ChampSelectPhase>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = Option::<String>::deserialize(deserializer)?;
    Ok(raw.as_deref().and_then(ChampSelectPhase::from_code))
}

/// Subconjunto del cronómetro de la sesión: solo nos interesa su fase
/// (`adjustedTimeLeftInPhase`, `internalNowInEpochMs`, etc. se ignoran).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChampSelectTimer {
    /// Fase vigente; `null` o código desconocido → `None`.
    #[serde(default, deserialize_with = "deserialize_optional_phase")]
    pub phase: Option<ChampSelectPhase>,
}

/// Sesión de champ select del LCU.
///
/// Subconjunto del payload real (`allowSkinSelection`, `bans`, `actions`,
/// `hasSimultaneousBans`, etc. se ignoran al deserializar), pensado para
/// extenderse en el futuro sin romper compatibilidad.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChampSelectSession {
    /// `cellId` del jugador local. `Option` porque un payload malformado no
    /// debe confundirse con el miembro de la celda 0. Alias
    /// `localPlayerCellId`: el nombre que usan payloads reales antiguos.
    #[serde(alias = "localPlayerCellId")]
    pub local_member_cell_id: Option<i64>,
    /// Equipo aliado completo.
    #[serde(default)]
    pub my_team: Vec<ChampSelectMember>,
    /// Equipo enemigo. Mismos miembros que `myTeam`; `championId` llega en 0
    /// hasta que cada enemigo confirma su pick. Alias `theirTeam`: el nombre
    /// que usan payloads reales antiguos.
    #[serde(default, alias = "theirTeam")]
    pub enemy_team: Vec<ChampSelectMember>,
    /// Cronómetro del draft; su campo `phase` es la única fase explícita que
    /// trae el payload (ver [`ChampSelectSession::phase`]). `Option` +
    /// `default` porque hay colas/transiciones que lo omiten o lo envían
    /// vacío.
    #[serde(default)]
    pub timer: Option<ChampSelectTimer>,
}

impl ChampSelectSession {
    /// Fase del champ select reportada por el LCU (`timer.phase`).
    ///
    /// `None` si el payload no trae `timer` o trae una fase desconocida o
    /// vacía (`""`).
    ///
    /// NOTA (recomendador): la fase macro de la partida (lobby → champ select
    /// → juego) **no** viene en este payload — no hay un campo de nivel
    /// superior confiable (`localPlayerPhaseId` no aparece en payloads
    /// reales). Esa transición se infiere de la presencia/cierre de la sesión
    /// ([`ChampSelectUpdate::Active`]/[`ChampSelectUpdate::Closed`]), que es
    /// lo que el orquestador ya usa para `AppState`.
    #[must_use]
    pub fn phase(&self) -> Option<ChampSelectPhase> {
        self.timer.and_then(|timer| timer.phase)
    }
}

/// Miembro de un equipo (aliado o enemigo) en champ select.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChampSelectMember {
    /// `cellId`: posición del miembro en la grilla de picks.
    #[serde(default)]
    pub cell_id: i64,
    /// Campeón confirmado o en pantalla de pick (0 = ninguno).
    #[serde(default)]
    pub champion_id: i64,
    /// Campeón marcado como "intent" en la fase de ban/pick (0 = ninguno).
    #[serde(default)]
    pub champion_pick_intent: i64,
    /// Posición asignada por el matchmaking (`"TOP"`, `"JUNGLE"`, ...). Solo
    /// algunas colas la traen; valor vacío, `null` o desconocido → `None`.
    #[serde(default, deserialize_with = "deserialize_optional_position")]
    pub assigned_position: Option<Position>,
}

/// Resultado de interpretar un payload crudo de sesión.
#[derive(Clone, Debug, PartialEq)]
pub enum SessionPayload {
    /// Sesión activa, ya parseada (lista para `MatchContext::from_session`).
    Active(ChampSelectSession),
    /// La sesión se cerró (`data: null`, evento `Delete`).
    Closed,
}

/// Interpreta el campo `data` de un evento de champ select (o el cuerpo del
/// GET inicial). `None` = payload irreconocible (se ignora sin tocar estado).
///
/// Generalización de [`parse_champ_select_payload`]: entrega la sesión
/// completa además del update de alto nivel, para que el orquestador construya
/// el `MatchContext` sin re-parsear el JSON.
#[must_use]
pub fn parse_session_payload(data: Value) -> Option<SessionPayload> {
    if data.is_null() {
        return Some(SessionPayload::Closed);
    }

    serde_json::from_value::<ChampSelectSession>(data)
        .ok()
        .map(SessionPayload::Active)
}

/// Proyección del payload interpretado al update de alto nivel que consume el
/// orquestador para `AppState`.
impl From<&SessionPayload> for ChampSelectUpdate {
    fn from(payload: &SessionPayload) -> Self {
        match payload {
            SessionPayload::Active(session) => {
                Self::Active(local_champion_id(session).map(champion_placeholder))
            }
            SessionPayload::Closed => Self::Closed,
        }
    }
}

/// Resultado de interpretar un payload de champ select.
#[derive(Clone, Debug, PartialEq)]
pub enum ChampSelectUpdate {
    /// Hay sesión activa; `Some` = el jugador local ya tiene campeón
    /// (confirmado o con intención), `None` = aún no eligió.
    Active(Option<ChampionInfo>),
    /// La sesión se cerró (evento `Delete` / `data: null` / 404 del GET).
    Closed,
}

/// Devuelve el id del campeón del jugador local: `championId` si es distinto
/// de 0, si no `championPickIntent`; `None` si aún no marcó ninguno o si no se
/// encontró al miembro local en `myTeam`.
pub fn local_champion_id(session: &ChampSelectSession) -> Option<i64> {
    let local_cell = session.local_member_cell_id?;

    session
        .my_team
        .iter()
        .find(|member| member.cell_id == local_cell)
        .and_then(|member| {
            let champion = if member.champion_id != 0 {
                member.champion_id
            } else {
                member.champion_pick_intent
            };
            (champion != 0).then_some(champion)
        })
}

/// Fallback de `ChampionInfo` cuando el catálogo de Data Dragon aún no cargó
/// o el id no figura en él: nombre genérico y sin imagen.
#[must_use]
pub fn champion_placeholder(id: i64) -> ChampionInfo {
    ChampionInfo {
        id,
        name: format!("Campeón {id}"),
        image_url: String::new(),
    }
}

/// Igual que [`parse_session_payload`], proyectado al [`ChampSelectUpdate`]
/// que consume el orquestador para `AppState`.
#[must_use]
pub fn parse_champ_select_payload(data: Value) -> Option<ChampSelectUpdate> {
    let payload = parse_session_payload(data)?;
    Some(ChampSelectUpdate::from(&payload))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lcu::models::Position;
    use serde_json::json;

    /// Payload realista (con campos extra que el LCU incluye y que deben
    /// ignorarse) con el jugador local ya confirmado (ChampionInfo 103 = Ahri).
    #[test]
    fn champ_elegido() {
        let payload = json!({
            "boostSkinId": 0,
            "championTradingEnabled": true,
            "hasSimultaneousBans": true,
            "hasSimultaneousPicks": true,
            "isCustomGame": false,
            "isSpectating": false,
            "localMemberCellId": 2,
            "myTeam": [
                {
                    "cellId": 0,
                    "championId": 0,
                    "championPickIntent": 0,
                    "spell1Id": 4,
                    "summonerId": 3948201592_i64,
                    "assignedPosition": "TOP"
                },
                {
                    "cellId": 1,
                    "championId": 25,
                    "championPickIntent": 0,
                    "spell1Id": 7,
                    "summonerId": 1_234_567_890,
                    "assignedPosition": "MIDDLE"
                },
                {
                    "cellId": 2,
                    "championId": 103,
                    "championPickIntent": 0,
                    "spell1Id": 12,
                    "summonerId": 987_654_321,
                    "assignedPosition": "BOTTOM"
                }
            ],
            "timer": {
                "adjustTimeStartedFrom": 30,
                "phase": "BAN_PICK",
                "timeLeftInPhase": 18_214
            }
        });

        let update = parse_champ_select_payload(payload).expect("payload válido");

        assert_eq!(
            update,
            ChampSelectUpdate::Active(Some(champion_placeholder(103)))
        );
    }

    /// `championId` 0 con `championPickIntent` > 0: el jugador aún no
    /// confirmó pero marcó intención (hover del pick).
    #[test]
    fn solo_intent_sin_champ() {
        let payload = json!({
            "localMemberCellId": 1,
            "myTeam": [
                { "cellId": 0, "championId": 22, "championPickIntent": 0 },
                { "cellId": 1, "championId": 0, "championPickIntent": 412 }
            ]
        });

        let update = parse_champ_select_payload(payload).expect("payload válido");

        assert_eq!(
            update,
            ChampSelectUpdate::Active(Some(champion_placeholder(412)))
        );
    }

    /// Ni `championId` ni `championPickIntent`: aún no eligió.
    #[test]
    fn nada_elegido() {
        let payload = json!({
            "localMemberCellId": 0,
            "myTeam": [
                { "cellId": 0, "championId": 0, "championPickIntent": 0 }
            ]
        });

        let update = parse_champ_select_payload(payload).expect("payload válido");

        assert_eq!(update, ChampSelectUpdate::Active(None));
    }

    /// Evento `Delete`: `data: null` significa que la sesión se cerró.
    #[test]
    fn data_null_sesion_cerrada() {
        let update = parse_champ_select_payload(Value::Null).expect("data null es válido");

        assert_eq!(update, ChampSelectUpdate::Closed);
    }

    /// Varios compañeros de equipo con el jugador local en el medio: debe
    /// elegir el campeón de la celda que coincide con `localMemberCellId`.
    #[test]
    fn elige_miembro_correcto_por_cell_id() {
        let payload = json!({
            "localMemberCellId": 5,
            "myTeam": [
                { "cellId": 0, "championId": 0, "championPickIntent": 0 },
                { "cellId": 1, "championId": 22, "championPickIntent": 0 },
                { "cellId": 3, "championId": 0, "championPickIntent": 57 },
                { "cellId": 5, "championId": 412, "championPickIntent": 0 },
                { "cellId": 7, "championId": 498, "championPickIntent": 0 }
            ]
        });

        let update = parse_champ_select_payload(payload).expect("payload válido");

        assert_eq!(
            update,
            ChampSelectUpdate::Active(Some(champion_placeholder(412)))
        );
    }

    /// Sin `localMemberCellId` no se puede identificar al jugador local:
    /// se trata como sesión activa sin campeón, nunca como otro miembro.
    #[test]
    fn sin_local_member_cell_id() {
        let payload = json!({
            "myTeam": [
                { "cellId": 0, "championId": 103, "championPickIntent": 0 }
            ]
        });

        let update = parse_champ_select_payload(payload).expect("payload válido");

        assert_eq!(update, ChampSelectUpdate::Active(None));
    }

    /// Un payload que no es ni `null` ni una sesión (p. ej. un string) se
    /// reporta como irreconocible.
    #[test]
    fn payload_irreconocible() {
        assert_eq!(parse_champ_select_payload(json!("nope")), None);
    }

    /// Cuerpo de un GET inicial con sesión activa también se parsea igual que
    /// un evento del websocket.
    #[test]
    fn get_inicial_parsea_igual_que_evento() {
        let session = ChampSelectSession {
            local_member_cell_id: Some(2),
            my_team: vec![ChampSelectMember {
                cell_id: 2,
                champion_id: 103,
                champion_pick_intent: 0,
                assigned_position: None,
            }],
            enemy_team: Vec::new(),
            timer: None,
        };

        assert_eq!(local_champion_id(&session), Some(103));
    }

    /// FASE 6: sesión completa con `enemyTeam`, posiciones asignadas y
    /// `timer.phase`. Los campos extra siguen ignorándose y la proyección al
    /// update de alto nivel no cambia respecto a fases previas.
    #[test]
    fn sesion_con_enemy_team_posiciones_y_fase() {
        let payload = json!({
            "localMemberCellId": 2,
            "myTeam": [
                { "cellId": 1, "championId": 22, "championPickIntent": 0, "assignedPosition": "JUNGLE" },
                { "cellId": 2, "championId": 103, "championPickIntent": 0, "assignedPosition": "MIDDLE" }
            ],
            "enemyTeam": [
                { "cellId": 5, "championId": 86, "championPickIntent": 0, "assignedPosition": "TOP" },
                { "cellId": 6, "championId": 0, "championPickIntent": 0, "assignedPosition": "" }
            ],
            "timer": { "phase": "BAN_PICK", "timeLeftInPhase": 12_345 },
            "boostSkinId": 0
        });

        let parsed = parse_session_payload(payload).expect("payload válido");
        let SessionPayload::Active(session) = parsed else {
            panic!("sesión activa esperada");
        };

        assert_eq!(session.my_team.len(), 2);
        assert_eq!(session.enemy_team.len(), 2);
        assert_eq!(session.enemy_team[0].cell_id, 5);
        assert_eq!(session.enemy_team[0].champion_id, 86);
        assert_eq!(session.enemy_team[0].assigned_position, Some(Position::Top));
        // Código vacío → `None`, sin invalidar el payload.
        assert_eq!(session.enemy_team[1].assigned_position, None);
        assert_eq!(session.phase(), Some(ChampSelectPhase::BanPick));

        assert_eq!(
            ChampSelectUpdate::from(&SessionPayload::Active(session.clone())),
            ChampSelectUpdate::Active(Some(champion_placeholder(103)))
        );
    }

    /// FASE 6: alias del payload real antiguo — `localPlayerCellId` y
    /// `theirTeam` se aceptan como nombres de campo.
    #[test]
    fn alias_de_payloads_antiguos() {
        let payload = json!({
            "localPlayerCellId": 1,
            "myTeam": [
                { "cellId": 1, "championId": 103, "championPickIntent": 0 }
            ],
            "theirTeam": [
                { "cellId": 6, "championId": 25, "championPickIntent": 0 }
            ]
        });

        let parsed = parse_session_payload(payload).expect("payload válido");
        let SessionPayload::Active(session) = parsed else {
            panic!("sesión activa esperada");
        };

        assert_eq!(session.local_member_cell_id, Some(1));
        assert_eq!(session.enemy_team.len(), 1);
        assert_eq!(session.enemy_team[0].champion_id, 25);
        assert_eq!(local_champion_id(&session), Some(103));
    }

    /// FASE 6: fases del timer — conocidas se interpretan; desconocidas,
    /// vacías o sin `timer` → `None`, nunca error de parsing.
    #[test]
    fn fase_del_timer_conocida_desconocida_y_ausente() {
        let fase_de = |phase: Value| {
            let payload = json!({
                "localMemberCellId": 0,
                "myTeam": [],
                "timer": { "phase": phase }
            });
            let parsed = parse_session_payload(payload).expect("payload válido");
            let SessionPayload::Active(session) = parsed else {
                panic!("sesión activa esperada");
            };
            session.phase()
        };

        assert_eq!(fase_de(json!("PLANNING")), Some(ChampSelectPhase::Planning));
        assert_eq!(
            fase_de(json!("FINALIZATION")),
            Some(ChampSelectPhase::Finalization)
        );
        assert_eq!(
            fase_de(json!("GAME_STARTING")),
            Some(ChampSelectPhase::GameStarting)
        );
        // Desconocida o vacía → None.
        assert_eq!(fase_de(json!("RANKED_LOBBY_REMAP")), None);
        assert_eq!(fase_de(json!("")), None);

        // Sin `timer` → None (y el resto de la sesión se parsea igual).
        let payload = json!({ "localMemberCellId": 0, "myTeam": [] });
        let parsed = parse_session_payload(payload).expect("payload válido");
        let SessionPayload::Active(session) = parsed else {
            panic!("sesión activa esperada");
        };
        assert_eq!(session.phase(), None);
    }

    /// FASE 6: posiciones desconocidas o `null` del payload se mapean a
    /// `None` sin invalidar al miembro ni la sesión.
    #[test]
    fn posicion_desconocida_o_null_es_none() {
        let payload = json!({
            "localMemberCellId": 0,
            "myTeam": [
                { "cellId": 0, "championId": 0, "championPickIntent": 0, "assignedPosition": "NONE" },
                { "cellId": 1, "championId": 0, "championPickIntent": 0, "assignedPosition": "top" },
                { "cellId": 2, "championId": 0, "championPickIntent": 0, "assignedPosition": null },
                { "cellId": 3, "championId": 0, "championPickIntent": 0, "assignedPosition": "UTILITY" }
            ]
        });

        let parsed = parse_session_payload(payload).expect("payload válido");
        let SessionPayload::Active(session) = parsed else {
            panic!("sesión activa esperada");
        };

        let positions: Vec<Option<Position>> = session
            .my_team
            .iter()
            .map(|m| m.assigned_position)
            .collect();
        assert_eq!(positions, vec![None, None, None, Some(Position::Utility)]);
    }
}
