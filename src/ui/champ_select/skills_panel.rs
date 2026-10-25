//! Tab HABILIDADES del champ select (FASE 11b-2a).
//!
//! Dos columnas según el prototipo (iteración 6):
//!
//! - **Izquierda · "// BASIC KIT"**: pasiva + QWER desde [`ChampionDetail`]
//!   (iconos + nombre + descripción limpia). Hover preselecciona, click fija
//!   (segundo click desfija), igual que el prototipo.
//! - **Derecha · panel contextual**: con selección → ficha completa del
//!   hechizo (tecla roja, nombre, descripción, HUD de enfriamiento/costo/
//!   rango/recurso); sin selección → estado honesto del orden de subida:
//!   spinner solo mientras el análisis corre; resuelto sin datos → estado
//!   vacío definitivo ("sin datos de skill order", cero datos fabricados).
//!
//! [`skill_items`] y [`spell_stats`] son la derivación pura (testeada): el
//! RSX solo pinta. Sin detalle (Data Dragon caído o análisis en curso) la
//! columna muestra el estado honesto correspondiente.

use dioxus::prelude::*;

use crate::data_dragon::champion_detail::{ChampionDetail, Spell};
use crate::data_dragon::models::ChampionCatalog;
use crate::stats::source::analysis::AnalysisState;
use crate::stats::source::analysis::{
    analysis_detail, analysis_failure_reason, analysis_in_progress, analysis_skills,
};
use crate::stats::source::skills::{SkillOrder, SkillSource};
use crate::ui::format::cost_type_label;

/// Una fila del Basic Kit (derivación pura, testeada).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkillItem {
    /// Tecla visible: `P`, `Q`, `W`, `E`, `R`.
    pub key: char,
    /// Nombre visible.
    pub name: String,
    /// Descripción limpia de HTML.
    pub desc: String,
    /// URL del icono en la CDN.
    pub icon: String,
}

/// Filas del Basic Kit desde el detalle (puro, testeado): pasiva primero y
/// hasta 4 hechizos (Q/W/E/R) después, con descripciones limpias de HTML.
#[must_use]
pub fn skill_items(detail: &ChampionDetail, version: &str) -> Vec<SkillItem> {
    let mut items = Vec::with_capacity(1 + detail.spells.len().min(4));
    if !detail.passive.name.is_empty() {
        items.push(SkillItem {
            key: 'P',
            name: detail.passive.name.clone(),
            desc: crate::ui::format::strip_html(&detail.passive.description),
            icon: detail.passive.icon_url(version),
        });
    }
    let keys = ['Q', 'W', 'E', 'R'];
    for (index, spell) in detail.spells.iter().take(4).enumerate() {
        items.push(SkillItem {
            key: keys[index],
            name: spell.name.clone(),
            desc: crate::ui::format::strip_html(&spell.description),
            icon: spell.icon_url(version),
        });
    }
    items
}

