//! Dataset propio de estadísticas de runas (FASE 8b).
//!
//! Piezas:
//!
//! - [`storage`]: persistencia JSONL del dataset de partidas
//!   ([`JsonlStatsStore`], path inyectable; `data_dir()` con override
//!   `KLIPY_DATA_DIR`).
//! - [`aggregation`]: agregación pura de partidas a filas de estadísticas
//!   ([`RuneStatRow`], una por `campeón + posición + parche + runas`), con
//!   el helper de consulta [`rows_for`] y [`patch_of`].
//! - [`confidence`]: winrate suavizado ([`smoothed_winrate`]) y el umbral
//!   [`MIN_GAMES`], base del scoring de fases siguientes.
//! - [`builder`]: la tarea [`run_dataset_sync`] que recorre el historial
//!   ranked del invocador con [`crate::riot_api`] y lo persiste
//!   incrementalmente (una vez por puuid; deshabilitada limpio sin key).
//! - [`source`] (FASE 9/10): adapters de estadísticas de runas desde fuentes
//!   públicas con fallback — Lolalytics (primaria) y U.GG — normalizados a
//!   [`source::RunePageStats`], cache en memoria por
//!   `(champ, posición, parche)` y la tarea [`source::run_recommender`]
//!   (FASE 10) que publica [`source::RecommendationsState`] y loguea las
//!   páginas al entrar a champ select.
//!
//! Fuera de alcance acá: generación de páginas candidatas y scoring
//! (fases siguientes); este módulo solo produce y consulta el dataset.

pub mod aggregation;
pub mod builder;
pub mod confidence;
pub mod source;
pub mod storage;

pub use aggregation::{aggregate, patch_of, rows_for, RuneStatKey, RuneStatRow};
pub use confidence::{is_reliable, smoothed_winrate, MIN_GAMES, PRIOR_ALPHA, PRIOR_BETA};
pub use source::{
    fetch_recommended_pages, fetch_recommended_pages_cached, resolve_style_id, run_recommender,
    CacheKey, PageTarget, RecommendationsState, RunePageStats, SourceError, SourceKind,
    STYLE_BY_INDEX,
};
pub use storage::{data_dir, JsonlStatsStore, StatsStore};
