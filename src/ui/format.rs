//! Formateo puro de datos para la UI (FASE 11b-1).
//!
//! Funciones sin estado ni IO, todas testeadas: duraciones `mm:ss`,
//! timestamps "hace X", KDA/WR, miles con separador y resolución de URLs de
//! imagen (icono de perfil, splash de fondo). La lógica vive acá —no en el
//! RSX— para poder testearla sin render.

use crate::data_dragon::champion_detail::splash_key_url;
use crate::data_dragon::models::CDN_BASE_URL;

/// Splash de fondo por defecto de la vista perfil (Kayn Rhaast, clave de
/// settings; se guarda así para que el roundtrip de `ProfileSettings` y el
/// default sean el mismo valor).
pub const DEFAULT_BACKGROUND_SPLASH: &str = "Kayn_1";

/// Quita las etiquetas HTML de una descripción de Data Dragon / LCU y colapsa
/// los espacios (puro, testeado).
///
/// Las descripciones de runas (`shortDesc`), hechizos y shards traen HTML
/// liviano de tooltips del cliente: `<lol-uikit-tooltipped-keyword>`,
/// `<font color='...'>`, `<scaleHealth>`, `<br>`, ... El texto visible se
/// conserva tal cual; los whitespace consecutivos (incluidos saltos) se
/// colapsan a un espacio porque las descripciones se muestran como párrafo.
#[must_use]
pub fn strip_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_tag = false;
    let mut last_was_space = true; // trims el arranque de paso
    for ch in text.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if c.is_whitespace() => {
                if !last_was_space {
                    out.push(' ');
                    last_was_space = true;
                }
            }
            c if !in_tag => {
                out.push(c);
                last_was_space = false;
            }
            _ => {}
        }
    }
    out.trim_end().to_string()
}

/// Etiqueta de recurso de un hechizo para la ficha de la UI (puro, testeado).
///
/// Data Dragon manda `costType: " {{ abilityresourcename }}"` en los
/// campeones con recurso nominal: el placeholder se resuelve con el
/// `partype` del detalle (ej. `"Mana"`). `"No Cost"` y otros textos literales
/// pasan tal cual; vacío → el partype, o `"—"` si tampoco hay.
#[must_use]
pub fn cost_type_label(cost_type: &str, partype: &str) -> String {
    let trimmed = cost_type.trim();
    if trimmed.is_empty() || trimmed.contains("{{") {
        let partype = partype.trim();
        return if partype.is_empty() {
            String::from("—")
        } else {
            partype.to_string()
        };
    }
    trimmed.to_string()
}

/// Iniciales de un nombre para el tile fallback (puro, testeado): primeras
/// letras de hasta dos palabras, mayúsculas. Vacío → `"?"`.
#[must_use]
pub fn initials(name: &str) -> String {
    let letters: Vec<char> = name
        .split_whitespace()
        .take(2)
        .filter_map(|word| word.chars().next())
        .collect();
    if letters.is_empty() {
        return String::from("?");
    }
    letters
        .iter()
        .flat_map(|letter| letter.to_uppercase())
        .collect()
}

/// Duración en `mm:ss` (o `h:mm:ss` si supera la hora).
#[must_use]
pub fn format_duration_mmss(total_secs: i64) -> String {
    let secs = total_secs.max(0);
    let hours = secs / 3600;
    let minutes = (secs % 3600) / 60;
    let seconds = secs % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

/// Timestamp "hace X" para la creación de una partida, contra un `now_ms`
/// inyectado (puro y testeable). Granularidad: minutos → horas → días.
/// Un timestamp futuro (reloj desincronizado) cae a "hace instantes".
#[must_use]
pub fn format_time_ago(creation_ms: i64, now_ms: i64) -> String {
    let delta_ms = (now_ms - creation_ms).max(0);
    let minutes = delta_ms / 60_000;
    if minutes < 1 {
        return String::from("hace instantes");
    }
    if minutes < 60 {
        return format!("hace {minutes} min");
    }
    let hours = minutes / 60;
    if hours < 24 {
        if hours == 1 {
            return String::from("hace 1 hora");
        }
        return format!("hace {hours} horas");
    }
    let days = hours / 24;
    if days == 1 {
        return String::from("hace 1 día");
    }
    format!("hace {days} días")
}

/// "K/D/A" de una partida (ej. `6/5/6`).
#[must_use]
pub fn format_kda(kills: i32, deaths: i32, assists: i32) -> String {
    format!("{kills}/{deaths}/{assists}")
}

/// Ratio KDA con un decimal fijo (ej. `3.3`; el prototipo siempre muestra
/// un decimal, incluso `2.0`).
#[must_use]
pub fn format_ratio(kda: f64) -> String {
    format!("{kda:.1}")
}

/// Fracción 0..1 como porcentaje con un decimal (ej. `0.581` → `"58.1%"`).
#[must_use]
pub fn format_percent(fraction: f64) -> String {
    format!("{:.1}%", fraction * 100.0)
}

/// Entero con separador de miles con punto (convención es-AR del
/// prototipo: `132460` → `"132.460"`).
#[must_use]
pub fn format_thousands(value: i64) -> String {
    let digits = value.abs().to_string();
    let mut grouped = String::new();
    for (index, byte) in digits.bytes().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            grouped.push('.');
        }
        grouped.push(char::from(byte));
    }
    if value < 0 {
        format!("-{grouped}")
    } else {
        grouped
    }
}

