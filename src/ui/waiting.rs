//! Vista "SIN CLIENTE" (FASE 11b-1): pantalla de espera del prototipo.
//!
//! Splash de Kayn ambiental (URL fija, opacity + mask radial), wordmark
//! XL, bloque de estado HUD con dos pasos (cliente / champ select), ghost
//! button decorativo y franja de 3 features con iconos de la CDN.
//!
//! La derivación del paso 01 es pura ([`client_status`]) y testeada: el
//! texto cambia según `AppState` (buscando vs conectado) y la plataforma.

use dioxus::prelude::*;

use crate::lcu::models::AppState;
use crate::ui::shell::{KlifyFooter, KlifyHeader};

/// Splash ambiental fijo de la pantalla de espera (Kayn base, como el
/// prototipo; decorativa: no depende del parche).
const WAITING_SPLASH_URL: &str =
    "https://ddragon.leagueoflegends.com/cdn/img/champion/splash/Kayn_0.jpg";

/// Estado del paso 01 del HUD (puro, testeado): (etiqueta, subdetalle).
///
/// - `Disconnected` → "BUSCANDO…" + "Buscando LeagueClientUx".
/// - Cliente detectado → "CONECTADO" + "LeagueClientUx". La plataforma
///   (`LA2`, ...) viaja en la prop `platform` de la vista y se anexa en el
///   render, separada del detalle puro.
#[must_use]
pub fn client_status(state: &AppState) -> (String, String) {
    match state {
        AppState::Disconnected => (
            String::from("BUSCANDO…"),
            String::from("Buscando LeagueClientUx"),
        ),
        AppState::WaitingForChampSelect | AppState::InChampSelect(_) => (
            String::from("CONECTADO"),
            String::from("LeagueClientUx"),
        ),
    }
}

/// (icono, título, descripción) de las 3 features del strip (puro,
/// testeado). Los iconos de campeón/ítem dependen de la versión de Data
/// Dragon; sin versión caen al parche fijo del prototipo.
#[must_use]
pub fn feature_strip(version: Option<&str>) -> [(String, String, String); 3] {
    let version = version
        .map(str::trim)
        .filter(|version| !version.is_empty())
        .unwrap_or("16.20.1");
    let cdn = crate::data_dragon::models::CDN_BASE_URL;
    [
        (
            String::from(
                "https://ddragon.leagueoflegends.com/cdn/img/perk-images/Styles/Inspiration/FirstStrike/FirstStrike.png",
            ),
            String::from("Runas recomendadas"),
            String::from("Millones de partidas por parche"),
        ),
        (
            format!("{cdn}/{version}/img/champion/Zed.png"),
            String::from("Counters en vivo"),
            String::from("WR contra los enemigos lockeados"),
        ),
        (
            format!("{cdn}/{version}/img/item/3157.png"),
            String::from("Items vs la comp"),
            String::from("El mejor build contra esta comp"),
        ),
    ]
}

