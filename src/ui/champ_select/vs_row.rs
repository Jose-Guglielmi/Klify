//! Fila 5v5 del champ select (prototipo aprobado, sección "CHAMP SELECT"
//! de `ui-preview-sylas.html`): mi equipo a la izquierda — MI card más
//! alta, con borde rojo y candado — badge "VS" dorado al centro y los
//! enemigos a la derecha. Los enemigos lockeados son clickeables (click o
//! Enter/Space) y eligen al OBJETIVO de la guía; el que comparte mi lane
//! lleva el chip "RIVAL DIRECTO". Mi lane es la posición asignada y, si el
//! payload real no la trajo (`assignedPosition` vacía/`UNSELECTED` →
//! `None`), se infiere de las lanes de mis aliados ([`infer_my_position`]).
//!
//! Arte siempre visible, en cascada: retrato vertical de Data Dragon
//! (`loading_url`, 308×560, centrado en el campeón) → cutout local
//! (`champ_art`, recurso para cuando no hay red; el CSS lo muestra
//! contain, sin recortar) → slot "?" gris cuando el id no figura en el
//! catálogo (campeón desconocido: sin nombre inventado, sin chips y no
//! clicable). Las clases `cs5-*` van SOLO como strings: el CSS real
//! vive en main.css (otra misión).
//!
//! Toda la derivación es pura y testeada: [`lane_opponent`],
//! [`my_team_slots`], [`enemy_slots`], [`card_visual`], [`card_models`] y
//! [`slot_key`]; el RSX solo pinta.

use dioxus::prelude::*;

use crate::data_dragon::models::ChampionCatalog;
use crate::lcu::models::{MatchContext, Position, TeamMember};

use super::position_label;

/// Casillas por lado en la fila (5v5).
const ROW_SLOTS: usize = 5;

/// El enemigo cuya posición coincide con la mía (rival directo de lane)
/// (puro, testeado): entre los lockeados (`champion_id: Some`), el primero
/// con mi misma posición. Sin posición propia o sin coincidencia → `None`.
/// Puro a propósito: NO ve el catálogo, así que puede devolver un id
/// desconocido — es [`card_models`] (que sí lo ve) el que lo descarta y
/// deja a todos sin chip.
#[must_use]
pub fn lane_opponent(enemies: &[TeamMember], my_position: Option<Position>) -> Option<i64> {
    let mine = my_position?;
    enemies
        .iter()
        .filter(|enemy| enemy.champion_id.is_some())
        .find(|enemy| enemy.position == Some(mine))
        .and_then(|enemy| enemy.champion_id)
}

/// Las cinco posiciones clásicas del draft (orden TOP/JGL/MID/BOT/SUP).
const DRAFT_POSITIONS: [Position; 5] = [
    Position::Top,
    Position::Jungle,
    Position::Middle,
    Position::Bottom,
    Position::Utility,
];

/// Mi posición cuando el payload no la trajo (puro, testeado): con datos
/// LCU reales la `assignedPosition` del miembro LOCAL a veces llega vacía o
/// `UNSELECTED` (→ `None`) mientras que las lanes de los DEMÁS sí se
/// reportan — el pill "MID" del rival de lane sale de ese mismo campo del
/// lado enemigo, y por eso el chip "RIVAL DIRECTO" era el único destacado
/// que desaparecía. Si mis aliados cubren exactamente cuatro de las cinco
/// posiciones —nadie sin lane, sin duplicados—, la libre es la mía.
/// Cualquier otro caso es ambiguo → `None` (honesto: sin dato no hay chip).
#[must_use]
fn infer_my_position(allies: &[TeamMember]) -> Option<Position> {
    // Si algún aliado tampoco tiene lane, el hueco libre no es
    // inequívocamente mío: mejor sin chip que con un rival inventado.
    if allies.iter().any(|ally| ally.position.is_none()) {
        return None;
    }
    let claimed: Vec<Position> = allies.iter().filter_map(|ally| ally.position).collect();
    let free: Vec<Position> = DRAFT_POSITIONS
        .iter()
        .copied()
        .filter(|position| !claimed.contains(position))
        .collect();
    if free.len() == 1 {
        Some(free[0])
    } else {
        None
    }
}

/// Card de un lado de la fila (pura, testeada): miembro real o casilla de
/// relleno.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RowSlot {
    /// `true` = MI card (más alta, borde rojo, candado).
    pub(crate) is_me: bool,
    /// `cellId` del draft (clave estable del reconciliador).
    pub(crate) cell_id: i64,
    /// Campeón lockeado (`None` = eligiendo / casilla vacía).
    pub(crate) champion_id: Option<i64>,
    /// Posición asignada por la cola, si la trajo.
    pub(crate) position: Option<Position>,
    /// `false` = relleno sin miembro (placeholder gris).
    pub(crate) filled: bool,
}