/// Etiqueta de cola para el detalle (solo las más comunes; resto →
/// `"Cola {id}"`). El historial del LCU trae el `queueId` crudo.
#[must_use]
pub fn format_queue_name(queue_id: i64) -> String {
    match queue_id {
        400 => String::from("Normal"),
        420 => String::from("Ranked Solo"),
        440 => String::from("Ranked Flex"),
        450 => String::from("ARAM"),
        900 => String::from("URF"),
        other => format!("Cola {other}"),
    }
}

/// URL del icono de perfil para una versión; `None` si falta la versión o
/// el id (la UI muestra un tile placeholder).
#[must_use]
pub fn profile_icon_url(version: Option<&str>, icon_id: Option<i64>) -> Option<String> {
    match (version, icon_id) {
        (Some(version), Some(icon_id)) if !version.is_empty() => Some(format!(
            "{CDN_BASE_URL}/{version}/img/profileicon/{icon_id}.png"
        )),
        _ => None,
    }
}

/// URL del splash de fondo: settings con clave → URL de la CDN; sin clave
/// (o clave vacía) → el default [`DEFAULT_BACKGROUND_SPLASH`]. Es la pieza
/// que une `ProfileSettings` con la vista (pure: testeable sin archivos).
#[must_use]
pub fn background_splash_url(setting: Option<&str>) -> String {
    let key = setting
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .unwrap_or(DEFAULT_BACKGROUND_SPLASH);
    splash_key_url(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Duraciones: minutos/segundos normales, borde 0 y más de una hora.
    #[test]
    fn duracion_mmss() {
        assert_eq!(format_duration_mmss(0), "0:00");
        assert_eq!(format_duration_mmss(59), "0:59");
        assert_eq!(format_duration_mmss(60), "1:00");
        assert_eq!(format_duration_mmss(1_934), "32:14");
        assert_eq!(format_duration_mmss(3_605), "1:00:05");
        // Negativos (payload capado) no rompen.
        assert_eq!(format_duration_mmss(-5), "0:00");
    }

    /// Timestamps "hace X": instantes, minutos, horas y días, con
    /// singular/plural y clamp de timestamps futuros.
    #[test]
    fn hace_x_con_granularidad() {
        let now: i64 = 1_792_187_581_712;
        assert_eq!(format_time_ago(now, now), "hace instantes");
        assert_eq!(format_time_ago(now + 5_000, now), "hace instantes");
        assert_eq!(format_time_ago(now - 60_000, now), "hace 1 min");
        assert_eq!(format_time_ago(now - 5 * 60_000, now), "hace 5 min");
        assert_eq!(format_time_ago(now - 3_600_000, now), "hace 1 hora");
        assert_eq!(format_time_ago(now - 5 * 3_600_000, now), "hace 5 horas");
        assert_eq!(format_time_ago(now - 86_400_000, now), "hace 1 día");
        assert_eq!(format_time_ago(now - 2 * 86_400_000, now), "hace 2 días");
        assert_eq!(format_time_ago(now - 30 * 86_400_000, now), "hace 30 días");
    }

    /// KDA y ratios: formato `K/D/A` y un decimal fijo.
    #[test]
    fn kda_y_ratio() {
        assert_eq!(format_kda(6, 5, 6), "6/5/6");
        assert_eq!(format_kda(12, 4, 9), "12/4/9");
        assert_eq!(format_ratio(2.4), "2.4");
        assert_eq!(format_ratio(2.0), "2.0");
        assert_eq!(format_ratio(9.0), "9.0");
    }

    /// Porcentajes con un decimal a partir de la fracción 0..1.
    #[test]
    fn porcentaje_con_un_decimal() {
        assert_eq!(format_percent(0.581), "58.1%");
        assert_eq!(format_percent(0.6), "60.0%");
        assert_eq!(format_percent(0.0), "0.0%");
        assert_eq!(format_percent(1.0), "100.0%");
    }

    /// Miles con punto: cero, tres dígitos exactos, millones y negativos.
    #[test]
    fn miles_con_separador_es() {
        assert_eq!(format_thousands(0), "0");
        assert_eq!(format_thousands(999), "999");
        assert_eq!(format_thousands(1_000), "1.000");
        assert_eq!(format_thousands(132_460), "132.460");
        assert_eq!(format_thousands(1_234_567), "1.234.567");
        assert_eq!(format_thousands(-12_345), "-12.345");
    }

    /// Nombre de cola conocido, flexible y desconocido.
    #[test]
    fn nombre_de_cola() {
        assert_eq!(format_queue_name(400), "Normal");
        assert_eq!(format_queue_name(420), "Ranked Solo");
        assert_eq!(format_queue_name(450), "ARAM");
        assert_eq!(format_queue_name(1234), "Cola 1234");
    }

    /// URL de icono de perfil: con versión e id; `None` ante cualquier
    /// dato faltante.
    #[test]
    fn url_de_icono_de_perfil() {
        assert_eq!(
            profile_icon_url(Some("16.20.1"), Some(588)),
            Some(String::from(
                "https://ddragon.leagueoflegends.com/cdn/16.20.1/img/profileicon/588.png"
            ))
        );
        assert_eq!(profile_icon_url(None, Some(588)), None);
        assert_eq!(profile_icon_url(Some("16.20.1"), None), None);
        assert_eq!(profile_icon_url(Some(""), Some(1)), None);
    }

    /// Splash de fondo: sin settings → `Kayn_1` (default), con clave → la
    /// URL expandida; claves vacías o con espacios caen al default.
    #[test]
    fn fondo_por_default_y_por_clave() {
        assert_eq!(
            background_splash_url(None),
            "https://ddragon.leagueoflegends.com/cdn/img/champion/splash/Kayn_1.jpg"
        );
        assert_eq!(
            background_splash_url(Some("Veigar_3")),
            "https://ddragon.leagueoflegends.com/cdn/img/champion/splash/Veigar_3.jpg"
        );
        assert_eq!(
            background_splash_url(Some("  ")),
            background_splash_url(None)
        );
        assert_eq!(background_splash_url(Some("")), background_splash_url(None));
    }

    /// FASE 11b-2a: `strip_html` quita las etiquetas de tooltip del cliente
    /// y colapsa espacios; el texto visible queda intacto.
    #[test]
    fn strip_html_limpia_tooltips_reales() {
        // shortDesc REAL de un shard (LCU 16.20, verificado en vivo).
        assert_eq!(
            strip_html(
                "+8 <lol-uikit-tooltipped-keyword key='LinkTooltip_Description_CDR'>Ability Haste</lol-uikit-tooltipped-keyword>"
            ),
            "+8 Ability Haste"
        );
        // `shortDesc` real de Adaptive Force: font coloreado anidado.
        assert_eq!(
            strip_html(
                "+9 <lol-uikit-tooltipped-keyword key='LinkTooltip_Description_Adaptive'><font color='#48C4B7'>Adaptive Force</font></lol-uikit-tooltipped-keyword>"
            ),
            "+9 Adaptive Force"
        );
        // Saltos y espacios múltiples colapsan a un espacio.
        assert_eq!(
            strip_html("Ataca primero\n a un campeón:   +oro y daño extra por 3s."),
            "Ataca primero a un campeón: +oro y daño extra por 3s."
        );
        // Texto plano pasa intacto; vacío sigue vacío.
        assert_eq!(strip_html("Sin etiquetas"), "Sin etiquetas");
        assert_eq!(strip_html(""), "");
        assert_eq!(strip_html("<solo-tag/>"), "");
    }

    /// `cost_type_label`: placeholder `{{ abilityresourcename }}` → partype;
    /// literales pasan tal cual; vacíos caen a partype o `—`.
    #[test]
    fn cost_type_label_resuelve_el_placeholder_del_partype() {
        assert_eq!(cost_type_label(" {{ abilityresourcename }}", "Mana"), "Mana");
        assert_eq!(cost_type_label("{{ abilityresourcename }}", "Fury"), "Fury");
        assert_eq!(cost_type_label("No Cost", "Mana"), "No Cost");
        assert_eq!(cost_type_label("", "Blood Well"), "Blood Well");
        assert_eq!(cost_type_label("  ", ""), "—");
        assert_eq!(cost_type_label("{{ abilityresourcename }}", ""), "—");
    }

    /// Iniciales: hasta dos palabras, mayúsculas; vacío → `?`.
    #[test]
    fn iniciales_de_nombres() {
        assert_eq!(initials("Veigar"), "V");
        assert_eq!(initials("Dark Matter"), "DM");
        assert_eq!(initials("Kai'Sa"), "K");
        assert_eq!(initials("monkey king 都"), "MK");
        assert_eq!(initials(""), "?");
        assert_eq!(initials("   "), "?");
    }
}
