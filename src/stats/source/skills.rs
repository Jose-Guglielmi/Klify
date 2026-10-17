//! Skill order + summoner spells (FASE 11a-1, pieza 4 — best-effort
//! honesto).
//!
//! **Resultado de la verificación en vivo (2026-10-17, parche 16.20):**
//! ninguna fuente pública accesible expone skill order ni summoners hoy:
//!
//! - **Lolalytics**: el mega-endpoint no los trae. `ep=rune` responde con
//!   `summary` = `{ runes, win, pick }` (solo runas, verificado); `ep=build-itemset`
//!   responde solo con `itemSets` (sin `summary` ni skills). Existe un
//!   `ep=build-skill` pero está **roto**: responde 500 vacío en todas las
//!   variantes probadas (`con/sin lane`, `qlty`, `keystone`), mientras que
//!   los eps desconocidos devuelven `"invalid end point"` — o sea, el ep
//!   existe server-side pero no sirve datos a clientes públicos.
//! - **U.GG**: `stats2.u.gg` sigue detrás del challenge de Cloudflare
//!   (**403** "Just a moment...") hacia clientes no-navegador; su overview
//!   (`pd[1]` spells, `pd[4]` skill order, formato de league-lean) es
//!   inaccesible hoy. Ver la nota equivalente en [`super::ugg`].
//!
//! Decisión del proyecto: **cero datos fabricados**. El modelo
//! [`SkillOrder`] ya está en su forma final (con `SkillSource`) y
//! [`fetch_skill_order`] queda listo: hoy intenta `ep=build-skill` (para
//! que el log del ciclo muestre cuando el ep reviva) y devuelve
//! [`SkillSource::Unavailable`] limpiamente hasta que una fuente
//! verificada exponga la forma real. La UI muestra "no disponible" y no
//! se inventan prioridades de skills ni summoners.

use serde::Serialize;

use super::{mega_url, SourceError};
use crate::lcu::models::Position;

/// Fuente de la que saldría el skill order / summoners.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum SkillSource {
    /// Lolalytics `ep=build-skill` (hoy: 500, ep roto).
    Lolalytics,
    /// U.GG overview `pd[1]`/`pd[4]` (hoy: 403 de Cloudflare).
    Ugg,
    /// Ninguna fuente accesible devolvió datos verificables.
    Unavailable,
}

impl std::fmt::Display for SkillSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Lolalytics => write!(f, "Lolalytics"),
            Self::Ugg => write!(f, "U.GG"),
            Self::Unavailable => write!(f, "no disponible"),
        }
    }
}

/// Prioridad de skills y summoners para un campeón/posición/parche.
///
/// - `priority`: orden de subida en early game (ej. `['Q', 'W', 'E']`).
/// - `levels`: asignación por nivel 1..18 si la fuente la expone (18
///   entradas); `None` si solo hay prioridad.
/// - `summoners`: ids de los dos hechizos de invocador si la fuente los
///   expone (ej. `(4, 14)` = Flash + Ignite).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillOrder {
    /// Fuente de los datos (o `Unavailable`).
    pub source: SkillSource,
    /// Orden de prioridad de skills (Q/W/E; la R se asume cuando aplica).
    pub priority: Vec<char>,
    /// Asignación por nivel (1..=18) si la fuente la expone.
    pub levels: Option<Vec<char>>,
    /// Ids de los dos summoners recomendados, si la fuente los expone.
    pub summoners: Option<(i64, i64)>,
}

impl SkillOrder {
    /// Valor limpio "sin datos": la UI muestra "no disponible" y nada se
    /// inventa.
    #[must_use]
    pub fn unavailable() -> Self {
        Self {
            source: SkillSource::Unavailable,
            priority: Vec::new(),
            levels: None,
            summoners: None,
        }
    }
}

/// URL de `ep=build-skill` (el endpoint candidato; hoy 500 — ver la doc del
/// módulo).
#[must_use]
pub fn skill_url(slug: &str, position: Option<Position>, patch: &str) -> String {
    mega_url("build-skill", slug, position, patch)
}

