//! Tipos de dominio compartidos por la app.

use serde::{Deserialize, Serialize};

use crate::lcu::events::{ChampSelectMember, ChampSelectSession, local_champion_id};

/// Información mínima de un campeón para mostrar en la UI.
///
/// Se construye resolviendo el championId contra el catálogo de Data Dragon
/// ([`crate::data_dragon::models::ChampionCatalog`]); si el catálogo aún no
/// cargó o el id no figura, cae al placeholder de
/// [`crate::lcu::events::champion_placeholder`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChampionInfo {
    /// Numeric id del campeón (Data Dragon / LCU).
    pub id: i64,
    /// Nombre a mostrar (ej. "Ahri").
    pub name: String,
    /// URL de la imagen del campeón (Data Dragon).
    pub image_url: String,
}

/// Estado global de la aplicación respecto al cliente de LoL.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum AppState {
    /// Cliente de LoL no detectado.
    Disconnected,
    /// Cliente detectado, aún no hay champ select.
    WaitingForChampSelect,
    /// En champ select; `None` = todavía no eligió.
    InChampSelect(Option<ChampionInfo>),
}

/// Deserializa `Option<String>` tolerante a payloads del LCU que traen el
/// valor como número JSON (verificado en vivo: `summonerId` llega como
/// `23727787`, sin comillas). Número → su texto decimal; string → tal cual;
/// `null`/ausente/otros → `None`, nunca error de parsing.
fn deserialize_string_or_number<'de, D>(
    deserializer: D,
) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(match raw {
        Some(serde_json::Value::String(text)) => Some(text),
        Some(serde_json::Value::Number(number)) => Some(number.to_string()),
        _ => None,
    })
}

/// Información mínima del invocador actual
/// (`GET /lol-summoner/v1/current-summoner`).
///
/// FASE 8a: expone el `puuid` del jugador local, raíz del historial que la
/// Riot API Match-V5 ([`crate::riot_api`]) va a recorrer para construir el
/// dataset de runas. Modelo tolerante: un payload sin Riot ID (cuentas
/// legacy) o con campos extra (`summonerId`, `profileIconId`, etc.) se
/// parsea igual; los campos faltantes o `null` quedan en `None`.
///
/// FASE 11a-2 (perfil): se suman `summoner_id` (id numérico del LCU, se
/// normaliza a texto), `profile_icon_id` y `summoner_level` para la UI del
/// perfil; todos tolerantes y con default.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SummonerInfo {
    /// PUUID de la cuenta de Riot del invocador (`None` si el payload no lo
    /// trae).
    #[serde(default)]
    pub puuid: Option<String>,
    /// Parte del Riot ID antes del `#` (puede faltar en cuentas legacy).
    #[serde(default)]
    pub game_name: Option<String>,
    /// Parte del Riot ID después del `#` (ej. `"LAS"`).
    #[serde(default)]
    pub tag_line: Option<String>,
    /// Id numérico del invocador dentro del servidor (el payload lo trae
    /// como número; se normaliza a texto). Lo usa la FASE 11a-2 para
    /// endpoints alternativos de maestría por summoner id.
    #[serde(default, deserialize_with = "deserialize_string_or_number")]
    pub summoner_id: Option<String>,
    /// Id del icono de perfil actual (URL de icono en Data Dragon).
    #[serde(default)]
    pub profile_icon_id: Option<i64>,
    /// Nivel de invocador (puede faltar mientras carga la sesión).
    #[serde(default)]
    pub summoner_level: Option<i64>,
}

/// Posición asignada de un miembro en el champ select (códigos LCU `"TOP"`,
/// `"JUNGLE"`, `"MIDDLE"`, `"BOTTOM"`, `"UTILITY"`).
///
/// Vive acá (tipos de dominio) y no en `events.rs` (payload) porque
/// [`MatchContext`] —que sobrevive al payload crudo— la expone: los códigos
/// string quedan como detalle del LCU tras deserializar.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Position {
    Top,
    Jungle,
    Middle,
    Bottom,
    Utility,
}

