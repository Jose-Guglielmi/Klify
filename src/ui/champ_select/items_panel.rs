//! Tab ITEMS del champ select (FASE 11b-2b).
//!
//! Panel a lo ancho con el [`crate::stats::source::items::ItemBuild`] de
//! Lolalytics en secciones apiladas, según el prototipo (ui-preview.html,
//! panel `#panel-items`):
//!
//! - **// Inicio** (starters), **// Botas** (boots), **// Build Core**
//!   (core: tiles numerados 1→N con flechas `→` en orden de compra) y
//!   **// Situacionales** (situational).
//! - Cada tile: icono 44px biselado + nombre del catálogo de ítems de Data
//!   Dragon ([`item_name`], fallback `#id`) + descripción de una línea
//!   (blurb limpio y truncado, [`truncate_line`]) + WR en blanco + partidas
//!   en `on-surface-var`.
//!
//! Regla del rojo (`design.md`): los WR destacados van en BLANCO grande;
//! `#ff0033` queda reservado (solo el orden de compra es metadata neutra).
//!
//! Estados honestos: `Loading` → spinner; sin build (fallos en `Partial`
//! o `Unavailable`) → "Build no disponible · {razón}" con la razón corta
//! del analizador. Toda la derivación de filas es pura y testeada
//! ([`item_rows`], [`row_blurb`], [`truncate_line`]); el RSX solo pinta.
//!
//! Zona de acción (al pie, solo con build): crea un ITEM SET en el cliente
//! LCU con el build destacado — el mismo que se muestra — vía
//! [`crate::lcu::actions::cargar_item_set`] (el set se llama "Klify ·
//! {campeón}", detalle interno de actions). Copy de los estados
//! ([`copy_de_carga`]): "CARGAR AL JUEGO" → "CARGANDO…" → éxito "CARGADO ✓"
//! con la nota de dónde aparece en la tienda, o error "REINTENTAR" con el
//! motivo corto. Sin cliente conectado, el botón queda deshabilitado con
//! el chip "CLIENTE NO CONECTADO" (mismo patrón que el botón de runas).

use std::sync::Arc;

use dioxus::prelude::*;

use crate::data_dragon::item::{item_name, ItemCatalog};
use crate::data_dragon::models::ChampionCatalog;
use crate::lcu::actions::cargar_item_set;
use crate::lcu::events::es_placeholder;
use crate::stats::source::analysis::{
    analysis_failure_reason, analysis_in_progress, analysis_items, AnalysisState,
};
use crate::stats::source::items::{ItemBuild, ItemStat};
use crate::ui::format::{format_thousands, strip_html};
use crate::ui::recommendations::format_winrate;

/// Máximo de caracteres de la descripción de una fila (una línea del
/// panel; el CSS además corta con line-clamp por si el ancho no da).
pub const BLURB_MAX_CHARS: usize = 90;

/// Fila de ítem lista para render (pura, testeada).
#[derive(Clone, Debug, PartialEq)]
pub struct ItemRow {
    /// Id numérico (clave del icono y fallback del nombre).
    pub item_id: i64,
    /// Nombre visible (`#id` sin catálogo o con nombre vacío).
    pub name: String,
    /// Descripción de una línea (blurb limpio y truncado; vacía sin
    /// catálogo — la fila solo muestra nombre y WR).
    pub desc: String,
    /// URL del icono en la CDN (`None` sin versión de Data Dragon).
    pub icon: Option<String>,
    /// Winrate en porcentaje (de la muestra de Lolalytics).
    pub winrate: f64,
    /// Partidas de la muestra.
    pub games: u64,
}

/// Trunca un texto a una línea de `max_chars` (puro, testeado). La
/// elipsis cuenta dentro del presupuesto: el resultado nunca excede
/// `max_chars` visibles. Un texto corto pasa tal cual (trim de bordes) y
/// el corte nunca parte a la mitad de un grafo Unicode (cuenta `char`s).
#[must_use]
pub fn truncate_line(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    if max_chars == 0 {
        return String::new();
    }
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }
    let cut: String = trimmed.chars().take(max_chars - 1).collect();
    format!("{}…", cut.trim_end())
}

/// Descripción de una fila desde el catálogo (puro, testeado): blurb del
/// ítem limpio de HTML y truncado. Sin catálogo o sin entrada → vacío.
#[must_use]
pub fn row_blurb(catalog: Option<&ItemCatalog>, item_id: i64) -> String {
    catalog
        .and_then(|catalog| catalog.get(item_id))
        .map(|info| truncate_line(&strip_html(&info.blurb), BLURB_MAX_CHARS))
        .unwrap_or_default()
}

