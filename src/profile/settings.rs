//! Settings del perfil (FASE 11a-2): el fondo de pantalla elegido para la
//! vista del perfil, persistido en `settings.json` dentro del directorio de
//! datos ([`crate::stats::storage::data_dir`], override `KLIPY_DATA_DIR`).
//!
//! Diseño:
//!
//! - **Path inyectable** (mismo criterio que `stats::storage`): `load`
//!   y `save` reciben el path completo, así los tests usan directorios
//!   temporales y NUNCA escriben en `./data` real.
//! - **Tolerante**: un archivo ausente (primera corrida) o corrupto cae al
//!   default (`background_splash: None`) con un log; los settings nunca
//!   rompen la app.
//! - **Formato del valor**: la clave de splash de Data Dragon
//!   (`"{ddragon_id}_{num}"`, ej. `"Kayn_1"`). [`ProfileSettings::
//!   background_url`] la expande a la URL pública de la CDN
//!   (`https://ddragon.leagueoflegends.com/cdn/img/champion/splash/
//!   {clave}.jpg`, verificado 200 en vivo). `None` → la UI 11b usa su
//!   fondo por defecto.

use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::data_dragon::champion_detail::splash_key_url;

/// Nombre del archivo de settings dentro del directorio de datos.
pub const SETTINGS_FILE_NAME: &str = "settings.json";

/// Path completo de los settings en producción: `data_dir()/settings.json`
/// (o el directorio del override `KLIPY_DATA_DIR`). Los tests NO usan esta
/// función: pasan paths temporales a [`load_settings`]/[`save_settings`].
#[must_use]
pub fn settings_path() -> PathBuf {
    crate::stats::storage::data_dir().join(SETTINGS_FILE_NAME)
}

/// Settings del perfil.
///
/// Deriva `Serialize`/`Deserialize` (camelCase, tolerante: archivo sin el
/// campo → default) para el roundtrip a `settings.json`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileSettings {
    /// Clave de splash elegida como fondo (`"Kayn_1"`); `None` = sin
    /// elección explícita → la UI usa su fondo por defecto.
    #[serde(default)]
    pub background_splash: Option<String>,
}

impl ProfileSettings {

    /// URL pública del splash de fondo para el valor guardado, o `None` si
    /// no hay elección (la UI usa su default).
    #[must_use]
    pub fn background_url(&self) -> Option<String> {
        self.background_splash
            .as_deref()
            .map(str::trim)
            .filter(|key| !key.is_empty())
            .map(splash_key_url)
    }

    /// Nueva settings con el splash dado (helper para la UI 11b).
    #[must_use]
    pub fn with_background_splash(splash: impl Into<String>) -> Self {
        Self {
            background_splash: Some(splash.into()),
        }
    }
}

/// Lee los settings del path dado. Archivo ausente o corrupto → default
/// (con log en el caso corrupto, que sí es una anomalía visible).
#[must_use]
pub fn load_settings(path: &Path) -> ProfileSettings {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        // No existe todavía: primera corrida, es el caso normal.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return ProfileSettings::default();
        }
        Err(error) => {
            eprintln!("klipy [profile]: no se pudo leer {}: {error}; usando defaults", path.display());
            return ProfileSettings::default();
        }
    };

    match serde_json::from_str(&raw) {
        Ok(settings) => settings,
        Err(error) => {
            eprintln!(
                "klipy [profile]: aviso: {} corrupto ({error}); usando defaults",
                path.display()
            );
            ProfileSettings::default()
        }
    }
}

