//! Vista MENÚ "FICHAS": grilla completa del catálogo con búsqueda.
//!
//! - Encabezado de sección (`.zone-head`): título `h-sec` "FICHAS", nota de
//!   modo (`zone-note`) y herramientas (`zone-tools`) con contador
//!   `kl-counter` (`{n}/{total} FICHAS`, n = resultados de la búsqueda) y
//!   búsqueda `kl-menu-search` ("BUSCÁ TU CAMPEÓN", filtra por nombre en
//!   vivo, case-insensitive).
//! - Grid `kl-champ-grid` con TODOS los campeones del catálogo, ordenado
//!   alfabético por nombre. Ya no existe el estado "FICHA EN CAMINO":
//!   `vs_guide_data` trae las 173 fichas embebidas, así que toda card es
//!   activa y clicable → [`EventHandler::call`] de `on_pick` con el
//!   [`ChampionInfo`] del catálogo. La navegación a la vista champ select
//!   la hace el llamador; esta vista SOLO emite el pick.
//! - Bajo el nombre de cada card, WR y partidas del historial del usuario
//!   ([`usage_por_campeon`] sobre `ProfileSummary.recent`), SOLO para
//!   campeones jugados: sin partidas no se muestra número (nada inventado).
//!
//! El header de perfil (sección A del prototipo `ui-preview-perfil.html`)
//! NO vive más acá: el perfil es una vista propia (`ProfileView`).
//! `profile` sigue llegando por la firma porque alimenta las stats de uso
//! de las cards; `on_ver_perfil` queda en la firma (app.rs lo pasa) aunque
//! esta vista ya no renderice el acceso.
//!
//! Convenciones de clases: las que la misión manda con prefijo `kl-`
//! (`kl-menu-search`, `kl-champ-grid`, `kl-champ-card`, `kl-counter`) más
//! los nombres estructurales del prototipo portados a `assets/main.css`
//! (`cut`, `zone-in`, `champ-media`, `tag-skew`, ...).

use std::collections::HashMap;

use dioxus::prelude::*;

use crate::data_dragon::models::{ChampionCatalog, ChampionEntry};
use crate::lcu::models::{AppState, ChampionInfo};
use crate::profile::models::{MatchSummary, ProfileSummary};
use crate::ui::champ_select::champ_art::champ_art;
use crate::ui::format::{format_percent, format_thousands};

/// Una card de la grilla ya resuelta contra el catálogo. Puro y testeado.
#[derive(Clone, Debug, PartialEq)]
pub struct RosterCard {
    /// Id numérico del campeón (clave de [`ChampionCatalog::by_id`]).
    pub champ_id: i64,
    /// Entrada del catálogo (nombre, slug, ddragon_id, URLs).
    pub entry: ChampionEntry,
}

impl RosterCard {
    /// [`ChampionInfo`] del campeón para `on_pick` (nombre + icono CDN del
    /// catálogo, el mismo contrato que consume el champ select).
    #[must_use]
    pub fn info(&self) -> ChampionInfo {
        ChampionInfo {
            id: self.champ_id,
            name: self.entry.name.clone(),
            image_url: self.entry.image_url.clone(),
        }
    }
}

/// Grid de fichas: filtra por `query` (nombre, case-insensitive; vacío o
/// solo espacios = todos) y ordena alfabético por nombre (desempate por id
/// para orden estable). Devuelve el catálogo completo: todas las fichas
/// están embebidas, no hay filtrado por disponibilidad.
#[must_use]
pub fn roster_cards(catalog: &ChampionCatalog, query: &str) -> Vec<RosterCard> {
    let needle = query.trim().to_lowercase();

    let mut cards: Vec<RosterCard> = catalog
        .by_id
        .iter()
        .filter(|(_, entry)| {
            needle.is_empty() || entry.name.to_lowercase().contains(&needle)
        })
        .map(|(champ_id, entry)| RosterCard {
            champ_id: *champ_id,
            entry: entry.clone(),
        })
        .collect();

    cards.sort_by(|a, b| {
        a.entry
            .name
            .to_lowercase()
            .cmp(&b.entry.name.to_lowercase())
            .then(a.champ_id.cmp(&b.champ_id))
    });
    cards
}