/// Fila de stat de la ficha de hechizo (puro, testeado): solo las stats con
/// dato real; el recurso siempre está (resuelto contra el `partype`).
#[must_use]
pub fn spell_stats(spell: &Spell, partype: &str) -> Vec<(&'static str, String)> {
    let mut rows: Vec<(&'static str, String)> = Vec::new();
    if !spell.cooldown_burn.is_empty() {
        rows.push(("Enfriamiento", format!("{}s", spell.cooldown_burn)));
    }
    if !spell.cost_burn.is_empty() {
        rows.push(("Costo", spell.cost_burn.clone()));
    }
    if !spell.range_burn.is_empty() {
        rows.push(("Rango", spell.range_burn.clone()));
    }
    rows.push(("Recurso", cost_type_label(&spell.cost_type, partype)));
    rows
}

/// Clases de una fila del kit según su estado (puro, testeado): `live` con
/// hover/selección vigente, `pinned` con click fijado.
#[must_use]
pub fn skill_row_classes(is_live: bool, is_pinned: bool) -> String {
    let mut classes = String::from("cs-skill-row");
    if is_live {
        classes.push_str(" cs-skill-row--live");
    }
    if is_pinned {
        classes.push_str(" cs-skill-row--pinned");
    }
    classes
}

/// ¿Hay skill order usable para pintar? (puro, testeado): solo una fuente
/// con prioridad no vacía cuenta como dato. `None` o el valor "sin datos"
/// ([`SkillOrder::unavailable`]) → `false`, que la UI pinta como estado
/// vacío definitivo (nunca como carga en curso; QA: panel sin spinner
/// infinito cuando el dataset no trae kit para el champ/patch).
#[must_use]
pub fn skill_order_available(skills: Option<&SkillOrder>) -> bool {
    matches!(skills, Some(skills) if !skills.priority.is_empty())
}

/// Tab HABILIDADES. Re-renderiza cuando el análisis publica.
#[component]
pub(crate) fn SkillsPanel(
    analysis: ReadSignal<AnalysisState>,
    champion_id: i64,
    catalog: ReadSignal<Option<ChampionCatalog>>,
) -> Element {
    // Snapshot corto: el guard no cruza la construcción del rsx.
    let analysis_snapshot: AnalysisState = analysis.read().clone();
    let version = catalog
        .read()
        .as_ref()
        .map(|catalog| catalog.version.clone())
        .unwrap_or_default();

    let detail = analysis_detail(&analysis_snapshot, champion_id);
    let skills = analysis_skills(&analysis_snapshot, champion_id);
    let loading = analysis_in_progress(&analysis_snapshot, champion_id);
    let failure = analysis_failure_reason(&analysis_snapshot, champion_id);

    // Selección efectiva: hover gana sobre lo fijado (mismo comportamiento
    // que el prototipo; al salir del bloque vuelve lo fijado).
    let mut hovered = use_signal(|| Option::<usize>::None);
    let mut pinned = use_signal(|| Option::<usize>::None);
    let selected = match hovered() {
        Some(index) => Some(index),
        None => pinned(),
    };

    // Filas del kit (solo con detalle).
    let items: Option<Vec<SkillItem>> = detail
        .as_deref()
        .map(|detail| skill_items(detail, &version));

    // Ficha del hechizo seleccionado (con stats del Spell crudo).
    let sheet: Option<(SkillItem, Vec<(&'static str, String)>)> = match (
        selected,
        detail.as_deref(),
        items.as_ref(),
    ) {
        (Some(index), Some(detail), Some(items)) => items.get(index).map(|item| {
            let stats = if index == 0 {
                Vec::new() // pasiva: sin HUD de stats
            } else {
                detail
                    .spells
                    .get(index - 1)
                    .map(|spell| spell_stats(spell, &detail.partype))
                    .unwrap_or_default()
            };
            (item.clone(), stats)
        }),
        _ => None,
    };

    rsx! {
        div { class: "cs-grid2",

            // COLUMNA A · BASIC KIT
            div { class: "cs-pane cs-pane--kit",
                div { class: "section-head",
                    span { class: "bar-skew bar-skew--silver", "style": "width: 24px;" }
                    span { class: "label label-md",
                        span { class: "sl sl--silver", "//" }
                        " Basic Kit"
                    }
                    span { class: "label fs-10 t-outline-var ml-auto",
                        "Hover "
                        span { class: "hud-sep", "·" }
                        " Click"
                    }
                }

                {match (items, detail.as_deref()) {
                    (Some(items), Some(_)) => rsx! {
                        div {
                            class: "cs-skills",
                            onmouseleave: move |_| hovered.set(None),
                            {items.iter().enumerate().map(|(index, item)| {
                                let row_class =
                                    skill_row_classes(selected == Some(index), pinned() == Some(index));
                                let item_key = item.key;
                                rsx! {
                                    div {
                                        class: "{row_class}",
                                        "role": "button",
                                        "tabindex": "0",
                                        "aria-pressed": if pinned() == Some(index) { "true" } else { "false" },
                                        onmouseenter: move |_| hovered.set(Some(index)),
                                        onclick: move |_| {
                                            if pinned() == Some(index) {
                                                pinned.set(None);
                                            } else {
                                                pinned.set(Some(index));
                                            }
                                        },
                                        div { class: "tile", "style": "width: 48px; height: 48px;",
                                            img { src: "{item.icon}", alt: "{item.name}" }
                                            span { class: "cs-skill-key", "{item_key}" }
                                        }
                                        div { class: "cs-skill-text",
                                            div { class: "cs-skill-name", "{item.name}" }
                                            div { class: "cs-skill-desc", "{item.desc}" }
                                        }
                                    }
                                }
                            })}
                        }
                    },
                    _ => rsx! { KitUnavailable { loading, failure } },
                }}
            }

            // COLUMNA B · PANEL CONTEXTUAL
            div { class: "cs-pane",
                {match sheet {
                    Some((item, stats)) => rsx! { SpellSheet { item, stats } },
                    None => rsx! { OrderBlock { skills, loading } },
                }}
            }
        }
    }
}

/// Estado honesto del Basic Kit sin detalle: cargando o fallo (parcial o
/// total) con la razón corta del analizador. Cero datos inventados.
#[component]
fn KitUnavailable(loading: bool, failure: Option<String>) -> Element {
    let reason = failure
        .unwrap_or_else(|| String::from("análisis no disponible"));

    rsx! {
        if loading {
            div { class: "cs-loading mt-4",
                div { class: "spinner" }
                span { class: "label label-md pulse-soft t-silver", "Cargando kit…" }
            }
        } else {
            div { class: "cs-unavailable mt-4",
                span { class: "label label-md t-error", "Habilidades no disponibles" }
                p { class: "cs-unavailable-reason", "{reason}" }
            }
        }
    }
}

/// Ficha completa del hechizo seleccionado (estilo tooltip del cliente).
#[component]
fn SpellSheet(
    item: SkillItem,
    stats: Vec<(&'static str, String)>,
) -> Element {
    let key = item.key;

    rsx! {
        div { class: "spell-view",
            div { class: "section-head",
                span { class: "bar-skew", "style": "width: 24px;" }
                span { class: "label label-md",
                    span { class: "sl", "//" }
                    " Spell Detail"
                }
            }

            article { class: "cs-sheet mt-4",
                header { class: "cs-sheet-head",
                    div { class: "tile", "style": "width: 56px; height: 56px;",
                        img { src: "{item.icon}", alt: "{item.name}" }
                    }
                    div { class: "min-w-0",
                        div { class: "cs-sheet-key", "{key}" }
                        div { class: "cs-sheet-name", "{item.name}" }
                    }
                }
                p { class: "cs-sheet-desc", "{item.desc}" }

                if !stats.is_empty() {
                    div { class: "cs-sheet-stats",
                        {stats.iter().map(|(label, value)| rsx! {
                            div { class: "cs-stat-row",
                                span { class: "label shrink-0", "{label}" }
                                span { class: "cell-value text-right", "{value}" }
                            }
                        })}
                    }
                }
            }

            span { class: "label fs-10 t-outline-var source-note", "FUENTE: DDragon" }
        }
    }
}

/// Panel "// ORDEN DE SUBIDA" con estado honesto en tres ramas: spinner solo
/// mientras el análisis del pick corre (`loading` real); prioridad visible
/// con dato verificado; y ya resuelto sin skill order (`None` o
/// [`SkillOrder::unavailable`]) → estado vacío definitivo "sin datos", no
/// carga en curso (QA: el dataset no trae kit para este champ/patch, no es
/// una caída temporal). Si una fuente verificada expone el dato algún día,
/// la prioridad fluye sin cambios de UI.
#[component]
fn OrderBlock(skills: Option<SkillOrder>, loading: bool) -> Element {
    let source_available = skill_order_available(skills.as_ref());
    let priority = skills
        .as_ref()
        .map(|skills| skills.priority.clone())
        .unwrap_or_default();
    let source = match skills.map(|skills| skills.source) {
        Some(SkillSource::Lolalytics) => "LOLALYTICS",
        Some(SkillSource::Ugg) => "U.GG",
        _ => "SIN FUENTE",
    };

    rsx! {
        div { class: "spell-view",
            div { class: "section-head",
                span { class: "bar-skew", "style": "width: 24px;" }
                span { class: "label label-md",
                    span { class: "sl", "//" }
                    " Orden de subida"
                }
            }

            if source_available {
                div { class: "mt-4",
                    span { class: "label fs-10 t-dim", "Prioridad" }
                    div { class: "num-display cs-priority-row mt-1",
                        {priority.iter().enumerate().map(|(index, key)| rsx! {
                            if index > 0 {
                                span { class: "cs-priority-arrow", "→" }
                            }
                            span { class: "cs-priority-key", "{key}" }
                        })}
                    }
                }
            } else if loading {
                div { class: "cs-loading mt-4",
                    div { class: "spinner" }
                    span { class: "label label-md pulse-soft t-silver",
                        "Consultando orden de subida…"
                    }
                }
            } else {
                // Resuelto sin skill order: estado vacío definitivo, no
                // carga en curso (cero datos fabricados).
                div { class: "cs-unavailable mt-4",
                    span { class: "label label-md t-dim",
                        "Sin datos de skill order"
                    }
                    p { class: "cs-unavailable-reason",
                        "El dataset no trae kit para este campeón en este patch."
                    }
                }
            }

            span { class: "label fs-10 t-outline-var source-note", "FUENTE: {source}" }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_dragon::champion_detail::{DetailImageDto, Passive, SpellRange};

    fn hechizo(name: &str, desc: &str, cd: &str, costo: &str) -> Spell {
        Spell {
            id: String::new(),
            name: String::from(name),
            description: String::from(desc),
            tooltip: None,
            maxrank: 5,
            cooldown: Vec::new(),
            cooldown_burn: String::from(cd),
            cost: Vec::new(),
            cost_burn: String::from(costo),
            cost_type: String::from(" {{ abilityresourcename }}"),
            range: SpellRange::default(),
            range_burn: String::from("900"),
            image: DetailImageDto {
                full: format!("{name}.png"),
            },
        }
    }

    /// Detalle mínimo de Veigar: pasiva + Q y R.
    fn detalle() -> ChampionDetail {
        ChampionDetail {
            id: String::from("Veigar"),
            name: String::from("Veigar"),
            partype: String::from("Mana"),
            passive: Passive {
                name: String::from("Phenomenal Evil Power"),
                description: String::from(
                    "Cada hechizo <font color='#FF0033'>otorga</font> AP permanente.",
                ),
                image: DetailImageDto {
                    full: String::from("VeigarEntropy.png"),
                },
            },
            spells: vec![
                hechizo("Baleful Strike", "Ráfaga de energía oscura.", "6/5.5/5/4.5/4", "40"),
                hechizo("Primordial Burst", "Explosión masiva.", "80/70/60", "100"),
            ],
            ..ChampionDetail::default()
        }
    }

    /// Pasiva primero con tecla P y hechizos después, descripciones limpias
    /// e iconos con la versión.
    #[test]
    fn skill_items_arma_pasiva_y_qwer_limpios() {
        let items = skill_items(&detalle(), "16.20.1");

        assert_eq!(items.len(), 3);
        assert_eq!(items[0].key, 'P');
        assert_eq!(items[0].name, "Phenomenal Evil Power");
        assert_eq!(items[0].desc, "Cada hechizo otorga AP permanente.");
        assert_eq!(
            items[0].icon,
            "https://ddragon.leagueoflegends.com/cdn/16.20.1/img/passive/VeigarEntropy.png"
        );

        assert_eq!(items[1].key, 'Q');
        assert_eq!(items[1].name, "Baleful Strike");
        assert_eq!(items[2].key, 'W');
        assert_eq!(
            items[2].icon,
            "https://ddragon.leagueoflegends.com/cdn/16.20.1/img/spell/Primordial Burst.png"
        );
    }

    /// Más de 4 hechizos (payload raro) se recorta a QWER.
    #[test]
    fn skill_items_recorta_a_qwer() {
        let mut detail = detalle();
        detail.spells = (0..6)
            .map(|index| hechizo(&format!("S{index}"), "desc", "10", "50"))
            .collect();
        let items = skill_items(&detail, "16.20.1");
        assert_eq!(items.len(), 5); // pasiva + 4
        let keys: Vec<char> = items.iter().map(|item| item.key).collect();
        assert_eq!(keys, vec!['P', 'Q', 'W', 'E', 'R']);
    }

    /// Detalle sin pasiva (payload capado) → solo hechizos; sin hechizos →
    /// solo pasiva.
    #[test]
    fn skill_items_tolera_detalle_capado() {
        let mut detail = detalle();
        detail.passive.name.clear();
        assert_eq!(skill_items(&detail, "16.20.1").len(), 2);

        let mut sin_spells = detalle();
        sin_spells.spells.clear();
        let items = skill_items(&sin_spells, "16.20.1");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].key, 'P');
    }

    /// Stats del hechizo: enfriamiento/costo/rango con datos y recurso
    /// resuelto contra el partype.
    #[test]
    fn spell_stats_arma_el_hud_con_el_recurso_resuelto() {
        let spell = hechizo("Q", "desc", "6/5.5/5/4.5/4", "40");
        let stats = spell_stats(&spell, "Mana");

        assert_eq!(
            stats,
            vec![
                ("Enfriamiento", String::from("6/5.5/5/4.5/4s")),
                ("Costo", String::from("40")),
                ("Rango", String::from("900")),
                ("Recurso", String::from("Mana")),
            ]
        );

        // Hechizo sin datos (payload capado): solo la fila de recurso.
        let vacio = Spell {
            range_burn: String::new(),
            ..hechizo("Rara", "", "", "")
        };
        let stats = spell_stats(&vacio, "");
        assert_eq!(stats, vec![("Recurso", String::from("—"))]);
    }

    /// Clases de fila del kit: base, live y pinned se combinan.
    #[test]
    fn skill_row_classes_combina_estados() {
        assert_eq!(skill_row_classes(false, false), "cs-skill-row");
        assert_eq!(
            skill_row_classes(true, false),
            "cs-skill-row cs-skill-row--live"
        );
        assert_eq!(
            skill_row_classes(true, true),
            "cs-skill-row cs-skill-row--live cs-skill-row--pinned"
        );
        assert_eq!(
            skill_row_classes(false, true),
            "cs-skill-row cs-skill-row--pinned"
        );
    }

    /// `skill_order_available`: solo dato con fuente y prioridad cuenta;
    /// `None` y el valor "sin datos" son estado vacío definitivo (la UI no
    /// los trata como carga en curso).
    #[test]
    fn skill_order_available_distingue_dato_de_vacio() {
        assert!(!skill_order_available(None));
        assert!(!skill_order_available(Some(&SkillOrder::unavailable())));

        let listo = SkillOrder {
            source: SkillSource::Lolalytics,
            priority: vec!['Q', 'W', 'E'],
            levels: None,
            summoners: None,
        };
        assert!(skill_order_available(Some(&listo)));
    }
}