/// Escribe los settings al path dado (crea el directorio padre si falta;
/// buffered write + flush, mismo criterio que el store JSONL). Devuelve el
/// error de IO al caller: la UI 11b decide qué informar.
///
/// # Errors
/// Error de IO al crear directorios, crear el archivo o escribir.
pub fn save_settings(path: &Path, settings: &ProfileSettings) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let file = File::create(path)?;
    let mut writer = BufWriter::new(file);
    serde_json::to_writer_pretty(&mut writer, settings)?;
    writer.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Directorio temporal único por test: NUNCA `./data` (el spec lo
    /// prohíbe para tests). Se limpia best-effort al crearlo.
    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "klipy_profile_settings_{}_{}",
            std::process::id(),
            tag
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    /// Roundtrip completo en un directorio temporal: save → load devuelve
    /// lo mismo, y el JSON usa camelCase.
    #[test]
    fn settings_hacen_roundtrip_en_temp_dir() {
        let dir = temp_dir("roundtrip");
        let path = dir.join(SETTINGS_FILE_NAME);

        let settings = ProfileSettings::with_background_splash("Kayn_1");
        save_settings(&path, &settings).expect("guarda");

        let loaded = load_settings(&path);
        assert_eq!(loaded, settings);
        assert_eq!(loaded.background_splash.as_deref(), Some("Kayn_1"));

        // El archivo es JSON legible con el campo en camelCase.
        let raw = fs::read_to_string(&path).expect("lee el archivo");
        assert!(raw.contains("\"backgroundSplash\": \"Kayn_1\""), "json: {raw}");

        let _ = fs::remove_dir_all(&dir);
    }

    /// Archivo inexistente (primera corrida) → default sin error.
    #[test]
    fn load_sin_archivo_da_default() {
        let dir = temp_dir("missing");
        let settings = load_settings(&dir.join(SETTINGS_FILE_NAME));

        assert_eq!(settings, ProfileSettings::default());
        assert_eq!(settings.background_url(), None);
    }

    /// Archivo corrupto → default (con log), nunca pánico.
    #[test]
    fn load_con_archivo_corrupto_da_default() {
        let dir = temp_dir("corrupt");
        fs::create_dir_all(&dir).expect("crea el dir");
        let path = dir.join(SETTINGS_FILE_NAME);
        fs::write(&path, "{ esto no es json").expect("escribe basura");

        assert_eq!(load_settings(&path), ProfileSettings::default());

        let _ = fs::remove_dir_all(&dir);
    }

    /// Un settings.json viejo sin el campo → default tolerante (serde con
    /// default), y un JSON `null` en el campo también.
    #[test]
    fn load_tolerante_a_payloads_parciales() {
        let dir = temp_dir("partial");
        fs::create_dir_all(&dir).expect("crea el dir");
        let path = dir.join(SETTINGS_FILE_NAME);

        fs::write(&path, "{}").expect("escribe vacío");
        assert_eq!(load_settings(&path), ProfileSettings::default());

        fs::write(&path, r#"{ "backgroundSplash": null }"#).expect("escribe null");
        assert_eq!(load_settings(&path).background_splash, None);

        let _ = fs::remove_dir_all(&dir);
    }

    /// `background_url` arma la URL pública de la CDN para el valor; vacío
    /// o `None` → `None` (la UI usa su default).
    #[test]
    fn background_url_arma_la_url_del_splash() {
        assert_eq!(
            ProfileSettings::with_background_splash("Kayn_1").background_url(),
            Some(String::from(
                "https://ddragon.leagueoflegends.com/cdn/img/champion/splash/Kayn_1.jpg"
            ))
        );

        // Espacios de más se recortan.
        assert_eq!(
            ProfileSettings::with_background_splash("  Veigar_3  ").background_url(),
            Some(String::from(
                "https://ddragon.leagueoflegends.com/cdn/img/champion/splash/Veigar_3.jpg"
            ))
        );

        // Vacío o None → None.
        assert_eq!(
            ProfileSettings::with_background_splash("").background_url(),
            None
        );
        assert_eq!(ProfileSettings::default().background_url(), None);
    }

    /// `save_settings` crea el directorio padre si falta (la UI puede
    /// guardar antes de que exista `data/`).
    #[test]
    fn save_crea_el_directorio_padre() {
        let dir = temp_dir("mkdir");
        let path = dir.join("anidado").join(SETTINGS_FILE_NAME);

        save_settings(&path, &ProfileSettings::default()).expect("guarda creando dirs");
        assert!(path.exists());

        let _ = fs::remove_dir_all(&dir);
    }
}
