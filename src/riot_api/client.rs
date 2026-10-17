//! Cliente HTTP de la Riot API oficial (Match-V5 + Account-V1) con rate
//! limiting defensivo.
//!
//! `RiotApi` es un cliente **secuencial**: cada llamada respeta un mínimo de
//! [`MIN_CALL_INTERVAL`] desde la anterior, y ante un `429` respeta el
//! header `Retry-After` (con default y tope defensivos, ver
//! [`parse_retry_after`]) hasta [`MAX_RATE_LIMIT_RETRIES`] reintentos antes
//! de rendirse con `RiotApiError::RateLimited`. Con una dev key personal
//! (20 req/s, 100 req/2 min) esto mantiene la recolección del dataset (fase
//! 8b) lejos del techo sin necesidad de un rate limiter completo.
//!
//! Endpoints:
//!
//! - **Match-V5** (`/lol/match/v5/...`): historial y detalle de partidas.
//!   El `puuid` que reciben es el **puuid unificado de la cuenta Riot**
//!   (namespace Account-V1), NO el puuid de juego que reporta el LCU: ese
//!   pertenece a otro namespace y Match-V5 lo rechaza con
//!   `HTTP 400 "Exception decrypting {puuid}"` (bug de producción
//!   verificado 2026-10 con LCU real de LA2).
//! - **Account-V1** (`/riot/account/v1/...`): `account_puuid_by_riot_id`
//!   resuelve ese puuid unificado a partir del Riot ID (`gameName#tagLine`)
//!   que el LCU sí reporta en `/lol-summoner/v1/current-summoner`. Usa el
//!   mismo routing regional que Match-V5 (ver [`crate::riot_api::routing`]).
//!   `gameName`/`tagLine` pueden traer espacios o acentos, así que el path
//!   pasa por [`percent_encode_path_segment`] (encoding manual RFC 3986,
//!   sin dependencias nuevas).
//!
//! La key viaja en el header `X-Riot-Token` (nunca en la URL ni en logs: los
//! errores de `reqwest` solo incluyen el host y el path). Sin key el cliente
//! no se construye (`RiotApiError::ApiKeyMissing`), así que el resto de la
//! app nunca se ve afectada por la falta de key.

use std::fmt;
use std::time::{Duration, Instant};

use serde::de::DeserializeOwned;
use serde::Deserialize;
use tokio::time::sleep;

use crate::riot_api::key::{api_key_from_env, normalize_key};
use crate::riot_api::models::MatchDetail;
use crate::riot_api::routing::{regional_base_url, routing_for_platform};

/// Límite para establecer conexión con los servidores de Riot.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Límite total por request (el detalle de partida es un JSON de cientos de
/// KB; 20 s es holgado incluso en enlaces lentos).
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
/// Mínimo entre llamadas (~6.6 req/s): muy por debajo del techo de una dev
/// key (20 req/s) y suficiente para recorrer un historial de 100 partidas en
/// ~15 s.
const MIN_CALL_INTERVAL: Duration = Duration::from_millis(150);
/// Espera por defecto ante un `429` sin header `Retry-After` legible.
const DEFAULT_RETRY_AFTER: Duration = Duration::from_secs(2);
/// Tope defensivo de la espera ante un `429`: la doc puede pedir minutos
/// (service limit) pero la app nunca debe quedarse bloqueada tanto tiempo;
/// con 60 s y hasta 3 reintentos, lo correcto es rendirse y reintentar la
/// recolección más tarde (fase 8b).
const MAX_RETRY_AFTER_WAIT: Duration = Duration::from_secs(60);
/// Reintentos máximos ante `429` por llamada (después del primer intento).
const MAX_RATE_LIMIT_RETRIES: u32 = 3;
/// Header de autenticación de la Riot API (la key nunca va en la URL).
const X_RIOT_TOKEN_HEADER: &str = "X-Riot-Token";
/// Header de espera ante `429`.
const RETRY_AFTER_HEADER: &str = "Retry-After";

