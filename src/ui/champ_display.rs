//! Muestra el campeón seleccionado (imagen + nombre).
//!
//! **DEPRECADO desde FASE 11b-2a**: el champ select rediseñado usa los
//! splashes de [`crate::data_dragon::models::ChampionEntry::splash_url`]
//! con las cards del prototipo (`crate::ui::champ_select`). Este componente
//! queda solo como referencia del MVP original; no hay usos activos.

use dioxus::prelude::*;

use crate::lcu::models::ChampionInfo;

/// Componente visual para un campeón. Sin estilos por ahora.
#[deprecated(
    since = "0.1.0",
    note = "usar champ_visual + las cards de ui::champ_select (11b-2a)"
)]
#[component]
pub fn ChampDisplay(champion: ChampionInfo) -> Element {
    rsx! {
        div {
            img { src: "{champion.image_url}", alt: "{champion.name}" }
            span { "{champion.name}" }
        }
    }
}
