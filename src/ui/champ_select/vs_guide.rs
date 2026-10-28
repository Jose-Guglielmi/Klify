//! Tab "CÓMO GANARLE" del champ select (reemplaza la tab vieja de tips):
//! guía del RIVAL elegido combinando dos fuentes honestas:
//!
//! - **Matchup real** (SIEMPRE visible si hay rival): [`MatchupBanner`], la
//!   sección standalone entre la fila 5v5 y las tabs, con `wr_vs` sobre el
//!   [`CounterSummary`] del análisis de MI campeón — caras MI champ vs
//!   RIVAL, lane del cruce, WR grande, partidas y chip de veredicto. En
//!   modo ficha (`manual = true`) el rival es el PROPIO champ de la ficha,
//!   así que el banner muestra solo su cara (ver [`BannerCaras`]).
//! - **Ficha cualitativa** ([`super::vs_guide_data`], JSONs embebidos por
//!   slug): dossier con stats globales del rival + "lo malo de {rival}" +
//!   los tips de counterplay por counter (`counter_tips` de la ficha,
//!   visibles en ambos modos) + cómo ganarle según el arquetipo de MI
//!   champ ([`VsGuidePanel`]; esa sección se oculta en modo ficha, donde
//!   el rival ES el champ de la ficha y no aplica). Los campeones sin
//!   JSON caen al empty-state "ficha no disponible" sin inventar
//!   contenido.
//! - **Tab "CONSEJOS"** ([`ConsejosPanel`]): consejos del PROPIO campeón
//!   leídos de SU ficha embebida (combos que ganan lane, tips de manejo y
//!   plan de juego por fases), sin red; sin champ/catálogo, sin ficha o
//!   sin `playstyle` cae a empty-states honestos. Los consejos CONTRA el
//!   rival viven en la tab "Cómo ganarle" ([`VsGuidePanel`]).
//!
//! Los estados cubiertos son: sin rival (el banner desaparece y el panel
//! muestra el hint), rival sin guía (banner + empty-state) y rival con
//! guía (banner + dossier + secciones). Toda la derivación es pura y
//! testeada ([`matchup_chip_text`], [`caras_del_banner`], [`banner_view`],
//! [`top_lanes`], [`dossier_view`], [`consejos_view`]); el RSX solo pinta
//! valores ya resueltos, con los snapshots de señales tomados ANTES de
//! construirlo.

use std::collections::HashMap;

use dioxus::prelude::*;

use crate::data_dragon::models::ChampionCatalog;
use crate::lcu::models::{MatchContext, Position};
use crate::stats::aggregation::patch_of;
use crate::stats::source::analysis::{analysis_counters, AnalysisState};
use crate::stats::source::counters::wr_vs;
use crate::ui::champ_select::counters_panel::{champ_identity, is_unfavourable};
use crate::ui::format::format_thousands;
use crate::ui::recommendations::format_winrate;

use super::position_label;
use super::vs_guide_data::{guide_for_slug, GuideCounterTip, GuidePlaystyle};
use super::vs_hero::rival_lane_from_context;

/// Umbral de pickrate bajo para el chip dorado del dossier: un rival con
/// `pr < 3.0` es poco practicado (poca gente sabe jugarlo bien).
pub const LOW_PICK_THRESHOLD: f64 = 3.0;

/// Tope de chips de lane en el dossier (las tres líneas más jugadas).
pub const MAX_LANE_CHIPS: usize = 3;

/// Etiqueta corta de una lane de `lane_dist` (pura, testeada): claves de
/// Lolalytics → convención del prototipo (`"middle"` → `"MID"`). Una clave
/// desconocida se muestra en mayúsculas tal cual.
#[must_use]
pub fn lane_label(lane: &str) -> String {
    match lane {
        "top" => String::from("TOP"),
        "jungle" => String::from("JGL"),
        "middle" => String::from("MID"),
        "bottom" => String::from("BOT"),
        "utility" | "support" => String::from("SUP"),
        other => other.to_uppercase(),
    }
}

/// Top `n` lanes por participación (puro, testeado): etiqueta corta por
/// clave, filtra las lanes sin uso (`0%`), ordena descendente por
/// porcentaje (empates en orden alfabético para que el output sea
/// determinista pese al `HashMap`) y recorta a `n`.
#[must_use]
pub fn top_lanes(lane_dist: &HashMap<String, f64>, n: usize) -> Vec<(String, f64)> {
    let mut lanes: Vec<(String, f64)> = lane_dist
        .iter()
        .filter(|(_, pct)| **pct > 0.0)
        .map(|(lane, pct)| (lane_label(lane), *pct))
        .collect();
    lanes.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    lanes.truncate(n);
    lanes
}

/// Texto del chip de veredicto del banner de matchup (puro, testeado):
/// con WR de la muestra y partidas → desfavorable (`< 50%`, convención de
/// [`is_unfavourable`]) o parejo; sin WR o sin partidas → sin veredicto.
#[must_use]
pub fn matchup_chip_text(vs_winrate: Option<f64>, games: i64) -> &'static str {
    match vs_winrate {
        Some(wr) if games > 0 => {
            if is_unfavourable(wr) {
                "MATCHUP DESFAVORABLE"
            } else {
                "MATCHUP PAREJO"
            }
        }
        _ => "SIN MUESTRA SUFICIENTE",
    }
}

/// Qué caras pinta el banner (puro, testeado): `Espejo` cuando el rival
/// es MI propio campeón — cruce que solo ocurre en modo ficha (`manual`,
/// el padre setea `rival` con el id de la ficha), porque en un draft real
/// los picks son únicos por partida y nadie se enfrenta a sí mismo — y
/// `Duelo` en cualquier otro caso.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BannerCaras {
    /// Solo la cara del rival con su frame (`BannerFace`): "MI icono VS
    /// icono rival" sería X vs X y ese matchup no existe.
    Espejo,
    /// MI cara + VS dorado + la cara del rival.
    Duelo,
}

/// Resuelve [`BannerCaras`] para el cruce (puro, testeado).
#[must_use]
fn caras_del_banner(my_champion_id: i64, rival_id: i64) -> BannerCaras {
    if rival_id == my_champion_id {
        BannerCaras::Espejo
    } else {
        BannerCaras::Duelo
    }
}