impl Position {
    /// Interpreta el código del payload. Cualquier otro valor —colas sin
    /// posiciones asignadas usan `""` o `"NONE"`, y hay códigos como
    /// `"FILL"`/`"UNSELECTED"`— se mapea a `None` en lugar de invalidar el
    /// payload completo.
    #[must_use]
    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "TOP" => Some(Self::Top),
            "JUNGLE" => Some(Self::Jungle),
            "MIDDLE" => Some(Self::Middle),
            "BOTTOM" => Some(Self::Bottom),
            "UTILITY" => Some(Self::Utility),
            _ => None,
        }
    }
}

/// Deserializa `Option<Position>` tolerante a valores desconocidos: código
/// raro, vacío o `null` → `None`, nunca error de parsing.
pub(crate) fn deserialize_optional_position<'de, D>(
    deserializer: D,
) -> Result<Option<Position>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = Option::<String>::deserialize(deserializer)?;
    Ok(raw.as_deref().and_then(Position::from_code))
}

/// Miembro de equipo en el snapshot de partida: lo mínimo que el futuro
/// recomendador necesita de cada jugador (RF-01..RF-04).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamMember {
    /// `cellId` del miembro: identifica la casilla del draft.
    pub cell_id: i64,
    /// Campeón confirmado (`championId != 0`). Los hovers/intents van como
    /// `None`: el algoritmo futuro no debe asumir equipos completos.
    pub champion_id: Option<i64>,
    /// Posición asignada por el matchmaking, si la cola la reporta.
    pub position: Option<Position>,
    /// Id numérico del invocador en el LCU (`summonerId`), si el payload lo
    /// trajo: es la clave que usa el panel de jugadores para pedir datos por
    /// jugador (runas, historial) sin mirar la sesión cruda.
    pub summoner_id: Option<i64>,
    /// Nombre de invocador (`summonerName`) para mostrar en la UI; `None` si
    /// el payload no lo trajo.
    pub summoner_name: Option<String>,
}

impl TeamMember {
    /// Convierte un miembro crudo del LCU. Solo los picks confirmados se
    /// publican; los intents son volátiles y no se exponen por miembro (el
    /// pick del jugador local sí conserva el fallback a intent, ver
    /// [`MatchContext::my_champion_id`]).
    #[must_use]
    pub fn from_member(member: &ChampSelectMember) -> Self {
        Self {
            cell_id: member.cell_id,
            champion_id: (member.champion_id != 0).then_some(member.champion_id),
            position: member.assigned_position,
            summoner_id: member.summoner_id,
            summoner_name: member.summoner_name.clone(),
        }
    }
}

/// Snapshot de la partida en curso, base del futuro recomendador de runas
/// (objetivo documental: RF-01..RF-04 del spec del recomendador).
///
/// Se publica como `Signal<MatchContext>` desde
/// [`crate::lcu::client::run_lcu_loop`] y se resetea a [`MatchContext::default`]
/// cuando el champ select se cierra o el cliente de LoL se desconecta.
///
/// Deriva `Serialize`/`Deserialize` para poder persistir sesiones a futuro.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchContext {
    /// `cellId` del jugador local (`None` si el payload no lo trajo).
    pub my_cell_id: Option<i64>,
    /// Campeón del jugador local: `championId` confirmado, si no el intent
    /// (hover), si no `None` — misma resolución que ya usa la UI.
    pub my_champion_id: Option<i64>,
    /// Posición asignada al jugador local, si la cola la trae.
    pub my_position: Option<Position>,
    /// Compañeros de equipo, excluido el jugador local.
    pub allies: Vec<TeamMember>,
    /// Equipo enemigo. `champion_id` pasa a `Some` recién cuando cada enemigo
    /// confirma su pick (el LCU reporta `championId: 0` hasta el lock-in).
    pub enemies: Vec<TeamMember>,
}