/// Cola `ranked solo/duo` de League of Legends: el dataset de runas se
/// construye solo con partidas de esta cola (spec de la fase 8b).
pub const RANKED_SOLO_QUEUE: i64 = 420;
/// Valor del filtro `type` de Match-V5 para partidas clasificatorias.
pub const RANKED_MATCH_TYPE: &str = "ranked";

/// Errores del cliente de la Riot API. Todos se loguean o propagan: ninguno
/// paniquea, y la app funciona sin Riot API si algo falla.
#[derive(Debug)]
pub enum RiotApiError {
    /// No hay key: `RIOT_API_KEY` ausente o vacía. La recolección del
    /// dataset queda deshabilitada; el resto de la app sigue normal.
    ApiKeyMissing,
    /// `429` persistente: se agotaron los [`MAX_RATE_LIMIT_RETRIES`]
    /// reintentos respetando `Retry-After`.
    RateLimited,
    /// Respuesta HTTP con código de error (ej. `403` key inválida/expirada,
    /// `404` partida inexistente).
    Http(u16),
    /// Error de red/transporte (conexión, TLS, timeout).
    Network(reqwest::Error),
    /// La respuesta no se pudo decodificar como el tipo esperado.
    Decode(reqwest::Error),
    /// El platform id no existe en la tabla de routing (ver
    /// `routing_for_platform`): se detecta al construir el cliente.
    InvalidPlatform(String),
}

impl fmt::Display for RiotApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ApiKeyMissing => write!(
                f,
                "la variable de entorno RIOT_API_KEY no está definida o está vacía: la Riot API queda deshabilitada"
            ),
            Self::RateLimited => write!(
                f,
                "rate limit persistente (429) tras {MAX_RATE_LIMIT_RETRIES} reintentos"
            ),
            Self::Http(status) => write!(f, "error HTTP de la Riot API: {status}"),
            Self::Network(error) => write!(f, "error de red contra la Riot API: {error}"),
            Self::Decode(error) => write!(f, "respuesta ilegible de la Riot API: {error}"),
            Self::InvalidPlatform(platform) => {
                write!(f, "platform id desconocido para el routing: {platform:?}")
            }
        }
    }
}

impl std::error::Error for RiotApiError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Network(error) | Self::Decode(error) => Some(error),
            _ => None,
        }
    }
}

/// Espera a aplicar ante un `429` según el header `Retry-After`.
///
/// - Header ausente, no-UTF8, no numérico o negativo → [`DEFAULT_RETRY_AFTER`].
/// - Valores mayores al tope se recortan a [`MAX_RETRY_AFTER_WAIT`] (la app
///   no debe bloquearse minutos por un service limit).
///
/// Puro y testeado: el caso "header ausente" entra como `None`.
#[must_use]
pub fn parse_retry_after(header: Option<&str>) -> Duration {
    header
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .map_or(DEFAULT_RETRY_AFTER, |secs| {
            Duration::from_secs(secs).min(MAX_RETRY_AFTER_WAIT)
        })
}

/// Tiempo a esperar antes de la próxima llamada para respetar
/// [`MIN_CALL_INTERVAL`] desde `last_call`. `None` = llamar ya.
/// Puro y testeado con `Instant`s sintéticos.
fn pace_delay(last_call: Option<Instant>, now: Instant) -> Option<Duration> {
    let elapsed = now.duration_since(last_call?);
    MIN_CALL_INTERVAL
        .checked_sub(elapsed)
        .filter(|wait| !wait.is_zero())
}

/// Hexadecimal en mayúsculas para el percent-encoding (`%XX`).
const HEX_UPPER: &[u8; 16] = b"0123456789ABCDEF";