/// Filas de una sección del build desde los stats crudos (puro, testeado):
/// nombre contra el catálogo (`#id` degradado), blurb de una línea e icono
/// de la CDN con la versión vigente (`None` sin versión — nunca una URL
/// rota con el componente de versión vacío).
#[must_use]
pub fn item_rows(
    stats: &[ItemStat],
    catalog: Option<&ItemCatalog>,
    version: &str,
) -> Vec<ItemRow> {
    stats
        .iter()
        .map(|stat| ItemRow {
            item_id: stat.item_id,
            name: item_name(catalog, stat.item_id),
            desc: row_blurb(catalog, stat.item_id),
            icon: (!version.is_empty()).then(|| stat.icon_url(version)),
            winrate: stat.winrate,
            games: stat.games,
        })
        .collect()
}

/// Partidas formateadas de una fila (los stats traen `u64` y el
/// formateador es `i64`): con saturación, nunca un pánico por overflow.
#[must_use]
pub fn row_games(games: u64) -> String {
    format_thousands(i64::try_from(games).unwrap_or(i64::MAX))
}

/// Tope de caracteres del motivo de error de la carga (es una nota corta
/// junto al botón, no un párrafo; el motivo completo queda en el log).
const MOTIVO_MAX_CHARS: usize = 60;

/// Estado de la creación del item set en el cliente LCU (Signal local del
/// panel; se reinicia cambiando de tab porque el componente se desmonta).
#[derive(Clone, Debug, PartialEq, Eq)]
enum Carga {
    /// Sin acción todavía (estado inicial).
    Inactiva,
    /// Spawn en curso contra el LCU.
    Cargando,
    /// El set quedó creado en el cliente.
    Ok,
    /// El LCU rechazó la carga o no hubo cliente (motivo corto para la UI).
    Error(String),
}

/// Copy y flag `disabled` del botón según el estado de la carga (puro,
/// testeado). El flag solo cubre la parte del estado: sin cliente
/// conectado el botón se deshabilita además desde el panel.
#[must_use]
fn copy_de_carga(carga: &Carga) -> (&'static str, bool) {
    match carga {
        Carga::Inactiva => ("CARGAR AL JUEGO", false),
        Carga::Cargando => ("CARGANDO…", true),
        Carga::Ok => ("CARGADO ✓", true),
        Carga::Error(_) => ("REINTENTAR", false),
    }
}

/// Clase del botón según el estado (puro, testeado): en éxito agrega
/// `.cs-apply-ok`. En error el botón queda primary ("REINTENTAR") y la
/// clase `.cs-apply-err` va en el motivo corto.
#[must_use]
fn clase_de_carga(carga: &Carga) -> &'static str {
    match carga {
        Carga::Ok => "btn-primary cs-apply-ok",
        _ => "btn-primary",
    }
}

/// Spawn de la creación del item set (función libre, no closure: así el
/// `onclick` es `Fn` y el handler es testeable/compartible, mismo criterio
/// que `spawn_match_fetch`). Mapea el `Result` de
/// [`cargar_item_set`] al Signal [`Carga`]; el handle del cliente lo
/// resuelve actions por dentro (`client_handle()`).
fn spawn_cargar_item_set(
    mut carga: Signal<Carga>,
    build: Arc<ItemBuild>,
    champion_id: i64,
    champ_nombre: String,
) {
    // Guard de placeholder: "Campeón {id}" = catálogo aún sin resolver; el
    // set quedaría con un título basura en el cliente.
    if es_placeholder(&champ_nombre) {
        carga.set(Carga::Error(String::from(
            "el catálogo todavía está resolviendo el campeón; esperá un segundo y probá de nuevo",
        )));
        return;
    }
    carga.set(Carga::Cargando);
    spawn(async move {
        match cargar_item_set(&build, champion_id, &champ_nombre).await {
            Ok(()) => carga.set(Carga::Ok),
            Err(motivo) => carga.set(Carga::Error(motivo)),
        }
    });
}