/// Vista plana del banner de matchup standalone (pura, testeada): todo lo
/// que el RSX pinta, resuelto antes de construirlo.
#[derive(Clone, Debug, PartialEq)]
struct MatchupBannerView {
    /// Nombre de MI champ (para el `alt` del icono).
    my_name: String,
    /// Icono cuadrado de MI champ (`None` sin versión/catálogo).
    my_icon: Option<String>,
    /// Nombre visible del rival (catálogo o `#id`).
    rival_name: String,
    /// Icono cuadrado del rival (`None` sin versión/catálogo).
    rival_icon: Option<String>,
    /// Lane del cruce (la del rival lockeado o la mía; `""` sin dato).
    lane_label: &'static str,
    /// WR de MI champ contra el rival + partidas (`None` fuera de muestra).
    matchup: Option<(f64, i64)>,
    /// Parche de la muestra (para la meta del banner).
    patch: String,
}

/// Entradas de [`banner_view`] agrupadas en un struct (mi champ elegido,
/// el rival y el entorno de draft) para que el helper puro quede por
/// debajo del límite de argumentos de clippy.
#[derive(Clone)]
struct BannerInputs {
    /// ID de MI champ elegido.
    my_champion_id: i64,
    /// Nombre de MI champ (para el `alt` del icono).
    my_name: String,
    /// MI posición asignada (fallback de la lane del cruce).
    my_position: Option<Position>,
    /// ID del rival lockeado.
    rival_id: i64,
    /// Contexto de draft (lane del rival lockeado).
    context: MatchContext,
    /// Catálogo de campeones (`None` degrada a `#id` sin icono).
    catalog: Option<ChampionCatalog>,
    /// Análisis de MI champ (counters con `wr_vs` reales).
    analysis: AnalysisState,
    /// Versión de data dragon para URLs de iconos (vacía = sin URLs).
    version: String,
}

/// Arma la vista del banner (pura, testeada): identidad de ambas caras
/// contra el catálogo con [`champ_identity`], lane del cruce con
/// [`rival_lane_from_context`] y matchup real sobre los counters del
/// análisis (mismo dato honesto que consume la fila 5v5). Toma las
/// entradas agrupadas en [`BannerInputs`].
#[must_use]
fn banner_view(inputs: &BannerInputs) -> MatchupBannerView {
    let BannerInputs {
        my_champion_id,
        my_name,
        my_position,
        rival_id,
        context,
        catalog,
        analysis,
        version,
    } = inputs;
    let (_, my_icon) = champ_identity(*my_champion_id, catalog.as_ref(), version);
    let (rival_name, rival_icon) = champ_identity(*rival_id, catalog.as_ref(), version);
    let lane_label =
        position_label(rival_lane_from_context(context, Some(*rival_id), *my_position));
    let matchup = analysis_counters(analysis, *my_champion_id)
        .as_deref()
        .and_then(|summary| wr_vs(&summary.counters, *rival_id))
        .map(|entry| (entry.vs_winrate, entry.games));
    MatchupBannerView {
        my_name: my_name.clone(),
        my_icon,
        rival_name,
        rival_icon,
        lane_label,
        matchup,
        patch: patch_of(version),
    }
}

/// Banner de matchup standalone (sección propia entre la fila 5v5 y las
/// tabs): caras MI champ vs RIVAL con frame rojo, título grande de lane y
/// WR real del matchup con meta de la muestra y chip de veredicto. Sin
/// rival elegido la sección entera no se renderiza; si el rival es MI
/// propio champ (modo ficha) solo se muestra su cara — ver [`BannerCaras`].
#[component]
pub(crate) fn MatchupBanner(
    my_champion_id: i64,
    my_champion_name: String,
    my_position: Option<Position>,
    match_context: ReadSignal<MatchContext>,
    rival: Signal<Option<i64>>,
    catalog: ReadSignal<Option<ChampionCatalog>>,
    analysis: ReadSignal<AnalysisState>,
    version: Option<String>,
) -> Element {
    // Sin rival elegido no hay matchup que mostrar.
    let Some(rival_id) = rival() else {
        return rsx! {};
    };

    // Snapshots cortos: los guards no cruzan la construcción del rsx.
    let catalog_snapshot = catalog.read().clone();
    let analysis_snapshot: AnalysisState = analysis.read().clone();
    let context: MatchContext = match_context.read().clone();

    // Versión efectiva para URLs de iconos: la del prop y, si viene vacía,
    // la del catálogo (nunca una URL con la versión vacía).
    let version_str = version
        .as_deref()
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| catalog_snapshot.as_ref().map(|data| data.version.clone()))
        .unwrap_or_default();

    let MatchupBannerView {
        my_name,
        my_icon,
        rival_name,
        rival_icon,
        lane_label,
        matchup,
        patch,
    } = banner_view(&BannerInputs {
        my_champion_id,
        my_name: my_champion_name,
        my_position,
        rival_id,
        context,
        catalog: catalog_snapshot,
        analysis: analysis_snapshot,
        version: version_str,
    });

    // El modificador pinta el fondo/borde Y el color del número (CSS hijo);
    // `cut-tr-bl` recorta las esquinas del prototipo sin duplicar borde.
    let banner_mod = match matchup {
        Some((wr, _)) if is_unfavourable(wr) => " vg-matchup--bad",
        Some(_) => " vg-matchup--good",
        None => "",
    };

    // Chip de veredicto: solo con muestra (sin datos no hay veredicto).
    let chip = matchup.map(|(wr, games)| {
        (
            if is_unfavourable(wr) {
                "chip-result vg-chip-bad"
            } else {
                "chip-result vg-chip-neutral"
            },
            matchup_chip_text(Some(wr), games),
        )
    });

    // En modo ficha el rival es el PROPIO champ de la ficha: "MI icono VS
    // icono rival" sería X vs X (en draft real ese cruce no existe). Se
    // elige mostrar solo la cara del rival con su frame — misma ancla
    // visual del banner con las clases existentes, sin CSS nuevo — en vez
    // de ocultar la fila de caras entera.
    let caras = caras_del_banner(my_champion_id, rival_id);

    rsx! {
        div { class: "vg-matchup cut-tr-bl{banner_mod}",
            // Caras: MI icono (frame rojo) + VS dorado + icono del rival;
            // solo la cara del rival en modo ficha (BannerCaras::Espejo).
            div { "style": "display: flex; align-items: center; gap: 10px;",
                if caras == BannerCaras::Duelo {
                    BannerFace { name: my_name, icon: my_icon }
                    span { class: "cs5-vs", "VS" }
                }
                BannerFace { name: &rival_name, icon: rival_icon }
            }

            // "// MATCHUP" + título grande de lane.
            div {
                div { class: "label label-md",
                    span { class: "sl", "//" }
                    " Matchup"
                }
                h2 {
                    class: "num-display",
                    "style": "font-size: 26px; line-height: 1; letter-spacing: 0.02em; text-transform: uppercase; margin: 3px 0 0;",
                    if !lane_label.is_empty() {
                        "{lane_label} "
                    }
                    span { "style": "color: var(--vs-gold);", "vs" }
                    " {rival_name}"
                }
            }

            // WR del matchup: número grande (el color lo decide el
            // modificador del banner) + meta de la muestra.
            div { "style": "display: flex; flex-direction: column; align-items: flex-end; gap: 2px; margin-left: auto; text-align: right;",
                {match matchup {
                    Some((wr, games)) => rsx! {
                        span { class: "vg-matchup-num", "{format_winrate(wr)}%" }
                        span { class: "vg-matchup-meta",
                            "{format_thousands(games.max(0))} partidas · platinum+ · patch {patch}"
                        }
                    },
                    None => rsx! {
                        span { class: "vg-matchup-meta", "Sin datos de matchup todavía" }
                    },
                }}
            }

            // Veredicto del matchup (solo con muestra).
            {match chip {
                Some((chip_class, chip_text)) => rsx! {
                    span { class: "{chip_class}", "{chip_text}" }
                },
                None => rsx! {},
            }}
        }
    }
}