impl RowSlot {
    /// Casilla de relleno: card gris con "?" (o la posición, si hubiera).
    fn empty() -> Self {
        Self {
            is_me: false,
            cell_id: -1,
            champion_id: None,
            position: None,
            filled: false,
        }
    }
}

/// Orden del draft: TOP, JGL, MID, BOT, SUP; sin posición al final.
#[must_use]
fn position_order(position: Option<Position>) -> u8 {
    match position {
        Some(Position::Top) => 0,
        Some(Position::Jungle) => 1,
        Some(Position::Middle) => 2,
        Some(Position::Bottom) => 3,
        Some(Position::Utility) => 4,
        None => u8::MAX,
    }
}

/// Lado izquierdo (puro, testeado): YO + aliados en orden TOP/JGL/MID/BOT/
/// SUP (estable; sin posición al final), rellenado a [`ROW_SLOTS`] con
/// casillas vacías.
#[must_use]
pub(crate) fn my_team_slots(
    my_champion_id: i64,
    my_position: Option<Position>,
    allies: &[TeamMember],
) -> Vec<RowSlot> {
    let mut slots: Vec<RowSlot> = Vec::with_capacity(allies.len() + 1);
    slots.push(RowSlot {
        is_me: true,
        // Sin celda real: la clave de esta card es "me".
        cell_id: i64::MIN,
        champion_id: Some(my_champion_id),
        position: my_position,
        filled: true,
    });
    slots.extend(allies.iter().map(|member| RowSlot {
        is_me: false,
        cell_id: member.cell_id,
        champion_id: member.champion_id,
        position: member.position,
        filled: true,
    }));
    slots.sort_by_key(|slot| position_order(slot.position));
    slots.resize(ROW_SLOTS, RowSlot::empty());
    slots.truncate(ROW_SLOTS);
    slots
}

/// Lado derecho (puro, testeado): enemigos en el orden del draft, rellenado
/// a [`ROW_SLOTS`].
#[must_use]
pub(crate) fn enemy_slots(enemies: &[TeamMember]) -> Vec<RowSlot> {
    let mut slots: Vec<RowSlot> = enemies
        .iter()
        .map(|member| RowSlot {
            is_me: false,
            cell_id: member.cell_id,
            champion_id: member.champion_id,
            position: member.position,
            filled: true,
        })
        .collect();
    slots.resize(ROW_SLOTS, RowSlot::empty());
    slots.truncate(ROW_SLOTS);
    slots
}

/// Visual de una card ya resuelto contra el catálogo (puro, testeado).
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum CardVisual {
    /// Arte visible: primario = retrato vertical de Data Dragon centrado
    /// en el campeón (`cutout: false`); `cutout: true` = asset local sin
    /// fondo, recurso para sin red (el CSS lo muestra contain, no cover).
    Art {
        /// URL del arte.
        src: String,
        /// `true` → clase `cs5-art--cutout`.
        cutout: bool,
        /// Nombre del catálogo.
        name: String,
    },
    /// Card gris sin arte: miembro eligiendo, casilla vacía o campeón
    /// desconocido (lock cuyo id el catálogo no resuelve).
    Dim {
        /// Texto de la card ("ELIGIENDO…", "?" o placeholder de posición).
        label: String,
    },
}

impl CardVisual {
    /// `true` solo cuando el catálogo resolvió al campeón (hay arte): es
    /// la condición para clickear la card y marcarle chips. Un lock sin
    /// entrada en el catálogo es un campeón desconocido y nunca es
    /// interactivo.
    #[must_use]
    fn resuelto(&self) -> bool {
        matches!(self, Self::Art { .. })
    }
}

/// Visual de una card (puro, testeado): lockeado con entrada en el
/// catálogo → arte en cascada (retrato de Data Dragon centrado en el
/// campeón → cutout local para sin red); lockeado que el catálogo no
/// resuelve (id nuevo, o catálogo ausente/cargando) → campeón desconocido,
/// slot "?" SIN nombre inventado; sin lock → "ELIGIENDO…"; casilla vacía
/// → placeholder con la posición.
#[must_use]
pub(crate) fn card_visual(slot: &RowSlot, catalog: Option<&ChampionCatalog>) -> CardVisual {
    match slot.champion_id {
        Some(id) => match catalog.and_then(|data| data.get(id)) {
            // PRIMARIO: retrato 308×560 de Data Dragon, centrado en el
            // campeón (el cutout webp se recorta mal en cards angostas).
            Some(entry) => CardVisual::Art {
                src: entry.loading_url(),
                cutout: false,
                name: entry.name.clone(),
            },
            // DESCONOCIDO: id fuera del catálogo (o catálogo aún sin
            // cargar) → slot "?" sin nombre inventado. Antes caía un
            // "#id" que parecía un campeón real y hasta era clicable.
            None => CardVisual::Dim {
                label: String::from("?"),
            },
        },
        None if slot.filled => CardVisual::Dim {
            label: String::from("ELIGIENDO…"),
        },
        None => {
            let position = position_label(slot.position);
            CardVisual::Dim {
                label: if position.is_empty() {
                    String::from("?")
                } else {
                    position.to_string()
                },
            }
        }
    }
}

