//! Panel "JUGADORES" del champ select (estilo Porofessor): una fila por
//! jugador del draft REAL —aliados y enemigos del [`MatchContext`]— con
//! posición, retrato del champ lockeado, nombre, rank solo-queue y el WR
//! global del campeón elegido. Los estilos viven en main.css
//! (`.jugadores-panel` y familia).
//!
//! Datos por jugador:
//!
//! - **Scout on-demand**: el effect de carga junta los `summonerId` válidos
//!   (> 0) de aliados y enemigos y pide [`buscar_scout`] con el handle
//!   global del cliente LCU ([`client_handle`], patrón `spawn_match_fetch`
//!   de `ui/profile.rs`; sin cliente → `Failed` honesto). El guard
//!   anti-refetch (claves ya presentes en el mapa, leídas con `peek` para
//!   no suscribirnos a nuestras propias escrituras) evita re-spawnear un id
//!   con entrada: cada jugador se pide UNA vez por draft, sin importar
//!   cuántas veces vuelva a publicar el contexto.
//! - **Rank**: [`etiqueta_rank`] formatea el resumen solo-queue y
//!   [`clase_wr_rank`] le da color (verde > 50%, rojo < 50%). Scout en
//!   vuelo → "CARGANDO…"; fallido (o miembro sin `summonerId`, que nunca va
//!   a cargar) → "SIN DATOS", tenue y honesto.
//! - **WR GLOBAL**: es el winrate GLOBAL del campeón según la ficha
//!   embebida ([`guide_for_slug`]), NO el historial del jugador. El LCU no
//!   expone el historial de OTROS invocadores, así que la etiqueta lleva
//!   "GLOBAL" explícito para no vender estadística del campeón como datos
//!   personales del invocador de la fila.
//!
//! Retratos en cascada como el menú (`ui/menu.rs`) y la vista vivo: art
//! local sin fondo ([`champ_art`]) → `loading_url` de Data Dragon →
//! placeholder con las iniciales del invocador (o "?" para un lock que el
//! catálogo no resuelve, mismo criterio que vs_row).
//!
//! Nota: el contexto NO incluye al jugador local en `allies` (así lo
//! construye [`MatchContext::from_session`]); el badge "VOS" cubre el caso
//! defensivo en que el payload no distinguió la celda local y el local
//! queda entre los aliados reportados.
//!
//! Toda la derivación del render es pura y testeada: [`ids_a_scoutear`],
//! [`ordenar_por_posicion`], [`iniciales`], [`nombre_mostrado`],
//! [`etiqueta_wr_global`], [`retrato_de`], [`champ_linea`], [`linea_rank`] y
//! [`fila_de`]; el RSX solo pinta.

use std::collections::{HashMap, HashSet};

use dioxus::prelude::*;

use crate::data_dragon::models::{ChampionCatalog, ChampionEntry};
use crate::lcu::models::{MatchContext, Position, TeamMember};
use crate::lcu::scouting::{buscar_scout, clase_wr_rank, etiqueta_rank, ScoutInfo};
use crate::profile::client_handle::client_handle;
use crate::ui::recommendations::format_winrate;

use super::champ_art::champ_art;
use super::position_label;
use super::vs_guide_data::guide_for_slug;

/// Estado del scout de un invocador: `Loading` mientras vuela el pedido,
/// `Ready` con la ficha o `Failed` sin datos (sin cliente, red, 404).
#[derive(Clone, Debug, PartialEq, Eq)]
enum ScoutState {
    Loading,
    Ready(ScoutInfo),
    Failed,
}

// ---------------------------------------------------------------------------
// Helpers puros (testeables sin señales ni red)
// ---------------------------------------------------------------------------

/// Ids de invocador a scoutea del contexto (puro, testeado): los
/// `summonerId` válidos (> 0) de aliados y enemigos, sin duplicados.
#[must_use]
fn ids_a_scoutear(context: &MatchContext) -> HashSet<i64> {
    context
        .allies
        .iter()
        .chain(context.enemies.iter())
        .filter_map(|miembro| miembro.summoner_id)
        .filter(|id| *id > 0)
        .collect()
}