impl MatchContext {
    /// Construye el snapshot desde la sesión parseada del champ select.
    ///
    /// Función pura (sin IO ni señales), cubierta por tests unitarios. Los
    /// miembros sin pick confirmado conservan su casilla y posición con
    /// `champion_id: None`.
    #[must_use]
    pub fn from_session(session: &ChampSelectSession) -> Self {
        let local_cell = session.local_member_cell_id;
        let local_member =
            local_cell.and_then(|cell| session.my_team.iter().find(|m| m.cell_id == cell));

        Self {
            my_cell_id: local_cell,
            my_champion_id: local_champion_id(session),
            my_position: local_member.and_then(|member| member.assigned_position),
            allies: session
                .my_team
                .iter()
                // Si no sabemos quién es el local (`local_cell: None`) no
                // excluye a nadie: todos los miembros son aliados.
                .filter(|member| Some(member.cell_id) != local_cell)
                .map(TeamMember::from_member)
                .collect(),
            enemies: session
                .enemy_team
                .iter()
                .map(TeamMember::from_member)
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Miembro de prueba con los campos que nos interesan.
    fn member(cell: i64, champ: i64, intent: i64, position: Option<Position>) -> ChampSelectMember {
        ChampSelectMember {
            cell_id: cell,
            champion_id: champ,
            champion_pick_intent: intent,
            assigned_position: position,
            summoner_id: None,
            summoner_name: None,
        }
    }

    /// Sesión de prueba: vos en la celda 2 (MIDDLE, Ahri confirmada), dos
    /// aliados y tres enemigos (uno lockeado, uno sin pick, uno con intent).
    fn sesion_de_ejemplo() -> ChampSelectSession {
        ChampSelectSession {
            local_member_cell_id: Some(2),
            my_team: vec![
                member(0, 412, 0, Some(Position::Bottom)),
                member(1, 0, 57, Some(Position::Top)), // solo intent, sin lock
                member(2, 103, 0, Some(Position::Middle)), // vos
            ],
            enemy_team: vec![
                member(5, 86, 0, Some(Position::Top)),   // lockeado
                member(6, 0, 0, Some(Position::Jungle)), // sin pick aún
                member(7, 0, 412, None),                 // solo intent
            ],
            timer: None,
        }
    }

    /// RF-01..RF-04: el snapshot excluye al local de `allies`, mantiene
    /// enemigos parciales (sin pick → `None`) y expone tu posición asignada.
    #[test]
    fn from_session_arma_el_contexto_con_ambos_equipos() {
        let context = MatchContext::from_session(&sesion_de_ejemplo());

        assert_eq!(context.my_cell_id, Some(2));
        assert_eq!(context.my_champion_id, Some(103));
        assert_eq!(context.my_position, Some(Position::Middle));

        // No apareces entre tus aliados.
        assert_eq!(context.allies.len(), 2);
        assert!(context.allies.iter().all(|ally| ally.cell_id != 2));
        assert_eq!(
            context.allies,
            vec![
                TeamMember {
                    cell_id: 0,
                    champion_id: Some(412),
                    position: Some(Position::Bottom),
                    summoner_id: None,
                    summoner_name: None,
                },
                TeamMember {
                    cell_id: 1,
                    champion_id: None,
                    position: Some(Position::Top),
                    summoner_id: None,
                    summoner_name: None,
                },
            ]
        );

        // Enemigos parciales: lockeado → `Some`; sin pick o con solo intent
        // → `None` (el intent del rival no se filtra como pick).
        assert_eq!(context.enemies.len(), 3);
        assert_eq!(context.enemies[0].champion_id, Some(86));
        assert_eq!(context.enemies[1].champion_id, None);
        assert_eq!(context.enemies[2].champion_id, None);
        assert_eq!(context.enemies[2].position, None);
    }

    /// El panel de jugadores (Porofessor-style) lee los equipos desde
    /// `MatchContext`: el id y el nombre de invocador de cada miembro tienen
    /// que sobrevivir la conversión tanto para aliados como para enemigos, y
    /// quedar en `None` donde el payload no los trajo.
    #[test]
    fn from_session_preserva_summoner_id_y_nombre_en_ambos_equipos() {
        let mut session = sesion_de_ejemplo();
        // Aliado (celda 0) y enemigo (celda 5) con identidad completa.
        session.my_team[0].summoner_id = Some(4242);
        session.my_team[0].summoner_name = Some("Klipi".to_string());
        session.enemy_team[0].summoner_id = Some(777);
        session.enemy_team[0].summoner_name = Some("MidKing".to_string());

        let context = MatchContext::from_session(&session);

        let aliado = context
            .allies
            .iter()
            .find(|ally| ally.cell_id == 0)
            .expect("aliado con celda 0 presente");
        assert_eq!(aliado.summoner_id, Some(4242));
        assert_eq!(aliado.summoner_name.as_deref(), Some("Klipi"));

        let enemigo = context
            .enemies
            .iter()
            .find(|enemy| enemy.cell_id == 5)
            .expect("enemigo con celda 5 presente");
        assert_eq!(enemigo.summoner_id, Some(777));
        assert_eq!(enemigo.summoner_name.as_deref(), Some("MidKing"));

        // Miembro sin identidad en el payload → `None`, nunca basura.
        let sin_identidad = context
            .allies
            .iter()
            .find(|ally| ally.cell_id == 1)
            .expect("aliado con celda 1 presente");
        assert_eq!(sin_identidad.summoner_id, None);
        assert_eq!(sin_identidad.summoner_name, None);
    }

    /// FASE 6: enemigo lockeado vs enemigo con solo intent. El pick real del
    /// rival llega recién con `championId != 0`.
    #[test]
    fn campeon_enemigo_lockeado_vs_solo_intent() {
        let session = sesion_de_ejemplo();

        assert_eq!(
            TeamMember::from_member(&session.enemy_team[0]).champion_id,
            Some(86)
        );
        assert_eq!(
            TeamMember::from_member(&session.enemy_team[2]).champion_id,
            None
        );
    }

    /// Sin `localMemberCellId` no hay a quién excluir: todos son aliados y no
    /// hay pick ni posición propia.
    #[test]
    fn from_session_sin_celda_local_no_excluye_a_nadie() {
        let mut session = sesion_de_ejemplo();
        session.local_member_cell_id = None;

        let context = MatchContext::from_session(&session);

        assert_eq!(context.my_cell_id, None);
        assert_eq!(context.my_champion_id, None);
        assert_eq!(context.my_position, None);
        assert_eq!(context.allies.len(), 3);
        assert_eq!(context.enemies.len(), 3);
    }

    /// La resolución del pick local mantiene el fallback a intent (hover).
    #[test]
    fn from_session_resuelve_el_pick_local_por_intent() {
        let mut session = sesion_de_ejemplo();
        session.my_team[2].champion_id = 0;
        session.my_team[2].champion_pick_intent = 103;

        let context = MatchContext::from_session(&session);

        assert_eq!(context.my_champion_id, Some(103));
    }

    /// El contexto serializa y deserializa idéntico (persistencia a futuro).
    #[test]
    fn match_context_hace_roundtrip_serde() {
        let context = MatchContext::from_session(&sesion_de_ejemplo());

        let json = serde_json::to_string(&context).expect("serializa");
        let back: MatchContext = serde_json::from_str(&json).expect("deserializa");

        assert_eq!(back, context);
    }

    /// Códigos LCU de posición: los cinco conocidos, y todo lo demás
    /// (`""`, `NONE`, `FILL`) → `None`. `serde` emite los mismos códigos.
    #[test]
    fn codigos_de_position() {
        assert_eq!(Position::from_code("TOP"), Some(Position::Top));
        assert_eq!(Position::from_code("JUNGLE"), Some(Position::Jungle));
        assert_eq!(Position::from_code("MIDDLE"), Some(Position::Middle));
        assert_eq!(Position::from_code("BOTTOM"), Some(Position::Bottom));
        assert_eq!(Position::from_code("UTILITY"), Some(Position::Utility));
        assert_eq!(Position::from_code(""), None);
        assert_eq!(Position::from_code("NONE"), None);
        assert_eq!(Position::from_code("FILL"), None);

        assert_eq!(
            serde_json::to_value(Position::Utility).expect("serializa"),
            json!("UTILITY")
        );
        assert_eq!(
            serde_json::to_value(Position::Jungle).expect("serializa"),
            json!("JUNGLE")
        );
    }

    /// FASE 8a: el payload real de `/lol-summoner/v1/current-summoner` trae
    /// muchos campos más (`summonerId`, `profileIconId`, ...): se parsea el
    /// subconjunto y el resto se ignora.
    ///
    /// FASE 11a-2: `summonerId`/`profileIconId`/`summonerLevel` ya no se
    /// ignoran; se modelan. El fixture de acá trae `summonerId` como string
    /// (caso tolerado); el caso numérico real se cubre en
    /// [`summoner_info_parsea_el_payload_numerico_real`].
    #[test]
    fn summoner_info_parsea_el_subconjunto_e_ignora_el_resto() {
        let info: SummonerInfo = serde_json::from_value(json!({
            "accountId": "abc",
            "puuid": "puuid-xyz",
            "gameName": "Klipi",
            "tagLine": "LAS",
            "summonerId": "xyz",
            "summonerLevel": 312,
            "profileIconId": 5555
        }))
        .expect("payload real parsea");

        assert_eq!(info.puuid.as_deref(), Some("puuid-xyz"));
        assert_eq!(info.game_name.as_deref(), Some("Klipi"));
        assert_eq!(info.tag_line.as_deref(), Some("LAS"));
        assert_eq!(info.summoner_id.as_deref(), Some("xyz"));
        assert_eq!(info.profile_icon_id, Some(5555));
        assert_eq!(info.summoner_level, Some(312));
    }

    /// FASE 11a-2: recorte REAL del cliente de LA2 (2026-10): `summonerId`
    /// llega como NÚMERO JSON y `summonerLevel`/`profileIconId` también; el
    /// Riot ID (`gameName`/`tagLine`) y el resto se parsean igual.
    #[test]
    fn summoner_info_parsea_el_payload_numerico_real() {
        let info: SummonerInfo = serde_json::from_value(json!({
            "accountId": 2440484679085024i64,
            "displayName": "",
            "gameName": "PKT MidKing",
            "nameChangeFlag": false,
            "percentCompleteForNextLevel": 29,
            "privacy": "PUBLIC",
            "profileIconId": 6601,
            "puuid": "f7e2b22b-d677-558c-95af-495e9807ada3",
            "summonerId": 23727787,
            "summonerLevel": 391,
            "tagLine": "Akali",
            "unnamed": false
        }))
        .expect("payload real parsea");

        assert_eq!(
            info.puuid.as_deref(),
            Some("f7e2b22b-d677-558c-95af-495e9807ada3")
        );
        assert_eq!(info.game_name.as_deref(), Some("PKT MidKing"));
        assert_eq!(info.tag_line.as_deref(), Some("Akali"));
        // Número JSON → texto decimal.
        assert_eq!(info.summoner_id.as_deref(), Some("23727787"));
        assert_eq!(info.profile_icon_id, Some(6601));
        assert_eq!(info.summoner_level, Some(391));
    }

    /// Tolerancia: objeto vacío, campos `null` o ausentes → `None` en todo,
    /// nunca error de parsing.
    #[test]
    fn summoner_info_tolerante_a_payloads_parciales() {
        let info: SummonerInfo = serde_json::from_value(json!({})).expect("vacio parsea");
        assert_eq!(info, SummonerInfo::default());

        let info: SummonerInfo = serde_json::from_value(json!({
            "puuid": null,
            "gameName": "Solo",
            "summonerId": null,
            "profileIconId": null
        }))
        .expect("nulls parsean");
        assert_eq!(info.puuid, None);
        assert_eq!(info.game_name.as_deref(), Some("Solo"));
        assert_eq!(info.tag_line, None);
        assert_eq!(info.summoner_id, None);
        assert_eq!(info.profile_icon_id, None);
        assert_eq!(info.summoner_level, None);
    }
}
