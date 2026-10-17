//! Tab RUNAS del champ select (FASE 11b-2a).
//!
//! Renderiza el [`RecommendationsState`] del recomendador como páginas
//! verticales por árbol, según el prototipo (iteración 8):
//!
//! - `Loading` → "Consultando estadísticas de runas…".
//! - `Unavailable` → motivo + botón REINTENTAR (incrementa el contador
//!   `retry` que limpia el anti-reintento de [`run_recommender`]).
//! - `Ready` → panel A (más jugada) destacado con borde/glow/brackets rojos
//!   y badge "MEJOR PICK" + panel B (mejor winrate). Una sola página → un
//!   panel a lo ancho.
//!
//! Cada panel: header (fuente · variante, WR grande, muestra), árbol
//! principal con keystone 64px de anillo rojo, runas 44px con descripción
//! ([`crate::ui::format::strip_html`] sobre `RuneDef::short_desc`), árbol
//! secundario, shards con nombre ([`shard_label`]) y —solo en A— el botón
//! "APLICAR PÁGINA" deshabilitado con tag "PRÓXIMAMENTE".
//!
//! El reparto keystone/principales/secundarias es [`split_page`] (puro,
//! testeado en [`crate::ui::recommendations`]); acá solo se pinta.

use dioxus::prelude::*;

use crate::data_dragon::runes::RuneCatalog;
use crate::stats::source::shard_name;
use crate::stats::source::{RecommendationsState, RunePageStats};
use crate::ui::format::format_thousands;
use crate::ui::recommendations::{format_winrate, split_page, RuneRow, TreeInfo};

/// Nombre visible de un shard para la línea del panel (puro, testeado):
/// nombre de la tabla [`SHARD_NAMES`] o `#id`.
#[must_use]
pub fn shard_label(id: i64) -> String {
    shard_name(id)
}

/// Tab RUNAS.
#[component]
pub(crate) fn RunesPanel(
    recommendations: ReadSignal<RecommendationsState>,
    runes: ReadSignal<Option<RuneCatalog>>,
    mut retry: Signal<u64>,
) -> Element {
    // Snapshot corto: el guard no cruza la construcción del rsx.
    let snapshot: RecommendationsState = recommendations.read().clone();

    rsx! {
        {match snapshot {
            RecommendationsState::Idle => rsx! {
                div { class: "cs-pane",
                    p { class: "cs-hint", "Sin recomendaciones todavía: aparecen al confirmar el pick." }
                }
            },
            RecommendationsState::Loading { .. } => rsx! {
                div { class: "cs-pane",
                    div { class: "cs-loading",
                        div { class: "spinner" }
                        span { class: "label label-md pulse-soft t-silver",
                            "Consultando estadísticas de runas…"
                        }
                    }
                }
            },
            RecommendationsState::Unavailable { reason, .. } => rsx! {
                div { class: "cs-pane",
                    div { class: "cs-unavailable",
                        span { class: "label label-md t-error", "Recomendaciones no disponibles" }
                        p { class: "cs-unavailable-reason", "{reason}" }
                        button {
                            class: "btn-ghost-sm mt-3",
                            "type": "button",
                            onclick: move |_| *retry.write() += 1,
                            "Reintentar"
                        }
                    }
                }
            },
            RecommendationsState::Ready { pages, .. } if !pages.is_empty() => rsx! {
                div { class: if pages.len() > 1 { "cs-runes" } else { "cs-runes cs-runes--single" },
                    {pages.iter().take(2).enumerate().map(|(index, page)| rsx! {
                        RunePageCard {
                            page: page.clone(),
                            runes,
                            best: index == 0,
                        }
                    })}
                }
            },
            RecommendationsState::Ready { .. } => rsx! {
                div { class: "cs-pane",
                    p { class: "cs-hint", "La fuente no devolvió páginas para este pick." }
                }
            },
        }}
    }
}

/// Un panel de página de runas (vertical por árbol). `best` = opción A:
/// borde/glow/brackets rojos, badge "MEJOR PICK" y botón (deshabilitado).
/// El catálogo llega como señal para no clonarlo por panel en cada render.
#[component]
fn RunePageCard(
    page: RunePageStats,
    runes: ReadSignal<Option<RuneCatalog>>,
    best: bool,
) -> Element {
    let split = split_page(&page, (*runes.read()).as_ref());
    let winrate = format_winrate(page.winrate_pct);
    let games = format_thousands(i64::try_from(page.games).unwrap_or(i64::MAX));

    rsx! {
        article { class: if best { "cs-page cs-page--best" } else { "cs-page" },

            // Header · fuente/variante + WR grande + muestra (+ badge en A)
            div { class: "cs-page-head",
                div { class: "min-w-0",
                    div { class: "label",
                        "{page.label}"
                    }
                    div { class: "cs-page-wr-row mt-1",
                        span { class: if best { "num-display tnum cs-page-wr t-bright" } else { "num-display tnum cs-page-wr t-on" },
                            "{winrate}%"
                        }
                        span { class: "label t-dim", "WR" }
                        span { class: "cell-value t-dim",
                            span { class: "hud-sep", "·" }
                            " {games} partidas"
                        }
                    }
                }
                if best {
                    span { class: "cs-badge cs-badge--best", "Mejor pick" }
                }
            }

            PrimaryTree { tree: split.primary_tree, keystone: split.keystone, runes: split.primary }

            SecondaryTree { tree: split.secondary_tree, runes: split.secondary }

            // SHARDS
            if !page.stat_shards.is_empty() {
                div { class: "cs-shards",
                    span { class: "label t-outline", "Shards" }
                    {page.stat_shards.iter().map(|id| rsx! {
                        span { class: "cs-shard", "{shard_label(*id)}" }
                    })}
                }
            }

            // Acción (solo opción A): deshabilitada hasta 11b-2c
            if best {
                div { class: "cs-page-action",
                    button { class: "btn-primary", disabled: true, "Aplicar página" }
                    span { class: "cs-soon",
                        span { class: "dot-pulse", "style": "width: 8px; height: 8px; border-radius: 9999px; background: var(--silver);" }
                        span { class: "label", "Próximamente" }
                    }
                }
            }

            // Brackets HUD (rojos solo en la destacada)
            span { class: if best { "bracket bracket--red" } else { "bracket" }, "aria-hidden": "true",
                i {}
                i {}
                i {}
                i {}
            }
        }
    }
}

