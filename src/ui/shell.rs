//! Shell compartido de las vistas (FASE 11b-1): sidebar de navegación +
//! header KLIFY + footer HUD.
//!
//! [`KlifySidebar`]: nav fija FICHAS · VIVO · PERFIL (prototipo aprobado)
//! con numeración HUD, punto LIVE según `in_match` y estado LCU chico al pie.
//! [`KlifyHeader`]: wordmark (K roja), slashes de impacto, tagline,
//! separador vertical, "DESKTOP · V0.1" y el estado de conexión chico.
//! [`KlifyFooter`]: "SYNC · {fuente}" a la izquierda (con dot pulsante) y
//! "PATCH {versión} · KLIFY D0.1" a la derecha.
//!
//! Toda la derivación de textos/clases es pura y testeada acá:
//! [`item_activo`], [`connection_label`], [`sync_label`] y [`patch_label`].

use dioxus::prelude::*;

use crate::app::NavVista;
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

/// Ítem activo de la sidebar: en partida gana VIVO; si no, la nav explícita
/// (`None` = inicio → FICHAS por el default del menú). Devuelve el slug del
/// ítem (`"fichas" | "vivo" | "perfil"`) para comparar contra cada botón
/// (pura, testeada).
#[must_use]
pub fn item_activo(nav: Option<NavVista>, in_match: bool) -> &'static str {
    if in_match {
        "vivo"
    } else {
        match nav {
            Some(NavVista::Perfil) => "perfil",
            Some(NavVista::Vivo) => "vivo",
            _ => "fichas",
        }
    }
}

/// Clase de un ítem de la nav: solo el slug activo lleva el modificador
/// `--activo` (pura, testeada).
#[must_use]
fn nav_item_clase(activo: &str, slug: &str) -> &'static str {
    if activo == slug {
        "klify-nav-item klify-nav-item--activo"
    } else {
        "klify-nav-item"
    }
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

/// Sidebar fija de navegación (prototipo aprobado): marca chita arriba,
/// ítems FICHAS · VIVO · PERFIL con numeración HUD e íconos de trazo, punto
/// LIVE en VIVO y el estado LCU chico al pie.
///
/// - [`item_activo`] decide el ítem `--activo`: en partida gana VIVO; si no,
///   la nav explícita (`None` = inicio → FICHAS).
/// - Ningún ítem se deshabilita: VIVO sin partida muestra la vista Vivo con
///   el buscador (así lo aprobó el usuario).
/// - Los estilos `.klify-sidebar*` / `.klify-nav*` viven en main.css; los
///   puntitos van con estilos inline (misma receta que los
///   dots del header/footer, y el verde `#86c39c` ya aprobado del theme).
#[component]
pub fn KlifySidebar(
    /// Navegación explícita actual (`None` = inicio → FICHAS activa).
    nav: Signal<Option<NavVista>>,
    /// Champ select vivo (cliente o modo manual): activa VIVO + LIVE.
    in_match: bool,
    /// Click en FICHAS: vuelve al menú de fichas.
    on_ver_fichas: EventHandler<()>,
    /// Click en VIVO: abre la vista Vivo (buscador, con o sin partida).
    on_ir_vivo: EventHandler<()>,
    /// Click en PERFIL: abre el perfil del invocador.
    on_ver_perfil: EventHandler<()>,
) -> Element {
    // Snapshot de la nav: el guard no cruza la construcción del rsx.
    let activo = item_activo(nav(), in_match);

    // Punto LIVE: verde aprobado del theme en partida, gris apagado mientras
    // se espera champ select (el tooltip lo explica en ambos estados).
    let live_dot = if in_match {
        "width: 6px; height: 6px; border-radius: 9999px; background: #86c39c;"
    } else {
        "width: 6px; height: 6px; border-radius: 9999px; background: var(--outline-var);"
    };

    // Dot del pie: rojo Klify en partida (como el header conectado), plata
    // fuera de ella.
    let pie_dot = if in_match {
        "width: 6px; height: 6px; border-radius: 9999px; background: var(--primary); margin-right: 8px; vertical-align: middle;"
    } else {
        "width: 6px; height: 6px; border-radius: 9999px; background: var(--silver); margin-right: 8px; vertical-align: middle;"
    };

    // Estado LCU chico del pie, estilo `connection_label` pero derivado solo
    // de `in_match` (la firma del shell no expone el `AppState`).
    let estado_pie = if in_match { "EN PARTIDA" } else { "EN ESPERA" };

    rsx! {
        aside { class: "klify-sidebar",
            div { class: "klify-sidebar-brand",
                // Espíritu de `.klify-brand`: K roja + wordmark, a tamaño chico.
                span { class: "klify-wordmark", "style": "font-size: 20px;",
                    span { class: "k", "K" }
                    "LIFY"
                }
            }
            nav { class: "klify-nav",
                button {
                    class: nav_item_clase(activo, "fichas"),
                    onclick: move |_| on_ver_fichas.call(()),
                    span { class: "klify-nav-num", "01" }
                    IconGrid {}
                    span { class: "klify-nav-text", "FICHAS" }
                }
                button {
                    class: nav_item_clase(activo, "vivo"),
                    onclick: move |_| on_ir_vivo.call(()),
                    span { class: "klify-nav-num", "02" }
                    IconBolt {}
                    span { class: "klify-nav-text", "VIVO" }
                    span {
                        class: if in_match {
                            "klify-nav-live klify-nav-live--on"
                        } else {
                            "klify-nav-live"
                        },
                        title: if in_match { "en partida" } else { "esperando champ select" },
                        span { class: "klify-nav-live-dot", "style": live_dot }
                    }
                }
                button {
                    class: nav_item_clase(activo, "perfil"),
                    onclick: move |_| on_ver_perfil.call(()),
                    span { class: "klify-nav-num", "03" }
                    IconUser {}
                    span { class: "klify-nav-text", "PERFIL" }
                }
            }
            div { class: "klify-nav-footer",
                span { class: "label fs-10 t-outline",
                    span { class: "dot-pulse", "style": pie_dot }
                    "LCU "
                    span { class: "hud-sep", "·" }
                    " {estado_pie}"
                }
            }
        }
    }
}