/// Lado de la fila: mi equipo (pills azules) o enemigo (pills rojas).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Side {
    /// Mi equipo (yo + aliados).
    Mine,
    /// Equipo enemigo (clickeable cuando lockea).
    Enemy,
}

/// Card lista para pintar (pura): slot + visual + destacados del lado
/// enemigo.
#[derive(Clone, Debug, PartialEq)]
struct CardModel {
    /// Slot de origen (clave y pill de posición).
    slot: RowSlot,
    /// Visual resuelto contra el catálogo.
    visual: CardVisual,
    /// Chip "RIVAL DIRECTO" (solo enemigos).
    rival_directo: bool,
    /// Chip "OBJETIVO" + clase `is-target` (solo enemigos).
    target: bool,
}

/// Un lock solo cuenta si el catálogo lo resuelve (puro, testeado): sin
/// catálogo, o con un id que no figura en él (campeón más nuevo que la
/// versión cacheada de ddragon), el campeón es desconocido.
#[must_use]
fn resuelto(champion_id: Option<i64>, catalog: Option<&ChampionCatalog>) -> bool {
    champion_id.is_some_and(|id| catalog.is_some_and(|data| data.get(id).is_some()))
}

/// Modelo de cada card de un lado (puro, testeado): el [`lane_opponent`]
/// lleva `rival_directo` y el rival elegido `target` — pero SOLO si el
/// catálogo resuelve ese id: un campeón desconocido no es rival de nadie
/// ni puede ser objetivo. Ambos destacados quedan en `false` para mi
/// equipo.
#[must_use]
fn card_models(
    slots: &[RowSlot],
    side: Side,
    catalog: Option<&ChampionCatalog>,
    rival_directo: Option<i64>,
    rival_id: Option<i64>,
) -> Vec<CardModel> {
    let is_enemy = side == Side::Enemy;
    // El filtrado por catálogo vive acá (y no en lane_opponent, que es
    // puro): si el id destacado no resuelve, no hay chip en nadie.
    let rival_directo_valido = rival_directo.filter(|id| resuelto(Some(*id), catalog));
    let objetivo_valido = rival_id.filter(|id| resuelto(Some(*id), catalog));
    slots
        .iter()
        .map(|slot| {
            let locked = slot.champion_id;
            CardModel {
                slot: slot.clone(),
                visual: card_visual(slot, catalog),
                rival_directo: is_enemy
                    && rival_directo_valido.is_some()
                    && rival_directo_valido == locked,
                target: is_enemy && objetivo_valido.is_some() && objetivo_valido == locked,
            }
        })
        .collect()
}

/// Clave estable para el reconciliador: cell_id real; índice para los
/// rellenos; "me" para MI card.
#[must_use]
fn slot_key(prefix: &str, slot: &RowSlot, index: usize) -> String {
    if slot.is_me {
        String::from("me")
    } else if slot.filled {
        format!("{prefix}-{}", slot.cell_id)
    } else {
        format!("{prefix}-fill-{index}")
    }
}

