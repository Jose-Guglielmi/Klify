//! Consejos de jugadores contra un campeón, desde counterstats.net
//! (FASE 12, a pedido explícito del dueño del proyecto: son tips de la
//! comunidad MOBAFire que el sitio agrega, genéricos del campeón — no
//! por-matchup).
//!
//! Fuente: `GET https://www.counterstats.net/league-of-legends/{slug}`
//! (verificado en vivo 2026-10-17: HTTP 200 sin Cloudflare con UA de
//! navegador; contenido en INGLÉS — el sitio no localiza server-side).
//!
//! Estructura del HTML (estable a la vista): bloques
//! `<span class='tip-box__tip'><span class='author'>X says</span> “texto”</span>`
//! seguidos por `tip-box__guide-wrap` con el link de guía y el rol del
//! autor. El parseo es por scanning de strings (sin crate de HTML): si el
//! sitio cambia, el parser devuelve menos/vacíos y la UI muestra su estado
//! honesto — nunca un crash. Todo indexado sobre el HTML remoto va por
//! `get`/`truncate_safe`: un recorte fijo de bytes cayendo adentro de un
//! char multi-byte (las comillas “” ocupan 3 bytes) hizo panic con exit 101
//! en producción (2026-10-22, champ select) y acá está el fix con tests.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

use super::SourceError;

/// URL de la página de un campeón en counterstats.
#[must_use]
pub fn counterstats_url(slug: &str) -> String {
    format!("https://www.counterstats.net/league-of-legends/{slug}")
}

/// Decodifica entidades HTML del texto visible (puro, testeado): apóstrofe
/// numérico (`&#039;` — el caso masivo del sitio), hex, y las nombradas
/// comunes. Lo demás pasa intacto.
#[must_use]
pub fn decode_entities(input: &str) -> String {
    if !input.contains('&') {
        return input.to_string();
    }
    let mut out = String::with_capacity(input.len());
    let mut cursor = 0usize;
    while cursor < input.len() {
        // Input remoto: todo indexado va por `get`, con fallback que no
        // paniquea jamás (cursor es char boundary por construcción, pero si
        // algo se rompe, cortamos la decodificación en vez de matar la app).
        let Some(rest) = input.get(cursor..) else {
            out.push('\u{FFFD}');
            break;
        };
        if !rest.starts_with('&') {
            let Some(ch) = rest.chars().next() else {
                out.push('\u{FFFD}');
                break;
            };
            out.push(ch);
            cursor += ch.len_utf8();
            continue;
        }
        let Some(semicolon) = rest.find(';').map(|offset| cursor + offset) else {
            out.push('&');
            cursor += 1;
            continue;
        };
        if semicolon - cursor > 10 {
            // Entidad demasiado larga para serlo: ampersand literal.
            out.push('&');
            cursor += 1;
            continue;
        }
        let Some(entity) = input.get(cursor + 1..semicolon) else {
            out.push('&');
            cursor += 1;
            continue;
        };
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            _ => {
                if let Some(digits) = entity.strip_prefix('#') {
                    let code = if let Some(hex) = digits.strip_prefix('x').or_else(|| digits.strip_prefix('X')) {
                        u32::from_str_radix(hex, 16).ok()
                    } else {
                        digits.parse::<u32>().ok()
                    };
                    code.and_then(char::from_u32)
                } else {
                    None
                }
            }
        };
        match decoded {
            Some(ch) => {
                out.push(ch);
                cursor = semicolon + 1;
            }
            None => {
                out.push('&');
                cursor += 1;
            }
        }
    }
    out
}

/// Un consejo de jugador contra el campeón.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PlayerTip {
    /// Autor del consejo (ej. `"Stkfear"`).
    pub author: String,
    /// Texto del consejo (inglés, fuente original).
    pub text: String,
    /// Título de la guía MOBAFire enlazada, si el tip la trae.
    pub guide_title: Option<String>,
    /// Rol/rango del autor tal como lo muestra la fuente (ej.
    /// `"Rengar Player"`).
    pub author_role: Option<String>,
}

/// Tope de tips parseados por página (la página trae cientos; el resto es
/// ruido para la UI).
pub const MAX_TIPS: usize = 30;

/// Ventana máxima de bytes que se escanea por bloque de tip. Antes se
/// recortaba con slicing directo (`&html[block_start..block_end]`) y un
/// límite cayendo adentro de un char multi-byte mataba la app entera.
const VENTANA_BLOQUE_BYTES: usize = 4000;

