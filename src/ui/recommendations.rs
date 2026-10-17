//! Recomendaciones de runas en champ select (FASE 10).
//!
//! [`RuneRecommendations`] renderiza el [`RecommendationsState`] que publica
//! [`crate::stats::source::run_recommender`]: nada en `Idle`, un aviso en
//! `Loading`/`Unavailable` y, en `Ready`, una sección numerada por página
//! con etiqueta, winrate/muestra, keystone + árboles, runas y shards.
//!
//! Todo el contenido es texto plano; las líneas de cada página las produce
//! [`page_lines`], una función pura testeable sin render que resuelve
//! nombres contra el catálogo de runas de Data Dragon y degrada a `#id` y a
//! los nombres de árbol constantes ([`tree_name_fallback`]) cuando el
//! catálogo no está.
//!
//! FASE 11b-2a: el rediseño del champ select reusa la capa pura de este
//! módulo — [`tree_name`], [`rune_name`], [`format_winrate`] y el nuevo
//! [`split_page`] (filas de runas con icono y descripción para el panel
//! vertical de la nueva UI). El componente [`RuneRecommendations`] queda como
//! render legacy (sin uso desde 11b-2a; se conserva de referencia).

use dioxus::prelude::*;

use crate::data_dragon::runes::{RuneCatalog, RuneDef};
use crate::stats::source::{RecommendationsState, RunePageStats};

/// Nombre de un árbol de maestrías sin catálogo, por id (los mismos ids que
/// [`crate::stats::source::STYLE_BY_INDEX`]). El catálogo se sirve en `en_US`,
/// así que las constantes usan el nombre inglés.
#[must_use]
pub(crate) fn tree_name_fallback(tree_id: i64) -> Option<&'static str> {
    match tree_id {
        8000 => Some("Precision"),
        8100 => Some("Domination"),
        8200 => Some("Sorcery"),
        8300 => Some("Inspiration"),
        8400 => Some("Resolve"),
        _ => None,
    }
}

/// Nombre visible de un árbol: catálogo primero, constante después,
/// `#id` como último recurso.
#[must_use]
pub(crate) fn tree_name(tree_id: i64, runes: Option<&RuneCatalog>) -> String {
    if let Some(runes) = runes {
        for tree in &runes.trees {
            if i64::from(tree.id) == tree_id {
                return tree.name.clone();
            }
        }
    }
    tree_name_fallback(tree_id)
        .map(String::from)
        .unwrap_or_else(|| format!("#{tree_id}"))
}

/// Nombre visible de una runa: catálogo primero, `#id` si no figura.
#[must_use]
pub(crate) fn rune_name(rune_id: i64, runes: Option<&RuneCatalog>) -> String {
    runes
        .and_then(|runes| runes.get(rune_id))
        .map(|rune| rune.name.clone())
        .unwrap_or_else(|| format!("#{rune_id}"))
}

/// Winrate formateado sin ceros colgantes (`52` en vez de `52.00`,
/// `51.06` queda igual) para la línea de stats de la UI.
#[must_use]
pub(crate) fn format_winrate(winrate_pct: f64) -> String {
    format!("{winrate_pct:.2}")
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
}

/// Icono de un árbol: URL absoluta si el catálogo lo trae, `None` si no
/// (la UI muestra el header sin icono; nunca una imagen rota).
#[must_use]
pub(crate) fn tree_icon(tree_id: i64, runes: Option<&RuneCatalog>) -> Option<String> {
    runes
        .and_then(|runes| {
            runes
                .trees
                .iter()
                .find(|tree| i64::from(tree.id) == tree_id)
        })
        .map(|tree| crate::data_dragon::runes::rune_icon_url(&tree.icon_path))
}