/// Header + filas del árbol principal (keystone 64px con anillo rojo).
#[component]
fn PrimaryTree(
    tree: TreeInfo,
    keystone: Option<RuneRow>,
    runes: Vec<RuneRow>,
) -> Element {
    rsx! {
        TreeHeader { tree, role: "Primario" }
        {match keystone {
            Some(keystone) => rsx! {
                KeystoneRow { rune: keystone }
            },
            None => rsx! {},
        }}
        div { class: "cs-rune-list",
            {runes.iter().map(|rune| rsx! {
                RuneSlotRow { rune: rune.clone() }
            })}
        }
    }
}

/// Header + filas del árbol secundario (2 runas 44px).
#[component]
fn SecondaryTree(tree: TreeInfo, runes: Vec<RuneRow>) -> Element {
    rsx! {
        TreeHeader { tree, role: "Secundario" }
        div { class: "cs-rune-list",
            {runes.iter().map(|rune| rsx! {
                RuneSlotRow { rune: rune.clone() }
            })}
        }
    }
}

/// Cabecera de árbol: icono 36px (si el catálogo lo trae) + "PRIMARIO · {árbol}".
#[component]
fn TreeHeader(tree: TreeInfo, role: &'static str) -> Element {
    rsx! {
        div { class: "cs-page-tree",
            {match tree.icon {
                Some(icon) => rsx! {
                    div { class: "tile", "style": "width: 36px; height: 36px;",
                        img { src: "{icon}", alt: "{tree.name}" }
                    }
                },
                None => rsx! {},
            }}
            span { class: "label t-outline",
                "{role} "
                span { class: "hud-sep", "·" }
                " {tree.name}"
            }
        }
    }
}

/// Fila de keystone: tile 64px con anillo rojo + nombre + descripción.
#[component]
fn KeystoneRow(rune: RuneRow) -> Element {
    rsx! {
        div { class: "cs-rune-keystone",
            {match rune.icon {
                Some(icon) => rsx! {
                    div { class: "tile tile--keystone", "style": "width: 64px; height: 64px;",
                        img { src: "{icon}", alt: "{rune.name}" }
                    }
                },
                None => rsx! {
                    div { class: "tile tile--keystone", "style": "width: 64px; height: 64px;",
                        span { class: "cs-qmark", "#" }
                    }
                },
            }}
            div { class: "min-w-0",
                div { class: "label fs-10 t-bright", "Keystone" }
                div { class: "cs-rune-name", "{rune.name}" }
                if !rune.desc.is_empty() {
                    div { class: "cs-rune-desc", "{rune.desc}" }
                }
            }
        }
    }
}

/// Fila de runa menor: icono 44px tile biselado + nombre + descripción.
#[component]
fn RuneSlotRow(rune: RuneRow) -> Element {
    rsx! {
        div { class: "cs-rune-row",
            {match rune.icon {
                Some(icon) => rsx! {
                    div { class: "tile", "style": "width: 44px; height: 44px;",
                        img { src: "{icon}", alt: "{rune.name}" }
                    }
                },
                None => rsx! {
                    div { class: "tile", "style": "width: 44px; height: 44px;",
                        span { class: "cs-qmark", "#" }
                    }
                },
            }}
            div { class: "min-w-0",
                div { class: "cs-rune-name", "{rune.name}" }
                if !rune.desc.is_empty() {
                    div { class: "cs-rune-desc", "{rune.desc}" }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// La tabla del juego cubre los shards que llegan de las fuentes y cae
    /// a `#id` para lo desconocido (5004/5006/5009 no existen).
    #[test]
    fn shard_label_resuelve_nombres_y_cae_a_id() {
        assert_eq!(shard_label(5001), "Vida (escala)");
        assert_eq!(shard_label(5002), "Armadura");
        assert_eq!(shard_label(5003), "Res. mágica");
        assert_eq!(shard_label(5005), "Vel. de ataque");
        assert_eq!(shard_label(5007), "Celeridad");
        assert_eq!(shard_label(5008), "Fuerza adaptativa");
        assert_eq!(shard_label(5010), "Vel. de movimiento");
        assert_eq!(shard_label(5011), "Vida");
        assert_eq!(shard_label(5012), "Resistencias (escala)");
        assert_eq!(shard_label(5013), "Tenacidad");
        assert_eq!(shard_label(5004), "#5004");
        assert_eq!(shard_label(5006), "#5006");
        assert_eq!(shard_label(5009), "#5009");
    }
}
