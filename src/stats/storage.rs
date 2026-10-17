//! Persistencia del dataset de partidas en JSONL (una línea JSON por
//! [`MatchDetail`]).
//!
//! Diseño:
//!
//! - **Path inyectable**: [`JsonlStatsStore::new`] recibe el path completo,
//!   así los tests usan directorios temporales y nunca tocan `./data` real.
//!   El path por defecto que arma el builder es [`data_dir`] +
//!   [`MATCHES_FILE_NAME`].
//! - **Índice en memoria**: al abrir se lee el archivo y se arma el set de
//!   `metadata.match_id` ya presentes. `append_matches` solo escribe los
//!   que no están (dedup O(1) por match) y el sync de la FASE 8b consulta
//!   [`JsonlStatsStore::contains`] para cortar la paginación apenas llega a
//!   historia ya conocida.
//! - **Append incremental**: buffered write + flush al final de cada
//!   llamada; si la app muere a mitad del sync, lo ya bajado queda en
//!   disco. El índice en memoria solo se actualiza tras un flush exitoso,
//!   así un fallo de IO nunca deja la memoria y el disco en estados
//!   distintos.
//! - **Tolerancia**: una línea corrupta o vacía se loguea y se saltea
//!   (nunca invalida el dataset completo), y `load_all` deduplica por
//!   `match_id` (gana la primera aparición) como defensa ante writes
//!   parciales de un crash.
//!
//! Errores: como en el resto del proyecto, no hay `unwrap()` en rutas de
//! usuario; los fallos de IO se loguean y las consultas devuelven lo que se
//! pudo leer. El store nunca paniquea.

use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use crate::riot_api::models::MatchDetail;

/// Variable de entorno que sobreescribe el directorio de datos.
pub const DATA_DIR_ENV: &str = "KLIPY_DATA_DIR";
/// Directorio de datos por defecto, relativo al directorio de trabajo.
pub const DEFAULT_DATA_DIR: &str = "data";
/// Nombre del archivo del dataset dentro del directorio de datos.
pub const MATCHES_FILE_NAME: &str = "matches.jsonl";

/// Directorio de datos del dataset: `KLIPY_DATA_DIR` si está definida (y no
/// es vacía), si no `./data` relativo al directorio de trabajo de la app.
///
/// El override existe para tests y para correr múltiples datasets en paralelo
/// sin mezclarlos.
#[must_use]
pub fn data_dir() -> PathBuf {
    data_dir_from_env(std::env::var(DATA_DIR_ENV).ok())
}

/// Variante pura de [`data_dir`] (testeable sin tocar el entorno del
/// proceso): mapea el valor crudo de la variable al directorio efectivo.
#[must_use]
pub fn data_dir_from_env(raw: Option<String>) -> PathBuf {
    match raw {
        Some(dir) if !dir.trim().is_empty() => PathBuf::from(dir),
        _ => PathBuf::from(DEFAULT_DATA_DIR),
    }
}

/// Store de partidas del dataset.
///
/// Abstrae el formato de persistencia para que el builder (FASE 8b) y los
/// futuros consumers de agregación no se casen con JSONL.
pub trait StatsStore {
    /// Agrega las partidas que todavía no están en el dataset y devuelve
    /// cuántas nuevas se escribieron. Nunca re-appendea un `match_id`
    /// existente.
    fn append_matches(&mut self, matches: &[MatchDetail]) -> usize;

    /// Lee todo el dataset. Las líneas corruptas se saltean (con log) y el
    /// resultado no contiene `match_id` duplicados.
    fn load_all(&self) -> Vec<MatchDetail>;
}

/// Implementación JSONL de [`StatsStore`]: una línea JSON por partida.
///
/// No hay locking interno: el builder lo usa desde una única tarea, y el
/// `&mut self` de [`StatsStore::append_matches`] impide concurrencia a nivel
/// de tipos.
pub struct JsonlStatsStore {
    /// Path completo del archivo JSONL (inyectado para testear).
    path: PathBuf,
    /// Índice `match_id → presente`, cargado al abrir (y actualizado solo
    /// tras escrituras exitosas).
    index: HashSet<String>,
}