/// WR/partidas de un campeón jugado, para la meta de la card. Solo existe
/// si el campeón tiene al menos una partida en el historial: sin datos no
/// se muestra número (nada inventado).
#[derive(Clone, Debug, PartialEq)]
pub struct CardUsage {
    /// WR del campeón dentro del historial traído (`"60.0%"`).
    pub wr: String,
    /// Partidas del campeón dentro del historial traído (`"3"`).
    pub games: String,
}

/// Agrega WR y partidas por campeón a partir del historial del perfil
/// (puro y testeado). Solo entran campeones con al menos una partida: el
/// resto no aparece en el mapa y su card muestra solo el nombre.
#[must_use]
pub fn usage_por_campeon(recent: &[MatchSummary]) -> HashMap<i64, CardUsage> {
    // (partidas, victorias) por campeón.
    let mut totals: HashMap<i64, (i32, i32)> = HashMap::new();
    for game in recent {
        let entry = totals.entry(game.champion_id).or_insert((0, 0));
        entry.0 += 1;
        if game.win {
            entry.1 += 1;
        }
    }

    totals
        .into_iter()
        .map(|(champ_id, (games, wins))| {
            let wr = format_percent(f64::from(wins) / f64::from(games));
            (
                champ_id,
                CardUsage {
                    wr,
                    games: format_thousands(i64::from(games)),
                },
            )
        })
        .collect()
}