/// Cara del banner (MI champ o el rival): icono cuadrado 44px con frame
/// rojo y esquina cortada 8px, como el prototipo; sin icono → `#`.
#[component]
fn BannerFace(name: String, icon: Option<String>) -> Element {
    rsx! {
        div {
            class: "frame",
            "style": "position: relative; background: var(--primary); clip-path: polygon(0 0, calc(100% - 8px) 0, 100% 8px, 100% 100%, 0 100%);",
            {match icon {
                Some(icon) => rsx! {
                    img {
                        "style": "display: block; width: 44px; height: 44px; object-fit: cover; clip-path: polygon(0 0, calc(100% - 8px) 0, 100% 8px, 100% 100%, 0 100%);",
                        src: "{icon}",
                        alt: "{name}",
                    }
                },
                None => rsx! {
                    span { class: "cs-tile-fallback", "#" }
                },
            }}
        }
    }
}

/// Panel de la tab "Cómo ganarle": dossier del rival, lo malo, los tips
/// de counterplay de su ficha y cómo ganarle según MI champ. El banner
/// del matchup real ahora es la sección standalone [`MatchupBanner`]
/// (fuera del panel); acá solo vive la guía cualitativa, con empty-states
/// honestos para los campeones sin JSON. Con `manual == true` (modo
/// ficha) el rival ES el campeón de la ficha — el padre setea la señal
/// `rival` con ese id y la tab no lleva selector — así que la sección
/// "según tu champ" se oculta (no hay matchup contra MI champ que
/// condicione el consejo) y el resto lee como la guía de ese campeón,
/// incluidos los counter tips: contenido del rival, sirve igual en los
/// dos modos. El hint sin rival queda como fallback: el padre del modo
/// ficha siempre setea `rival`.
#[component]
pub(crate) fn VsGuidePanel(
    rival: Signal<Option<i64>>,
    catalog: ReadSignal<Option<ChampionCatalog>>,
    version: Option<String>,
    manual: bool,
) -> Element {
    // Snapshots cortos: los guards no cruzan la construcción del rsx.
    let rival_id = rival();
    let catalog_snapshot = catalog.read().clone();

    // Versión efectiva para URLs de iconos: la del prop y, si viene vacía,
    // la del catálogo (nunca una URL con la versión vacía).
    let version_str = version
        .as_deref()
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| catalog_snapshot.as_ref().map(|data| data.version.clone()))
        .unwrap_or_default();

    // Identidad del rival (nombre + icono; degrada a "#id" sin catálogo).
    let (rival_name, rival_icon) = match rival_id {
        Some(id) => champ_identity(id, catalog_snapshot.as_ref(), &version_str),
        None => (String::from("—"), None),
    };

    // Ficha cualitativa por slug del catálogo (None → campeón sin JSON).
    let slug = rival_id
        .and_then(|id| catalog_snapshot.as_ref().and_then(|data| data.get(id)))
        .map(|entry| entry.slug.clone())
        .unwrap_or_default();
    let guide = (!slug.is_empty()).then(|| guide_for_slug(&slug)).flatten();

    // Derivación pura ANTES del rsx. El cualitativo va aparte porque las
    // guías `depth=basic` no lo traen: dossier sí, secciones cualitativas no.
    let dossier =
        guide
            .as_ref()
            .map(|guide| dossier_view(&guide.meta.title_es, guide.stats.wr, guide.stats.pr, guide.stats.br, &guide.stats.lane_dist));
    let qualitative = guide.as_ref().and_then(|guide| guide.qualitative.clone());
    let weaknesses: Vec<String> = qualitative
        .as_ref()
        .map(|qualitative| qualitative.weaknesses.clone())
        .unwrap_or_default();
    let has_weaknesses = !weaknesses.is_empty();

    // Tips de counterplay de la ficha: cada counter del rival y cómo le
    // gana. En ambos modos es contenido del rival; sin tips no hay
    // sección (los JSON `depth=basic` ni siquiera los traen).
    let counter_tips: Vec<CounterTipItem> = guide
        .as_ref()
        .and_then(|guide| guide.qualitative.as_ref())
        .map(|qualitative| counter_tip_items(&qualitative.counter_tips))
        .unwrap_or_default();
    let has_counter_tips = !counter_tips.is_empty();

    // Mini-cards por arquetipo: título + variante de color + bullets, ya
    // filtradas (un grupo vacío no se muestra).
    let arch_cards: Vec<(&'static str, &'static str, Vec<String>)> = qualitative
        .as_ref()
        .map(|qualitative| {
            [
                ("Si jugás RANGO", "", &qualitative.vs_archetypes.range),
                ("Si jugás TANQUE", "", &qualitative.vs_archetypes.tanks),
                (
                    "Si jugás ASESINO",
                    " vg-arch-title--red",
                    &qualitative.vs_archetypes.assassins,
                ),
                (
                    "Si jugás CON CC",
                    " vg-arch-title--cyan",
                    &qualitative.vs_archetypes.crowd_control,
                ),
            ]
            .into_iter()
            .filter(|(_, _, bullets)| !bullets.is_empty())
            .map(|(title, title_mod, bullets)| (title, title_mod, bullets.clone()))
            .collect()
        })
        .unwrap_or_default();
    let has_arch_cards = !arch_cards.is_empty();

    // Hint del estado sin rival (fallback: en modo ficha el padre siempre
    // setea `rival` con el champ de la ficha, así que no debería verse).
    // En manual no hay fila 5v5 que clickee; con lobby real sí, y el hint
    // apunta a ella.
    let empty_hint = if manual {
        "Elegí un rival para ver cómo ganarle"
    } else {
        "Elegí un enemigo en la fila de arriba para ver cómo ganarle"
    };

    rsx! {
        div { class: "vg",
            {match rival_id {
                // Sin rival elegido: hint elegante, nada más (el banner
                // standalone de arriba tampoco se renderiza).
                None => rsx! {
                    div { class: "vg-empty",
                        p { class: "cs-hint", "{empty_hint}" }
                    }
                },
                Some(_) => rsx! {
                    // Header del panel: "// Cómo ganarle a {rival}"
                    div { class: "vg-head",
                        span { class: "bar-skew", "style": "width: 24px;" }
                        span { class: "label label-md",
                            span { class: "sl", "//" }
                            " Cómo ganarle a {rival_name}"
                        }
                    }

                    {match dossier {
                        Some(view) => rsx! {
                            GuideDossier {
                                name: rival_name.clone(),
                                icon: rival_icon.clone(),
                                view,
                            }

                            // "// LO MALO DE {RIVAL}"
                            if has_weaknesses {
                                div { class: "vg-sec",
                                    span { class: "bar-skew", "style": "width: 24px;" }
                                    span { class: "label label-md",
                                        span { class: "sl", "//" }
                                        " Lo malo de {rival_name}"
                                    }
                                }
                                WeaknessList { items: weaknesses }
                            }

                            // "// CÓMO GANARLE CON CADA COUNTER": los
                            // tips de counterplay de la ficha (qué
                            // campeones le ganan y con qué jugadas).
                            // Orden elegido: primero "lo malo" (el
                            // diagnóstico de por qué es vencible) y
                            // después el cómo, accionado counter por
                            // counter. Sin guard de `manual`: es
                            // contenido del rival y en modo ficha es LA
                            // respuesta concreta a "cómo ganarle".
                            if has_counter_tips {
                                div { class: "vg-sec",
                                    span { class: "bar-skew", "style": "width: 24px;" }
                                    span { class: "label label-md",
                                        span { class: "sl", "//" }
                                        " Cómo ganarle con cada counter"
                                    }
                                }
                                {counter_tips.into_iter().enumerate().map(|(index, item)| rsx! {
                                    CounterTipCard { item, key: "ctip-{index}" }
                                })}
                            }

                            // "// CÓMO GANARLE SEGÚN TU CHAMP": oculto en
                            // modo ficha — el champ de la ficha no es un
                            // rival distinto de MI champ, así que el
                            // consejo por arquetipo no aplica.
                            if !manual && has_arch_cards {
                                div { class: "vg-sec",
                                    span { class: "bar-skew", "style": "width: 24px;" }
                                    span { class: "label label-md",
                                        span { class: "sl", "//" }
                                        " Cómo ganarle según tu champ"
                                    }
                                }
                                div { class: "vg-arch-grid",
                                    {arch_cards.into_iter().map(|(title, title_mod, bullets)| rsx! {
                                        ArchetypeCard { title, title_mod, bullets, key: "{title}" }
                                    })}
                                }
                            }
                        },
                        // Sin JSON embebido: empty-state honesto (el banner
                        // de arriba, fuera del panel, sigue siendo dato real).
                        None => rsx! {
                            div { class: "vg-empty",
                                span { class: "label label-md",
                                    span { class: "sl", "//" }
                                    " Ficha no disponible"
                                }
                                p { class: "cs-hint",
                                    "Todavía no hay ficha cualitativa de {rival_name}. El matchup de arriba es dato real de Lolalytics."
                                }
                            }
                        },
                    }}
                },
            }}
        }
    }
}

