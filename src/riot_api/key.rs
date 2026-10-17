//! Lectura de la API key de Riot (Match-V5) desde el entorno del proceso.
//!
//! La key **nunca** se hardcodea en el código ni se commitea (política de
//! seguridad de Riot: "This API key may not be included in your code"). Se
//! lee una única vez de la variable de entorno [`API_KEY_ENV`] con
//! [`api_key_from_env`]; no se usan crates tipo `dotenvy` (restricción del
//! proyecto: cero dependencias nuevas), así que la forma de proveerla es
//! exportarla en la shell antes de lanzar la app:
//!
//! ```sh
//! export RIOT_API_KEY="RGAPI-xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx"
//! ```
//!
//! o pasársela al proceso en el entorno que lo lance. Sin key,
//! `RiotApi::from_env` devuelve `RiotApiError::ApiKeyMissing` y la
//! recolección del dataset queda deshabilitada sin afectar al resto de la
//! app (nunca se paniquea ni se crashea por falta de key).

/// Variable de entorno de la que se lee la API key de Riot.
pub const API_KEY_ENV: &str = "RIOT_API_KEY";

/// Normaliza el valor crudo de la variable de entorno: recorta espacios en
/// los bordes y descarta el vacío (`None`). Pura y testeada sin tocar el
/// entorno real del proceso.
#[must_use]
pub fn normalize_key(raw: Option<String>) -> Option<String> {
    let trimmed = raw.as_deref()?.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// Lee la API key una única vez del entorno del proceso. `None` si la
/// variable no está definida o quedó vacía tras recortar espacios.
#[must_use]
pub fn api_key_from_env() -> Option<String> {
    normalize_key(std::env::var(API_KEY_ENV).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Un valor con espacios de más se recorta, no se descarta.
    #[test]
    fn recorta_espacios_del_valor() {
        assert_eq!(
            normalize_key(Some(String::from("  RGAPI-abc  \n"))),
            Some(String::from("RGAPI-abc"))
        );
    }

    /// `None`, vacío y solo espacios no son una key: `None`.
    #[test]
    fn valores_vacios_o_ausentes_son_none() {
        assert_eq!(normalize_key(None), None);
        assert_eq!(normalize_key(Some(String::new())), None);
        assert_eq!(normalize_key(Some(String::from("   \t"))), None);
    }
}