/// Extrae los consejos de la página (puro, testeado con fixture real).
/// Tolerante: bloques sin autor/texto se descartan; sin bloques → vacío.
#[must_use]
pub fn parse_player_tips(html: &str) -> Vec<PlayerTip> {
    const TIP_MARKER: &str = "<span class='tip-box__tip'>";
    let mut tips = Vec::new();

    let mut cursor = 0usize;
    while tips.len() < MAX_TIPS {
        // Input remoto: indexado por `get` con fallback; el directo puede
        // paniquear si el cursor no es boundary.
        let Some(rest) = html.get(cursor..) else {
            break;
        };
        let Some(start) = rest.find(TIP_MARKER) else {
            break;
        };
        let block_start = cursor + start;
        let Some(tail) = html.get(block_start..) else {
            break;
        };
        // El límite fijo de bytes puede caer adentro de un char multi-byte
        // (ej. las comillas “” del sitio, 3 bytes): truncate_safe retrocede
        // al boundary anterior en vez de partir el char. Panic real con
        // exit 101 en producción (línea 126) — no volver a usar slicing.
        let block = truncate_safe(tail, VENTANA_BLOQUE_BYTES);

        let author = between(block, "<span class='author'>", "</span>")
            .map(|raw| raw.trim().to_string());
        let text = between(block, "</span> ", "</span>").and_then(quoted);

        if let (Some(author), Some(text)) = (author, text) {
            let author = author
                .strip_suffix(" says")
                .unwrap_or(&author)
                .trim()
                .to_string();
            let guide_title = between(block, "target=\"_blank\">", "</a>")
                .map(|raw| raw.trim().to_string());
            let author_role = between(block, "</a>", "</span>")
                .and_then(|raw| raw.split('|').nth(1))
                .map(|role| role.trim().to_string())
                .filter(|role| !role.is_empty());

            tips.push(PlayerTip {
                author: decode_entities(&author),
                text: clean_tip_text(&decode_entities(&text)),
                guide_title: guide_title.map(|raw| decode_entities(&raw)),
                author_role: author_role.map(|role| decode_entities(&role)),
            });
        }

        cursor = block_start + TIP_MARKER.len();
    }

    tips
}

/// Recorta `text` a lo sumo a `max_bytes` **sin partir un char UTF-8**:
/// si el límite cae adentro de un carácter multi-byte (las comillas
/// tipográficas “” del sitio ocupan 3 bytes, los acentos 2), retrocede
/// hasta el boundary anterior. Nunca paniquea, sea cual sea el input.
#[must_use]
fn truncate_safe(text: &str, max_bytes: usize) -> &str {
    if text.len() <= max_bytes {
        return text;
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    // Seguro: `end` es boundary (o 0, que siempre lo es).
    &text[..end]
}

/// Contenido entre dos marcadores literales (primera ocurrencia de cada
/// uno, en orden).
fn between<'a>(html: &'a str, open: &str, close: &str) -> Option<&'a str> {
    let start = html.find(open)? + open.len();
    let end = start + html.get(start..)?.find(close)?;
    html.get(start..end)
}

/// Limpia el marcado de links del sitio (`[[champ]]`, `[[item]]` → texto
/// plano) que counterstats deja en el cuerpo de los tips (puro, testeado).
#[must_use]
pub fn clean_tip_text(text: &str) -> String {
    text.replace("[[", "").replace("]]", "")
}

/// Texto entre comillas tipográficas `“…”` (el contenido real del consejo).
fn quoted(raw: &str) -> Option<String> {
    let open = '\u{201C}';
    let close = '\u{201D}';
    let start = raw.find(open)? + open.len_utf8();
    let end = start + raw.get(start..)?.find(close)?;
    let text = raw.get(start..end)?.trim();
    (!text.is_empty()).then(|| text.to_string())
}

// ---------------------------------------------------------------------------
// Cache en memoria: una descarga por slug por corrida de la app.
// ---------------------------------------------------------------------------

/// Cache global (slug → tips). Envenenada → miss y re-descarga.
fn tips_cache() -> &'static Mutex<HashMap<String, Vec<PlayerTip>>> {
    static TIPS_CACHE: OnceLock<Mutex<HashMap<String, Vec<PlayerTip>>>> = OnceLock::new();
    TIPS_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Descarga (o sirve de cache) los consejos de jugadores del campeón.
