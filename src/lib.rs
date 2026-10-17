//! klipy: companion desktop para League of Legends.
//!
//! - FASE 2: conexión LCU real (detección de cliente + champ select por
//!   websocket, con GET inicial REST).
//! - FASE 3: catálogo Data Dragon (`data_dragon`) para nombres e imágenes
//!   reales, con placeholders como fallback mientras no carga.
//! - FASE 6: contexto de partida (`lcu::models::MatchContext`) con ambos
//!   equipos, posiciones asignadas y pick local, expuesto como señal en vivo
//!   (base del futuro recomendador de runas).
//! - FASE 7: catálogo de runas (`data_dragon::runes`) pineado al parche que
//!   reporta el cliente (`/lol-game-data/v1/version`), con fallback a la
//!   versión más reciente de Data Dragon si no hay cliente.
//! - FASE 8a: cliente de la Riot API oficial (`riot_api`, Match-V5) para el
//!   dataset propio de estadísticas de runas, más el puuid del invocador
//!   actual expuesto como señal desde el loop LCU.
//! - FASE 8b: dataset de estadísticas (`stats`): persistencia JSONL con
//!   dedup, agregación pura a filas de runas (`RuneStatRow`), winrate
//!   suavizado para el scoring futuro y la tarea `builder::run_dataset_sync`
//!   que recorre el historial ranked del invocador con la Riot API
//!   (deshabilitada limpio sin `RIOT_API_KEY`).
//! - FASE 9: adapters de estadísticas de runas desde fuentes públicas
//!   (`stats::source`): Lolalytics (primaria) con fallback a U.GG,
//!   normalizados a `RunePageStats`, con resolución de árboles contra el
//!   catálogo de Data Dragon, cache en memoria y una tarea de validación
//!   que loguea las páginas recomendadas durante el champ select.
//! - FASE 11a-2: perfil del invocador (`profile`) 100% por LCU (sin gastar
//!   la API key de Riot): rank solo-queue, top maestría, historial reciente
//!   y detalle de partida on-demand (rutas verificadas en vivo), agregados
//!   en `ProfileSummary` publicado como señal por `run_profile_loop`, y
//!   settings del fondo de splash persistidos en `settings.json`.

pub mod app;
pub mod data_dragon;
pub mod lcu;
pub mod live;
pub mod profile;
pub mod riot_api;
pub mod stats;
pub mod ui;

pub use app::App;