/// Percent-encoding de un segmento de path (RFC 3986) sin dependencias
/// nuevas: deja intactos los caracteres no reservados (`A-Z`, `a-z`, `0-9`,
/// `-`, `.`, `_`, `~`) y codifica cada byte restante como `%XX` (hexadecimal
/// en mayúsculas, UTF-8 byte a byte: `"Más"` → `"M%C3%A1s"`).
///
/// Codificar `#`, `/`, espacios y no-ASCII es lo seguro para un segmento:
/// el `#` del Riot ID (`gameName#tagLine`) NUNCA llega acá — los dos
/// componentes se pasan por separado y el `#` es solo el separador textual
/// de entrada —, pero si algún día apareciera en un componente, `%23`
/// evita truncar el path. Testeado con espacios, acentos y símbolos.
#[must_use]
pub(crate) fn percent_encode_path_segment(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for &byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char);
            }
            _ => {
                out.push('%');
                out.push(HEX_UPPER[usize::from(byte >> 4)] as char);
                out.push(HEX_UPPER[usize::from(byte & 0x0F)] as char);
            }
        }
    }
    out
}

/// URL completa de una llamada: base regional + path + querystring.
///
/// Los valores del query los genera este módulo (números y `"ranked"`, ver
/// `match_ids`), así que no hace falta percent-encoding ni el feature
/// `query` de reqwest (restricción del proyecto: cero dependencias nuevas).
/// Los segmentos de path con caracteres libres (Riot ID en
/// `account_puuid_by_riot_id`) llegan ya codificados con
/// [`percent_encode_path_segment`].
fn build_url(routing: &str, path: &str, query: &[(&str, String)]) -> String {
    let mut url = format!("{}{path}", regional_base_url(routing));
    if !query.is_empty() {
        let pairs = query
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>()
            .join("&");
        url.push('?');
        url.push_str(&pairs);
    }
    url
}

/// Modelo mínimo y tolerante de `GET /riot/account/v1/accounts/by-riot-id/...`:
/// solo nos interesa `puuid` (el resto del payload — `gameName`, `tagLine`,
/// campos futuros — se ignora, que es el default de serde).
#[derive(Debug, Deserialize)]
struct AccountDto {
    puuid: String,
}

/// Cliente de la Riot API oficial para Match-V5.
///
/// Uso (fase 8b): `RiotApi::from_env("la1")` y luego llamadas secuenciales
/// con `&mut self` — el propio préstamo mutable impide llamadas
/// concurrentes, que es exactamente el comportamiento de rate limiting que
/// queremos a este nivel.
pub struct RiotApi {
    /// Cliente HTTP propio (connection pooling + timeouts acotados); la key
    /// viaja en el header `X-Riot-Token` de cada request.
    client: reqwest::Client,
    /// Routing regional resuelto (`"americas"`, `"europe"`, `"asia"`,
    /// `"sea"`): host de todos los endpoints Match-V5.
    routing: String,
    /// API key (header `X-Riot-Token`). Nunca se loguea.
    key: String,
    /// Instante de la última llamada, para el paceo defensivo.
    last_call: Option<Instant>,
}

impl RiotApi {
    /// Construye el cliente leyendo la key del entorno (`RIOT_API_KEY`,
    /// leída una única vez acá). `platform` es el platform id del invocador
    /// (ej. `"la1"`), que se resuelve a routing regional de una vez.
    ///
    /// Errores: `ApiKeyMissing` si no hay key, `InvalidPlatform` si el
    /// platform id no está en la tabla de routing, `Network` si no se pudo
    /// construir el cliente HTTP (falla de TLS local).
    pub fn from_env(platform: &str) -> Result<Self, RiotApiError> {
        Self::from_key(platform, api_key_from_env())
    }