/// Orden del draft: TOP, JGL, MID, BOT, SUP; sin posición al final (misma
/// convención que `position_order` en vs_row, replicada acá porque es
/// privada a ese módulo).
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

/// Orden interno de un equipo (puro, testeado): TOP → JGL → MID → BOT →
/// SUP y los miembros sin posición al final. Estable: clave igual conserva
/// el orden del payload.
#[must_use]
fn ordenar_por_posicion(miembros: &[TeamMember]) -> Vec<TeamMember> {
    let mut ordenados = miembros.to_vec();
    ordenados.sort_by_key(|miembro| position_order(miembro.position));
    ordenados
}

/// Iniciales de un nombre para el placeholder sin pick (puro, testeado):
/// primera letra de hasta dos palabras en mayúsculas ("PKT MidKing" →
/// "PM"); sin palabras útiles → "?".
#[must_use]
fn iniciales(nombre: &str) -> String {
    let letras: String = nombre
        .split_whitespace()
        .take(2)
        .filter_map(|palabra| palabra.chars().next())
        .collect();
    if letras.is_empty() {
        String::from("?")
    } else {
        letras.to_uppercase()
    }
}

/// Cadena de fallback del nombre (puro, testeado): `summonerName` del
/// miembro → `scout.name` → "INVOCADOR". Nombres vacíos o de puros espacios
/// no cuentan en ningún escalón.
#[must_use]
fn nombre_mostrado(summoner_name: Option<&str>, scout: Option<&ScoutState>) -> String {
    summoner_name
        .map(str::trim)
        .filter(|nombre| !nombre.is_empty())
        .map(str::to_string)
        .or_else(|| match scout {
            Some(ScoutState::Ready(info)) => {
                let nombre = info.name.trim();
                if nombre.is_empty() {
                    None
                } else {
                    Some(nombre.to_string())
                }
            }
            _ => None,
        })
        .unwrap_or_else(|| String::from("INVOCADOR"))
}

/// Etiqueta del WR GLOBAL del campeón (puro, testeado): `"WR GLOBAL
/// 51.23%"`, con el formato de [`format_winrate`] (sin ceros colgantes).
#[must_use]
fn etiqueta_wr_global(wr: f64) -> String {
    format!("WR GLOBAL {}%", format_winrate(wr))
}

/// Entrada de catálogo de un lock (puro): `None` sin lock o con un id que
/// el catálogo no resuelve (catálogo ausente, cargando o champ más nuevo
/// que la versión cacheada).
#[must_use]
fn entrada_de(
    champion_id: Option<i64>,
    catalog: Option<&ChampionCatalog>,
) -> Option<&ChampionEntry> {
    champion_id.and_then(|id| catalog.and_then(|data| data.get(id)))
}

/// Retrato resuelto para la fila (puro, testeado), en cascada como el menú:
/// art local sin fondo → loading de Data Dragon → placeholder. Sin pick el
/// placeholder lleva las iniciales del invocador; un lock que el catálogo
/// no resuelve lleva "?" (sin nombre inventado, criterio de vs_row).
#[derive(Clone, Debug, PartialEq)]
enum Retrato {
    /// `assets/champs/{ddragon_id}.webp` compilado.
    Local(dioxus::prelude::Asset),
    /// Loading screen de Data Dragon.
    Url(String),
    /// Placeholder sin imagen: iniciales del invocador o "?".
    SinPick(String),
}

#[must_use]
fn retrato_de(
    champion_id: Option<i64>,
    catalog: Option<&ChampionCatalog>,
    nombre: &str,
) -> Retrato {
    match entrada_de(champion_id, catalog) {
        Some(entry) => match champ_art(&entry.ddragon_id) {
            Some(art) => Retrato::Local(art),
            None => Retrato::Url(entry.loading_url()),
        },
        None if champion_id.is_some() => Retrato::SinPick(String::from("?")),
        None => Retrato::SinPick(iniciales(nombre)),
    }
}