/// Tab ITEMS. `client_connected` habilita la zona de acción (creación del
/// item set en el LCU) y `champ_nombre` nombra la sección del set en la
/// tienda ("Klify · {nombre}", lo arma actions por dentro).
#[component]
pub(crate) fn ItemsPanel(
    analysis: ReadSignal<AnalysisState>,
    champion_id: i64,
    catalog: ReadSignal<Option<ChampionCatalog>>,
    items: ReadSignal<Option<Arc<ItemCatalog>>>,
    client_connected: bool,
    champ_nombre: String,
) -> Element {
    // Snapshot corto: el guard no cruza la construcción del rsx.
    let snapshot: AnalysisState = analysis.read().clone();
    let build = analysis_items(&snapshot, champion_id);
    let loading = analysis_in_progress(&snapshot, champion_id);
    let failure = analysis_failure_reason(&snapshot, champion_id);
    let items_catalog: Option<Arc<ItemCatalog>> = items.read().clone();
    let version = catalog
        .read()
        .as_ref()
        .map(|data| data.version.clone())
        .unwrap_or_default();
    let carga = use_signal(|| Carga::Inactiva);

    rsx! {
        div { class: "cs-items",
            {match build {
                Some(build) => {
                    let catalog = items_catalog.as_deref();
                    let starters = item_rows(&build.starters, catalog, &version);
                    let boots = item_rows(&build.boots, catalog, &version);
                    let core = item_rows(&build.core, catalog, &version);
                    let situational = item_rows(&build.situational, catalog, &version);

                    if starters.is_empty() && boots.is_empty() && core.is_empty()
                        && situational.is_empty()
                    {
                        rsx! {
                            p { class: "cs-hint", "La fuente no devolvió ítems para este pick." }
                        }
                    } else {
                        rsx! {
                            div { class: "cs-item-sections",
                                ItemSection { label: "Inicio", rows: starters, numbered: false }
                                ItemSection { label: "Botas", rows: boots, numbered: false }
                                ItemSection { label: "Build Core", rows: core, numbered: true }
                                ItemSection { label: "Situacionales", rows: situational, numbered: false }
                            }
                            ItemsAction {
                                item_build: build.clone(),
                                champion_id,
                                champ_nombre: champ_nombre.clone(),
                                client_connected,
                                carga,
                            }
                        }
                    }
                }
                None if loading => rsx! {
                    div { class: "cs-loading",
                        div { class: "spinner" }
                        span { class: "label label-md pulse-soft t-silver",
                            "Consultando build de ítems…"
                        }
                    }
                },
                None => {
                    let reason =
                        failure.unwrap_or_else(|| String::from("análisis no disponible"));
                    rsx! {
                        div { class: "cs-unavailable",
                            span { class: "label label-md t-error", "Build no disponible" }
                            p { class: "cs-unavailable-reason", "{reason}" }
                        }
                    }
                }
            }}
        }
    }
}

/// Zona de acción al pie del panel: crea el item set "Klify · {champ}" en
/// el cliente con el build destacado (`item_build`, el mismo que se
/// muestra). Sin cliente: botón deshabilitado + chip "CLIENTE NO
/// CONECTADO". Con cliente: el copy sigue a [`copy_de_carga`] y la nota
/// lateral muestra dónde aparece el set (éxito) o el motivo corto
/// truncado (error).
#[component]
fn ItemsAction(
    item_build: Arc<ItemBuild>,
    champion_id: i64,
    champ_nombre: String,
    client_connected: bool,
    carga: Signal<Carga>,
) -> Element {
    // Snapshot corto: el guard no cruza la construcción del rsx.
    let carga_snapshot: Carga = carga.read().clone();
    let (label, carga_disabled) = copy_de_carga(&carga_snapshot);
    // Copias propias para el closure del click: cada spawn necesita valores
    // 'static (puede reintentarse) y los props originales quedan vivos para
    // el rsx (la nota de éxito interpola `champ_nombre`).
    let build_para_click = item_build.clone();
    let champ_para_click = champ_nombre.clone();

    rsx! {
        div { class: "cs-page-action",
            button {
                class: clase_de_carga(&carga_snapshot),
                disabled: !client_connected || carga_disabled,
                "type": "button",
                onclick: move |_| {
                    spawn_cargar_item_set(
                        carga,
                        build_para_click.clone(),
                        champion_id,
                        champ_para_click.clone(),
                    );
                },
                "{label}"
            }
            {match (client_connected, &carga_snapshot) {
                (false, _) => rsx! {
                    span { class: "cs-soon",
                        span { class: "dot-pulse", "style": "width: 8px; height: 8px; border-radius: 9999px; background: var(--silver);" }
                        span { class: "label t-error", "CLIENTE NO CONECTADO" }
                    }
                },
                (true, Carga::Ok) => rsx! {
                    span { class: "cs-soon",
                        span { class: "label t-dim",
                            "Aparece en la tienda, sección Klify · {champ_nombre}"
                        }
                    }
                },
                (true, Carga::Error(motivo)) => rsx! {
                    span { class: "cs-soon",
                        span { class: "label cs-apply-err",
                            "{truncate_line(motivo, MOTIVO_MAX_CHARS)}"
                        }
                    }
                },
                _ => rsx! {},
            }}
        }
    }
}