impl JsonlStatsStore {
    /// Abre (conceptualmente) el dataset en `path`: lee lo existente para
    /// armar el índice de dedup. Crea los directorios padres si faltan y no
    /// crea el archivo hasta el primer append (un dataset vacío no deja
    /// basura en disco). Nunca falla: los problemas de IO se loguean y se
    /// ven como dataset vacío.
    #[must_use]
    pub fn new(path: PathBuf) -> Self {
        if let Some(parent) = path.parent() {
            if let Err(error) = fs::create_dir_all(parent) {
                eprintln!(
                    "klipy [stats]: no se pudo crear el directorio de datos {}: {error}",
                    parent.display()
                );
            }
        }

        let index = load_index(&path);
        Self { path, index }
    }

    /// Path del archivo JSONL.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// ¿Ya está esta partida en el dataset? Consulta el índice en memoria
    /// (O(1)): el builder la usa para cortar la paginación del historial.
    #[must_use]
    pub fn contains(&self, match_id: &str) -> bool {
        self.index.contains(match_id)
    }

    /// Cantidad de partidas indexadas (las legibles del archivo).
    #[must_use]
    pub fn len(&self) -> usize {
        self.index.len()
    }

    /// Dataset sin partidas.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }
}

impl StatsStore for JsonlStatsStore {
    fn append_matches(&mut self, matches: &[MatchDetail]) -> usize {
        let fresh: Vec<&MatchDetail> = matches
            .iter()
            .filter(|detail| !self.contains(&detail.metadata.match_id))
            .collect();
        if fresh.is_empty() {
            return 0;
        }

        let mut added = 0_usize;
        // Apertura diferida: solo abre el archivo si hay algo nuevo que
        // escribir. Un fallo de apertura/escritura se loguea y devuelve 0:
        // el índice en memoria no cambia, así el próximo reintento vuelve a
        // intentar las mismas partidas.
        let result = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|error| error.to_string())
            .and_then(|file| {
                let mut writer = BufWriter::new(file);
                for detail in &fresh {
                    write_match_line(&mut writer, detail)?;
                    added += 1;
                }
                writer.flush().map_err(|error| format!("flush: {error}"))
            });

        match result {
            Ok(()) => {
                for detail in &fresh {
                    self.index.insert(detail.metadata.match_id.clone());
                }
                added
            }
            Err(error) => {
                eprintln!(
                    "klipy [stats]: no se pudo escribir en {}: {error} ({} partidas no persistidas)",
                    self.path.display(),
                    fresh.len()
                );
                0
            }
        }
    }

    fn load_all(&self) -> Vec<MatchDetail> {
        read_matches(&self.path)
    }
}

/// Serializa una partida como línea JSONL (JSON + `\n`). El error va como
/// string porque mezcla dos orígenes (`serde_json` y IO) y solo se loguea.
fn write_match_line<W: Write>(writer: &mut W, detail: &MatchDetail) -> Result<(), String> {
    let line = serde_json::to_string(detail).map_err(|error| error.to_string())?;
    writer
        .write_all(line.as_bytes())
        .and_then(|()| writer.write_all(b"\n"))
        .map_err(|error| error.to_string())
}

/// Lee el archivo y arma el índice de `match_id` presentes (salteando líneas
/// corruptas/vacías y deduplicando: gana la primera aparición).
fn load_index(path: &Path) -> HashSet<String> {
    let mut index = HashSet::new();
    for detail in read_matches(path) {
        index.insert(detail.metadata.match_id);
    }
    index
}