/// Línea de datos del campeón de la fila (pura, testeada).
#[derive(Clone, Debug, PartialEq, Eq)]
struct ChampLinea {
    /// Nombre del champ, `#id` si el catálogo no lo resuelve o "SIN PICK".
    etiqueta: String,
    /// `true` con lock confirmado (habilita el WR GLOBAL).
    con_pick: bool,
    /// WR GLOBAL del champ desde la ficha embebida; `None` sin ficha.
    wr_global: Option<String>,
}

/// Línea de champ (pura, testeada): sin lock → "SIN PICK" sin WR; con lock
/// resuelto → nombre + WR GLOBAL si el slug tiene ficha embebida; id
/// desconocido → `#id` (criterio de counters_panel).
#[must_use]
fn champ_linea(champion_id: Option<i64>, catalog: Option<&ChampionCatalog>) -> ChampLinea {
    let Some(id) = champion_id else {
        return ChampLinea {
            etiqueta: String::from("SIN PICK"),
            con_pick: false,
            wr_global: None,
        };
    };

    let entry = entrada_de(Some(id), catalog);
    let etiqueta = entry
        .map(|entrada| entrada.name.clone())
        .unwrap_or_else(|| format!("#{id}"));
    let wr_global = entry
        .and_then(|entrada| guide_for_slug(&entrada.slug))
        .map(|guide| etiqueta_wr_global(guide.stats.wr));

    ChampLinea {
        etiqueta,
        con_pick: true,
        wr_global,
    }
}

/// Línea de rank de la fila según el estado del scout (pura, testeada).
#[derive(Clone, Debug, PartialEq, Eq)]
enum RankLinea {
    /// Scout en vuelo: "CARGANDO…".
    Cargando,
    /// Scout fallido o nunca pedible (miembro sin `summonerId`): "SIN
    /// DATOS", tenue y honesto.
    SinDatos,
    /// Con ficha: etiqueta formateada + clase de color ("" = neutro).
    Dato {
        etiqueta: String,
        clase: &'static str,
    },
}

#[must_use]
fn linea_rank(scout: Option<&ScoutState>) -> RankLinea {
    match scout {
        Some(ScoutState::Ready(info)) => RankLinea::Dato {
            etiqueta: etiqueta_rank(info.rank.as_ref()),
            clase: clase_wr_rank(info.rank.as_ref()),
        },
        Some(ScoutState::Loading) => RankLinea::Cargando,
        Some(ScoutState::Failed) | None => RankLinea::SinDatos,
    }
}

/// Fila de jugador lista para pintar (pura): toda la derivación queda
/// afuera del RSX (convención de la app).
#[derive(Clone, Debug, PartialEq)]
struct FilaJugador {
    /// `cellId` del draft: clave estable del reconciliador.
    cell_id: i64,
    /// Etiqueta corta de posición ("" sin rol reportado).
    posicion: &'static str,
    /// `true` → badge "VOS" (la celda del jugador local).
    vos: bool,
    /// Retrato resuelto (art local → ddragon → placeholder).
    retrato: Retrato,
    /// Texto `alt` de la imagen (nombre del champ resuelto o del
    /// invocador).
    retrato_alt: String,
    /// Nombre visible (miembro → scout → "INVOCADOR").
    nombre: String,
    /// Línea de rank según el estado del scout.
    rank: RankLinea,
    /// Línea de datos del campeón.
    champ: ChampLinea,
}