/// Fila 5v5: mi equipo + VS + enemigos clickeables (objetivo de la guía).
///
/// Re-renderiza cuando el contexto (locks en vivo) o el catálogo publican;
/// los snapshots se toman ANTES del rsx (el guard no cruza la
/// construcción). Click (o Enter/Space) sobre un enemigo lockeado Y
/// resuelto en el catálogo lo convierte en el objetivo de la guía vía
/// `rival`; un desconocido no es clicable.
#[component]
pub(crate) fn VsTeamRow(
    my_champion_id: i64,
    my_position: Option<Position>,
    match_context: ReadSignal<MatchContext>,
    catalog: ReadSignal<Option<ChampionCatalog>>,
    rival: Signal<Option<i64>>,
) -> Element {
    // Snapshots cortos: el guard no cruza la construcción del rsx.
    let context: MatchContext = match_context.read().clone();
    let catalog_snapshot: Option<ChampionCatalog> = catalog.read().clone();
    let rival_id = rival();

    // Derivación pura antes del rsx: lados rellenados + modelo por card.
    // Posición efectiva para el rival de lane: la asignada por la cola y,
    // si el payload real no la trajo, la inferida de las lanes de mis
    // aliados (ver [`infer_my_position`]). Solo alimenta el chip: MI card
    // sigue mostrando únicamente lo que reportó la cola.
    let my_position_efectiva = my_position.or_else(|| infer_my_position(&context.allies));
    let rival_directo = lane_opponent(&context.enemies, my_position_efectiva);
    let mine = card_models(
        &my_team_slots(my_champion_id, my_position, &context.allies),
        Side::Mine,
        catalog_snapshot.as_ref(),
        rival_directo,
        rival_id,
    );
    let enemies = card_models(
        &enemy_slots(&context.enemies),
        Side::Enemy,
        catalog_snapshot.as_ref(),
        rival_directo,
        rival_id,
    );

    // Click (o Enter/Space) en un enemigo lockeado → objetivo de la guía.
    let on_pick = move |picked: i64| rival.set(Some(picked));

    rsx! {
        div { class: "cs5-scroll",
            div { class: "cs5-row",
                {mine.iter().enumerate().map(|(index, card)| {
                    let key = slot_key("mine", &card.slot, index);
                    rsx! {
                        RowCard {
                            key: "{key}",
                            card: card.clone(),
                            side: Side::Mine,
                            on_pick,
                        }
                    }
                })}

                // Badge VS dorado entre equipos.
                div { class: "cs5-vs", "VS" }

                {enemies.iter().enumerate().map(|(index, card)| {
                    let key = slot_key("enemy", &card.slot, index);
                    rsx! {
                        RowCard {
                            key: "{key}",
                            card: card.clone(),
                            side: Side::Enemy,
                            on_pick,
                        }
                    }
                })}
            }

            p { class: "cs5-hint", "▸ CLICK EN UN ENEMIGO PARA VER CÓMO GANARLE" }
        }
    }
}

