//! Routing regional de los endpoints Match-V5 de la Riot API.
//!
//! Fuente de la tabla (doc oficial vigente, consultada 2026-10): sección
//! "Routing Values → Platform / Regional Routing Values" de
//! <https://developer.riotgames.com/docs/lol>. Los endpoints de Match-V5 se
//! sirven con **routing regional** en el host (`https://{routing}.
//! api.riotgames.com`), y el routing se deduce del platform id del
//! invocador (el que reporta el cliente LCU):
//!
//! | Routing regional | Host | Platform ids |
//! |------------------|------|--------------|
//! | `americas` | americas.api.riotgames.com | `br1`, `la1`, `la2`, `na1` |
//! | `asia` | asia.api.riotgames.com | `jp1`, `kr` |
//! | `europe` | europe.api.riotgames.com | `eun1`, `euw1`, `tr1`, `ru` |
//! | `sea` | sea.api.riotgames.com | `oc1`, `ph2`, `sg2`, `th2`, `tw2`, `vn2` |
//!
//! NOTA sobre `apac` vs `sea`: la doc oficial de LoL lista hoy exactamente
//! `AMERICAS`, `ASIA`, `EUROPE` y `SEA`; `apac` solo sobrevive como valor
//! deprecado en APIs de otros juegos (p. ej. `lor-match-v1`), así que el
//! routing para Oceanía/sudeste asiático es `sea`.
//!
//! NOTA sobre `me1` (Medio Oriente): existe en algunas APIs globales pero no
//! figura en la tabla oficial de plataformas de LoL, así que no se mapea.

/// Routing regional de un platform id de LoL (ej. `"la1"` → `"americas"`).
///
/// Acepta el id con cualquier combinación de mayúsculas/minúsculas y espacios
/// de más (el LCU y la doc de Riot varían entre `"LA1"` y `"la1"`). Un
/// platform desconocido devuelve `None`: nunca se adivina un routing.
#[must_use]
pub fn routing_for_platform(platform: &str) -> Option<&'static str> {
    match platform.trim().to_ascii_lowercase().as_str() {
        "br1" | "la1" | "la2" | "na1" => Some("americas"),
        "jp1" | "kr" => Some("asia"),
        "eun1" | "euw1" | "tr1" | "ru" => Some("europe"),
        "oc1" | "ph2" | "sg2" | "th2" | "tw2" | "vn2" => Some("sea"),
        _ => None,
    }
}

/// Normaliza un platform id crudo (trim + minúsculas) y lo valida contra la
/// tabla de routing: devuelve la forma canónica (`"LA1 "` → `"la1"`) o `None`
/// si no es una plataforma conocida.
///
/// Es el punto único de validación de platform ids (FASE 8b): tanto la
/// variable de entorno `RIOT_PLATFORM` como el valor que reporta el LCU
/// pasan por acá, de modo que ninguna fuente puede inyectar un platform
/// desconocido en el cliente de la Riot API.
#[must_use]
pub fn normalize_platform(raw: &str) -> Option<String> {
    let normalized = raw.trim().to_ascii_lowercase();
    routing_for_platform(&normalized).map(|_| normalized)
}

/// URL base regional de la Riot API para un routing dado:
/// `https://{routing}.api.riotgames.com`.
#[must_use]
pub fn regional_base_url(routing: &str) -> String {
    format!("https://{routing}.api.riotgames.com")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Los 16 platform ids de la tabla oficial, cada uno con su routing.
    #[test]
    fn cada_region_mapea_sus_platforms_de_la_doc_oficial() {
        for (platform, expected) in [
            ("br1", "americas"),
            ("la1", "americas"),
            ("la2", "americas"),
            ("na1", "americas"),
            ("jp1", "asia"),
            ("kr", "asia"),
            ("eun1", "europe"),
            ("euw1", "europe"),
            ("tr1", "europe"),
            ("ru", "europe"),
            ("oc1", "sea"),
            ("ph2", "sea"),
            ("sg2", "sea"),
            ("th2", "sea"),
            ("tw2", "sea"),
            ("vn2", "sea"),
        ] {
            assert_eq!(
                routing_for_platform(platform),
                Some(expected),
                "platform {platform}"
            );
        }
    }

    /// La doc de Riot usa mayúsculas (`LA1`) y el LCU minúsculas (`la1`):
    /// ambos caminos funcionan, con o sin espacios de más.
    #[test]
    fn es_insensible_a_caso_y_espacios() {
        assert_eq!(routing_for_platform("LA1"), Some("americas"));
        assert_eq!(routing_for_platform("KR"), Some("asia"));
        assert_eq!(routing_for_platform("Euw1"), Some("europe"));
        assert_eq!(routing_for_platform("OC1"), Some("sea"));
        assert_eq!(routing_for_platform("  la2  "), Some("americas"));
    }

    /// Platform desconocidas (códigos viejos u otros juegos) y entradas
    /// vacías → `None`, nunca un routing inventado.
    #[test]
    fn platforms_desconocidas_devuelven_none() {
        assert_eq!(routing_for_platform(""), None);
        assert_eq!(routing_for_platform("   "), None);
        assert_eq!(routing_for_platform("lan"), None);
        assert_eq!(routing_for_platform("las"), None);
        assert_eq!(routing_for_platform("na"), None);
        assert_eq!(routing_for_platform("euw"), None);
        assert_eq!(routing_for_platform("oce"), None);
        assert_eq!(routing_for_platform("apac"), None);
        assert_eq!(routing_for_platform("me1"), None);
        assert_eq!(routing_for_platform("na1 "), Some("americas")); // trim sí
        assert_eq!(routing_for_platform("na11"), None);
    }

    /// Normalización + validación: mayúsculas/espacios se recortan y el
    /// resultado siempre pasa la tabla; lo desconocido es `None`.
    #[test]
    fn normaliza_y_valida_platforms() {
        assert_eq!(normalize_platform("LA1"), Some(String::from("la1")));
        assert_eq!(normalize_platform("  KR  "), Some(String::from("kr")));
        assert_eq!(normalize_platform("Euw1"), Some(String::from("euw1")));
        assert_eq!(normalize_platform("lan"), None);
        assert_eq!(normalize_platform(""), None);
        assert_eq!(normalize_platform("   "), None);
    }

    /// La URL base respeta el formato oficial de host regional.
    #[test]
    fn url_base_regional() {
        assert_eq!(
            regional_base_url("americas"),
            "https://americas.api.riotgames.com"
        );
        assert_eq!(regional_base_url("sea"), "https://sea.api.riotgames.com");
    }
}