/// Una sección del build: label `// {label}` + tiles con nombre/WR/juegos.
/// `numbered` (core) agrega el badge de orden 1..N sobre cada tile y la
/// flecha `→` entre tiles (orden de compra del build más jugado).
#[component]
fn ItemSection(label: &'static str, rows: Vec<ItemRow>, numbered: bool) -> Element {
    rsx! {
        section { class: "cs-item-section",
            div { class: "section-head",
                span { class: "bar-skew bar-skew--silver", "style": "width: 24px;" }
                span { class: "label label-md",
                    span { class: "sl sl--silver", "//" }
                    " {label}"
                }
            }

            div { class: "cs-item-rowlist",
                {rows.into_iter().enumerate().map(|(index, row)| {
                    let order = index + 1;
                    rsx! {
                        div { class: "cs-item-step", key: "{row.item_id}",
                            if numbered && index > 0 {
                                span { class: "cs-item-arrow", "aria-hidden": "true", "→" }
                            }
                            div { class: "cs-item",
                                div { class: "cs-item-tilewrap",
                                    {match row.icon {
                                        Some(icon) => rsx! {
                                            div { class: "tile",
                                                img { src: "{icon}", alt: "{row.name}" }
                                            }
                                        },
                                        None => rsx! {
                                            div { class: "tile",
                                                span { class: "cs-tile-fallback", "#" }
                                            }
                                        },
                                    }}
                                    if numbered {
                                        span { class: "cs-item-order", "{order}" }
                                    }
                                }
                                div { class: "cs-item-text",
                                    div { class: "cs-item-name", "{row.name}" }
                                    if !row.desc.is_empty() {
                                        div { class: "cs-item-desc", "{row.desc}" }
                                    }
                                    div { class: "cs-item-stats",
                                        span { class: "num-display tnum cs-item-wr",
                                            "{format_winrate(row.winrate)}%"
                                        }
                                        span { class: "cs-item-games tnum",
                                            span { class: "hud-sep", "·" }
                                            " {row_games(row.games)} partidas"
                                        }
                                    }
                                }
                            }
                        }
                    }
                })}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_dragon::item::ItemInfo;
    use std::collections::HashMap;

    fn stat(item_id: i64, games: u64, wins: u64) -> ItemStat {
        ItemStat {
            item_id,
            games,
            wins,
            winrate: wins as f64 / games as f64 * 100.0,
        }
    }

    /// Catálogo mínimo con dos ítems (uno sin nombre) y un blurb con HTML
    /// más largo que el tope de una línea.
    fn catalogo() -> ItemCatalog {
        let mut by_id = HashMap::new();
        by_id.insert(
            3157,
            ItemInfo {
                name: String::from("Zhonya's Hourglass"),
                blurb: String::from(
                    // Suficientemente largo para truncar con BLURB_MAX_CHARS = 90.
                    "<mainText>Activate to become invincible but unable to take actions for 2.5 seconds, additionally granting massive armor and spell resistance while it lasts</mainText>",
                ),
            },
        );
        by_id.insert(
            3020,
            ItemInfo {
                name: String::new(),
                blurb: String::from("Enhances Move Speed"),
            },
        );
        ItemCatalog {
            version: String::from("16.20.1"),
            by_id,
        }
    }

    /// `truncate_line`: corta con elipsis, respeta cortos y bordes.
    #[test]
    fn truncate_line_corta_con_ellipsis_y_respeta_cortos() {
        assert_eq!(truncate_line("  corto  ", 90), "corto");
        assert_eq!(truncate_line("1234567890", 10), "1234567890");
        assert_eq!(truncate_line("12345678901", 10), "123456789…");
        assert_eq!(truncate_line("", 10), "");
        // Cuenta chars, no bytes (el corte no parte un acento).
        assert_eq!(truncate_line("áéíóú áéíóú", 5), "áéíó…");
        // La elipsis cuenta dentro del presupuesto (6 visibles > 5).
        assert_eq!(truncate_line("123456", 5), "1234…");
        assert_eq!(truncate_line("", 10), "");
    }

    /// Filas con catálogo: nombre real, blurb limpio y truncado a una
    /// línea, icono con la versión y WR/juegos de la muestra.
    #[test]
    fn item_rows_resuelve_nombres_blurb_e_icono_con_catalogo() {
        let stats = vec![stat(3157, 775, 443), stat(3020, 9, 4)];
        let rows = item_rows(&stats, Some(&catalogo()), "16.20.1");

        assert_eq!(rows.len(), 2);

        let zhonyas = &rows[0];
        assert_eq!(zhonyas.item_id, 3157);
        assert_eq!(zhonyas.name, "Zhonya's Hourglass");
        assert_eq!(
            zhonyas.desc,
            truncate_line(
                &strip_html(
                    // Mismo blurb que el fixture de `catalogo()`.
                    "<mainText>Activate to become invincible but unable to take actions for 2.5 seconds, additionally granting massive armor and spell resistance while it lasts</mainText>"
                ),
                BLURB_MAX_CHARS
            )
        );
        assert!(zhonyas.desc.ends_with('…'), "el blurb largo se trunca");
        assert!(zhonyas.desc.len() <= BLURB_MAX_CHARS + "…".len());
        assert_eq!(
            zhonyas.icon.as_deref(),
            Some("https://ddragon.leagueoflegends.com/cdn/16.20.1/img/item/3157.png")
        );
        assert!((zhonyas.winrate - 443.0 / 775.0 * 100.0).abs() < 1e-9);
        assert_eq!(zhonyas.games, 775);

        // Entrada con nombre vacío → #id (degradación honesta).
        assert_eq!(rows[1].name, "#3020");
    }

    /// Sin catálogo: `#id`, descripción vacía; sin versión: sin icono
    /// (nunca una URL con la versión vacía).
    #[test]
    fn item_rows_sin_catalogo_degrada_a_id_y_sin_version_a_sin_icono() {
        let stats = vec![stat(4645, 100, 55)];

        let sin_catalogo = item_rows(&stats, None, "16.20.1");
        assert_eq!(sin_catalogo[0].name, "#4645");
        assert!(sin_catalogo[0].desc.is_empty());
        assert!(sin_catalogo[0].icon.is_some());

        // Con catálogo, el nombre resuelve; sin versión, el icono no se
        // arma con la URL vacía.
        let stats_named = vec![stat(3157, 100, 55)];
        let sin_version = item_rows(&stats_named, Some(&catalogo()), "");
        assert_eq!(sin_version[0].name, "Zhonya's Hourglass");
        assert_eq!(sin_version[0].icon, None);

        let nada = item_rows(&stats_named, None, "");
        assert_eq!(nada[0].name, "#3157");
        assert_eq!(nada[0].icon, None);
    }

    /// Partidas formateadas con miles y saturación para u64 grande.
    #[test]
    fn row_games_formatea_miles_y_satura() {
        assert_eq!(row_games(0), "0");
        assert_eq!(row_games(48_200), "48.200");
        assert_eq!(row_games(u64::MAX), format_thousands(i64::MAX));
    }

    /// Copy del botón por estado: label esperado y disabled solo mientras
    /// carga o ya cargado (en error el botón vuelve a habilitarse para
    /// reintentar).
    #[test]
    fn copy_de_carga_recorre_los_estados() {
        assert_eq!(copy_de_carga(&Carga::Inactiva), ("CARGAR AL JUEGO", false));
        assert_eq!(copy_de_carga(&Carga::Cargando), ("CARGANDO…", true));
        assert_eq!(copy_de_carga(&Carga::Ok), ("CARGADO ✓", true));
        assert_eq!(
            copy_de_carga(&Carga::Error(String::from("boom"))),
            ("REINTENTAR", false)
        );
    }

    /// Clase del botón: solo el éxito agrega la clase de OK; el resto de
    /// los estados queda en primary.
    #[test]
    fn clase_de_carga_marca_solo_el_exito() {
        assert_eq!(clase_de_carga(&Carga::Inactiva), "btn-primary");
        assert_eq!(clase_de_carga(&Carga::Cargando), "btn-primary");
        assert_eq!(clase_de_carga(&Carga::Ok), "btn-primary cs-apply-ok");
        assert_eq!(
            clase_de_carga(&Carga::Error(String::from("boom"))),
            "btn-primary"
        );
    }

    /// El motivo de error se muestra truncado a una línea corta (el tope
    /// de la nota, no el de los blurbs de ítem).
    #[test]
    fn motivo_de_error_se_trunca_corto() {
        let largo = "x".repeat(MOTIVO_MAX_CHARS + 40);
        let truncado = truncate_line(&largo, MOTIVO_MAX_CHARS);
        assert!(truncado.chars().count() <= MOTIVO_MAX_CHARS);

        let corto = String::from("cliente no conectado");
        assert_eq!(truncate_line(&corto, MOTIVO_MAX_CHARS), corto);
    }
}