/// Fila de runa lista para la UI del champ select (FASE 11b-2a): identidad
/// resuelta contra el catálogo, con degradación honesta a `#id` y sin icono
/// ni descripción cuando el catálogo no está.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuneRow {
    /// Id numérico (el `perkId`).
    pub id: i64,
    /// Nombre visible (`#id` si el catálogo no lo resuelve).
    pub name: String,
    /// URL absoluta del icono (`None` sin catálogo).
    pub icon: Option<String>,
    /// Descripción corta ya limpia de HTML (vacía sin catálogo).
    pub desc: String,
    /// Keystone según el catálogo (fila 0 de su árbol).
    pub is_keystone: bool,
}

/// Cabecera de árbol para la UI: nombre + icono opcional.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeInfo {
    /// Nombre visible (catálogo, constante o `#id`).
    pub name: String,
    /// URL absoluta del icono del árbol (`None` sin catálogo).
    pub icon: Option<String>,
}

/// Reparto de una página para el panel vertical (puro, testeado): keystone
/// destacada + runas del árbol principal (sin la keystone) + runas del
/// secundario, en el orden en que la fuente las lista.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageSplit {
    /// Árbol principal.
    pub primary_tree: TreeInfo,
    /// Keystone de la página (`None` sin catálogo o sin keystone resuelta).
    pub keystone: Option<RuneRow>,
    /// Resto de runas principales.
    pub primary: Vec<RuneRow>,
    /// Árbol secundario.
    pub secondary_tree: TreeInfo,
    /// Runas secundarias.
    pub secondary: Vec<RuneRow>,
}

/// Fila de runa resuelta contra el catálogo (o degradada).
#[must_use]
pub(crate) fn rune_row(rune_id: i64, runes: Option<&RuneCatalog>) -> RuneRow {
    match runes.and_then(|runes| runes.get(rune_id)) {
        Some(rune) => RuneRow {
            id: rune.id,
            name: rune.name.clone(),
            icon: Some(runes.expect("catálogo del Some de arriba").icon_url(rune)),
            desc: crate::ui::format::strip_html(&rune.short_desc),
            is_keystone: rune.is_keystone,
        },
        None => RuneRow {
            id: rune_id,
            name: format!("#{rune_id}"),
            icon: None,
            desc: String::new(),
            is_keystone: false,
        },
    }
}

/// Reparte los perks de una página en keystone / principales / secundarias
/// según el catálogo (los ids son la verdad, igual que
/// [`crate::stats::source::resolve_style_id`]). La keystone (si la página
/// la trae) sale del reparto y ocupa su fila destacada, resuelta o `#id`.
#[must_use]
pub fn split_page(page: &RunePageStats, runes: Option<&RuneCatalog>) -> PageSplit {
    let keystone = page.keystone_id.map(|id| rune_row(id, runes));

    let mut primary: Vec<RuneRow> = Vec::new();
    let mut secondary: Vec<RuneRow> = Vec::new();

    for perk_id in &page.perk_ids {
        // La keystone ya tiene su fila destacada.
        if page.keystone_id == Some(*perk_id) {
            continue;
        }
        let is_primary = runes
            .and_then(|runes| runes.get(*perk_id))
            .is_some_and(|rune: &RuneDef| i64::from(rune.tree_id) == page.primary_style_id);
        if is_primary {
            primary.push(rune_row(*perk_id, runes));
        } else {
            secondary.push(rune_row(*perk_id, runes));
        }
    }

    PageSplit {
        primary_tree: TreeInfo {
            name: tree_name(page.primary_style_id, runes),
            icon: tree_icon(page.primary_style_id, runes),
        },
        keystone,
        primary,
        secondary_tree: TreeInfo {
            name: tree_name(page.sub_style_id, runes),
            icon: tree_icon(page.sub_style_id, runes),
        },
        secondary,
    }
}