/// Dossier del rival: icono + nombre + título localizado y stats HUD
/// (WR global con color semántico, PR, BAN, top lanes y chip de pick bajo).
#[component]
fn GuideDossier(name: String, icon: Option<String>, view: DossierView) -> Element {
    let wr_class = if is_unfavourable(view.wr) { "t-bright" } else { "t-on" };

    rsx! {
        div { class: "frame",
            div { class: "vg-dossier",
                div { class: "vg-dossier-head",
                    div { class: "tile", "style": "width: 64px; height: 64px;",
                        {match icon {
                            Some(icon) => rsx! { img { src: "{icon}", alt: "{name}" } },
                            None => rsx! { span { class: "cs-tile-fallback", "#" } },
                        }}
                    }
                    div {
                        div { class: "vg-dossier-name", "{name}" }
                        p { class: "cs-hint", "{view.title_es}" }
                    }
                }
                div { class: "vg-dossier-stats",
                    div {
                        span { class: "label fs-10 t-dim", "WR global" }
                        div { class: "num-display tnum {wr_class}", "{format_winrate(view.wr)}%" }
                    }
                    div {
                        span { class: "label fs-10 t-dim", "PR" }
                        div { class: "num-display tnum t-on", "{format_winrate(view.pr)}%" }
                    }
                    div {
                        span { class: "label fs-10 t-dim", "BAN" }
                        div { class: "num-display tnum t-on", "{format_winrate(view.br)}%" }
                    }
                    div { class: "vg-roles",
                        {view.lanes.iter().enumerate().map(|(index, (role, pct))| rsx! {
                            span {
                                class: if index == 0 {
                                    "vg-role-tile vg-role-tile--gold"
                                } else {
                                    "vg-role-tile"
                                },
                                key: "lane-{role}",
                                "{role}"
                                b { "{format_winrate(*pct)}%" }
                            }
                        })}
                        if view.low_pick {
                            span { class: "vg-chip-gold", "BAJO PICK = POCO PRACTICADO" }
                        }
                    }
                }
            }
        }
    }
}