/// Vista MENÚ: zona "FICHAS" con búsqueda y grid del catálogo completo.
/// No navega: emite eventos y el llamador resuelve la navegación. Al
/// elegir un campeón emite `on_pick` con su [`ChampionInfo`] (→ champ
/// select).
///
/// # Props
///
/// - `catalog`: catálogo de Data Dragon (`None` mientras carga → grid con
///   "CARGANDO CATÁLOGO…" y contador sin total).
/// - `profile`: perfil LCU; solo alimenta WR/partidas de las cards
///   jugadas. (`None` → cards sin stats de uso, nunca error.)
/// - `manual_mode`: `true` cuando el menú se abre fuera de una partida en
///   curso (cambia la nota de la zona).
/// - `app_state`: sin uso desde que el header de perfil migró a la vista
///   Perfil; queda en la firma porque app.rs lo pasa.
/// - `on_pick`: canal de salida con el campeón elegido.
/// - `on_ver_perfil`: sin uso en esta vista (el perfil tiene vista
///   propia); queda en la firma porque app.rs lo pasa.
#[component]
pub fn MenuView(
    catalog: Signal<Option<ChampionCatalog>>,
    profile: Signal<Option<ProfileSummary>>,
    manual_mode: Signal<bool>,
    app_state: Signal<AppState>,
    on_pick: EventHandler<ChampionInfo>,
    on_ver_perfil: EventHandler<()>,
) -> Element {
    let mut query = use_signal(String::new);

    // Snapshots cortos: los guards no cruzan el rsx (convención del proyecto).
    let catalog_data = catalog.read().clone();
    let profile_data = profile.read().clone();
    let manual = *manual_mode.read();
    let query_text = query.read().clone();

    // Props retenidos por contrato de firma (app.rs los pasa) pero sin uso
    // en este diseño: el badge de conexión vive en la vista Perfil.
    let _ = app_state;
    let _ = on_ver_perfil;

    let total = catalog_data.as_ref().map_or(0, |data| data.by_id.len());
    // Sin catálogo no hay total honesto: "{n} FICHAS" en vez de "{n}/0".
    let counter_rest = if total > 0 {
        format!("/{total} FICHAS")
    } else {
        String::from(" FICHAS")
    };

    let cards = catalog_data
        .as_ref()
        .map_or_else(Vec::new, |data| {
            roster_cards(data, &query_text)
        });
    let mostradas = cards.len();

    // WR/partidas por campeón del historial: solo jugados (ver docs de
    // `usage_por_campeon`), el resto de las cards va sin número.
    let usage: HashMap<i64, CardUsage> = profile_data
        .as_ref()
        .map_or_else(HashMap::new, |data| usage_por_campeon(&data.recent));

    let zone_note = if manual {
        String::from("Modo manual — elegí un campeón para abrir su ficha")
    } else {
        String::from("Click para abrir la ficha — los jugados muestran WR y partidas")
    };

    rsx! {
        // ======== ZONA FICHAS ========
        div { class: "cut",
            div { class: "cut-in zone-in",
                div { class: "zone-head",
                    h2 { class: "h-sec", "Fichas" }
                    span { class: "zone-note", "{zone_note}" }
                    div { class: "zone-tools",
                        span { class: "kl-counter",
                            b { class: "tnum", "{mostradas}" }
                            "{counter_rest}"
                        }
                        input {
                            class: "kl-menu-search",
                            r#type: "text",
                            placeholder: "BUSCÁ TU CAMPEÓN",
                            "aria-label": "Buscar campeón",
                            autocomplete: "off",
                            autocorrect: "off",
                            spellcheck: "false",
                            value: "{query_text}",
                            oninput: move |e| query.set(e.value()),
                        }
                    }
                }

                div { class: "kl-champ-grid",
                    for card in &cards {
                        RosterCardView {
                            key: "{card.champ_id}",
                            champ_id: card.champ_id,
                            entry: card.entry.clone(),
                            usage: usage.get(&card.champ_id).cloned(),
                            on_pick,
                        }
                    }
                }

                if cards.is_empty() {
                    div { class: "empty",
                        span { class: "label",
                            if catalog_data.is_none() {
                                "CARGANDO CATÁLOGO DE CAMPEONES…"
                            } else if query_text.trim().is_empty() {
                                "EL CATÁLOGO ESTÁ VACÍO"
                            } else {
                                "SIN RESULTADOS PARA «{query_text}»"
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Card del grid: toda card es activa (hover a color, corner brackets,
/// "VER FICHA →" y click/Enter que emiten `on_pick`). Si el campeón tiene
/// partidas en el historial del usuario, la meta muestra su WR y partidas
/// ([`CardUsage`]); sin partidas no se muestra número.
#[component]
fn RosterCardView(
    champ_id: i64,
    entry: ChampionEntry,
    usage: Option<CardUsage>,
    on_pick: EventHandler<ChampionInfo>,
) -> Element {
    let info_click = ChampionInfo {
        id: champ_id,
        name: entry.name.clone(),
        image_url: entry.image_url.clone(),
    };
    let info_key = ChampionInfo {
        id: champ_id,
        name: entry.name.clone(),
        image_url: entry.image_url.clone(),
    };

    rsx! {
        div {
            class: "kl-champ-card",
            role: "button",
            tabindex: "0",
            "aria-label": format!("Abrir ficha de {}", entry.name),
            onclick: move |_| on_pick.call(info_click.clone()),
            onkeydown: move |e| {
                if e.key() == Key::Enter {
                    on_pick.call(info_key.clone());
                }
            },
            div { class: "champ-media",
                {match champ_art(&entry.ddragon_id) {
                    Some(art) => rsx! { img { src: art, alt: "{entry.name}" } },
                    None => rsx! { img { src: entry.loading_url(), alt: "{entry.name}" } },
                }}
                span { class: "champ-tint" }
                span { class: "champ-shade" }
                span { class: "tag-skew", span { "FICHA LISTA" } }
                span { class: "champ-open", span { "VER FICHA →" } }
            }
            div { class: "champ-meta",
                span { class: "champ-name", "{entry.name}" }
                if let Some(usage) = &usage {
                    span { class: "label label-xs tnum", "{usage.wr} · {usage.games} partidas" }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Catálogo de 3 campeones en orden de inserción NO alfabético.
    fn catalogo_fixture() -> ChampionCatalog {
        let entries = [
            ("Annie", "annie", "Annie", 1),
            ("Ahri", "ahri", "Ahri", 103),
            ("Akali", "akali", "Akali", 84),
        ];
        let by_id: HashMap<i64, ChampionEntry> = entries
            .into_iter()
            .map(|(name, slug, ddragon_id, id)| {
                (
                    id,
                    ChampionEntry {
                        name: name.to_string(),
                        image_url: format!("https://ddragon.test/{ddragon_id}.png"),
                        slug: slug.to_string(),
                        ddragon_id: ddragon_id.to_string(),
                    },
                )
            })
            .collect();

        ChampionCatalog {
            version: String::from("16.21.1"),
            by_id,
        }
    }

    /// Sin query: catálogo completo en orden alfabético por nombre. Con las
    /// 173 fichas embebidas NO se filtra por disponibilidad: entran todos.
    #[test]
    fn roster_cards_orden_alfabetico_sin_filtro_de_fichas() {
        let cards = roster_cards(&catalogo_fixture(), "");

        let names: Vec<&str> = cards.iter().map(|card| card.entry.name.as_str()).collect();
        assert_eq!(names, vec!["Ahri", "Akali", "Annie"]);
        assert_eq!(cards.len(), 3);
    }

    /// El filtro por nombre es case-insensitive y sobre el nombre visible.
    #[test]
    fn roster_cards_filtra_por_nombre_case_insensitive() {
        let catalog = catalogo_fixture();

        let cards = roster_cards(&catalog, "AN");
        let names: Vec<&str> = cards.iter().map(|card| card.entry.name.as_str()).collect();
        assert_eq!(names, vec!["Annie"]);

        let cards = roster_cards(&catalog, "ak");
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].entry.name, "Akali");

        // Query sin resultados → lista vacía, nunca panic.
        assert!(roster_cards(&catalog, "zzz").is_empty());
        // Query con espacios = sin filtro.
        assert_eq!(roster_cards(&catalog, "   ").len(), 3);
    }

    /// `info()` mapea el `ChampionInfo` que consume `on_pick`.
    #[test]
    fn roster_card_info_mapea_champion_info() {
        let cards = roster_cards(&catalogo_fixture(), "ahri");
        let info = cards[0].info();

        assert_eq!(info.id, 103);
        assert_eq!(info.name, "Ahri");
        assert_eq!(info.image_url, "https://ddragon.test/Ahri.png");
    }

    /// `usage_por_campeon`: agrega partidas y WR por campeón del historial.
    #[test]
    fn usage_agrega_por_campeon_del_historial() {
        let recent = [
            MatchSummary {
                champion_id: 103,
                win: true,
                ..MatchSummary::default()
            },
            MatchSummary {
                champion_id: 103,
                win: false,
                ..MatchSummary::default()
            },
            MatchSummary {
                champion_id: 84,
                win: true,
                ..MatchSummary::default()
            },
        ];

        let usage = usage_por_campeon(&recent);

        // Ahri: 1/2 → 50.0% en 2 partidas.
        let ahri = usage.get(&103).expect("Ahri jugó");
        assert_eq!(ahri.wr, "50.0%");
        assert_eq!(ahri.games, "2");

        // Akali: 1/1 → 100.0% en 1 partida.
        let akali = usage.get(&84).expect("Akali jugó");
        assert_eq!(akali.wr, "100.0%");
        assert_eq!(akali.games, "1");

        // Annie no jugó: no está en el mapa (su card no muestra número).
        assert!(usage.get(&1).is_none());
    }

    /// Historial vacío → mapa vacío: ninguna card inventa WR.
    #[test]
    fn usage_historial_vacio_devuelve_mapa_vacio() {
        assert!(usage_por_campeon(&[]).is_empty());
    }
}