/// Consulta el skill order + summoners para `slug + posición + parche`.
///
/// Nunca falla: cualquier error de red o payload no verificable degenera en
/// [`SkillOrder::unavailable`] (con log). La forma del payload de
/// `ep=build-skill` sigue sin verificar (500 en la última prueba en vivo),
/// así que un 200 eventual también termina en `Unavailable` hasta
/// implementar un parser contra una forma real documentada — no se fabrica
/// un parser a ciegas.
pub async fn fetch_skill_order(
    client: &reqwest::Client,
    slug: &str,
    position: Option<Position>,
    patch: &str,
) -> SkillOrder {
    let url = skill_url(slug, position, patch);

    match probe_build_skill(client, &url).await {
        Ok(value) => {
            // El endpoint algún día pudo revivir: sin forma verificada no
            // hay parser que valga. Se registra y se degrada.
            eprintln!(
                "klipy [skills]: ep=build-skill respondió datos para {slug} ({patch}) pero la forma no está verificada; revisar y documentar el parser. Claves: {:?}",
                keys_of(&value)
            );
            SkillOrder::unavailable()
        }
        Err(error) => {
            eprintln!(
                "klipy [skills]: aviso: sin skill order para {slug} ({patch}) ({error}); la UI muestra 'no disponible'"
            );
            SkillOrder::unavailable()
        }
    }
}

/// Claves de nivel superior de un payload JSON, para el log de diagnóstico.
fn keys_of(value: &serde_json::Value) -> Vec<String> {
    match value {
        serde_json::Value::Object(map) => map.keys().cloned().collect(),
        _ => Vec::new(),
    }
}

/// GET a `ep=build-skill` con tolerancia total: `Ok(value)` si llegó JSON
/// (revivió el endpoint), `Err` ante cualquier fallo de red/HTTP/parseo
/// (hoy: 500 verificada en vivo).
async fn probe_build_skill(
    client: &reqwest::Client,
    url: &str,
) -> Result<serde_json::Value, SourceError> {
    let response = client
        .get(url)
        .send()
        .await
        .and_then(|response| response.error_for_status())
        .map_err(|error| SourceError::Lolalytics(format!("GET {url} falló: {error}")))?;

    response
        .json::<serde_json::Value>()
        .await
        .map_err(|error| {
            SourceError::Lolalytics(format!("respuesta de {url} no es JSON: {error}"))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_de_skills_usa_los_common_params() {
        assert_eq!(
            skill_url("veigar", Some(Position::Middle), "16.20"),
            "https://a1.lolalytics.com/mega/?ep=build-skill&v=1&patch=16.20&c=veigar&tier=platinum_plus&queue=ranked&region=all&lane=middle"
        );
    }

    /// El valor "no disponible" es limpio: fuente `Unavailable`, sin
    /// prioridad, sin niveles ni summoners (cero datos fabricados).
    #[test]
    fn unavailable_no_trae_datos_inventados() {
        let empty = SkillOrder::unavailable();
        assert_eq!(empty.source, SkillSource::Unavailable);
        assert!(empty.priority.is_empty());
        assert_eq!(empty.levels, None);
        assert_eq!(empty.summoners, None);
        assert_eq!(empty.source.to_string(), "no disponible");

        // La forma final del modelo (para cuando una fuente abra):
        let ready = SkillOrder {
            source: SkillSource::Lolalytics,
            priority: vec!['Q', 'W', 'E'],
            levels: Some(vec!['Q'; 18]),
            summoners: Some((4, 14)),
        };
        assert_eq!(ready.source.to_string(), "Lolalytics");
        assert_eq!(ready.levels.as_ref().map(Vec::len), Some(18));
    }

    /// `keys_of` extrae las claves de nivel superior para el log de
    /// diagnóstico (y tolera payloads no-objeto).
    #[test]
    fn keys_of_expone_las_claves_del_payload() {
        assert_eq!(
            keys_of(&serde_json::json!({ "skills": [], "summary": {} })),
            vec![String::from("skills"), String::from("summary")]
        );
        assert!(keys_of(&serde_json::json!([1, 2])).is_empty());
    }
}