/// Icono grilla del ítem FICHAS: cuatro celdas con trazo heredado de
/// `currentColor` (el CSS pinta según el estado del ítem).
#[component]
fn IconGrid() -> Element {
    rsx! {
        svg {
            view_box: "0 0 24 24",
            width: "17",
            height: "17",
            fill: "none",
            stroke: "currentColor",
            stroke_width: "2",
            stroke_linecap: "round",
            stroke_linejoin: "round",
            "aria-hidden": "true",
            rect { x: "3", y: "3", width: "7", height: "7" }
            rect { x: "14", y: "3", width: "7", height: "7" }
            rect { x: "14", y: "14", width: "7", height: "7" }
            rect { x: "3", y: "14", width: "7", height: "7" }
        }
    }
}

/// Icono rayo del ítem VIVO (outline tipo "zap", trazo de `currentColor`).
#[component]
fn IconBolt() -> Element {
    rsx! {
        svg {
            view_box: "0 0 24 24",
            width: "17",
            height: "17",
            fill: "none",
            stroke: "currentColor",
            stroke_width: "2",
            stroke_linecap: "round",
            stroke_linejoin: "round",
            "aria-hidden": "true",
            path { d: "M13 2 3 14 12 14 11 22 21 10 12 10 13 2" }
        }
    }
}

/// Icono usuario del ítem PERFIL (cabeza + hombros, trazo `currentColor`).
#[component]
fn IconUser() -> Element {
    rsx! {
        svg {
            view_box: "0 0 24 24",
            width: "17",
            height: "17",
            fill: "none",
            stroke: "currentColor",
            stroke_width: "2",
            stroke_linecap: "round",
            stroke_linejoin: "round",
            "aria-hidden": "true",
            path { d: "M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2" }
            circle { cx: "12", cy: "7", r: "4" }
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

    /// Ítem activo de la sidebar: en partida VIVO pisa cualquier navegación.
    #[test]
    fn item_activo_en_partida_es_siempre_vivo() {
        assert_eq!(item_activo(None, true), "vivo");
        assert_eq!(item_activo(Some(NavVista::Menu), true), "vivo");
        assert_eq!(item_activo(Some(NavVista::Perfil), true), "vivo");
        assert_eq!(item_activo(Some(NavVista::Vivo), true), "vivo");
    }

    /// Ítem activo fuera de partida: manda la nav explícita; `None` (inicio)
    /// y `Menu` caen en FICHAS por el default del menú.
    #[test]
    fn item_activo_sin_partida_sigue_la_nav() {
        assert_eq!(item_activo(None, false), "fichas");
        assert_eq!(item_activo(Some(NavVista::Menu), false), "fichas");
        assert_eq!(item_activo(Some(NavVista::Perfil), false), "perfil");
        assert_eq!(item_activo(Some(NavVista::Vivo), false), "vivo");
    }

    /// Clase del ítem: solo el slug activo lleva el modificador `--activo`.
    #[test]
    fn clase_del_item_segun_activo() {
        assert_eq!(
            nav_item_clase("fichas", "fichas"),
            "klify-nav-item klify-nav-item--activo"
        );
        assert_eq!(nav_item_clase("vivo", "fichas"), "klify-nav-item");
        assert_eq!(nav_item_clase("perfil", "perfil"), "klify-nav-item klify-nav-item--activo");
    }
}
