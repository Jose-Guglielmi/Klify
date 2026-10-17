//! Cliente de la Riot API oficial (Match-V5) para construir el dataset propio
//! de estadísticas de runas (FASE 8a).
//!
//! A diferencia del LCU (champ select en vivo), esto consulta partidas ya
//! terminadas contra los servidores de Riot con una API key personal:
//!
//! - [`key`]: la key **nunca** se hardcodea ni se commitea. Se lee una única
//!   vez de la variable de entorno `RIOT_API_KEY` (exportarla en la shell o
//!   pasarla al proceso; sin crates tipo `dotenvy` por restricción del
//!   proyecto). Sin key, `RiotApi::from_env` devuelve
//!   `RiotApiError::ApiKeyMissing` y el resto de la app sigue funcionando con
//!   normalidad: la recolección queda simplemente deshabilitada.
//! - [`routing`]: los endpoints Match-V5 usan **routing regional**
//!   (`https://{routing}.api.riotgames.com`), que se deduce del platform id
//!   del invocador con [`routing_for_platform`]. Fuente de la tabla: doc
//!   oficial "Routing Values" de <https://developer.riotgames.com/docs/lol>
//!   (consultada 2026-10): `americas`, `europe`, `asia` y **`sea`** — el
//!   valor `apac` de otros enunciados ya no figura en la doc vigente de LoL.
//! - [`models`]: el modelo serde de `/lol/match/v5/matches/...`, tolerante a
//!   payloads raros (modos sin perks ni posiciones, bans ausentes, campos
//!   extra ignorados) y con el quirk conocido de `championId` negativo
//!   (skins) normalizado con `abs()` documentado. Expone helpers puros para
//!   la fase de scoring: `primary_style`, `sub_style`, `keystone_perk` y
//!   `selection_perk_ids`.
//! - [`client`]: `RiotApi` con `reqwest` propio (header `X-Riot-Token`),
//!   rate limiting defensivo —llamadas secuenciales con mínimo de 150 ms y
//!   ante 429 respeta `Retry-After` (default 2 s, tope 60 s) hasta 3
//!   reintentos— y errores tipados en [`RiotApiError`].
//!
//! Fuera de alcance acá (FASE 8b): persistencia del dataset (JSONL),
//! recorrido del historial y agregación/scoring de runas. Este módulo solo
//! expone las piezas de acceso y parseo.

pub mod client;
pub mod key;
pub mod models;
pub mod routing;

pub use client::{RiotApi, RiotApiError};
pub use key::{api_key_from_env, API_KEY_ENV};
pub use models::{
    MatchDetail, MatchInfo, MatchMetadata, Participant, PerkSelection, PerkStyle, Perks, StatPerks,
    TeamBan, TeamInfo,
};
pub use routing::{normalize_platform, regional_base_url, routing_for_platform};