/// Lee y parsea todas las partidas del archivo. Línea corrupta o vacía → log
/// + skip; archivo ausente → dataset vacío; `match_id` repetido → solo la
///   primera aparición (defensa ante writes parciales de un crash).
fn read_matches(path: &Path) -> Vec<MatchDetail> {
    let Ok(mut file) = File::open(path) else {
        return Vec::new();
    };

    let mut contents = String::new();
    if let Err(error) = file.read_to_string(&mut contents) {
        eprintln!("klipy [stats]: no se pudo leer {}: {error}", path.display());
        return Vec::new();
    }

    let mut matches = Vec::new();
    let mut seen = HashSet::new();
    for (line_no, line) in contents.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match serde_json::from_str::<MatchDetail>(trimmed) {
            // Línea OK: solo entra si su match_id no apareció antes.
            Ok(detail) if seen.insert(detail.metadata.match_id.clone()) => {
                matches.push(detail);
            }
            Ok(_) => {
                eprintln!(
                    "klipy [stats]: {}: línea {} con match_id duplicado, se ignora",
                    path.display(),
                    line_no + 1
                );
            }
            Err(error) => {
                eprintln!(
                    "klipy [stats]: {}: línea {} corrupta, se saltea ({error})",
                    path.display(),
                    line_no + 1
                );
            }
        }
    }
    matches
}

