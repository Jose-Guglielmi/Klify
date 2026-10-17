//! Shell compartido de las vistas (FASE 11b-1): header KLIFY + footer HUD.
//!
//! [`KlifyHeader`]: wordmark (K roja), slashes de impacto, tagline,
//! separador vertical, "DESKTOP · V0.1" y el estado de conexión chico.
//! [`KlifyFooter`]: "SYNC · {fuente}" a la izquierda (con dot pulsante) y
//! "PATCH {versión} · KLIFY D0.1" a la derecha.
//!
//! Toda la derivación de textos es pura y testeada acá: [`connection_label`],
//! [`sync_label`] y [`patch_label`].

use dioxus::prelude::*;

use crate::lcu::models::AppState;

/// Versión de escritorio que viaja en el footer.
const DESKTOP_VERSION: &str = "KLIFY D0.1";

/// Etiqueta del estado de conexión del header (pura, testeada).
///
/// El cliente cuenta como "en línea" desde que el loop LCU lo detecta
/// (champ select incluido); `Disconnected` → "SIN CLIENTE".
#[must_use]
pub fn connection_label(state: &AppState) -> &'static str {
    match state {
        AppState::Disconnected => "SIN CLIENTE",
        AppState::WaitingForChampSelect | AppState::InChampSelect(_) => "EN LÍNEA",
    }
}

/// Etiqueta de sync del footer: "LOLALYTICS" cuando la fuente sirvió al
/// menos una tanda de recomendaciones, "EN ESPERA" mientras tanto (pura).
#[must_use]
pub fn sync_label(source_served: bool) -> &'static str {
    if source_served {
        "LOLALYTICS"
    } else {
        "EN ESPERA"
    }
}

/// Tramo derecho del footer: `"PATCH 16.20.1 · KLIFY D0.1"`, con `—` si
/// todavía no hay versión de Data Dragon (puro, testeado).
#[must_use]
pub fn patch_label(version: Option<&str>) -> String {
    let version = version
        .map(str::trim)
        .filter(|version| !version.is_empty())
        .unwrap_or("—");
    format!("PATCH {version} · {DESKTOP_VERSION}")
}

/// Header HUD compartido por las tres vistas. `connected` pinta el estado
/// chico de la derecha ("EN LÍNEA" pulsante vs "SIN CLIENTE").
#[component]
pub fn KlifyHeader(state: ReadSignal<AppState>) -> Element {
    // Snapshot corto: el guard no cruza la construcción del rsx.
    let state_snapshot: AppState = state.read().clone();
    let connected = !matches!(state_snapshot, AppState::Disconnected);
    let status = connection_label(&state_snapshot);

    rsx! {
        header { class: "klify-header",
            div { class: "klify-header-row",
                div { class: "klify-brand",
                    span { class: "slash-stack", "aria-hidden": "true",
                        i {}
                        i {}
                        i {}
                    }
                    span { class: "klify-wordmark",
                        span { class: "k", "K" }
                        "LIFY"
                    }
                    span { class: "klify-divider-v" }
                    span { class: "label label-md klify-tagline",
                        "Analiza "
                        span { class: "hud-sep", "·" }
                        " Mejora "
                        span { class: "hud-sep", "·" }
                        " Domina"
                    }
                }
                div { class: "klify-header-status",
                    if connected {
                        span { class: "dot-pulse", "style": "width: 8px; height: 8px; border-radius: 9999px; background: var(--primary);" }
                    } else {
                        span { class: "dot-pulse", "style": "width: 8px; height: 8px; border-radius: 9999px; background: var(--silver);" }
                    }
                    span { class: "label", "DESKTOP · V0.1 "
                        span { class: "hud-sep", "·" }
                        " {status}"
                    }
                }
            }
            div { class: "klify-redline", "aria-hidden": "true" }
        }
    }
}

/// Footer HUD compartido. `source_served` enciende "LOLALYTICS" cuando el
/// recomendador ya sirvió páginas; `version` es la de Data Dragon.
#[component]
pub fn KlifyFooter(
    source_served: bool,
    version: Option<String>,
) -> Element {
    let patch = patch_label(version.as_deref());

    rsx! {
        footer { class: "klify-footer",
            div { class: "klify-footer-row",
                span { class: "label fs-10 t-outline",
                    span { class: "dot-pulse", "style": "width: 6px; height: 6px; border-radius: 9999px; background: var(--silver); margin-right: 8px; vertical-align: middle;" }
                    "SYNC "
                    span { class: "hud-sep", "·" }
                    " {sync_label(source_served)}"
                }
                span { class: "label fs-10 t-outline-var", "{patch}" }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Estado de conexión: desconectado vs cliente detectado/champ select.
    #[test]
    fn etiqueta_de_conexion_segun_estado() {
        assert_eq!(connection_label(&AppState::Disconnected), "SIN CLIENTE");
        assert_eq!(
            connection_label(&AppState::WaitingForChampSelect),
            "EN LÍNEA"
        );
        assert_eq!(
            connection_label(&AppState::InChampSelect(None)),
            "EN LÍNEA"
        );
        assert_eq!(
            connection_label(&AppState::InChampSelect(Some(
                crate::lcu::events::champion_placeholder(103)
            ))),
            "EN LÍNEA"
        );
    }

    /// Sync del footer: en espera hasta que la fuente sirvió.
    #[test]
    fn etiqueta_de_sync() {
        assert_eq!(sync_label(false), "EN ESPERA");
        assert_eq!(sync_label(true), "LOLALYTICS");
    }

    /// Patch del footer: con versión, con espacios, sin versión y vacía.
    #[test]
    fn etiqueta_de_patch() {
        assert_eq!(patch_label(Some("16.20.1")), "PATCH 16.20.1 · KLIFY D0.1");
        assert_eq!(patch_label(Some("  16.20.1  ")), "PATCH 16.20.1 · KLIFY D0.1");
        assert_eq!(patch_label(None), "PATCH — · KLIFY D0.1");
        assert_eq!(patch_label(Some("")), "PATCH — · KLIFY D0.1");
    }
}