/// Reparte los perk ids de la página en (principales, secundarias) ya con
/// nombres visibles:
///
/// - Con catálogo: el `tree_id` de cada runa decide (los ids son la verdad,
///   igual que [`crate::stats::source::resolve_style_id`]); ids desconocidos
///   van a secundarias.
/// - Sin catálogo: convención de página 4 principales + resto secundarias
///   (ambas fuentes listan en orden de fila).
#[must_use]
fn split_perks(page: &RunePageStats, runes: Option<&RuneCatalog>) -> (Vec<String>, Vec<String>) {
    match runes {
        Some(runes) => {
            let mut primary = Vec::new();
            let mut secondary = Vec::new();
            for perk_id in &page.perk_ids {
                let is_primary = runes
                    .get(*perk_id)
                    .is_some_and(|rune| i64::from(rune.tree_id) == page.primary_style_id);
                if is_primary {
                    primary.push(rune_name(*perk_id, Some(runes)));
                } else {
                    secondary.push(rune_name(*perk_id, Some(runes)));
                }
            }
            (primary, secondary)
        }
        None => {
            let primary_count = page.perk_ids.len().min(4);
            let names: Vec<String> = page
                .perk_ids
                .iter()
                .map(|perk_id| rune_name(*perk_id, None))
                .collect();
            let primary = names[..primary_count].to_vec();
            let secondary = names[primary_count..].to_vec();
            (primary, secondary)
        }
    }
}

/// Líneas de texto de una página recomendada (puro, testeado sin render):
///
/// 1. Etiqueta (ej. `"Lolalytics · más jugada"`).
/// 2. Winrate + muestra: `"51.06% WR · 559936 partidas"`.
/// 3. Config: `"Keystone: {nombre} · Principal: {árbol} · Secundario: {árbol}"`;
///    sin keystone resuelta → `—`.
/// 4. `"Runas: {principales}"` (solo si hay).
/// 5. `"Secundarias: {a | b}"` (solo si hay).
/// 6. `"Shards: {ids}"` (solo si hay; ids crudos: 5001/5008/...).
#[must_use]
fn page_lines(page: &RunePageStats, runes: Option<&RuneCatalog>) -> Vec<String> {
    let keystone = match page.keystone_id {
        None => String::from("—"),
        Some(keystone_id) => rune_name(keystone_id, runes),
    };

    let mut lines = vec![
        page.label.clone(),
        format!(
            "{}% WR · {} partidas",
            format_winrate(page.winrate_pct),
            page.games
        ),
        format!(
            "Keystone: {keystone} · Principal: {} · Secundario: {}",
            tree_name(page.primary_style_id, runes),
            tree_name(page.sub_style_id, runes),
        ),
    ];

    let (primary, secondary) = split_perks(page, runes);
    if !primary.is_empty() {
        lines.push(format!("Runas: {}", primary.join(", ")));
    }
    if !secondary.is_empty() {
        lines.push(format!("Secundarias: {}", secondary.join(" | ")));
    }
    if !page.stat_shards.is_empty() {
        let shards: Vec<String> = page.stat_shards.iter().map(ToString::to_string).collect();
        lines.push(format!("Shards: {}", shards.join(", ")));
    }
    lines
}