    /// Variante de [`RiotApi::from_env`] con key inyectada (ya normalizada
    /// con `normalize_key`: espacios recortados, vacío = sin key). Útil para
    /// tests e inyección explícita.
    ///
    /// # Errors
    /// `ApiKeyMissing` si `key` es `None` o vacía, `InvalidPlatform` si el
    /// platform id no es reconocido, `Network` si falla la construcción del
    /// cliente HTTP.
    pub fn from_key(platform: &str, key: Option<String>) -> Result<Self, RiotApiError> {
        let key = normalize_key(key).ok_or(RiotApiError::ApiKeyMissing)?;
        let routing = routing_for_platform(platform)
            .ok_or_else(|| RiotApiError::InvalidPlatform(platform.to_string()))?
            .to_string();

        let client = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(RiotApiError::Network)?;

        Ok(Self {
            client,
            routing,
            key,
            last_call: None,
        })
    }

    /// Routing regional resuelto para este cliente (ej. `"americas"`).
    #[must_use]
    pub fn routing(&self) -> &str {
        &self.routing
    }

    /// Ids de las partidas de un jugador:
    /// `GET /lol/match/v5/matches/by-puuid/{puuid}/ids`.
    ///
    /// `queue` filtra por cola (el dataset usa [`RANKED_SOLO_QUEUE`]);
    /// `match_type` agrega el filtro `type=ranked` del spec (usar
    /// [`RANKED_MATCH_TYPE`], o `None` para no filtrar). Devuelve hasta
    /// `count` ids a partir de `start` (0 = más reciente primero).
    pub async fn match_ids(
        &mut self,
        puuid: &str,
        start: i64,
        count: i64,
        queue: i64,
        match_type: Option<&str>,
    ) -> Result<Vec<String>, RiotApiError> {
        let mut query: Vec<(&str, String)> = vec![
            ("start", start.to_string()),
            ("count", count.to_string()),
            ("queue", queue.to_string()),
        ];
        if let Some(match_type) = match_type {
            query.push(("type", match_type.to_string()));
        }

        self.get_json(&format!("/lol/match/v5/matches/by-puuid/{puuid}/ids"), &query)
            .await
    }

    /// Detalle completo de una partida:
    /// `GET /lol/match/v5/matches/{matchId}`.
    pub async fn match_detail(&mut self, match_id: &str) -> Result<MatchDetail, RiotApiError> {
        self.get_json(&format!("/lol/match/v5/matches/{match_id}"), &[])
            .await
    }

    /// Puuid unificado de la cuenta Riot (namespace Account-V1) para un
    /// Riot ID: `GET /riot/account/v1/accounts/by-riot-id/{gameName}/{tagLine}`.
    ///
    /// Es el puuid que Match-V5 espera en `match_ids`/`match_detail`: el
    /// puuid de juego que reporta el LCU pertenece a otro namespace y Match-V5
    /// lo rechaza con `HTTP 400 "Exception decrypting"` (ver la doc del
    /// módulo). El Riot ID viene del LCU
    /// (`/lol-summoner/v1/current-summoner`, campos `gameName`/`tagLine`);
    /// `gameName` puede traer espacios o acentos y `tagLine` no incluye el
    /// `#` (ese carácter solo separa los dos componentes en el Riot ID
    /// textual), así que ambos se codifican con
    /// [`percent_encode_path_segment`].
    ///
    /// Usa el mismo routing regional que Match-V5 (`americas`/`europe`/...).
    /// Un Riot ID inexistente devuelve `RiotApiError::Http(404)`.
    ///
    /// # Errors
    /// Los mismos de toda llamada del cliente: `Http` (404 = Riot ID
    /// desconocido, 403 = key inválida), `RateLimited`, `Network`, `Decode`.
    pub async fn account_puuid_by_riot_id(
        &mut self,
        game_name: &str,
        tag_line: &str,
    ) -> Result<String, RiotApiError> {
        let path = format!(
            "/riot/account/v1/accounts/by-riot-id/{}/{}",
            percent_encode_path_segment(game_name),
            percent_encode_path_segment(tag_line),
        );
        let account: AccountDto = self.get_json(&path, &[]).await?;
        Ok(account.puuid)
    }