/// Directorio temporal único para los tests (pid + contador: sin colisiones
/// entre tests ni entre corridas). Nunca usa `./data`.
#[cfg(test)]
pub(crate) fn temp_data_dir(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("klipy-stats-{tag}-{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("temp dir se crea");
    dir
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lcu::models::Position;
    use crate::riot_api::models::{MatchInfo, MatchMetadata, Participant, Perks, StatPerks};

    /// Partida de prueba con `count` participantes con perks idénticos.
    fn fixture(match_id: &str, champion_id: i64, win: bool) -> MatchDetail {
        MatchDetail {
            metadata: MatchMetadata {
                match_id: match_id.to_string(),
            },
            info: MatchInfo {
                game_version: String::from("16.7.623.1234"),
                participants: vec![Participant {
                    champion_id,
                    champion_name: Some(String::from("Ahri")),
                    team_id: 100,
                    win,
                    team_position: Some(Position::Middle),
                    perks: Some(Perks {
                        stat_perks: Some(StatPerks {
                            offense: 5005,
                            flex: 5008,
                            defense: 5002,
                        }),
                        ..Perks::default()
                    }),
                    ..Participant::default()
                }],
                ..MatchInfo::default()
            },
        }
    }

    #[test]
    fn append_load_y_dedup_por_match_id() {
        let dir = temp_data_dir("dedup");
        let path = dir.join(MATCHES_FILE_NAME);
        let mut store = JsonlStatsStore::new(path.clone());

        assert!(store.is_empty());

        let a = fixture("LA1_1", 103, true);
        let b = fixture("LA1_2", 412, false);

        // Primera pasada: entran las dos.
        assert_eq!(store.append_matches(&[a.clone(), b.clone()]), 2);
        assert_eq!(store.len(), 2);
        assert!(store.contains("LA1_1"));
        assert!(store.contains("LA1_2"));

        // Segunda pasada idéntica: cero nuevas (dedup).
        assert_eq!(store.append_matches(&[a.clone(), b.clone()]), 0);

        // Mezcla: una vieja + una nueva → solo la nueva se escribe.
        let c = fixture("LA1_3", 22, true);
        assert_eq!(store.append_matches(&[b, c]), 1);
        assert_eq!(store.len(), 3);

        // Reabrir el store reconstruye el índice desde el archivo.
        let reopened = JsonlStatsStore::new(path);
        assert_eq!(reopened.len(), 3);

        // El orden del archivo se conserva en load_all.
        let all = reopened.load_all();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].metadata.match_id, "LA1_1");
        assert_eq!(all[1].metadata.match_id, "LA1_2");
        assert_eq!(all[2].metadata.match_id, "LA1_3");

        // Roundtrip fiel: lo leído es igual al fixture original.
        assert_eq!(all[0], a);

        let _ = fs::remove_dir_all(dir);
    }

    /// Línea corrupta (y líneas en blanco) se saltean con log, sin invalidar
    /// el resto del dataset; el índice permite seguir appendeando encima.
    #[test]
    fn linea_corrupta_se_saltea() {
        let dir = temp_data_dir("corrupta");
        let path = dir.join(MATCHES_FILE_NAME);

        // Sembrar el archivo a mano: válido + corrupto + en blanco + válido.
        let a = fixture("LA1_1", 103, true);
        let b = fixture("LA1_2", 412, false);
        let mut contents = String::new();
        contents.push_str(&serde_json::to_string(&a).expect("serializa"));
        contents.push('\n');
        contents.push_str("{ esto no es json");
        contents.push('\n');
        contents.push('\n'); // línea en blanco
        contents.push_str(&serde_json::to_string(&b).expect("serializa"));
        contents.push('\n');
        fs::write(&path, contents).expect("siembra el archivo");

        let mut store = JsonlStatsStore::new(path);
        assert_eq!(store.len(), 2);
        assert!(!store.contains("LA1_3"));

        let all = store.load_all();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].metadata.match_id, "LA1_1");
        assert_eq!(all[1].metadata.match_id, "LA1_2");

        // Seguir appendeando funciona pese a la línea corrupta.
        let c = fixture("LA1_3", 22, true);
        assert_eq!(store.append_matches(&[c]), 1);
        assert_eq!(store.load_all().len(), 3);

        let _ = fs::remove_dir_all(dir);
    }

    /// `match_id` duplicado en el archivo (defensa ante un write parcial de
    /// un crash): `load_all` y el índice se quedan con la primera aparición,
    /// y un re-append del mismo id no duplica nada.
    #[test]
    fn match_id_duplicado_en_archivo_gana_el_primero() {
        let dir = temp_data_dir("duplicado");
        let path = dir.join(MATCHES_FILE_NAME);

        let a = fixture("LA1_1", 103, true);
        let line = serde_json::to_string(&a).expect("serializa");
        fs::write(&path, format!("{line}\n{line}\n")).expect("siembra");

        let mut store = JsonlStatsStore::new(path);
        assert_eq!(store.len(), 1);
        assert_eq!(store.load_all().len(), 1);
        assert_eq!(store.append_matches(&[a]), 0);

        let _ = fs::remove_dir_all(dir);
    }

    /// Dataset vacío inexistente: abrir no crea el archivo y append sí (con
    /// los directorios padres si faltan).
    #[test]
    fn store_nuevo_no_crea_el_archivo_hasta_el_primer_append() {
        let dir = temp_data_dir("nuevo");
        let nested = dir.join("sub").join("dir");
        let path = nested.join(MATCHES_FILE_NAME);

        let mut store = JsonlStatsStore::new(path.clone());
        assert!(store.is_empty());
        assert_eq!(store.load_all(), Vec::new());
        assert!(!path.exists(), "no debe crear el archivo vacío");

        assert_eq!(store.append_matches(&[fixture("LA1_9", 103, false)]), 1);
        assert!(path.exists());

        let _ = fs::remove_dir_all(dir);
    }

    /// El trait se usa como objeto de tipo dinámico (el builder podría
    /// aceptar cualquier store a futuro).
    #[test]
    fn trait_como_dyn_funciona() {
        let dir = temp_data_dir("dyn");
        let mut store: Box<dyn StatsStore> =
            Box::new(JsonlStatsStore::new(dir.join(MATCHES_FILE_NAME)));

        assert_eq!(store.append_matches(&[fixture("LA1_1", 103, true)]), 1);
        assert_eq!(store.append_matches(&[fixture("LA1_1", 103, true)]), 0);
        assert_eq!(store.load_all().len(), 1);

        let _ = fs::remove_dir_all(dir);
    }

    /// Resolución del directorio de datos: override con la variable, vacío o
    /// ausente → `./data`.
    #[test]
    fn data_dir_desde_la_variable_de_entorno() {
        assert_eq!(
            data_dir_from_env(Some(String::from("/tmp/klipy-datos"))),
            PathBuf::from("/tmp/klipy-datos")
        );
        assert_eq!(
            data_dir_from_env(Some(String::from("  "))),
            PathBuf::from(DEFAULT_DATA_DIR)
        );
        assert_eq!(data_dir_from_env(None), PathBuf::from(DEFAULT_DATA_DIR));
        assert_eq!(DEFAULT_DATA_DIR, "data");
        assert_eq!(DATA_DIR_ENV, "KLIPY_DATA_DIR");
        assert_eq!(MATCHES_FILE_NAME, "matches.jsonl");
    }
}