/// Modelo de una fila (puro, testeado): cruza el miembro del draft con su
/// scout (si hay pedido en vuelo o terminado) y el catálogo.
#[must_use]
fn fila_de(
    miembro: &TeamMember,
    my_cell_id: Option<i64>,
    catalog: Option<&ChampionCatalog>,
    scout: Option<&ScoutState>,
) -> FilaJugador {
    let nombre = nombre_mostrado(miembro.summoner_name.as_deref(), scout);
    let retrato_alt = entrada_de(miembro.champion_id, catalog)
        .map(|entrada| entrada.name.clone())
        .unwrap_or_else(|| nombre.clone());

    FilaJugador {
        cell_id: miembro.cell_id,
        posicion: position_label(miembro.position),
        vos: Some(miembro.cell_id) == my_cell_id,
        retrato: retrato_de(miembro.champion_id, catalog, &nombre),
        retrato_alt,
        nombre,
        rank: linea_rank(scout),
        champ: champ_linea(miembro.champion_id, catalog),
    }
}

// ---------------------------------------------------------------------------
// Componentes
// ---------------------------------------------------------------------------

/// Panel "JUGADORES": los dos equipos del draft con scouting on-demand.
///
/// Re-renderiza cuando el contexto (locks en vivo) o el catálogo publican;
/// los snapshots se toman ANTES del rsx (el guard no cruza la construcción
/// de nodos). El fetch de scouts vive acá: un effect junta los
/// `summonerId` vigentes y el guard anti-refetch evita re-pedir un id que
/// ya tiene entrada en el mapa.
#[component]
pub fn PlayersPanel(
    match_context: ReadSignal<MatchContext>,
    catalog: ReadSignal<Option<ChampionCatalog>>,
) -> Element {
    let mut scouts: Signal<HashMap<i64, ScoutState>> = use_signal(HashMap::new);

    use_effect(move || {
        // Snapshot del contexto: suscribe el effect a los locks del draft.
        let context: MatchContext = match_context.read().clone();
        let ids = ids_a_scoutear(&context);

        // `peek`: NO suscribe a `scouts` (nuestras inserciones de Loading
        // no deben re-disparar el effect). Guard anti-refetch: un id con
        // entrada (Loading/Ready/Failed) no se pide de nuevo.
        let ya_pedidos: HashSet<i64> = scouts.peek().keys().copied().collect();

        for id in ids {
            if ya_pedidos.contains(&id) {
                continue;
            }
            scouts.write().insert(id, ScoutState::Loading);
            spawn(async move {
                // Handle on-demand (patrón `spawn_match_fetch` de
                // ui/profile.rs): sin cliente conectado → Failed honesto.
                let Some(client) = client_handle() else {
                    eprintln!(
                        "klipy [players_panel]: scout de {id} sin datos: cliente LCU no conectado"
                    );
                    scouts.write().insert(id, ScoutState::Failed);
                    return;
                };
                match buscar_scout(&client, id).await {
                    Ok(info) => {
                        scouts.write().insert(id, ScoutState::Ready(info));
                    }
                    Err(_) => {
                        scouts.write().insert(id, ScoutState::Failed);
                    }
                }
            });
        }
    });

    // Snapshots ANTES del rsx (convención de la app): el guard de señales
    // no cruza la construcción de nodos.
    let context: MatchContext = match_context.read().clone();
    let catalog_snapshot: Option<ChampionCatalog> = catalog.read().clone();
    let scouts_snapshot: HashMap<i64, ScoutState> = scouts.read().clone();
    let my_cell_id = context.my_cell_id;
    let catalog_ref = catalog_snapshot.as_ref();

    // Scout vigente de un miembro (solo ids válidos tienen entrada).
    let scout_de = |miembro: &TeamMember| -> Option<&ScoutState> {
        miembro
            .summoner_id
            .filter(|id| *id > 0)
            .and_then(|id| scouts_snapshot.get(&id))
    };

    // Derivación pura antes del rsx: un modelo por jugador, cada equipo en
    // el orden del draft (TOP/JGL/MID/BOT/SUP, sin posición al final).
    let filas_aliados: Vec<FilaJugador> = ordenar_por_posicion(&context.allies)
        .iter()
        .map(|miembro| fila_de(miembro, my_cell_id, catalog_ref, scout_de(miembro)))
        .collect();
    let filas_enemigos: Vec<FilaJugador> = ordenar_por_posicion(&context.enemies)
        .iter()
        .map(|miembro| fila_de(miembro, my_cell_id, catalog_ref, scout_de(miembro)))
        .collect();

    rsx! {
        section { class: "jugadores-panel",
            // Título del panel con la gramática de labels del repo.
            div { class: "section-head",
                span { class: "bar-skew", "style": "width: 24px;" }
                span { class: "label label-md",
                    span { class: "sl", "//" }
                    " JUGADORES"
                }
            }

            div { class: "jugadores-equipo",
                h4 { class: "jugadores-titulo", "EQUIPO ALIADO" }
                {filas_aliados.into_iter().map(|fila| {
                    let key = fila.cell_id;
                    rsx! { JugadorFila { fila, key: "{key}" } }
                })}
            }

            div { class: "jugadores-equipo",
                h4 { class: "jugadores-titulo", "EQUIPO ENEMIGO" }
                {filas_enemigos.into_iter().map(|fila| {
                    let key = fila.cell_id;
                    rsx! { JugadorFila { fila, key: "{key}" } }
                })}
            }
        }
    }
}