    /// GET genérico con paceo y reintentos ante `429`. Toda llamada HTTP de
    /// este módulo pasa por acá.
    async fn get_json<T: DeserializeOwned>(
        &mut self,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<T, RiotApiError> {
        let url = build_url(&self.routing, path, query);
        let mut retries = 0_u32;

        loop {
            // Paceo defensivo: mínimo entre inicios de llamada, incluso
            // entre reintentos.
            if let Some(wait) = pace_delay(self.last_call, Instant::now()) {
                sleep(wait).await;
            }
            self.last_call = Some(Instant::now());

            let response = self
                .client
                .get(url.as_str())
                .header(X_RIOT_TOKEN_HEADER, self.key.as_str())
                .send()
                .await;

            let response = match response {
                Ok(response) => response,
                Err(error) => return Err(RiotApiError::Network(error)),
            };

            let status = response.status();
            if status.as_u16() == 429 {
                retries += 1;
                if retries > MAX_RATE_LIMIT_RETRIES {
                    return Err(RiotApiError::RateLimited);
                }
                let wait = parse_retry_after(
                    response
                        .headers()
                        .get(RETRY_AFTER_HEADER)
                        .and_then(|value| value.to_str().ok()),
                );
                sleep(wait).await;
                continue;
            }

            if !status.is_success() {
                return Err(RiotApiError::Http(status.as_u16()));
            }

            return response
                .json::<T>()
                .await
                .map_err(RiotApiError::Decode);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;

    /// Header `Retry-After` legible → esos segundos (con trim).
    #[test]
    fn retry_after_legible() {
        assert_eq!(parse_retry_after(Some("3")), Duration::from_secs(3));
        assert_eq!(parse_retry_after(Some(" 7 ")), Duration::from_secs(7));
        assert_eq!(parse_retry_after(Some("0")), Duration::from_secs(0));
    }

    /// Header ausente, vacío, no numérico o negativo → default de 2 s.
    #[test]
    fn retry_after_ausente_o_invalido_usa_default() {
        assert_eq!(parse_retry_after(None), DEFAULT_RETRY_AFTER);
        assert_eq!(parse_retry_after(Some("")), DEFAULT_RETRY_AFTER);
        assert_eq!(parse_retry_after(Some("abc")), DEFAULT_RETRY_AFTER);
        assert_eq!(parse_retry_after(Some("-5")), DEFAULT_RETRY_AFTER);
        assert_eq!(parse_retry_after(Some("2.5")), DEFAULT_RETRY_AFTER);
    }

    /// Valores enormes (service limit de minutos) se recortan al tope de 60
    /// s para no bloquear la app.
    #[test]
    fn retry_after_se_recorta_al_tope_defensivo() {
        assert_eq!(
            parse_retry_after(Some("3600")),
            MAX_RETRY_AFTER_WAIT,
        );
    }

    /// Bordes del tope: 60 s exactos se respetan (no se recorta a 2 s ni a
    /// otro valor), 61 ya recorta, y un número que desborda `u64` cae al
    /// default en vez de paniquear.
    #[test]
    fn retry_after_en_el_borde_del_tope_y_con_overflow() {
        assert_eq!(parse_retry_after(Some("60")), Duration::from_secs(60));
        assert_eq!(parse_retry_after(Some("61")), MAX_RETRY_AFTER_WAIT);
        assert_eq!(
            parse_retry_after(Some("99999999999999999999")), // > u64::MAX
            DEFAULT_RETRY_AFTER
        );
    }

    /// Paceo: sin llamada previa no se espera; con una llamada reciente se
    /// espera el remanente; con una llamada vieja no se espera nada.
    #[test]
    fn paceo_respeta_el_minimo_entre_llamadas() {
        let now = Instant::now();

        assert_eq!(pace_delay(None, now), None);
        assert_eq!(
            pace_delay(Some(now - Duration::from_millis(10)), now),
            Some(Duration::from_millis(140))
        );
        assert_eq!(
            pace_delay(Some(now - Duration::from_millis(150)), now),
            None
        );
        assert_eq!(
            pace_delay(Some(now - Duration::from_millis(500)), now),
            None
        );
    }

    /// La URL de ids de partida: base regional + path con puuid + query con
    /// `start`/`count`/`queue` y `type` solo cuando se pide.
    #[test]
    fn url_de_match_ids_con_y_sin_filtro_de_tipo() {
        let query = vec![
            ("start", String::from("0")),
            ("count", String::from("100")),
            ("queue", String::from("420")),
            ("type", String::from("ranked")),
        ];
        assert_eq!(
            build_url(
                "americas",
                "/lol/match/v5/matches/by-puuid/puuid-a/ids",
                &query
            ),
            "https://americas.api.riotgames.com/lol/match/v5/matches/by-puuid/puuid-a/ids?start=0&count=100&queue=420&type=ranked"
        );

        assert_eq!(
            build_url("sea", "/lol/match/v5/matches/LA1_1", &[]),
            "https://sea.api.riotgames.com/lol/match/v5/matches/LA1_1"
        );
    }

    /// Percent-encoding de segmentos: los caracteres no reservados quedan
    /// intactos y todo lo demás sale como `%XX` (UTF-8 byte a byte).
    #[test]
    fn percent_encoding_de_segmentos() {
        // Alfanuméricos y no reservados: idéntico.
        assert_eq!(
            percent_encode_path_segment("PKT-Mid.King_1~"),
            "PKT-Mid.King_1~"
        );

        // El caso real del Riot ID de prueba: espacio en gameName.
        assert_eq!(percent_encode_path_segment("PKT MidKing"), "PKT%20MidKing");

        // Acentos y eñes: cada byte UTF-8 se codifica por separado.
        assert_eq!(percent_encode_path_segment("Más"), "M%C3%A1s");
        assert_eq!(percent_encode_path_segment("España"), "Espa%C3%B1a");

        // `#` es el separador del Riot ID y nunca debería llegar acá, pero
        // si apareciera se codifica (%23) para no truncar el path.
        assert_eq!(percent_encode_path_segment("Tag#1"), "Tag%231");

        // Vacío sigue vacío.
        assert_eq!(percent_encode_path_segment(""), "");
    }

    /// URL completa de Account-V1 con un Riot ID real (espacio incluido):
    /// mismo host regional que Match-V5, path con el espacio codificado.
    #[test]
    fn url_de_account_by_riot_id_con_espacio_y_acentos() {
        let path = format!(
            "/riot/account/v1/accounts/by-riot-id/{}/{}",
            percent_encode_path_segment("PKT MidKing"),
            percent_encode_path_segment("Akali")
        );
        assert_eq!(
            build_url("americas", &path, &[]),
            "https://americas.api.riotgames.com/riot/account/v1/accounts/by-riot-id/PKT%20MidKing/Akali"
        );

        let path_acento = format!(
            "/riot/account/v1/accounts/by-riot-id/{}/{}",
            percent_encode_path_segment("José"),
            percent_encode_path_segment("LAS")
        );
        assert_eq!(
            build_url("americas", &path_acento, &[]),
            "https://americas.api.riotgames.com/riot/account/v1/accounts/by-riot-id/Jos%C3%A9/LAS"
        );
    }

    /// El DTO de Account-V1 es tolerante: extrae `puuid` e ignora el resto
    /// del payload real.
    #[test]
    fn account_dto_extrae_puuid_e_ignora_el_resto() {
        let account: AccountDto = serde_json::from_str(
            r#"{"puuid":"puuid-api-1234","gameName":"PKT MidKing","tagLine":"Akali"}"#,
        )
        .expect("payload real parsea");
        assert_eq!(account.puuid, "puuid-api-1234");
    }

    /// Sin key (`RIOT_API_KEY` ausente) no se construye el cliente: error
    /// `ApiKeyMissing`, sin pánico — la app funciona sin Riot API.
    #[test]
    fn sin_key_es_api_key_missing() {
        assert!(matches!(
            RiotApi::from_key("la1", None),
            Err(RiotApiError::ApiKeyMissing)
        ));
        assert!(matches!(
            // Vacía o solo espacios cuenta como sin key.
            RiotApi::from_key("la1", Some(String::from("   "))),
            Err(RiotApiError::ApiKeyMissing)
        ));
    }

    /// Un platform id fuera de la tabla se rechaza al construir, no al
    /// mitad de la recolección.
    #[test]
    fn platform_desconocida_es_invalid_platform() {
        assert!(matches!(
            RiotApi::from_key("lan", Some(String::from("RGAPI-test"))),
            Err(RiotApiError::InvalidPlatform(platform)) if platform == "lan"
        ));
    }

    /// Construcción correcta: la key se normaliza y el platform se resuelve
    /// a routing regional.
    #[test]
    fn from_key_resuelve_routing_y_normaliza_la_key() {
        let api = RiotApi::from_key("la1", Some(String::from("  RGAPI-test  ")))
            .expect("construye con key y platform válidas");

        assert_eq!(api.routing(), "americas");
    }

    /// `from_env` nunca paniquea: con `RIOT_API_KEY` presente construye el
    /// cliente y sin ella devuelve `ApiKeyMissing` (el branch depende del
    /// entorno real, así que se verifica contra lo que reporta
    /// `api_key_from_env` en cada máquina).
    #[test]
    fn from_env_nunca_paniquea_y_refleja_la_presencia_de_la_key() {
        let hay_key = api_key_from_env().is_some();
        let resultado = RiotApi::from_env("la1");

        if hay_key {
            assert!(resultado.is_ok(), "con key en el entorno debe construir");
        } else {
            assert!(
                matches!(resultado, Err(RiotApiError::ApiKeyMissing)),
                "sin key debe ser ApiKeyMissing, sin pánico"
            );
        }
    }

    /// Invariante de seguridad (regresión): la key viaja SOLO en el header
    /// `X-Riot-Token`; las URLs que construye el cliente jamás contienen
    /// material de la key (ni como query ni en el path).
    #[test]
    fn la_key_nunca_aparece_en_las_urls_construidas() {
        let api = RiotApi::from_key("la1", Some(String::from("RGAPI-secreta-123")))
            .expect("construye");

        // El header de auth es el oficial de Riot, no un query param.
        assert_eq!(X_RIOT_TOKEN_HEADER, "X-Riot-Token");

        let url_ids = build_url(
            api.routing(),
            "/lol/match/v5/matches/by-puuid/puuid-a/ids",
            &[
                ("start", String::from("0")),
                ("count", String::from("100")),
                ("queue", RANKED_SOLO_QUEUE.to_string()),
                ("type", String::from(RANKED_MATCH_TYPE)),
            ],
        );
        let url_detail = build_url(api.routing(), "/lol/match/v5/matches/LA1_1", &[]);

        for url in [url_ids, url_detail] {
            assert!(!url.contains("RGAPI"), "la key no viaja en la URL: {url}");
            assert!(
                !url.contains("api_key") && !url.contains("api-key"),
                "sin query de key: {url}"
            );
        }
    }

    /// Constantes del spec: cola ranked solo/duo (420), filtro `type=ranked`
    /// y paceo defensivo de 150 ms entre llamadas.
    #[test]
    fn constantes_del_spec_de_coleccion() {
        assert_eq!(RANKED_SOLO_QUEUE, 420);
        assert_eq!(RANKED_MATCH_TYPE, "ranked");
        assert_eq!(MIN_CALL_INTERVAL, Duration::from_millis(150));
    }
}
