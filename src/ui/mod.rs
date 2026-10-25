//! Componentes de interfaz.
//!
//! - [`shell`]: header KLIFY + footer HUD compartidos por las tres vistas.
//! - [`vivo`]: vista VIVO (espejo de la partida en curso vía Live Client
//!   Data API); se visita desde la sidebar.
//! - [`waiting`]: vista "SIN CLIENTE" (pantalla de espera del prototipo).
//! - [`profile`]: vista PERFIL (hero + top champs + historial + modales).
//! - [`champ_select`]: vista champ select rediseñada (FASE 11b-2a): equipo
//!   en vivo con splashes, hero con métricas reales y tabs HABILIDADES /
//!   RUNAS (items y counters llegan en 11b-2b, submódulos
//!   `champ_select/{skills_panel,runes_panel}`).
//! - [`match_detail`]: modal de detalle de partida (fetch on-demand por
//!   [`crate::profile::client_handle`]).
//! - [`format`]: formateo puro (duraciones, "hace X", KDA/WR, miles,
//!   strip_html de descripciones).
//! - [`recommendations`]: render legacy de FASE 10 (sin uso desde 11b-2a;
//!   conserva la capa pura de páginas de runas que el champ select reusa).
//! - [`champ_display`]: deprecado desde 11b-2a (reemplazado por las cards
//!   del champ select).

pub mod champ_display;
pub mod champ_select;
pub mod format;
pub mod match_detail;
pub mod menu;
pub mod profile;
pub mod recommendations;
pub mod shell;
pub mod vivo;
pub mod waiting;