/// Una fila de jugador: posición + badge VOS + retrato + nombre + rank +
/// datos del champ. Las clases y textos de estado ("CARGANDO…", "SIN
/// DATOS") se resuelven antes del rsx.
#[component]
fn JugadorFila(fila: FilaJugador) -> Element {
    // Chip de rank: variante de color solo con dato; estados sin datos en
    // tenue (`jugador-sindatos`), como pide el CSS del panel.
    let rank_class = match &fila.rank {
        RankLinea::Dato { clase, .. } if clase.is_empty() => String::from("jugador-rank"),
        RankLinea::Dato { clase, .. } => format!("jugador-rank {clase}"),
        RankLinea::Cargando | RankLinea::SinDatos => {
            String::from("jugador-rank jugador-sindatos")
        }
    };
    let rank_texto = match &fila.rank {
        RankLinea::Dato { etiqueta, .. } => etiqueta.clone(),
        RankLinea::Cargando => String::from("CARGANDO…"),
        RankLinea::SinDatos => String::from("SIN DATOS"),
    };

    // Línea de champ: "SIN PICK" en tenue; con pick, nombre + WR GLOBAL de
    // la ficha embebida (si la hay).
    let champ_class = if fila.champ.con_pick {
        String::from("jugador-champ")
    } else {
        String::from("jugador-champ jugador-sindatos")
    };
    let wr_global = fila.champ.wr_global.clone();

    // Retrato como nodo previo con valores OWNED (patrón de vs_row):
    // `Asset` es Copy y la URL se clona, así el `src` del img recibe el
    // tipo que el atributo espera. El wrapper `jugador-retrato` es el
    // marco 32px del CSS (`.jugador-retrato img` fuerza el fill).
    let retrato_alt = fila.retrato_alt.clone();
    let retrato_nodo = match fila.retrato.clone() {
        Retrato::Local(art) => rsx! {
            span { class: "jugador-retrato",
                img { src: art, alt: "{retrato_alt}" }
            }
        },
        Retrato::Url(src) => rsx! {
            span { class: "jugador-retrato",
                img { src: "{src}", alt: "{retrato_alt}" }
            }
        },
        Retrato::SinPick(texto) => rsx! {
            span { class: "jugador-retrato jugador-sinpick", "{texto}" }
        },
    };

    rsx! {
        div { class: "jugador-fila",
            if !fila.posicion.is_empty() {
                span { class: "jugador-pos", "{fila.posicion}" }
            }
            if fila.vos {
                span { class: "jugador-vos", "VOS" }
            }
            {retrato_nodo}
            span { class: "jugador-nombre", "{fila.nombre}" }
            span { class: "{rank_class}", "{rank_texto}" }
            span { class: "{champ_class}", "{fila.champ.etiqueta}" }
            if let Some(wr) = wr_global {
                span { class: "jugador-wr", "{wr}" }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Miembro de prueba.
    fn miembro(
        cell: i64,
        champion_id: Option<i64>,
        position: Option<Position>,
        summoner_id: Option<i64>,
    ) -> TeamMember {
        TeamMember {
            cell_id: cell,
            champion_id,
            position,
            summoner_id,
            summoner_name: None,
        }
    }

    /// Entrada de catálogo mínima: slug en minúsculas y ddragon_id = nombre.
    fn entrada(nombre: &str) -> ChampionEntry {
        ChampionEntry {
            name: String::from(nombre),
            image_url: format!("https://example/{nombre}.png"),
            slug: nombre.to_ascii_lowercase(),
            ddragon_id: String::from(nombre),
        }
    }

    /// Catálogo con Akali (art local en champ_art + ficha embebida) y un
    /// champ desconocido para ambas cosas.
    fn catalogo_akali() -> ChampionCatalog {
        let mut by_id = HashMap::new();
        by_id.insert(34, entrada("Akali"));
        by_id.insert(999, entrada("NuevoChamp"));
        ChampionCatalog {
            version: String::from("16.20.1"),
            by_id,
        }
    }

    /// Scout Ready con el nombre dado (sin rank: etiqueta "SIN RANK").
    fn scout_ready(nombre: &str) -> ScoutState {
        ScoutState::Ready(ScoutInfo {
            summoner_id: 1,
            name: String::from(nombre),
            tag_line: None,
            level: 30,
            profile_icon_id: 0,
            puuid: String::new(),
            rank: None,
        })
    }

    // ---- ids_a_scoutear ----

    /// Toma los ids válidos (> 0) de los dos lados, sin duplicados ni
    /// miembros sin id.
    #[test]
    fn ids_a_scoutear_toma_los_validos_de_los_dos_lados() {
        let context = MatchContext {
            my_cell_id: Some(1),
            allies: vec![
                miembro(2, None, None, Some(7)),
                miembro(3, None, None, Some(0)),
                miembro(4, None, None, None),
            ],
            enemies: vec![
                miembro(8, None, None, Some(7)),
                miembro(9, None, None, Some(-3)),
                miembro(10, None, None, Some(12)),
            ],
            ..MatchContext::default()
        };

        let mut ids: Vec<i64> = ids_a_scoutear(&context).into_iter().collect();
        ids.sort_unstable();
        assert_eq!(ids, vec![7, 12]);
    }

    // ---- ordenar_por_posicion ----

    /// Orden del draft y los sin posición al final.
    #[test]
    fn ordenar_por_posicion_sigue_el_draft_y_deja_none_al_final() {
        let miembros = vec![
            miembro(1, None, None, Some(11)),
            miembro(2, None, Some(Position::Utility), Some(12)),
            miembro(3, None, Some(Position::Top), Some(13)),
            miembro(4, None, Some(Position::Middle), Some(14)),
            miembro(5, None, Some(Position::Jungle), Some(15)),
        ];

        let orden: Vec<Option<Position>> = ordenar_por_posicion(&miembros)
            .into_iter()
            .map(|m| m.position)
            .collect();

        assert_eq!(
            orden,
            vec![
                Some(Position::Top),
                Some(Position::Jungle),
                Some(Position::Middle),
                Some(Position::Utility),
                None,
            ]
        );
    }

    /// Claves iguales conservan el orden del payload (sort estable).
    #[test]
    fn ordenar_por_posicion_es_estable_con_claves_iguales() {
        let miembros = vec![
            miembro(1, None, Some(Position::Bottom), Some(11)),
            miembro(2, None, Some(Position::Bottom), Some(12)),
        ];
        let orden: Vec<i64> = ordenar_por_posicion(&miembros)
            .into_iter()
            .map(|m| m.cell_id)
            .collect();
        assert_eq!(orden, vec![1, 2]);
    }

    // ---- iniciales ----

    /// Hasta dos palabras, mayúsculas, y "?" sin material.
    #[test]
    fn iniciales_toma_dos_palabras_y_cae_en_signo() {
        assert_eq!(iniciales("PKT MidKing"), "PM");
        assert_eq!(iniciales("faker"), "F");
        assert_eq!(iniciales(""), "?");
        assert_eq!(iniciales("   "), "?");
    }

    // ---- nombre_mostrado ----

    /// Cadena de fallback: miembro → scout → "INVOCADOR"; los estados sin
    /// ficha del scout no aportan nombre.
    #[test]
    fn nombre_mostrado_prefiere_al_miembro_y_cae_hasta_invocador() {
        assert_eq!(
            nombre_mostrado(Some("ElPrimo"), Some(&scout_ready("DelScout"))),
            "ElPrimo"
        );
        assert_eq!(
            nombre_mostrado(None, Some(&scout_ready("DelScout"))),
            "DelScout"
        );
        assert_eq!(nombre_mostrado(Some("   "), Some(&scout_ready("  "))), "INVOCADOR");
        assert_eq!(nombre_mostrado(None, None), "INVOCADOR");
        assert_eq!(nombre_mostrado(None, Some(&ScoutState::Loading)), "INVOCADOR");
        assert_eq!(nombre_mostrado(None, Some(&ScoutState::Failed)), "INVOCADOR");
    }

    // ---- etiqueta_wr_global ----

    /// Formato "WR GLOBAL …%" sin ceros colgantes.
    #[test]
    fn etiqueta_wr_global_recorta_ceros_colgantes() {
        assert_eq!(etiqueta_wr_global(51.234), "WR GLOBAL 51.23%");
        assert_eq!(etiqueta_wr_global(52.0), "WR GLOBAL 52%");
        assert_eq!(etiqueta_wr_global(49.5), "WR GLOBAL 49.5%");
    }

    // ---- retrato_de ----

    /// Cascada: art local → loading de ddragon → "?" (lock no resuelto) →
    /// iniciales (sin pick).
    #[test]
    fn retrato_de_cascada_art_loading_y_placeholder() {
        let catalog = catalogo_akali();

        // Akali tiene art local en champ_art → Local.
        assert_eq!(
            retrato_de(Some(34), Some(&catalog), "Alguien"),
            Retrato::Local(champ_art("Akali").expect("akali tiene art local"))
        );

        // Champ fuera de la tabla local → loading de Data Dragon.
        match retrato_de(Some(999), Some(&catalog), "Alguien") {
            Retrato::Url(url) => assert!(url.contains("NuevoChamp")),
            otro => panic!("esperaba Url, hubo {otro:?}"),
        }

        // Lock que el catálogo no resuelve (id nuevo o catálogo ausente)
        // → "?" sin nombre inventado.
        assert_eq!(
            retrato_de(Some(123), Some(&catalog), "Alguien"),
            Retrato::SinPick(String::from("?"))
        );
        assert_eq!(
            retrato_de(Some(34), None, "Alguien"),
            Retrato::SinPick(String::from("?"))
        );

        // Sin pick → iniciales del invocador.
        assert_eq!(
            retrato_de(None, Some(&catalog), "PKT MidKing"),
            Retrato::SinPick(String::from("PM"))
        );
    }

    // ---- champ_linea ----

    /// "SIN PICK" sin lock; nombre + WR GLOBAL con ficha; "#id" para el
    /// desconocido; sin WR inventado cuando no hay ficha embebida.
    #[test]
    fn champ_linea_sin_pick_con_nombre_y_wr_global() {
        assert_eq!(
            champ_linea(None, None),
            ChampLinea {
                etiqueta: String::from("SIN PICK"),
                con_pick: false,
                wr_global: None,
            }
        );

        let catalog = catalogo_akali();

        // Con lock resuelto → nombre + WR GLOBAL de la ficha embebida.
        let akali = champ_linea(Some(34), Some(&catalog));
        assert_eq!(akali.etiqueta, "Akali");
        assert!(akali.con_pick);
        assert!(
            akali.wr_global
                .as_deref()
                .is_some_and(|wr| wr.starts_with("WR GLOBAL ")),
            "akali tiene ficha embebida"
        );

        // Champ sin ficha embebida → sin WR (nunca inventado).
        let nuevo = champ_linea(Some(999), Some(&catalog));
        assert_eq!(nuevo.etiqueta, "NuevoChamp");
        assert_eq!(nuevo.wr_global, None);

        // Id fuera del catálogo → "#id" (criterio counters_panel).
        let desconocido = champ_linea(Some(123), Some(&catalog));
        assert_eq!(desconocido.etiqueta, "#123");
        assert_eq!(desconocido.wr_global, None);
    }

    // ---- linea_rank ----

    /// Estados del scout: sin pedido o fallido → "SIN DATOS"; en vuelo →
    /// "CARGANDO…"; con ficha → etiqueta + clase (vacía sin rank).
    #[test]
    fn linea_rank_segun_el_estado_del_scout() {
        assert_eq!(
            linea_rank(None),
            RankLinea::SinDatos,
            "sin id no va a cargar nunca: honesto desde ya"
        );
        assert_eq!(linea_rank(Some(&ScoutState::Loading)), RankLinea::Cargando);
        assert_eq!(linea_rank(Some(&ScoutState::Failed)), RankLinea::SinDatos);

        match linea_rank(Some(&scout_ready("Alguien"))) {
            RankLinea::Dato { etiqueta, clase } => {
                assert_eq!(etiqueta, "SIN RANK", "fixture sin rank");
                assert_eq!(clase, "");
            }
            otro => panic!("esperaba Dato, hubo {otro:?}"),
        }
    }

    // ---- fila_de ----

    /// Cruce completo: badge VOS en mi celda, posición corta, nombre del
    /// miembro, rank según scout y champ resuelto contra el catálogo.
    #[test]
    fn fila_de_cruza_miembro_scout_y_catalogo() {
        let catalog = catalogo_akali();
        let mut akali = miembro(5, Some(34), Some(Position::Middle), Some(77));
        akali.summoner_name = Some(String::from("PKT MidKing"));

        let fila = fila_de(&akali, Some(5), Some(&catalog), Some(&ScoutState::Loading));

        assert_eq!(fila.cell_id, 5);
        assert!(fila.vos, "mi celda lleva el badge VOS");
        assert_eq!(fila.posicion, "MID");
        assert_eq!(fila.nombre, "PKT MidKing");
        assert_eq!(fila.rank, RankLinea::Cargando);
        assert_eq!(fila.champ.etiqueta, "Akali");
        assert!(matches!(fila.retrato, Retrato::Local(_)));

        // Enemigo sin id y sin lock: posición vacía, nombre del fallback,
        // rank sin datos y placeholder con las iniciales de "INVOCADOR".
        let enemigo = miembro(9, None, None, None);
        let fila = fila_de(&enemigo, Some(5), Some(&catalog), None);
        assert!(!fila.vos);
        assert_eq!(fila.posicion, "");
        assert_eq!(fila.nombre, "INVOCADOR");
        assert_eq!(fila.rank, RankLinea::SinDatos);
        assert_eq!(
            fila.retrato,
            Retrato::SinPick(String::from("I")),
            "iniciales del fallback INVOCADOR"
        );
        assert_eq!(
            fila.champ,
            ChampLinea {
                etiqueta: String::from("SIN PICK"),
                con_pick: false,
                wr_global: None,
            }
        );
    }
}