/// Una card de la fila (yo / aliado / enemigo / casilla vacía). Solo los
/// enemigos con campeón resuelto en el catálogo son interactivos: eligen
/// al objetivo de la guía. Un lock desconocido queda como "?" muerto.
#[component]
fn RowCard(card: CardModel, side: Side, on_pick: EventHandler<i64>) -> Element {
    let slot = &card.slot;
    // Solo un campeón RESUELTO en el catálogo es clicable: un id
    // desconocido nunca elige al objetivo.
    let selectable = side == Side::Enemy && card.visual.resuelto();

    // Wrapper: lado + destacados (`is-target` va sobre el wrapper objetivo).
    let mut classes = String::from("cs5-slot");
    if slot.is_me {
        classes.push_str(" cs5-slot--me");
    } else if side == Side::Enemy {
        classes.push_str(" cs5-slot--enemy");
    }
    if card.rival_directo {
        classes.push_str(" cs5-slot--rival-directo");
    }
    if card.target {
        classes.push_str(" cs5-slot--target is-target");
    }
    if selectable {
        classes.push_str(" is-selectable");
    }

    // Nombre: XL en MI card; LG en el rival directo u objetivo.
    let name_class = if slot.is_me {
        "cs5-name cs5-name--xl"
    } else if card.rival_directo || card.target {
        "cs5-name cs5-name--lg"
    } else {
        "cs5-name"
    };

    // Pill de posición: roja en MI card y enemigos, azul para aliados.
    let pill_class = if slot.is_me || side == Side::Enemy {
        "cs5-pill cs5-pill--red"
    } else {
        "cs5-pill cs5-pill--blue"
    };
    let position = position_label(slot.position);
    let show_pill = slot.filled && !position.is_empty();

    let name_text = match &card.visual {
        CardVisual::Art { name, .. } => name.clone(),
        CardVisual::Dim { label } => label.clone(),
    };

    // Arte SIEMPRE visible: retrato de ddragon centrado → cutout local
    // (sin red) → "?" gris (id desconocido, sin lock o casilla vacía).
    let art = match &card.visual {
        CardVisual::Art { src, cutout, name } => rsx! {
            img {
                class: if *cutout { "cs5-art cs5-art--cutout" } else { "cs5-art" },
                src: "{src}",
                alt: "{name}",
            }
        },
        CardVisual::Dim { .. } => rsx! {
            div { class: "cs5-art cs5-art--empty", "?" }
        },
    };

    let inner = rsx! {
        div { class: "cs5-frame",
            div { class: "cs5-card",
                {art}
                div { class: "cs5-fade" }
                div { class: "cs5-info",
                    div { class: "{name_class}", "{name_text}" }
                    div { class: "cs5-chips",
                        if card.rival_directo {
                            span { class: "cs5-chip-rival", "RIVAL DIRECTO" }
                        }
                        if card.target {
                            span { class: "cs5-chip-target", "OBJETIVO" }
                        }
                        if show_pill {
                            span { class: "{pill_class}", "{position}" }
                        }
                        if slot.is_me {
                            span {
                                class: "cs5-lock",
                                title: "Lockeado",
                                "aria-hidden": "true",
                                svg {
                                    view_box: "0 0 24 24",
                                    width: "13",
                                    height: "13",
                                    fill: "currentColor",
                                    path { d: "M12 2a5 5 0 0 0-5 5v3H6a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-8a2 2 0 0 0-2-2h-1V7a5 5 0 0 0-5-5zm-3 8V7a3 3 0 1 1 6 0v3H9z" }
                                }
                            }
                        }
                    }
                }
            }
        }
    };

    if selectable {
        // Enemigo lockeado: botón accesible que elige al objetivo.
        let pick_id = slot.champion_id;
        rsx! {
            div {
                class: "{classes}",
                "role": "button",
                "tabindex": "0",
                "aria-pressed": if card.target { "true" } else { "false" },
                "aria-label": "Ver cómo ganarle a {name_text}",
                onclick: move |_| {
                    if let Some(id) = pick_id {
                        on_pick.call(id);
                    }
                },
                onkeydown: move |event| {
                    let key = event.key();
                    if key == Key::Enter || key == Key::Character(" ".to_string()) {
                        if let Some(id) = pick_id {
                            on_pick.call(id);
                        }
                    }
                },
                {inner}
            }
        }
    } else {
        rsx! {
            div { class: "{classes}", {inner} }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    use crate::data_dragon::models::ChampionEntry;

    /// Miembro de prueba.
    fn member(cell: i64, champion_id: Option<i64>, position: Option<Position>) -> TeamMember {
        TeamMember {
            cell_id: cell,
            champion_id,
            position,
        }
    }

    /// Entrada mínima de catálogo.
    fn entry(ddragon: &str) -> ChampionEntry {
        ChampionEntry {
            name: String::from(ddragon),
            image_url: format!("https://example/{ddragon}.png"),
            slug: String::from(ddragon),
            ddragon_id: String::from(ddragon),
        }
    }

    /// Catálogo con Akali (tiene cutout local) y "NuevoChamp" (sin asset).
    fn catalogo() -> ChampionCatalog {
        let mut by_id = HashMap::new();
        by_id.insert(34, entry("Akali"));
        by_id.insert(999, entry("NuevoChamp"));
        ChampionCatalog {
            version: String::from("16.20.1"),
            by_id,
        }
    }

    /// Catálogo mínimo con ids numéricos genéricos: para probar ids que
    /// resuelven contra otros que no.
    fn catalogo_con(ids: &[i64]) -> ChampionCatalog {
        let mut by_id = HashMap::new();
        for &id in ids {
            by_id.insert(id, entry(&format!("Campeon{id}")));
        }
        ChampionCatalog {
            version: String::from("16.20.1"),
            by_id,
        }
    }

    // ---- lane_opponent ----

    /// Rival directo: entre los lockeados, el primero con mi posición.
    #[test]
    fn lane_opponent_encuentra_al_primero_de_mi_posicion() {
        let enemies = vec![
            member(10, Some(64), Some(Position::Jungle)),
            member(11, Some(34), Some(Position::Middle)),
            member(12, Some(238), Some(Position::Middle)),
        ];

        assert_eq!(
            lane_opponent(&enemies, Some(Position::Middle)),
            Some(34),
            "el primero que matchea gana"
        );
        assert_eq!(lane_opponent(&enemies, Some(Position::Jungle)), Some(64));
        assert_eq!(
            lane_opponent(&enemies, Some(Position::Bottom)),
            None,
            "nadie en BOT"
        );
        assert_eq!(lane_opponent(&[], Some(Position::Top)), None);
    }

    /// Sin mi posición no hay rival directo (ni con lockeados).
    #[test]
    fn lane_opponent_sin_mi_posicion_devuelve_none() {
        let enemies = vec![member(10, Some(64), Some(Position::Jungle))];
        assert_eq!(lane_opponent(&enemies, None), None);
    }

    /// Los no lockeados no cuentan aunque matcheen la posición.
    #[test]
    fn lane_opponent_ignora_a_los_no_lockeados() {
        let enemies = vec![
            member(20, None, Some(Position::Middle)),
            member(21, Some(34), Some(Position::Middle)),
        ];
        assert_eq!(lane_opponent(&enemies, Some(Position::Middle)), Some(34));

        let solo_hover = vec![member(20, None, Some(Position::Middle))];
        assert_eq!(lane_opponent(&solo_hover, Some(Position::Middle)), None);
    }

    // ---- my_team_slots / enemy_slots ----

    /// Orden del draft (TOP/JGL/MID/BOT/SUP) y relleno a 5 en mi equipo.
    #[test]
    fn my_team_slots_ordena_por_posicion_y_rellena() {
        let allies = vec![
            member(1, Some(412), Some(Position::Bottom)),
            member(2, None, Some(Position::Top)),
        ];

        let slots = my_team_slots(103, Some(Position::Middle), &allies);

        assert_eq!(slots.len(), 5);
        assert_eq!(slots[0].position, Some(Position::Top), "TOP primero");
        assert!(slots[1].is_me, "yo (MID) en el medio");
        assert_eq!(slots[1].champion_id, Some(103));
        assert_eq!(slots[2].position, Some(Position::Bottom));
        assert!(!slots[3].filled && !slots[4].filled, "relleno sin miembro");
        assert_eq!(slots[4].champion_id, None);
    }

    /// Sin posición asignada se va al final (orden estable).
    #[test]
    fn my_team_slots_sin_posicion_va_al_final() {
        let allies = vec![member(1, Some(412), None)];
        let slots = my_team_slots(103, Some(Position::Jungle), &allies);

        assert!(slots[0].is_me, "JGL antes del sin posición");
        assert_eq!(slots[1].champion_id, Some(412), "el sin posición va después mío");
        assert_eq!(slots[1].position, None, "aliado sin posición al final");
        assert!(!slots[2].filled, "el resto son rellenos");
        assert_eq!(slots[2].champion_id, None);
    }

    /// Los enemigos mantienen el orden del draft y se rellenan a 5.
    #[test]
    fn enemy_slots_mantiene_orden_y_rellena() {
        let slots = enemy_slots(&[member(7, Some(238), Some(Position::Top))]);

        assert_eq!(slots.len(), 5);
        assert_eq!(slots[0].cell_id, 7);
        assert_eq!(slots[0].champion_id, Some(238));
        assert!(!slots[1].filled);
    }

    // ---- card_visual ----

    /// Arte en cascada: retrato de Data Dragon primero (aunque haya cutout
    /// local) → "?" para el desconocido; estados sin lock con etiquetas
    /// honestas.
    #[test]
    fn card_visual_resuelve_arte_y_fallbacks() {
        let catalog = catalogo();
        let lockeado = |id: i64| RowSlot {
            is_me: false,
            cell_id: 100 + id,
            champion_id: Some(id),
            position: Some(Position::Middle),
            filled: true,
        };

        // PRIMARIO: Akali tiene cutout local en la tabla de champ_art,
        // pero el retrato de Data Dragon (centrado en el campeón) va
        // primero: flag cutout=false.
        match card_visual(&lockeado(34), Some(&catalog)) {
            CardVisual::Art { cutout, name, .. } => {
                assert!(!cutout);
                assert_eq!(name, "Akali");
            }
            other => panic!("esperaba Art loading, hubo {other:?}"),
        }

        // Campeón sin asset local → mismo camino primario (retrato).
        match card_visual(&lockeado(999), Some(&catalog)) {
            CardVisual::Art { cutout, name, .. } => {
                assert!(!cutout);
                assert_eq!(name, "NuevoChamp");
            }
            other => panic!("esperaba Art loading, hubo {other:?}"),
        }

        // Id fuera del catálogo → campeón desconocido: slot "?" SIN nombre
        // inventado (antes caía un "#777" que parecía un campeón real).
        assert_eq!(
            card_visual(&lockeado(777), Some(&catalog)),
            CardVisual::Dim {
                label: String::from("?")
            }
        );

        // Sin catálogo entero → también desconocido, sin panic.
        assert_eq!(
            card_visual(&lockeado(34), None),
            CardVisual::Dim {
                label: String::from("?")
            }
        );

        // Miembro sin lock → "ELIGIENDO…".
        assert_eq!(
            card_visual(
                &RowSlot {
                    is_me: false,
                    cell_id: 5,
                    champion_id: None,
                    position: Some(Position::Top),
                    filled: true,
                },
                None,
            ),
            CardVisual::Dim {
                label: String::from("ELIGIENDO…")
            }
        );

        // Casilla vacía → placeholder con la posición (o "?" sin posición).
        assert_eq!(
            card_visual(
                &RowSlot {
                    is_me: false,
                    cell_id: -1,
                    champion_id: None,
                    position: Some(Position::Middle),
                    filled: false,
                },
                None,
            ),
            CardVisual::Dim {
                label: String::from("MID")
            }
        );
        assert_eq!(
            card_visual(&RowSlot::empty(), None),
            CardVisual::Dim {
                label: String::from("?")
            }
        );
    }

    // ---- card_models ----

    /// Los destacados solo aplican al lado enemigo: rival directo y
    /// objetivo elegido; mi equipo nunca los lleva.
    #[test]
    fn card_models_marca_rival_directo_y_objetivo_solo_en_enemigos() {
        let slots = enemy_slots(&[
            member(10, Some(34), Some(Position::Middle)),
            member(11, Some(64), Some(Position::Jungle)),
        ]);

        // Rival directo (34) y objetivo elegido (64), cruzados a propósito;
        // el catálogo resuelve a ambos (condición para llevar chip).
        let catalog = catalogo_con(&[34, 64]);
        let models = card_models(&slots, Side::Enemy, Some(&catalog), Some(34), Some(64));
        assert!(models[0].rival_directo);
        assert!(!models[0].target);
        assert!(models[1].target);
        assert!(!models[1].rival_directo);
        assert!(
            !models[2].rival_directo && !models[2].target,
            "relleno sin destacar"
        );

        // El mismo id como destacado NO resalta a mi equipo.
        let mios = card_models(
            &my_team_slots(34, Some(Position::Middle), &[]),
            Side::Mine,
            Some(&catalog),
            Some(34),
            Some(34),
        );
        let yo = mios
            .iter()
            .find(|model| model.slot.is_me)
            .expect("yo está en mi equipo");
        assert!(!yo.rival_directo && !yo.target);
    }

    /// CASO REAL reportado en champ select: YO lockeada en MID (Senna) y
    /// ANIVIA lockeada en MID → la derivación completa de la fila
    /// (`lane_opponent` → `enemy_slots` → `card_models`) la marca como
    /// rival directo, que es exactamente la condición con la que RowCard
    /// pinta el chip `cs5-chip-rival`
    /// (`if card.rival_directo { span { class: "cs5-chip-rival", … } }`).
    #[test]
    fn caso_real_senna_mid_vs_anivia_mid_marca_rival_directo() {
        // Yo: Senna (235) en MID; enemigos lockeados con Anivia (26) en MID.
        let enemies = vec![
            member(10, Some(64), Some(Position::Jungle)),
            member(11, Some(26), Some(Position::Middle)),
            member(12, Some(238), Some(Position::Top)),
        ];

        let rival_directo = lane_opponent(&enemies, Some(Position::Middle));
        assert_eq!(rival_directo, Some(26), "Anivia es mi rival de lane");

        let catalog = catalogo_con(&[64, 26, 238]);
        let models = card_models(
            &enemy_slots(&enemies),
            Side::Enemy,
            Some(&catalog),
            rival_directo,
            None, // sin OBJETIVO elegido todavía
        );

        let anivia = models
            .iter()
            .find(|model| model.slot.champion_id == Some(26))
            .expect("Anivia está en la fila enemiga");
        assert!(anivia.rival_directo, "ANIVIA lleva el chip RIVAL DIRECTO");
        assert!(!anivia.target, "sin click previo no hay OBJETIVO");

        let otros_limpios = models
            .iter()
            .filter(|model| model.slot.champion_id != Some(26))
            .all(|model| !model.rival_directo && !model.target);
        assert!(otros_limpios, "solo el rival de lane lleva el destacado");
    }

    // ---- infer_my_position ----

    /// Caso real del bug: mi `assignedPosition` no llegó (el payload la
    /// manda vacía/`UNSELECTED` para el miembro LOCAL), pero mis aliados
    /// sí reportaron TOP/JGL/BOT/SUP → la única lane libre es la mía.
    #[test]
    fn infer_my_position_deduce_la_ultima_lane_libre() {
        let allies = vec![
            member(1, Some(412), Some(Position::Top)),
            member(2, None, Some(Position::Jungle)),
            member(3, None, Some(Position::Bottom)),
            member(4, None, Some(Position::Utility)),
        ];
        assert_eq!(infer_my_position(&allies), Some(Position::Middle));
    }

    /// Ambiguo: dos lanes libres y un aliado también sin lane → None
    /// honesto (sin dato no hay chip, ni rivales inventados).
    #[test]
    fn infer_my_position_con_huecos_ambiguos_devuelve_none() {
        let allies = vec![
            member(1, None, Some(Position::Top)),
            member(2, None, Some(Position::Jungle)),
            member(3, None, Some(Position::Bottom)),
            member(4, None, None),
        ];
        assert_eq!(infer_my_position(&allies), None);
    }

    /// Sin aliados, o con todos sin lane reportada, no hay de dónde deducir.
    #[test]
    fn infer_my_position_sin_datos_devuelve_none() {
        assert_eq!(infer_my_position(&[]), None);
        let sin_lanes = vec![member(1, None, None), member(2, None, None)];
        assert_eq!(infer_my_position(&sin_lanes), None);
    }

    /// El caso real PUNTO A PUNTO con la misma derivación que hace
    /// VsTeamRow: prop `my_position: None` → inferencia MID desde mis
    /// aliados → `lane_opponent` encuentra a ANIVIA (lockeada MID) y
    /// `card_models` la marca, así el chip `cs5-chip-rival` sí aparece.
    #[test]
    fn caso_real_sin_mi_assigned_position_igual_marca_al_rival_de_lane() {
        let enemies = vec![member(11, Some(26), Some(Position::Middle))];
        let allies = vec![
            member(1, None, Some(Position::Top)),
            member(2, None, Some(Position::Jungle)),
            member(3, None, Some(Position::Bottom)),
            member(4, None, Some(Position::Utility)),
        ];

        let my_position_efectiva = None::<Position>.or_else(|| infer_my_position(&allies));
        assert_eq!(my_position_efectiva, Some(Position::Middle));

        let rival_directo = lane_opponent(&enemies, my_position_efectiva);
        assert_eq!(rival_directo, Some(26), "Anivia es mi rival de lane");

        let catalog = catalogo_con(&[26]);
        let models =
            card_models(&enemy_slots(&enemies), Side::Enemy, Some(&catalog), rival_directo, None);
        assert!(models[0].rival_directo, "el chip RIVAL DIRECTO aparece");
    }

    /// CASO REAL del bug: un enemigo lockea un id que el catálogo NO
    /// conoce (campeón más nuevo que la versión cacheada de ddragon).
    /// `lane_opponent` —puro, sin catálogo— lo devuelve igual, pero
    /// `card_models` lo descarta: la card queda Dim con "?" (sin nombre
    /// inventado), nadie lleva chip de rival directo ni de objetivo y no
    /// es clicable (`resuelto()` false es justo lo que RowCard usa para
    /// `is-selectable`).
    #[test]
    fn enemigo_desconocido_queda_en_dim_sin_chip_y_no_seleccionable() {
        let enemies = vec![
            member(10, Some(64), Some(Position::Jungle)),
            member(11, Some(8999), Some(Position::Middle)),
        ];

        // Catálogo vacío Y catálogo parcial (conoce a Lee Sin pero no al
        // id nuevo): en ambos el desconocido es "?", sin chip en nadie.
        let vacio = catalogo_con(&[]);
        let parcial = catalogo_con(&[64]);
        for catalog in [&vacio, &parcial] {
            let rival_directo = lane_opponent(&enemies, Some(Position::Middle));
            assert_eq!(rival_directo, Some(8999), "el helper puro no ve el catálogo");

            let models = card_models(
                &enemy_slots(&enemies),
                Side::Enemy,
                Some(catalog),
                rival_directo,
                Some(8999), // ni un click previo lo vuelve objetivo
            );

            let desconocido = models
                .iter()
                .find(|model| model.slot.champion_id == Some(8999))
                .expect("el enemigo nuevo está en la fila");
            assert_eq!(
                desconocido.visual,
                CardVisual::Dim {
                    label: String::from("?")
                },
                "sin nombre inventado: slot ?"
            );
            assert!(
                !desconocido.visual.resuelto(),
                "no seleccionable: RowCard no pinta is-selectable"
            );
            assert!(
                models.iter().all(|model| !model.rival_directo && !model.target),
                "si el rival de lane no resuelve, nadie lleva chip"
            );
        }

        // Contraste: el id que SÍ resuelve sigue siendo clickeable y
        // marcable como siempre.
        let models = card_models(
            &enemy_slots(&enemies),
            Side::Enemy,
            Some(&parcial),
            Some(64),
            Some(64),
        );
        let lee = models
            .iter()
            .find(|model| model.slot.champion_id == Some(64))
            .expect("Lee Sin está en la fila");
        assert!(lee.visual.resuelto());
        assert!(lee.rival_directo && lee.target);
    }

    /// Claves estables: "me", cell_id real e índice de relleno.
    #[test]
    fn slot_key_distingue_me_miembro_y_relleno() {
        assert_eq!(
            slot_key(
                "mine",
                &RowSlot {
                    is_me: true,
                    cell_id: i64::MIN,
                    champion_id: Some(103),
                    position: None,
                    filled: true,
                },
                0
            ),
            "me"
        );
        assert_eq!(
            slot_key(
                "enemy",
                &RowSlot {
                    is_me: false,
                    cell_id: 42,
                    champion_id: None,
                    position: None,
                    filled: true,
                },
                3
            ),
            "enemy-42"
        );
        assert_eq!(
            slot_key(
                "mine",
                &RowSlot {
                    is_me: false,
                    cell_id: -1,
                    champion_id: None,
                    position: None,
                    filled: false,
                },
                3
            ),
            "mine-fill-3"
        );
    }
}