/// Vista plana del dossier del rival (pura): lo que el RSX pinta de la
/// guía, sin tipos de `vs_guide_data` de por medio.
#[derive(Clone, Debug, PartialEq)]
struct DossierView {
    /// Título localizado del rival (ej. `"la Asesina Furtiva"`).
    title_es: String,
    /// WR global del rival (color semántico sobre 50 en la UI).
    wr: f64,
    /// Pickrate global del rival.
    pr: f64,
    /// Banrate global del rival.
    br: f64,
    /// Top lanes por participación, ya ordenadas y recortadas.
    lanes: Vec<(String, f64)>,
    /// `true` si el pickrate cae bajo [`LOW_PICK_THRESHOLD`] (chip dorado).
    low_pick: bool,
}

/// Arma la vista del dossier (pura, testeada): recorta las lanes con
/// [`top_lanes`] y marca el pick bajo contra [`LOW_PICK_THRESHOLD`].
#[must_use]
fn dossier_view(
    title_es: &str,
    wr: f64,
    pr: f64,
    br: f64,
    lane_dist: &HashMap<String, f64>,
) -> DossierView {
    DossierView {
        title_es: title_es.to_string(),
        wr,
        pr,
        br,
        lanes: top_lanes(lane_dist, MAX_LANE_CHIPS),
        low_pick: pr < LOW_PICK_THRESHOLD,
    }
}

/// Lista de debilidades del rival: cada fila con el marcador ✕ rojo.
#[component]
fn WeaknessList(items: Vec<String>) -> Element {
    rsx! {
        ul { class: "vg-weak",
            {items.iter().enumerate().map(|(index, text)| rsx! {
                li { key: "weak-{index}",
                    span { class: "vg-weak-x", "✕" }
                    p { "{text}" }
                }
            })}
        }
    }
}

/// Mini-card de arquetipo ("Si jugás RANGO / TANQUE / ASESINO / CON CC")
/// con los bullets de la guía. Los grupos vacíos no llegan acá.
#[component]
fn ArchetypeCard(
    title: &'static str,
    title_mod: &'static str,
    bullets: Vec<String>,
) -> Element {
    rsx! {
        div { class: "vg-arch",
            h4 { class: "vg-arch-title{title_mod}", "{title}" }
            ul {
                {bullets.iter().enumerate().map(|(index, text)| rsx! {
                    li { key: "arch-{index}", "{text}" }
                })}
            }
        }
    }
}

/// Ítem de counterplay ya plano para el RSX: el counter y sus tips, sin
/// tipos de `vs_guide_data` de por medio (mismo patrón que [`DossierView`]).
#[derive(Clone, Debug, PartialEq)]
struct CounterTipItem {
    /// Nombre visible del counter, ej. `"Talon"`.
    name: String,
    /// Tips concretos para ese matchup, en el orden de la ficha.
    tips: Vec<String>,
}

/// Arma los ítems de counterplay (pura, testeada): conserva el orden de
/// la ficha y descarta los counters sin tips (un título suelto sin
/// consejos no se pinta).
#[must_use]
fn counter_tip_items(counter_tips: &[GuideCounterTip]) -> Vec<CounterTipItem> {
    counter_tips
        .iter()
        .filter(|tip| !tip.tips.is_empty())
        .map(|tip| CounterTipItem {
            name: tip.name.clone(),
            tips: tip.tips.clone(),
        })
        .collect()
}

/// Card de counterplay contra el rival: el counter como título bold
/// (la marca de `vg-arch-title`) y sus tips tenues como bullets
/// (`.vg-arch ul li` ya trae el guioncito rojo sesgado y el gris de
/// texto). A lo ancho y apiladas por el `gap` de `.vg`, sin
/// `vg-arch-grid`: cada tip es una oración larga y las columnas de 220px
/// la harían ilegibles.
#[component]
fn CounterTipCard(item: CounterTipItem) -> Element {
    rsx! {
        div { class: "vg-arch",
            h4 { class: "vg-arch-title", "{item.name}" }
            ul {
                {item.tips.iter().enumerate().map(|(index, tip)| rsx! {
                    li { key: "ctip-{index}", "{tip}" }
                })}
            }
        }
    }
}

// ---------------------------------------------------------------------
// Tab CONSEJOS (ConsejosPanel): consejos de MI campeón desde su ficha
// ---------------------------------------------------------------------