/// Recomendaciones de runas para el campeón elegido (debajo del champ en
/// champ select). Texto plano: un renglón por línea de [`page_lines`], con
/// la primera numerada (`"1. ..."`) para distinguir las páginas.
#[component]
pub fn RuneRecommendations(
    state: ReadSignal<RecommendationsState>,
    runes: ReadSignal<Option<RuneCatalog>>,
) -> Element {
    // Snapshots cortos: los guards no cruzan la construcción del rsx.
    let snapshot: RecommendationsState = state.read().clone();
    let runes_snapshot: Option<RuneCatalog> = runes.read().clone();

    match snapshot {
        RecommendationsState::Idle => rsx! {},
        RecommendationsState::Loading { .. } => rsx! {
            p { "Consultando estadísticas de runas…" }
        },
        RecommendationsState::Unavailable { reason, .. } => rsx! {
            p { "Recomendaciones no disponibles: {reason}" }
        },
        RecommendationsState::Ready { pages, .. } => rsx! {
            div {
                // Una sección por página: título numerado + líneas de
                // contenido. El iterador entra directo como hijo del rsx.
                {pages.iter().enumerate().map(|(index, page)| {
                    let lines = page_lines(page, runes_snapshot.as_ref());
                    let title = match lines.first() {
                        Some(first_line) => format!("{}. {first_line}", index + 1),
                        None => format!("{}.", index + 1),
                    };
                    rsx! {
                        div {
                            p { "{title}" }
                            for line in lines.iter().skip(1) {
                                p { "{line}" }
                            }
                        }
                    }
                })}
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_dragon::runes::RuneTreeDto;
    use crate::stats::source::SourceKind;
    use serde_json::json;

    /// Fixture de catálogo (subconjunto del `runesReforged.json` real, 16.20):
    /// Dominación (8100) con sus keystones y dos runas menores, Precisión
    /// (8000) con keystones y fila de runas menores.
    fn catalogo_de_runas() -> RuneCatalog {
        let trees: Vec<RuneTreeDto> = serde_json::from_value(json!([
            {
                "id": 8100,
                "key": "Domination",
                "icon": "perk-images/Styles/7200_Domination.png",
                "name": "Domination",
                "slots": [
                    {
                        "runes": [
                            { "id": 8112, "key": "Electrocute", "icon": "a.png", "name": "Electrocute" },
                            { "id": 8128, "key": "DarkHarvest", "icon": "b.png", "name": "Dark Harvest" },
                            { "id": 9923, "key": "HailOfBlades", "icon": "c.png", "name": "Hail of Blades" }
                        ]
                    },
                    {
                        "runes": [
                            { "id": 8126, "key": "CheapShot", "icon": "d.png", "name": "Cheap Shot" },
                            { "id": 8139, "key": "TasteOfBlood", "icon": "e.png", "name": "Taste of Blood" }
                        ]
                    }
                ]
            },
            {
                "id": 8000,
                "key": "Precision",
                "icon": "perk-images/Styles/7201_Precision.png",
                "name": "Precision",
                "slots": [
                    {
                        "runes": [
                            { "id": 8005, "key": "PressTheAttack", "icon": "f.png", "name": "Press the Attack" },
                            { "id": 8008, "key": "LethalTempo", "icon": "g.png", "name": "Lethal Tempo" }
                        ]
                    },
                    {
                        "runes": [
                            { "id": 9103, "key": "LegendAlacrity", "icon": "h.png", "name": "Legend: Alacrity" },
                            { "id": 8014, "key": "CoupDeGrace", "icon": "i.png", "name": "Coup de Grace" }
                        ]
                    }
                ]
            }
        ]))
        .expect("runesReforged.json válido");
        RuneCatalog::build(String::from("16.20.1"), trees)
    }

    /// Página realista de Lolalytics: Precisión con sus 4 runas principales
    /// y Dominación con las 2 secundarias.
    fn pagina_base() -> RunePageStats {
        RunePageStats {
            source: SourceKind::Lolalytics,
            label: String::from("Lolalytics · más jugada"),
            games: 559_936,
            wins: 285_903,
            winrate_pct: 51.06,
            primary_style_id: 8000,
            sub_style_id: 8100,
            perk_ids: vec![8005, 8008, 9103, 8014, 8126, 8139],
            stat_shards: vec![5005, 5008, 5001],
            keystone_id: Some(8005),
        }
    }

    /// Con catálogo: nombres reales de runas y árboles; principales por
    /// `tree_id`, secundarias separadas con ` | `, shards como ids crudos.
    #[test]
    fn page_lines_con_catalogo_resuelve_nombres_reales() {
        let runes = catalogo_de_runas();

        assert_eq!(
            page_lines(&pagina_base(), Some(&runes)),
            vec![
                String::from("Lolalytics · más jugada"),
                String::from("51.06% WR · 559936 partidas"),
                String::from(
                    "Keystone: Press the Attack · Principal: Precision · Secundario: Domination"
                ),
                String::from("Runas: Press the Attack, Lethal Tempo, Legend: Alacrity, Coup de Grace"),
                String::from("Secundarias: Cheap Shot | Taste of Blood"),
                String::from("Shards: 5005, 5008, 5001"),
            ]
        );
    }

    /// Sin catálogo: runas → `#id`, árboles → nombres constantes; el reparto
    /// principal/secundaria cae a la convención 4 + resto.
    #[test]
    fn page_lines_sin_catalogo_cae_a_ids_y_nombres_constantes_de_arbol() {
        assert_eq!(
            page_lines(&pagina_base(), None),
            vec![
                String::from("Lolalytics · más jugada"),
                String::from("51.06% WR · 559936 partidas"),
                String::from("Keystone: #8005 · Principal: Precision · Secundario: Domination"),
                String::from("Runas: #8005, #8008, #9103, #8014"),
                String::from("Secundarias: #8126 | #8139"),
                String::from("Shards: 5005, 5008, 5001"),
            ]
        );
    }

    /// Árboles fuera del fixture pero dentro de las constantes (Valor +
    /// Brujería); sin keystone resuelta → `—`; sin shards → sin línea.
    #[test]
    fn page_lines_usa_arboles_constantes_omite_shards_vacios_y_raya_sin_keystone() {
        let page = RunePageStats {
            primary_style_id: 8400,
            sub_style_id: 8200,
            perk_ids: vec![8437],
            keystone_id: None,
            stat_shards: vec![],
            ..pagina_base()
        };

        assert_eq!(
            page_lines(&page, None),
            vec![
                String::from("Lolalytics · más jugada"),
                String::from("51.06% WR · 559936 partidas"),
                String::from("Keystone: — · Principal: Resolve · Secundario: Sorcery"),
                String::from("Runas: #8437"),
            ]
        );
    }

    /// Árbol desconocido hasta para las constantes → `#id`.
    #[test]
    fn page_lines_con_arbol_desconocido_muestra_el_id() {
        let page = RunePageStats {
            primary_style_id: 9999,
            ..pagina_base()
        };

        assert!(
            page_lines(&page, None)[2]
                .contains("Principal: #9999")
        );
    }

    /// Winrate sin ceros colgantes: enteros quedan enteros, decimales se
    /// conservan tal cual.
    #[test]
    fn format_winrate_recorta_ceros_colgantes() {
        assert_eq!(format_winrate(52.0), "52");
        assert_eq!(format_winrate(51.06), "51.06");
        assert_eq!(format_winrate(48.5), "48.5");
        assert_eq!(format_winrate(0.0), "0");
    }

    /// FASE 11b-2a: `split_page` separa keystone / principales / secundarias
    /// con iconos y descripciones limpias; los árboles traen nombre e icono.
    #[test]
    fn split_page_separa_por_arbol_con_iconos_y_desc() {
        let runes = catalogo_de_runas();
        let page = pagina_base(); // 8005 keystone + 8008/9103/8014 (8000) + 8126/8139 (8100)

        let split = split_page(&page, Some(&runes));

        assert_eq!(split.primary_tree.name, "Precision");
        assert_eq!(
            split.primary_tree.icon.as_deref(),
            Some("https://ddragon.leagueoflegends.com/cdn/img/perk-images/Styles/7201_Precision.png")
        );
        assert_eq!(split.secondary_tree.name, "Domination");

        // Keystone destacada, fuera del reparto de principales.
        let keystone = split.keystone.expect("keystone");
        assert_eq!(keystone.id, 8005);
        assert_eq!(keystone.name, "Press the Attack");
        assert!(keystone.is_keystone);

        // Principales: los otros 3 del árbol 8000, en orden.
        let primary_ids: Vec<i64> = split.primary.iter().map(|row| row.id).collect();
        assert_eq!(primary_ids, vec![8008, 9103, 8014]);
        // La keystone de la PÁGINA (8005) no aparece entre las principales;
        // 8008 conserva su flag de keystone de fila 0 (dato real del
        // catálogo: es otra keystone del árbol, solo que la página no la
        // eligió).
        assert!(split.primary.iter().all(|row| row.id != 8005));

        // Secundarias: las 2 de Dominación.
        let secondary_ids: Vec<i64> = split.secondary.iter().map(|row| row.id).collect();
        assert_eq!(secondary_ids, vec![8126, 8139]);
    }

    /// Sin catálogo: keystone y runas degradan a `#id` sin icono ni desc;
    /// los árboles caen a las constantes sin icono. Los perk ids de esta
    /// página (8005/8008/9103/8014) no están en el catálogo del fixture
    /// sin catálogo, así que todos van a secundarias salvo los que resuelve
    /// el árbol — sin catálogo no hay árbol que consulte: van TODAS a
    /// secundarias (la convención 4+resto era del render legacy).
    #[test]
    fn split_page_sin_catalogo_degrada_a_ids_y_constantes() {
        let page = pagina_base();

        let split = split_page(&page, None);

        assert_eq!(split.primary_tree.name, "Precision");
        assert_eq!(split.primary_tree.icon, None);
        assert_eq!(split.secondary_tree.name, "Domination");

        let keystone = split.keystone.expect("keystone degradada");
        assert_eq!(keystone.name, "#8005");
        assert_eq!(keystone.icon, None);
        assert_eq!(keystone.desc, "");

        assert!(split.primary.is_empty());
        let secondary_ids: Vec<i64> = split.secondary.iter().map(|row| row.id).collect();
        assert_eq!(secondary_ids, vec![8008, 9103, 8014, 8126, 8139]);
    }

    /// Página sin keystone resuelta (`None`) → fila de keystone `None` y el
    /// resto del reparto intacto.
    #[test]
    fn split_page_sin_keystone_omite_la_fila_destacada() {
        let runes = catalogo_de_runas();
        let page = RunePageStats {
            keystone_id: None,
            ..pagina_base()
        };

        let split = split_page(&page, Some(&runes));
        assert!(split.keystone.is_none());
        // 8005 ya no se saltea: es una runa más del primario.
        let primary_ids: Vec<i64> = split.primary.iter().map(|row| row.id).collect();
        assert_eq!(primary_ids, vec![8005, 8008, 9103, 8014]);
    }

    /// La descripción de una runa del catálogo llega limpia de HTML (pasa
    /// por `strip_html`).
    #[test]
    fn split_page_limpia_el_html_de_las_descripciones() {
        let trees: Vec<RuneTreeDto> = serde_json::from_value(json!([
            {
                "id": 8100,
                "key": "Domination",
                "name": "Domination",
                "slots": [
                    {
                        "runes": [
                            {
                                "id": 8112,
                                "key": "Electrocute",
                                "icon": "perk-images/Styles/Domination/Electrocute/Electrocute.png",
                                "name": "Electrocute",
                                "shortDesc": "Daña a un campeón con 3 ataques <font color='#48C4B7'>separados</font> y suma daño extra."
                            }
                        ]
                    }
                ]
            }
        ]))
        .expect("runasReforged válido");
        let runes = RuneCatalog::build(String::from("16.20.1"), trees);

        let page = RunePageStats {
            keystone_id: Some(8112),
            perk_ids: vec![8112],
            primary_style_id: 8100,
            sub_style_id: 8000,
            ..pagina_base()
        };
        let split = split_page(&page, Some(&runes));

        assert_eq!(
            split.keystone.expect("keystone").desc,
            "Daña a un campeón con 3 ataques separados y suma daño extra."
        );
    }

    /// Las constantes de árbol cubren exactamente los cinco ids oficiales.
    #[test]
    fn tree_name_fallback_cubre_los_cinco_arboles_y_nada_mas() {
        assert_eq!(tree_name_fallback(8000), Some("Precision"));
        assert_eq!(tree_name_fallback(8100), Some("Domination"));
        assert_eq!(tree_name_fallback(8200), Some("Sorcery"));
        assert_eq!(tree_name_fallback(8300), Some("Inspiration"));
        assert_eq!(tree_name_fallback(8400), Some("Resolve"));
        assert_eq!(tree_name_fallback(9000), None);
        assert_eq!(tree_name_fallback(0), None);
    }
}