/// El UA de navegador es requisito del sitio (verificado en vivo).
pub async fn fetch_player_tips_cached(
    client: &reqwest::Client,
    slug: &str,
) -> Result<Vec<PlayerTip>, SourceError> {
    if let Ok(cache) = tips_cache().lock() {
        if let Some(tips) = cache.get(slug) {
            return Ok(tips.clone());
        }
    }

    let response = client
        .get(counterstats_url(slug))
        .header(
            reqwest::header::USER_AGENT,
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0 Safari/537.36",
        )
        .send()
        .await
        .map_err(|error| SourceError::Lolalytics(format!("counterstats: {error}")))?;
    if !response.status().is_success() {
        return Err(SourceError::Lolalytics(format!(
            "counterstats http {}",
            response.status()
        )));
    }
    let html = response
        .text()
        .await
        .map_err(|error| SourceError::Lolalytics(format!("counterstats body: {error}")))?;
    let tips = parse_player_tips(&html);

    if let Ok(mut cache) = tips_cache().lock() {
        cache.insert(slug.to_string(), tips.clone());
    }
    Ok(tips)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fixture: recorte REAL de la página de Kayn (2026-10-17), dos bloques
    /// consecutivos con guía y rol.
    const FIXTURE: &str = r#"
<div class="champ-box__tips__wrap jungle" data-lane="jungle">
    <div class="tip-box" target="_blank" >
        <span class='tip-box__tip'><span class='author'>Stkfear says</span> “Is a good matchup if he goes blue, can litteraly cleanse all of his dmg.”</span>
        <div class="tip-box__divider"></div>
        <div class="tip-box__guide-wrap">
            <img src="https://www.mobafire.com/images/champion/square/rengar.png">
            <a href="https://www.mobafire.com/league-of-legends/build/rengar-jng-26-20-build-master-653727" rel="nofollow" target="_blank">Rengar Jng 26.20 Build Master+</a>
            <span> by <strong>Stkfear</strong> |  Rengar Player</span>
        </div>
    </div>
    <div class="tip-box" target="_blank" >
        <span class='tip-box__tip'><span class='author'>nanic says</span> “Invade his jungle and go vertical.”</span>
        <div class="tip-box__divider"></div>
        <div class="tip-box__guide-wrap">
            <a href="https://www.mobafire.com/league-of-legends/build/talon-guide" rel="nofollow" target="_blank">Mestre+ Talon Guide</a>
            <span> by <strong>nanic</strong> |  Talon Player</span>
        </div>
    </div>
"#;

    #[test]
    fn parsea_el_fixture_real_completo() {
        let tips = parse_player_tips(FIXTURE);

        assert_eq!(tips.len(), 2);
        assert_eq!(tips[0].author, "Stkfear");
        assert_eq!(
            tips[0].text,
            "Is a good matchup if he goes blue, can litteraly cleanse all of his dmg."
        );
        assert_eq!(
            tips[0].guide_title.as_deref(),
            Some("Rengar Jng 26.20 Build Master+")
        );
        assert_eq!(tips[0].author_role.as_deref(), Some("Rengar Player"));

        assert_eq!(tips[1].author, "nanic");
        assert_eq!(tips[1].author_role.as_deref(), Some("Talon Player"));
    }

    /// Sin bloques de tips → vacío (la UI muestra su estado honesto).
    #[test]
    fn html_sin_tips_devuelve_vacio() {
        assert!(parse_player_tips("<html><body>nope</body></html>").is_empty());
        assert!(parse_player_tips("").is_empty());
    }

    /// Bloque sin texto entre comillas o sin autor se descarta sin panic.
    #[test]
    fn bloques_incompletos_se_descartan() {
        let roto = r#"
        <span class='tip-box__tip'><span class='author'>Solo dice</span> sin comillas acá</span>
        <span class='tip-box__tip'><span class='author'>X says</span> “ok”</span>
        "#;
        let tips = parse_player_tips(roto);
        assert_eq!(tips.len(), 1);
        assert_eq!(tips[0].text, "ok");
    }

    /// URL de página por slug.
    #[test]
    fn url_arma_por_slug() {
        assert_eq!(
            counterstats_url("kayn"),
            "https://www.counterstats.net/league-of-legends/kayn"
        );
    }

    /// Entidades HTML del sitio (el caso visible: `Vi&#039;s`) quedan
    /// decodificadas en el texto final.
    #[test]
    fn decodifica_entidades_html() {
        assert_eq!(decode_entities("Vi&#039;s dueling"), "Vi's dueling");
        assert_eq!(decode_entities("Rek&#x27;Sai"), "Rek'Sai");
        assert_eq!(decode_entities("a &amp; b &lt;c&gt;"), "a & b <c>");
        assert_eq!(decode_entities("sin entidades"), "sin entidades");
        assert_eq!(decode_entities("&desconocida; queda"), "&desconocida; queda");
        assert_eq!(decode_entities("á é í ñ"), "á é í ñ", "UTF-8 intacto");

        let tips = parse_player_tips(
            r#"<span class='tip-box__tip'><span class='author'>Vambient says</span> “Be careful for Vi&#039;s dueling.”</span>
            <div class="tip-box__guide-wrap">
            <a href="https://www.mobafire.com/build/x" rel="nofollow" target="_blank">Vambient Guide</a>
            <span> by <strong>Vambient</strong> | Rek&#039;Sai Player</span>
            </div>"#,
        );
        assert_eq!(tips[0].text, "Be careful for Vi's dueling.");
        assert_eq!(tips[0].author_role.as_deref(), Some("Rek'Sai Player"));
    }

    // ------------------------------------------------------------------
    // truncate_safe: el fix del crash real (exit 101 en champ select).
    // ------------------------------------------------------------------

    /// Límite cayendo justo adentro de una comilla tipográfica (3 bytes):
    /// exactamente el caso de producción. Retrocede al boundary y no
    /// paniquea.
    #[test]
    fn truncate_safe_limite_dentro_de_comilla_tipografica() {
        // bytes: a(0..1) b(1..2) “(2..5) c(5..6) d(6..7)
        let texto = "ab\u{201C}cd";
        assert_eq!(texto.len(), 7);

        assert_eq!(truncate_safe(texto, 3), "ab", "el byte 3 cae adentro de “");
        assert_eq!(truncate_safe(texto, 4), "ab", "el byte 4 también");
        assert_eq!(truncate_safe(texto, 5), "ab\u{201C}", "el byte 5 es boundary");
        assert_eq!(truncate_safe(texto, 6), "ab\u{201C}c");
        assert_eq!(truncate_safe(texto, 7), texto);
    }

    /// Con comillas y acentos, cualquier límite de bytes posible devuelve
    /// un prefijo válido del original (nunca un corte adentro de un char).
    #[test]
    fn truncate_safe_no_parte_ningun_char_con_acentos_y_comillas() {
        let texto = "“Vi’s dueling” — cuidado con á, é, ñ y 説";
        for max in 0..=texto.len() {
            let cortado = truncate_safe(texto, max);
            assert!(cortado.len() <= max, "max {max}: se pasó del límite");
            assert!(
                texto.starts_with(cortado),
                "max {max}: cortó adentro de un char"
            );
        }
    }

    /// Texto más corto que el máximo → se devuelve completo e intacto.
    #[test]
    fn truncate_safe_texto_corto_vuelve_completo() {
        let texto = "“tips” con acentos áé í";
        assert!(texto.len() < 4000);
        assert_eq!(truncate_safe(texto, 4000), texto);
        assert_eq!(truncate_safe("", 4000), "");
        assert_eq!(truncate_safe("corto", 0), "");
    }

    /// ASCII puro: todos los índices son boundary, así que el resultado es
    /// idéntico al slicing directo de antes.
    #[test]
    fn truncate_safe_ascii_igual_al_slicing_directo() {
        let texto = "x".repeat(10_000);
        for max in [0usize, 1, 999, 4_000, 9_999, 10_000, 20_000] {
            assert_eq!(
                truncate_safe(&texto, max),
                &texto[..max.min(texto.len())],
                "max {max}"
            );
        }
    }

    /// Réplica exacta del crash de producción: la “ arranca en el byte 3999
    /// del bloque, así que el límite viejo (`block_start + 4000`) caía
    /// adentro del char → `&html[..block_end]` paniqueaba y la app entera
    /// moría con exit 101. Ahora el bloque queda truncado en el boundary
    /// anterior y el tip incompleto se descarta sin drama.
    #[test]
    fn parse_no_paniquea_cuando_la_ventana_corta_un_multibyte() {
        let prefixo =
            "<span class='tip-box__tip'><span class='author'>X says</span> ".to_string();
        let padding = "a".repeat(3999 - prefixo.len());
        assert_eq!(prefixo.len() + padding.len(), 3999);
        let html = format!("{prefixo}{padding}\u{201C}tip real\u{201D}</span>");
        assert!(html.len() > 4000);

        let tips = parse_player_tips(&html); // antes: panic acá
        assert!(
            tips.is_empty(),
            "el bloque truncado antes de la “ no puede parsear un tip"
        );
    }
}