/// Vista plana de la tab CONSEJOS (pura, testeada): combos, tips y las
/// fases del plan con texto. Lo que el RSX pinta, sin tipos de
/// `vs_guide_data` de por medio.
#[derive(Clone, Debug, PartialEq)]
struct ConsejosView {
    /// Combos que ganan lane, en el orden de la ficha.
    combos: Vec<String>,
    /// Tips de manejo, en el orden de la ficha.
    tips: Vec<String>,
    /// Fases del plan con texto no vacío, en orden early → mid → late.
    plan: Vec<(&'static str, String)>,
}

/// Arma la vista de la tab CONSEJOS (pura, testeada): a partir del
/// `playstyle` de MI champ separa combos, tips y las fases del plan con
/// texto (una fase vacía no se pinta). `None` si el playstyle no trae
/// nada utilizable, para caer al empty-state honesto.
#[must_use]
fn consejos_view(playstyle: &GuidePlaystyle) -> Option<ConsejosView> {
    let plan = playstyle
        .plan
        .as_ref()
        .map(|plan| {
            [
                ("EARLY", &plan.early),
                ("MID", &plan.mid),
                ("LATE", &plan.late),
            ]
            .into_iter()
            .filter_map(|(phase, text)| {
                let text = text.as_deref()?;
                (!text.is_empty()).then_some((phase, text.to_owned()))
            })
            .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    if playstyle.combos.is_empty() && playstyle.tips.is_empty() && plan.is_empty() {
        return None;
    }
    Some(ConsejosView {
        combos: playstyle.combos.clone(),
        tips: playstyle.tips.clone(),
        plan,
    })
}

/// Tab CONSEJOS: consejos del PROPIO campeón, desde SU ficha embebida
/// (sin red): combos que ganan lane, tips de manejo y plan de juego por
/// fases. Sin champ/catálogo (slug `None`), con ficha sin `playstyle` o
/// con un `playstyle` vacío → empty-states honestos.
#[component]
pub(crate) fn ConsejosPanel(my_slug: Option<String>) -> Element {
    // Ficha de MI campeón por slug + nombre visible para los mensajes
    // (el slug viene del catálogo; `None` → no hay champ ni catálogo).
    let guide = my_slug.as_deref().and_then(guide_for_slug);
    let name = guide.as_ref().map(|guide| guide.meta.name.clone());

    // Derivación pura ANTES del rsx: `None` acá = sin ficha, ficha
    // `depth=basic` sin `playstyle`, o `playstyle` sin nada utilizable.
    let view = guide
        .as_ref()
        .and_then(|guide| guide.qualitative.as_ref())
        .and_then(|qualitative| qualitative.playstyle.as_ref())
        .and_then(consejos_view);

    rsx! {
        div { class: "vg",
            {match view {
                Some(view) => {
                    let ConsejosView { combos, tips, plan } = view;
                    rsx! {
                        // "// COMBOS QUE GANAN LANE": lista numerada con
                        // el chip rojo sesgado.
                        if !combos.is_empty() {
                            div { class: "vg-sec",
                                span { class: "bar-skew", "style": "width: 24px;" }
                                span { class: "label label-md",
                                    span { class: "sl", "//" }
                                    " Combos que ganan lane"
                                }
                            }
                            ComboList { items: combos }
                        }

                        // "// TIPS DE MANEJO": bullets.
                        if !tips.is_empty() {
                            div { class: "vg-sec",
                                span { class: "bar-skew", "style": "width: 24px;" }
                                span { class: "label label-md",
                                    span { class: "sl", "//" }
                                    " Tips de manejo"
                                }
                            }
                            TipsList { items: tips }
                        }

                        // "// PLAN DE JUEGO": una card por fase con texto
                        // (las fases vacías no llegan acá).
                        if !plan.is_empty() {
                            div { class: "vg-sec",
                                span { class: "bar-skew", "style": "width: 24px;" }
                                span { class: "label label-md",
                                    span { class: "sl", "//" }
                                    " Plan de juego"
                                }
                            }
                            div { class: "vg-arch-grid",
                                {plan.into_iter().map(|(phase, body)| rsx! {
                                    PlanCard { title: phase, body, key: "{phase}" }
                                })}
                            }
                        }
                    }
                }
                // Sin slug/catálogo, sin ficha o sin `playstyle` utilizable:
                // empty honesto (el matchup real sigue en "Cómo ganarle").
                None => rsx! {
                    div { class: "vg-empty",
                        span { class: "label label-md",
                            span { class: "sl", "//" }
                            " Ficha no disponible aún"
                        }
                        {match name {
                            Some(name) => rsx! {
                                p { class: "cs-hint",
                                    "La ficha de {name} todavía no tiene consejos de juego."
                                }
                            },
                            None => rsx! {
                                p { class: "cs-hint",
                                    "Todavía no hay ficha de juego para este campeón."
                                }
                            },
                        }}
                    }
                },
            }}
        }
    }
}

/// Lista numerada de combos: la misma caja del prototipo (`vg-weak`) con
/// el número en el chip rojo sesgado (`vg-weak-x`), que ya da el "número
/// rojo" sin CSS nuevo.
#[component]
fn ComboList(items: Vec<String>) -> Element {
    rsx! {
        ul { class: "vg-weak",
            {items.iter().enumerate().map(|(index, text)| rsx! {
                li { key: "combo-{index}",
                    span { class: "vg-weak-x", "{index + 1}" }
                    p { "{text}" }
                }
            })}
        }
    }
}

/// Lista de tips de manejo con bullets: reusa la caja y el marcador rojo
/// sesgado de `vg-arch` (`.vg-arch ul li`) sin CSS nuevo.
#[component]
fn TipsList(items: Vec<String>) -> Element {
    rsx! {
        div { class: "vg-arch",
            ul {
                {items.iter().enumerate().map(|(index, text)| rsx! {
                    li { key: "tip-{index}", "{text}" }
                })}
            }
        }
    }
}

/// Card de una fase del plan (EARLY/MID/LATE): título display sobre la
/// marca de `vg-arch` + texto corrido con la misma tipografía de sus
/// bullets (inline, sin CSS nuevo).
#[component]
fn PlanCard(title: &'static str, body: String) -> Element {
    rsx! {
        div { class: "vg-arch",
            h4 { class: "vg-arch-title", "{title}" }
            p {
                "style": "margin: 0; font-size: 12.5px; line-height: 1.45; color: var(--on-surface-var);",
                "{body}"
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_dragon::models::ChampionEntry;
    use crate::lcu::models::TeamMember;
    use crate::stats::source::counters::{CounterEntry, CounterSummary};
    use crate::stats::source::skills::SkillOrder;
    use crate::ui::champ_select::vs_guide_data::{GuideCounterTip, GuidePlan};
    use std::sync::Arc;

    /// Mapa de lanes a partir de pares (clave Lolalytics, porcentaje).
    fn lanes(values: &[(&str, f64)]) -> HashMap<String, f64> {
        values
            .iter()
            .map(|(lane, pct)| ((*lane).to_string(), *pct))
            .collect()
    }

    /// Catálogo mínimo: Zed (238) y Akali (34), versión pineada 16.20.1.
    fn catalog() -> ChampionCatalog {
        let mut by_id = HashMap::new();
        for (id, name, ddragon) in [(238, "Zed", "Zed"), (34, "Akali", "Akali")] {
            by_id.insert(
                id,
                ChampionEntry {
                    name: String::from(name),
                    image_url: format!("https://example/{ddragon}.png"),
                    slug: String::from(name),
                    ddragon_id: String::from(ddragon),
                },
            );
        }
        ChampionCatalog {
            version: String::from("16.20.1"),
            by_id,
        }
    }

    /// Chip del banner: < 50% desfavorable, ≥ 50% parejo, sin muestra →
    /// sin veredicto.
    #[test]
    fn matchup_chip_distingue_desfavorable_parejo_y_sin_datos() {
        assert_eq!(matchup_chip_text(Some(45.2), 142_900), "MATCHUP DESFAVORABLE");
        assert_eq!(matchup_chip_text(Some(49.99), 1), "MATCHUP DESFAVORABLE");
        assert_eq!(matchup_chip_text(Some(50.0), 12_649), "MATCHUP PAREJO");
        assert_eq!(matchup_chip_text(Some(55.8), 931), "MATCHUP PAREJO");
        // Sin WR o sin partidas: no hay veredicto que mostrar.
        assert_eq!(matchup_chip_text(None, 500), "SIN MUESTRA SUFICIENTE");
        assert_eq!(matchup_chip_text(Some(52.0), 0), "SIN MUESTRA SUFICIENTE");
    }

    /// Banner standalone: caras resueltas contra el catálogo, lane del
    /// cruce (la del rival lockeado manda) y matchup real sobre counters.
    #[test]
    fn banner_view_resuelve_caras_lane_y_matchup_real() {
        let catalog = catalog();
        let context = MatchContext {
            enemies: vec![TeamMember {
                cell_id: 1,
                champion_id: Some(34),
                position: Some(Position::Middle),
                summoner_id: None,
                summoner_name: None,
            }],
            ..MatchContext::default()
        };
        let analysis = AnalysisState::Partial {
            champion_id: 238,
            detail: None,
            counters: Some(Arc::new(CounterSummary {
                my_wr: Some(51.0),
                pickrate: Some(9.0),
                analysed: Some(48_400),
                counters: vec![CounterEntry {
                    champion_id: 34,
                    vs_winrate: 45.2,
                    games: 142_900,
                }],
            })),
            items: None,
            skills: SkillOrder::unavailable(),
            reason: String::from("sin habilidades"),
        };

        let view = banner_view(&BannerInputs {
            my_name: String::from("Zed"),
            my_champion_id: 238,
            my_position: Some(Position::Jungle),
            context,
            rival_id: 34,
            catalog: Some(catalog),
            analysis,
            version: String::from("16.20.1"),
        });

        assert_eq!(view.my_name, "Zed");
        assert!(view.my_icon.is_some(), "con versión hay icono mío");
        assert_eq!(view.rival_name, "Akali");
        assert!(view.rival_icon.is_some());
        assert_eq!(
            view.lane_label, "MID",
            "la lane del rival lockeado manda sobre la mía"
        );
        assert_eq!(view.matchup, Some((45.2, 142_900)));
        assert_eq!(view.patch, "16.20");
    }

    /// Banner sin muestra, rival fuera del catálogo y sin versión en el
    /// prop: `#id`, MI lane y cero URLs/parches inventados.
    #[test]
    fn banner_view_degrada_honesto_sin_datos() {
        let catalog = catalog();

        let view = banner_view(&BannerInputs {
            my_name: String::from("Zed"),
            my_champion_id: 238,
            my_position: Some(Position::Utility),
            context: MatchContext::default(),
            rival_id: 999,
            catalog: Some(catalog),
            analysis: AnalysisState::Idle,
            version: String::new(),
        });

        assert_eq!(view.rival_name, "#999");
        assert_eq!(view.rival_icon, None, "sin versión no hay URL de icono");
        assert_eq!(view.my_icon, None, "sin versión no hay URL de icono mío");
        assert_eq!(view.lane_label, "SUP", "rival sin casilla → MI posición");
        assert_eq!(view.matchup, None, "análisis Idle → sin WR");
        assert_eq!(view.patch, "", "sin versión no hay parche que mostrar");
    }

    /// laneDist: etiquetas cortas, orden desc por participación, filtro de
    /// lanes sin uso y recorte a tres chips.
    #[test]
    fn top_lanes_ordena_desc_recorta_a_tres_y_filtra_ceros() {
        let lanes = lanes(&[
            ("top", 29.2),
            ("middle", 70.2),
            ("jungle", 0.0),
            ("support", 5.0),
        ]);

        let top = top_lanes(&lanes, 3);
        assert_eq!(
            top,
            vec![
                (String::from("MID"), 70.2),
                (String::from("TOP"), 29.2),
                (String::from("SUP"), 5.0),
            ],
            "jungle al 0% queda afuera y el orden es por participación"
        );

        // Pedir menos de lo que hay recorta sin paniquear.
        assert_eq!(top_lanes(&lanes, 2).len(), 2);
        // Mapa vacío → sin chips.
        assert!(top_lanes(&HashMap::new(), 3).is_empty());
    }

    /// Empates: orden alfabético para que el output no dependa del orden
    /// interno del HashMap.
    #[test]
    fn top_lanes_desempata_alfabeticamente() {
        let lanes = lanes(&[("support", 10.0), ("top", 10.0)]);
        assert_eq!(
            top_lanes(&lanes, 3),
            vec![(String::from("SUP"), 10.0), (String::from("TOP"), 10.0)]
        );
    }

    /// Etiquetas cortas de lane, con fallback en mayúsculas para claves
    /// desconocidas.
    #[test]
    fn lane_label_usa_las_etiquetas_cortas_del_prototipo() {
        assert_eq!(lane_label("top"), "TOP");
        assert_eq!(lane_label("jungle"), "JGL");
        assert_eq!(lane_label("middle"), "MID");
        assert_eq!(lane_label("bottom"), "BOT");
        assert_eq!(lane_label("utility"), "SUP");
        assert_eq!(lane_label("support"), "SUP");
        assert_eq!(lane_label("arena"), "ARENA", "clave desconocida → mayúsculas");
    }

    /// Dossier: chip de pick bajo contra el umbral y lanes ya recortadas.
    #[test]
    fn dossier_view_marca_el_pick_bajo() {
        let view = dossier_view("la Prueba", 49.9, 2.99, 1.0, &lanes(&[("middle", 80.0)]));
        assert!(view.low_pick, "PR < 3.0 → chip dorado");
        assert_eq!(view.lanes, vec![(String::from("MID"), 80.0)]);
        assert_eq!(view.title_es, "la Prueba");

        let normal = dossier_view("Otra", 51.0, 7.68, 15.92, &HashMap::new());
        assert!(!normal.low_pick, "PR 7.68 ≥ 3.0 → sin chip");
        assert!(normal.lanes.is_empty());
        assert!((normal.wr - 51.0).abs() < f64::EPSILON);
        assert!((normal.pr - 7.68).abs() < f64::EPSILON);
        assert!((normal.br - 15.92).abs() < f64::EPSILON);

        // Justo en el umbral NO es pick bajo (comparación estricta).
        assert!(!dossier_view("Borde", 50.0, 3.0, 0.0, &HashMap::new()).low_pick);
    }

    /// Playstyle de prueba: `n` combos, `m` tips y plan opcional
    /// (early, mid, late).
    fn playstyle(
        combos: usize,
        tips: usize,
        plan: Option<(&str, &str, &str)>,
    ) -> GuidePlaystyle {
        GuidePlaystyle {
            combos: (0..combos).map(|i| format!("combo {i}")).collect(),
            tips: (0..tips).map(|i| format!("tip {i}")).collect(),
            plan: plan.map(|(early, mid, late)| GuidePlan {
                early: Some(String::from(early)),
                mid: Some(String::from(mid)),
                late: Some(String::from(late)),
            }),
        }
    }

    /// Vista de la tab desde un playstyle completo: combos y tips llegan
    /// en el orden de la ficha y el plan queda etiquetado EARLY/MID/LATE.
    #[test]
    fn consejos_view_arma_combos_tips_y_plan() {
        let view = consejos_view(&playstyle(
            3,
            2,
            Some(("farmá temprano", "rotá con tu jungla", "cerrá por mid")),
        ))
        .expect("hay contenido");

        assert_eq!(
            view.combos,
            vec![
                String::from("combo 0"),
                String::from("combo 1"),
                String::from("combo 2"),
            ]
        );
        assert_eq!(view.tips, vec![String::from("tip 0"), String::from("tip 1")]);
        assert_eq!(
            view.plan.iter().map(|(phase, _)| *phase).collect::<Vec<_>>(),
            vec!["EARLY", "MID", "LATE"]
        );
        assert_eq!(view.plan[0].1, "farmá temprano");
        assert_eq!(view.plan[2].1, "cerrá por mid");
    }

    /// Vista parcial y vacía: una fase sin texto no se pinta; sin plan
    /// queda solo lo que haya; sin nada utilizable → `None` (empty-state).
    #[test]
    fn consejos_view_descarta_fases_vacias_y_da_none_si_no_hay_nada() {
        // Plan con la fase del medio vacía: solo quedan EARLY y LATE.
        let view = consejos_view(&playstyle(1, 0, Some(("temprano", "", "tarde"))))
            .expect("hay combos");
        assert_eq!(
            view.plan,
            vec![
                ("EARLY", String::from("temprano")),
                ("LATE", String::from("tarde")),
            ]
        );

        // Sin plan: la vista solo trae tips.
        let view = consejos_view(&playstyle(0, 2, None)).expect("hay tips");
        assert!(view.combos.is_empty());
        assert_eq!(view.tips.len(), 2);
        assert!(view.plan.is_empty());

        // Sin nada que mostrar (ni siquiera con plan todo vacío): None.
        assert_eq!(consejos_view(&playstyle(0, 0, None)), None);
        assert_eq!(consejos_view(&playstyle(0, 0, Some(("", "", "")))), None);
    }

    /// Caras del banner: contra MI propio champ (cruce que solo ocurre en
    /// modo ficha — en un draft real los picks son únicos por partida) va
    /// una sola cara; contra cualquier otro rival, el duelo completo.
    #[test]
    fn caras_del_banner_espeja_solo_contra_mi_propio_champ() {
        assert_eq!(caras_del_banner(238, 34), BannerCaras::Duelo);
        assert_eq!(caras_del_banner(238, 999), BannerCaras::Duelo);
        assert_eq!(
            caras_del_banner(238, 238),
            BannerCaras::Espejo,
            "rival == MI champ → solo la cara del rival"
        );
    }

    /// Counter tip de prueba: nombre + tips (vacío para el caso sin datos).
    fn counter_tip(name: &str, tips: &[&str]) -> GuideCounterTip {
        GuideCounterTip {
            name: String::from(name),
            tips: tips.iter().map(|tip| String::from(*tip)).collect(),
        }
    }

    /// Counter tips: se conservan el orden y los tips de cada counter y
    /// las entradas sin tips quedan afuera (nada de títulos sueltos).
    #[test]
    fn counter_tip_items_conserva_orden_y_filtra_los_sin_tips() {
        let items = counter_tip_items(&[
            counter_tip("Talon", &["castigá antes del 6", "pegate a la pared"]),
            counter_tip("Kassadin", &[]),
            counter_tip("Lissandra", &["ruteala al aterrizar"]),
        ]);

        assert_eq!(
            items,
            vec![
                CounterTipItem {
                    name: String::from("Talon"),
                    tips: vec![
                        String::from("castigá antes del 6"),
                        String::from("pegate a la pared"),
                    ],
                },
                CounterTipItem {
                    name: String::from("Lissandra"),
                    tips: vec![String::from("ruteala al aterrizar")],
                },
            ],
            "Kassadin no trae tips → no se pinta; el orden de la ficha se conserva"
        );

        // Sin tips que mostrar (nada o solo counters vacíos) → sin sección.
        assert!(counter_tip_items(&[]).is_empty());
        assert!(counter_tip_items(&[counter_tip("Kassadin", &[])]).is_empty());
    }
}