/// Vista de espera: se muestra cuando no hay champ select activo y aún no
/// hay perfil cargado.
#[component]
pub fn WaitingView(
    state: ReadSignal<AppState>,
    platform: Option<String>,
    feature_version: Option<String>,
    patch_version: Option<String>,
    source_served: bool,
) -> Element {
    // Snapshot corto: el guard no cruza la construcción del rsx.
    let state_snapshot: AppState = state.read().clone();
    let (client_label, client_detail) = client_status(&state_snapshot);
    let features = feature_strip(feature_version.as_deref());

    rsx! {
        div { class: "view",
            // Splash ambiental + franjas
            div { class: "bg-splash", "aria-hidden": "true",
                img { src: WAITING_SPLASH_URL, alt: "" }
                div { class: "stripes-hdr" }
            }

            KlifyHeader { state }

            div { class: "waiting-center",
                // Wordmark + tagline
                div { class: "panel",
                    div { "style": "display: flex; align-items: center; gap: 24px;",
                        span { class: "slash-stack slash-stack--xl", "aria-hidden": "true",
                            i {} i {} i {}
                        }
                        h1 { class: "klify-wordmark klify-wordmark--xl",
                            span { class: "k", "K" }
                            "LIFY"
                        }
                        span { class: "slash-stack slash-stack--xl slash-stack--right", "aria-hidden": "true",
                            i {} i {} i {}
                        }
                    }
                    p { class: "label label-md waiting-tagline",
                        "Analiza "
                        span { class: "hud-sep", "·" }
                        " Mejora "
                        span { class: "hud-sep", "·" }
                        " Domina"
                    }
                    p { class: "label fs-10 waiting-sub",
                        "Estadísticas de League of Legends"
                    }
                }

                // Bloque de estado HUD (el corazón de la pantalla)
                div { class: "status-grid",
                    section { class: "status-card panel",
                        div { class: "frame cut-tr-bl",
                            div { class: "cut-tr-bl-i bg-low status-inner",
                                div { class: "section-head", "style": "padding-bottom: 12px;",
                                    span { class: "bar-skew", "style": "width: 24px;" }
                                    span { class: "label label-md",
                                        span { class: "sl", "//" }
                                        " Estado"
                                    }
                                    span { class: "label fs-10 t-outline-var ml-auto",
                                        "Ciclo de espera"
                                    }
                                }

                                // PASO 01 · CLIENTE
                                div { class: "status-step",
                                    span { class: "status-ring",
                                        if matches!(state_snapshot, AppState::Disconnected) {
                                            span { class: "dot-pulse", "style": "width: 10px; height: 10px; border-radius: 9999px; background: var(--silver);" }
                                        } else {
                                            span { class: "dot-pulse", "style": "width: 10px; height: 10px; border-radius: 9999px; background: var(--primary);" }
                                            span { class: "status-ring-soft pulse-soft" }
                                        }
                                    }
                                    div { "style": "min-width: 0; flex: 1;",
                                        div { "style": "display: flex; flex-wrap: wrap; align-items: baseline; justify-content: space-between; gap: 4px 12px;",
                                            span { class: "label label-md t-on", "01 · Cliente de League of Legends" }
                                            span { class: "label fs-13 t-on", "{client_label}" }
                                        }
                                        p { "style": "margin: 4px 0 0; font-size: 12px; color: var(--on-surface-var);",
                                            "{client_detail}"
                                            if let Some(platform) = platform.as_deref() {
                                                span { class: "hud-sep", " · " }
                                                "{platform.to_uppercase()}"
                                            }
                                        }
                                    }
                                }

                                // PASO 02 · CHAMP SELECT (siempre esperando acá)
                                div { class: "status-step",
                                    span { class: "status-ring",
                                        span { class: "dot-pulse", "style": "width: 10px; height: 10px; border-radius: 9999px; background: var(--silver);" }
                                    }
                                    div { "style": "min-width: 0; flex: 1;",
                                        div { "style": "display: flex; flex-wrap: wrap; align-items: baseline; justify-content: space-between; gap: 4px 12px;",
                                            span { class: "label label-md t-on", "02 · Champ Select" }
                                            span { class: "label fs-13 t-dim", "ESPERANDO…" }
                                        }
                                        p { "style": "margin: 4px 0 0; font-size: 12px; color: var(--on-surface-var);",
                                            "Entrá a una partida para continuar"
                                        }
                                    }
                                }
                            }
                        }
                        span { class: "bracket", "aria-hidden": "true", i {} i {} i {} i {} }
                    }
                }

                // Ghost decorativo (sin acción por ahora)
                button { class: "btn-ghost-live mt-9", type: "button",
                    "Probar con partida de práctica"
                }

                // Franja de features
                div { class: "features-strip",
                    for (icon, title, description) in features {
                        article { class: "feature-card panel",
                            div { class: "frame cut-tr",
                                div { class: "cut-tr-i bg-base feature-inner",
                                    div { "style": "display: flex; align-items: center; gap: 12px;",
                                        div { class: "tile", "style": "width: 40px; height: 40px;",
                                            img { src: "{icon}", alt: "{title}" }
                                        }
                                        div { class: "label label-md t-on", "{title}" }
                                    }
                                    p { "{description}" }
                                }
                            }
                        }
                    }
                }
            }

            KlifyFooter { source_served, version: patch_version }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Paso 01: buscando sin cliente, conectado con y sin plataforma.
    #[test]
    fn paso_01_segun_estado() {
        let (label, detail) = client_status(&AppState::Disconnected);
        assert_eq!(label, "BUSCANDO…");
        assert_eq!(detail, "Buscando LeagueClientUx");

        let (label, detail) = client_status(&AppState::WaitingForChampSelect);
        assert_eq!(label, "CONECTADO");
        // Sin plataforma publicada: solo el proceso (la plataforma viaja en
        // la prop `platform`, no en AppState).
        assert_eq!(detail, "LeagueClientUx");
    }

    /// El strip de features: los 3 items con versión real, y el fallback
    /// al parche fijo sin versión.
    #[test]
    fn strip_de_features_con_y_sin_version() {
        let features = feature_strip(Some("16.20.1"));
        assert_eq!(features.len(), 3);
        assert!(features[0].0.ends_with("/FirstStrike/FirstStrike.png"));
        assert!(features[1].0.ends_with("/16.20.1/img/champion/Zed.png"));
        assert!(features[2].0.ends_with("/16.20.1/img/item/3157.png"));
        assert_eq!(features[1].1, "Counters en vivo");

        let fallback = feature_strip(None);
        assert!(fallback[1].0.ends_with("/16.20.1/img/champion/Zed.png"));

        let spaces = feature_strip(Some("  "));
        assert!(spaces[2].0.ends_with("/16.20.1/img/item/3157.png"));
    }

    /// La cola por defecto de la espera no se muestra, pero el helper de
    /// nombre de cola del formato sigue disponible para detalles (smoke).
    #[test]
    fn formato_de_cola_disponible() {
        assert_eq!(
            crate::ui::format::format_queue_name(400),
            "Normal"
        );
    }
}
