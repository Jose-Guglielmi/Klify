//! Traducción EN→ES best-effort vía el endpoint público de Google
//! Translate (`translate.googleapis.com/translate_a/single?client=gtx`),
//! sin API key. Es un endpoint no oficial: puede fallar o cambiar — por
//! eso el contrato es `Option<String>` y el caller SIEMPRE tiene el texto
//! original como fallback (la UI nunca se queda sin contenido).

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// Codifica texto para query string (RFC 3986 unreserved + `/` tal cual):
/// sin `serde_urlencoded` (reqwest va sin la feature `query` en este
/// proyecto). Puro, testeado.
#[must_use]
fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(*byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Traduce un texto EN→ES. `None` = traducción no disponible (red, cambio
/// de formato) → el caller muestra el original.
pub async fn translate_en_to_es(client: &reqwest::Client, text: &str) -> Option<String> {
    if text.trim().is_empty() {
        return None;
    }
    if let Some(hit) = cache_get(text) {
        return Some(hit);
    }

    let url = format!(
        "https://translate.googleapis.com/translate_a/single?client=gtx&sl=en&tl=es&dt=t&q={}",
        percent_encode(text)
    );
    let response = client.get(&url).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    let value: serde_json::Value = response.json().await.ok()?;
    let translated = parse_gtx_response(&value)?;

    cache_put(text, &translated);
    Some(translated)
}

/// Traduce una tanda de textos (mismo orden y misma longitud que la
/// entrada; los que fallen quedan con el original). Cacheada por texto.
pub async fn translate_all_en_to_es(client: &reqwest::Client, texts: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(texts.len());
    for text in texts {
        out.push(match translate_en_to_es(client, text).await {
            Some(translated) => translated,
            None => text.clone(),
        });
    }
    out
}

/// Extrae la traducción de la respuesta gtx: `[[["es","en",...], ...], ...]`
/// — concatena los fragmentos del primer bloque (puro, testeado).
#[must_use]
pub fn parse_gtx_response(value: &serde_json::Value) -> Option<String> {
    let segments = value.get(0)?.as_array()?;
    let mut translated = String::new();
    for segment in segments {
        let piece = segment.get(0)?.as_str()?;
        translated.push_str(piece);
    }
    let trimmed = translated.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

// ---------------------------------------------------------------------------
// Cache en memoria (texto original → traducción), una por corrida.
// ---------------------------------------------------------------------------

fn translation_cache() -> &'static Mutex<HashMap<String, String>> {
    static CACHE: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cache_get(key: &str) -> Option<String> {
    let cache = translation_cache().lock().ok()?;
    cache.get(key).cloned()
}

fn cache_put(original: &str, translated: &str) {
    if let Ok(mut cache) = translation_cache().lock() {
        cache.insert(original.to_string(), translated.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Forma real de la respuesta gtx (verificada en vivo 2026-10-17):
    /// fragmentos en la primera entrada; la traducción es la concatenación
    /// de los primeros elementos.
    #[test]
    fn parsea_la_respuesta_real_por_fragmentos() {
        let value: serde_json::Value = serde_json::json!([
            [
                ["Ten cuidado con el duelo de Vi. ", "Be careful for Vi's dueling. ", null, null],
                ["Puede ser bastante opresiva.\n", "She can be quite oppressive.\n", null, null]
            ],
            null,
            "en"
        ]);

        assert_eq!(
            parse_gtx_response(&value).as_deref(),
            Some("Ten cuidado con el duelo de Vi. Puede ser bastante opresiva.")
        );
    }

    /// Respuestas rotas → None (el caller cae al original).
    #[test]
    fn respuestas_rotas_devuelven_none() {
        assert_eq!(parse_gtx_response(&serde_json::json!([])), None);
        assert_eq!(parse_gtx_response(&serde_json::json!([["solo texto"]])), None,
            "segmento sin par traducción → None");
        assert_eq!(parse_gtx_response(&serde_json::json!([[]])), None);
        assert_eq!(parse_gtx_response(&serde_json::json!(null)), None);
    }
}

#[cfg(test)]
mod percent_tests {
    use super::percent_encode;

    #[test]
    fn codifica_reservados_y_deja_seguros() {
        assert_eq!(percent_encode("Vi's dueling"), "Vi%27s%20dueling");
        assert_eq!(percent_encode("a/b_c-d.e~f"), "a/b_c-d.e~f");
        assert_eq!(percent_encode("ñ"), "%C3%B1");
        assert_eq!(percent_encode(""), "");
    }
}

#[cfg(test)]
mod network_probe {
    use super::*;

    /// Diagnóstico temporal: replica el fetch exacto de la app contra gtx.
    /// Correr con: cargo test probe_gtx -- --ignored --nocapture
    #[tokio::test]
    #[ignore = "network probe"]
    async fn probe_gtx() {
        let client = crate::data_dragon::http_client().expect("client");
        let text = String::from("Be careful for Vi's dueling. She can be quite oppressive.");
        match translate_en_to_es(&client, &text).await {
            Some(translated) => println!("OK: {translated}"),
            None => {
                println!("FALLÓ — probando pasos individuales:");
                let url = format!(
                    "https://translate.googleapis.com/translate_a/single?client=gtx&sl=en&tl=es&dt=t&q={}",
                    percent_encode(&text)
                );
                let response = client.get(&url).send().await.expect("send");
                println!("status: {}", response.status());
                let body = response.text().await.expect("body");
                println!("body head: {}", &body[..body.len().min(300)]);
                let value: serde_json::Value = serde_json::from_str(&body).expect("json");
                println!("parse: {:?}", parse_gtx_response(&value));
            }
        }
    }
}
