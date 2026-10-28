//! Vista VIVO (tab del menú lateral): champ select de espera + partida en
//! vivo, con el layout del prototipo aprobado.
//!
//! Header (`.vivo-head`, grid `1fr auto 1fr`): título "VIVO" (el `//` lo
//! pinta el CSS de `.h-sec`), en el centro las pestañas (`.vivo-tabs`):
//! "CHAMP SELECT" y "PARTIDA EN VIVO" (botones `.vivo-tab`, la activa con
//! `vivo-tab--on`, señal local [`VivoTab`]) y a la derecha el tag
//! `.kl-badge-offline` con "ESTADO · ESPERANDO" (o "ESTADO · LIVE" con
//! champ select real, [`AppState::InChampSelect`]).
//!
//! Tab CHAMP SELECT (contenido histórico, sin cambios de comportamiento),
//! `main-container` → `.vivo-layout` de dos columnas:
//!
//! - **Columna izquierda** (`.vivo-estado-panel`): dot de estado (reusa
//!   `.dot-pulse`), título de estado ([`estado_titulo`]), texto de espera
//!   en rioplatense y dos chips (`.vivo-chip-estado`, encendidos con
//!   `--on`): "SIN CHAMP SELECT" (encendido mientras no hay champ select
//!   real) y "CHAMP SELECT ACTIVO · LIVE" (encendido solo con champ select
//!   real). Con cliente ya detectado ([`AppState::WaitingForChampSelect`])
//!   agrega "detecté el cliente, entrando…" ([`estado_vivo`]).
//! - **Columna derecha** (`.vivo-buscador-panel`): micro-label "BUSCAR
//!   CAMPEÓN" + input `.kl-menu-search` + lista vertical scrolleable
//!   (`.vivo-lista`) con TODO el catálogo filtrado en vivo por
//!   [`filtra_campeones`] (case/acento-insensitive). Cada fila
//!   ([`VivoFila`], un `button`) es: retrato cuadrado de 40px (mecanismo
//!   EXACTO del menú: `champ_art` → `loading_url` → `image_url`) + nombre
//!   + tag de lane + WR sacados de la FICHA embebida via
//!     [`guide_for_slug`](crate::ui::champ_select::vs_guide_data::guide_for_slug)
//!     (clave: `ChampionEntry::slug`). Sin ficha o sin guide → "—" en ambos,
//!     sin inventar datos.
//!
//! Tab PARTIDA EN VIVO (`.vivo-panel-live`, misma gramática visual que
//! `.vivo-estado-panel`): con `in_game` + [`LiveGame`] presente, dot verde
//! pulso, título "PARTIDA EN VIVO", modo (`game_mode`), "TU CAMPEÓN" y dos
//! mini-listas "ALIADOS/ENEMIGOS n" (`.vivo-live-grupo` con
//! `.vivo-live-nombre`). Como [`LiveGame`] trae aliados/enemigos como IDS
//! (`allies`/`enemies: Vec<(i64, Option<Position>)>`, sin nombres), la
//! resolución id → nombre sale del catálogo que la vista ya recibe
//! ([`nombres_equipo`]); id sin resolver o catálogo ausente → "—". Sin
//! partida: dot gris, título "SIN PARTIDA EN VIVO", texto de espera en
//! rioplatense y chip apagado "ESPERANDO PARTIDA".
//!
//! Auto-switch: un `use_effect` pisa la pestaña a PARTIDA EN VIVO cuando
//! `in_game` pasa a true (solo ese sentido; volver a CHAMP SELECT es
//! manual, y el efecto no lo pisa de vuelta).
//!
//! Con champ select real el usuario NUNCA llega a la tab champ select:
//! `derive_view` (app.rs) pisa la navegación y manda al champ select. Esta
//! tab es solo el camino manual/espera.
//!
//! Pie: sin [`KlifyFooter`](crate::ui::shell::KlifyFooter) (esta vista no
//! recibe `source_served`/`version`); el pie es un `.cs-hint`.
//!
//! Clases: reusa `.view`, `.main-container`, `.h-sec`, `.kl-badge-offline`,
//! `.dot-pulse`, `.kl-menu-search`, `.empty`, `.cs-hint`, `.label`,
//! `.t-dim` y `.ml-auto` (existentes en `assets/main.css`). Las nuevas
//! `.vivo-*` las define la misión de CSS (`.vivo-head`, `.vivo-tabs`,
//! `.vivo-tab` + `--on`, `.vivo-layout`, `.vivo-estado-panel`,
//! `.vivo-estado-titulo`, `.vivo-estado-chips`, `.vivo-chip-estado` +
//! `--on`, `.vivo-buscador-panel`, `.vivo-buscador-titulo`, `.vivo-lista`,
//! `.vivo-fila*`, `.vivo-panel-live`, `.vivo-live-filas`,
//! `.vivo-live-grupo`, `.vivo-live-nombre`).
//!
//! Nota de rutas: [`ChampionCatalog`] vive en
//! `crate::data_dragon::models` y [`ChampionInfo`] en
//! `crate::lcu::models` (verificado contra `src/ui/menu.rs` y
//! `src/app.rs`; no existe un módulo `data_dragon::champions`).

use dioxus::prelude::*;

use crate::data_dragon::models::{ChampionCatalog, ChampionEntry};
use crate::live::LiveGame;
use crate::lcu::models::{AppState, ChampionInfo, Position};
use crate::ui::champ_select::champ_art::champ_art;
use crate::ui::champ_select::vs_guide_data::guide_for_slug;
use crate::ui::recommendations::format_winrate;
use crate::ui::shell::KlifyHeader;

/// Pestaña activa del header de la vista ([`VivoView`]). Arranca en champ
/// select; cuando `in_game` pasa a true, un `use_effect` pisa la tab a
/// `PartidaVivo` (solo ese sentido: volver es manual).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
enum VivoTab {
    #[default]
    ChampSelect,
    PartidaVivo,
}

/// Minúsculas + sin tildes/diacríticos comunes (`á`→`a`, `ü`→`u`, `ñ`→`n`).
/// Puro: la búsqueda de campeones compara strings normalizados en ambos
/// lados, así buscar "kaisa" encuentra "Kaisá" (futuro-proof; los nombres
/// de Data Dragon hoy no traen tildes).
#[must_use]
fn normalizar(texto: &str) -> String {
    texto
        .to_lowercase()
        .chars()
        .map(|caracter| match caracter {
            'á' | 'à' | 'ä' | 'â' | 'ã' | 'å' => 'a',
            'é' | 'è' | 'ë' | 'ê' => 'e',
            'í' | 'ì' | 'ï' | 'î' => 'i',
            'ó' | 'ò' | 'ö' | 'ô' | 'õ' => 'o',
            'ú' | 'ù' | 'ü' | 'û' => 'u',
            'ñ' => 'n',
            'ç' => 'c',
            'ý' => 'y',
            resto => resto,
        })
        .collect()
}

/// Filtro del buscador (puro, testeado): nombres que contienen `busqueda`
/// normalizada (case/acento-insensitive). Búsqueda vacía o solo espacios →
/// todos, en el orden de entrada. Es el criterio de `menu.rs::roster_cards`
/// (trim + lowercase + contains) extendido con el normalize de tildes.
#[must_use]
pub fn filtra_campeones<'a>(
    catalogo: &'a [ChampionInfo],
    busqueda: &str,
) -> Vec<&'a ChampionInfo> {
    let aguja = normalizar(busqueda.trim());
    if aguja.is_empty() {
        return catalogo.iter().collect();
    }
    catalogo
        .iter()
        .filter(|champ| normalizar(&champ.name).contains(&aguja))
        .collect()
}

/// Texto extra del punto de estado (puro, testeado): con el cliente ya
/// detectado (`WaitingForChampSelect`) avisa que el champ select real está
/// por tomar la pantalla; en otro estado no hay texto extra.
#[must_use]
pub fn estado_vivo(state: &AppState) -> Option<&'static str> {
    match state {
        AppState::WaitingForChampSelect => Some("detecté el cliente, entrando…"),
        AppState::Disconnected | AppState::InChampSelect(_) => None,
    }
}

/// Título del panel de estado (puro, testeado): "CHAMP SELECT ACTIVO" con
/// champ select real, "ESPERANDO CHAMP SELECT" en el resto de los casos.
#[must_use]
pub fn estado_titulo(hay_champ_select: bool) -> &'static str {
    if hay_champ_select {
        "CHAMP SELECT ACTIVO"
    } else {
        "ESPERANDO CHAMP SELECT"
    }
}

/// Tag de estado del encabezado (puro, testeado): "ESTADO · LIVE" con
/// champ select real, "ESTADO · ESPERANDO" en el resto de los casos.
#[must_use]
pub fn estado_tag(hay_champ_select: bool) -> &'static str {
    if hay_champ_select {
        "ESTADO · LIVE"
    } else {
        "ESTADO · ESPERANDO"
    }
}

/// Etiqueta visible de una pestaña del header (puro, testeado).
#[must_use]
fn tab_label(tab: VivoTab) -> &'static str {
    match tab {
        VivoTab::ChampSelect => "CHAMP SELECT",
        VivoTab::PartidaVivo => "PARTIDA EN VIVO",
    }
}

/// Clase de una pestaña según cuál está activa (puro, testeado): la activa
/// suma `vivo-tab--on` (tinte rojo de marca), la otra queda apagada.
#[must_use]
fn tab_clase(tab: VivoTab, activa: VivoTab) -> &'static str {
    if tab == activa {
        "vivo-tab vivo-tab--on"
    } else {
        "vivo-tab"
    }
}

/// Título del panel de partida en vivo (puro, testeado): "PARTIDA EN VIVO"
/// con `in_game` + [`LiveGame`] resuelto, "SIN PARTIDA EN VIVO" en el resto
/// de los casos.
#[must_use]
fn live_titulo(en_partida: bool) -> &'static str {
    if en_partida {
        "PARTIDA EN VIVO"
    } else {
        "SIN PARTIDA EN VIVO"
    }
}

/// Chip de estado del panel live (puro, testeado): solo hay chip
/// ("ESPERANDO PARTIDA", apagado) cuando NO hay partida.
#[must_use]
fn live_chip(en_partida: bool) -> Option<&'static str> {
    if en_partida {
        None
    } else {
        Some("ESPERANDO PARTIDA")
    }
}

/// Color del dot del panel live (puro, testeado): verde aprobado del
/// sistema (`#86c39c`, el mismo del dot del header y los chips `--on`) con
/// partida real; plata apagado mientras espera.
#[must_use]
fn live_dot_color(en_partida: bool) -> &'static str {
    if en_partida {
        "#86c39c"
    } else {
        "var(--silver)"
    }
}

/// Nombre de campeón para un id resuelto por la Live API (puro, testeado).
/// [`LiveGame`] trae aliados/enemigos como `(champion_id, position)` — SIN
/// nombres — así que la resolución id → nombre sale del catálogo que la
/// vista ya recibe. Sin catálogo o id desconocido → `None` (la fila
/// muestra "—", sin inventar datos).
#[must_use]
fn nombre_por_id(id: i64, catalogo: Option<&ChampionCatalog>) -> Option<String> {
    catalogo
        .and_then(|catalogo| catalogo.get(id))
        .map(|entry| entry.name.clone())
}

/// Nombres visibles de un equipo de [`LiveGame`] (puro, testeado): un
/// nombre por miembro, resuelto contra el catálogo; los irresolubles
/// quedan como "—" para que el conteo n del grupo coincida con la lista.
#[must_use]
fn nombres_equipo(
    miembros: &[(i64, Option<Position>)],
    catalogo: Option<&ChampionCatalog>,
) -> Vec<String> {
    miembros
        .iter()
        .map(|(id, _)| nombre_por_id(*id, catalogo).unwrap_or_else(|| String::from("—")))
        .collect()
}

/// Etiqueta corta de una lane de la ficha (puro, testeado): claves de
/// Lolalytics → convención del prototipo (`"middle"` → `"MID"`). Replica
/// `vs_guide::lane_label` (ese módulo es privado para `vivo`); una clave
/// desconocida se muestra en mayúsculas tal cual.
#[must_use]
fn lane_corto(lane: &str) -> String {
    match lane {
        "top" => String::from("TOP"),
        "jungle" => String::from("JGL"),
        "middle" => String::from("MID"),
        "bottom" => String::from("BOT"),
        "utility" | "support" => String::from("SUP"),
        other => other.to_uppercase(),
    }
}

/// Lane + WR de la ficha embebida de un campeón (puro, testeado), por slug
/// (`ChampionEntry::slug`, id ddragon en minúsculas). Sin ficha embebida o
/// con guide que no parsea → `(None, None)`: la fila muestra "—" sin
/// inventar datos. La lane puede seguir en `None` si el JSON no la trae.
#[must_use]
fn lane_wr_de_ficha(slug: &str) -> (Option<String>, Option<f64>) {
    guide_for_slug(slug).map_or((None, None), |guide| {
        (
            guide.meta.lane.as_deref().map(lane_corto),
            Some(guide.stats.wr),
        )
    })
}

/// WR formateado con etiqueta (puro, testeado): `"51.82%"` (recorta ceros
/// colgantes via [`format_winrate`]).
#[must_use]
fn wr_label(wr: f64) -> String {
    format!("{}%", format_winrate(wr))
}

/// Clase de color del WR (pura, testeada): verde por encima de 50, rojo
/// por debajo, sin clase (neutro) en 50 exacto. El coloreado lo hace CSS
/// con los modificadores `vivo-fila-wr--verde` / `vivo-fila-wr--rojo`.
#[must_use]
pub fn clase_wr(wr: f64) -> &'static str {
    if wr > 50.0 {
        "vivo-fila-wr--verde"
    } else if wr < 50.0 {
        "vivo-fila-wr--rojo"
    } else {
        ""
    }
}

/// Catálogo → lista de [`ChampionInfo`] ordenada alfabético por nombre
/// (desempate por id, mismo criterio que `menu.rs::roster_cards`). El
/// catálogo es un `HashMap` y NO garantiza orden: sin sort la lista
/// "salta" entre renders. Puro, testeado.
#[must_use]
fn infos_ordenadas(catalogo: &ChampionCatalog) -> Vec<ChampionInfo> {
    let mut infos: Vec<ChampionInfo> = catalogo
        .by_id
        .iter()
        .map(|(id, entry)| ChampionInfo {
            id: *id,
            name: entry.name.clone(),
            image_url: entry.image_url.clone(),
        })
        .collect();
    infos.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then(a.id.cmp(&b.id))
    });
    infos
}

/// Vista VIVO: header con pestañas (CHAMP SELECT / PARTIDA EN VIVO); la
/// tab de champ select mantiene el layout de dos columnas (`.vivo-layout`)
/// y la tab de partida muestra el espejo de la Live Client Data API
/// (`.vivo-panel-live`).
///
/// # Props
///
/// - `state`: estado del cliente (tag ESTADO ·, dot, chips y texto extra
///   "detecté el cliente, entrando…").
/// - `catalog`: catálogo de Data Dragon (`None` → hint "Catálogo
///   cargando…"; sin búsqueda la lista muestra todo). También resuelve los
///   ids de aliados/enemigos de la partida en vivo a nombres.
/// - `on_pick`: canal de salida con el campeón elegido (el padre entra al
///   modo manual con el mismo handler que usa el menú).
/// - `live_game`: partida en curso resuelta por la Live Client Data API
///   (`None` → sin partida).
/// - `in_game`: true mientras la Live API marca partida en curso; su
///   flanco subiente pisa la pestaña a PARTIDA EN VIVO.
#[component]
pub fn VivoView(
    state: ReadSignal<AppState>,
    catalog: Signal<Option<ChampionCatalog>>,
    on_pick: EventHandler<ChampionInfo>,
    live_game: Signal<Option<LiveGame>>,
    in_game: Signal<bool>,
) -> Element {
    let mut busqueda = use_signal(String::new);
    let mut tab = use_signal(VivoTab::default);

    // Auto-switch de pestaña: cuando arranca una partida real, el header
    // salta a PARTIDA EN VIVO. El efecto solo lee `in_game`, así que si el
    // usuario vuelve manualmente a CHAMP SELECT con la partida en curso,
    // no se lo molesta (el efecto no corre de nuevo hasta que `in_game`
    // cambie; solo pisa en el flanco subiente).
    use_effect(move || {
        if in_game() {
            tab.set(VivoTab::PartidaVivo);
        }
    });

    // Snapshots cortos: los guards no cruzan la construcción del rsx
    // (convención del proyecto).
    let state_snapshot: AppState = state.read().clone();
    let catalog_data = catalog.read().clone();
    let busqueda_texto = busqueda.read().clone();
    let live_data: Option<LiveGame> = live_game.read().clone();
    let tab_actual = tab();

    // Estados derivados del cliente: hay champ select REAL activo → tag
    // LIVE, título de estado y chips conmutan.
    let hay_champ_select = matches!(state_snapshot, AppState::InChampSelect(_));
    let tag_estado = estado_tag(hay_champ_select);
    let titulo_estado = estado_titulo(hay_champ_select);

    // Dot de estado: gris sin cliente, color de marca con cliente vivo
    // (en esta vista: WaitingForChampSelect).
    let conectado = !matches!(state_snapshot, AppState::Disconnected);
    let punto_color = if conectado {
        "var(--primary)"
    } else {
        "var(--silver)"
    };
    let estado_extra = estado_vivo(&state_snapshot);

    // Chips: "SIN CHAMP SELECT" queda encendido mientras NO hay champ
    // select real; "CHAMP SELECT ACTIVO · LIVE" se enciende solo con uno.
    let chip_sin_cs = if hay_champ_select {
        "vivo-chip-estado"
    } else {
        "vivo-chip-estado vivo-chip-estado--on"
    };
    let chip_cs_live = if hay_champ_select {
        "vivo-chip-estado vivo-chip-estado--on"
    } else {
        "vivo-chip-estado"
    };

    // Catálogo → ordenado → filtrado en vivo. `catalogo_listo` distingue
    // "cargando" (catálogo None) de "sin resultados" (con catálogo). Los
    // items de `filtra_campeones` se clonan a owned EAGER AL RENDER: cada
    // fila recibe un `ChampionInfo` propio (que [`VivoFila`] a su vez
    // clona para el closure 'static del click), mismo patrón que `menu.rs`.
    let catalogo_listo = catalog_data.is_some();
    let catalogo_infos = catalog_data.as_ref().map(infos_ordenadas);
    let resultados: Vec<ChampionInfo> = catalogo_infos
        .as_deref()
        .map(|infos| {
            filtra_campeones(infos, &busqueda_texto)
                .into_iter()
                .cloned()
                .collect()
        })
        .unwrap_or_default();

    // Partida en vivo: hay partida solo con in_game + LiveGame resuelto.
    // Todos los strings del panel se arman ANTES del rsx (guards fuera de
    // la construcción; modo vacío o id sin resolver → "—", sin inventar).
    let en_partida = in_game() && live_data.is_some();
    let titulo_live = live_titulo(en_partida);
    let punto_live = live_dot_color(en_partida);
    let modo_crudo = live_data
        .as_ref()
        .map(|game| game.game_mode.trim().to_string())
        .unwrap_or_default();
    let modo_texto = if modo_crudo.is_empty() {
        String::from("—")
    } else {
        modo_crudo
    };
    let mi_champ = live_data
        .as_ref()
        .map(|game| game.my_champion_name.clone())
        .unwrap_or_default();
    let nombres_aliados = live_data
        .as_ref()
        .map(|game| nombres_equipo(&game.allies, catalog_data.as_ref()))
        .unwrap_or_default();
    let nombres_enemigos = live_data
        .as_ref()
        .map(|game| nombres_equipo(&game.enemies, catalog_data.as_ref()))
        .unwrap_or_default();
    let n_aliados = nombres_aliados.len();
    let n_enemigos = nombres_enemigos.len();

    // Pestañas del header: etiquetas, clases y aria resueltos afuera del
    // rsx (los captures de formato de texto son solo identificadores).
    let label_cs = tab_label(VivoTab::ChampSelect);
    let label_pv = tab_label(VivoTab::PartidaVivo);
    let clase_cs = tab_clase(VivoTab::ChampSelect, tab_actual);
    let clase_pv = tab_clase(VivoTab::PartidaVivo, tab_actual);
    let seleccionada_cs = tab_actual == VivoTab::ChampSelect;
    let seleccionada_pv = tab_actual == VivoTab::PartidaVivo;

    rsx! {
        div { class: "view",
            KlifyHeader { state }

            main { class: "main-container",

                // ===== HEADER CON PESTAÑAS (grid: título | tabs | tag) =====
                div { class: "vivo-head",
                    h2 { class: "h-sec", style: "font-size: 17px;", "VIVO" }
                    div { class: "vivo-tabs", role: "tablist",
                        button {
                            class: "{clase_cs}",
                            r#type: "button",
                            role: "tab",
                            "aria-selected": "{seleccionada_cs}",
                            onclick: move |_| tab.set(VivoTab::ChampSelect),
                            "{label_cs}"
                        }
                        button {
                            class: "{clase_pv}",
                            r#type: "button",
                            role: "tab",
                            "aria-selected": "{seleccionada_pv}",
                            onclick: move |_| tab.set(VivoTab::PartidaVivo),
                            "{label_pv}"
                        }
                    }
                    span { class: "kl-badge-offline ml-auto",
                        span { "{tag_estado}" }
                    }
                }

                // ===== CONTENIDO SEGÚN PESTAÑA =====
                if seleccionada_cs {
                    // ----- TAB CHAMP SELECT: dos columnas (intacto) -----
                    div { class: "vivo-layout",

                        // COLUMNA IZQUIERDA · ESTADO
                        div { class: "vivo-estado-panel", role: "status",
                            span {
                                class: "dot-pulse",
                                style: "width: 10px; height: 10px; border-radius: 9999px; background: {punto_color};",
                            }
                            h3 { class: "vivo-estado-titulo", "{titulo_estado}" }
                            p { class: "vivo-sub t-dim",
                                "Esperando que abras una partida… Podés buscar un campeón y practicar en modo manual."
                            }
                            if let Some(extra) = estado_extra {
                                p { class: "cs-hint", "{extra}" }
                            }
                            div { class: "vivo-estado-chips",
                                span { class: "{chip_sin_cs}", "SIN CHAMP SELECT" }
                                span { class: "{chip_cs_live}", "CHAMP SELECT ACTIVO · LIVE" }
                            }
                        }

                        // COLUMNA DERECHA · BUSCADOR + LISTA
                        div { class: "vivo-buscador-panel",
                            label {
                                class: "vivo-buscador-titulo",
                                r#for: "vivo-buscador-input",
                                "BUSCAR CAMPEÓN"
                            }
                            input {
                                id: "vivo-buscador-input",
                                class: "kl-menu-search",
                                r#type: "text",
                                placeholder: "Buscar campeón…",
                                "aria-label": "Buscar campeón",
                                autocomplete: "off",
                                autocorrect: "off",
                                spellcheck: "false",
                                value: "{busqueda_texto}",
                                oninput: move |e| busqueda.set(e.value()),
                            }

                            if !catalogo_listo {
                                p { class: "cs-hint", "Catálogo cargando…" }
                            } else {
                                div { class: "vivo-lista",
                                    for champ in &resultados {
                                        VivoFila {
                                            key: "{champ.id}",
                                            champ: champ.clone(),
                                            entry: catalog_data
                                                .as_ref()
                                                .and_then(|d| d.get(champ.id))
                                                .cloned(),
                                            on_pick,
                                        }
                                    }

                                    if resultados.is_empty() {
                                        div { class: "empty",
                                            span { class: "label",
                                                "SIN RESULTADOS PARA «{busqueda_texto}»"
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // ----- PIE (sin footer: esta vista no recibe
                    // source_served/version) -----
                    p { class: "cs-hint vivo-pie",
                        "Sin champ select real: con partida abierta, el champ select del cliente pisa esta vista automáticamente."
                    }
                } else {
                    // ----- TAB PARTIDA EN VIVO: espejo de la Live API -----
                    div { class: "vivo-panel-live", role: "status",
                        span {
                            class: "dot-pulse",
                            style: "width: 10px; height: 10px; border-radius: 9999px; background: {punto_live};",
                        }
                        h3 { class: "vivo-estado-titulo", "{titulo_live}" }
                        if en_partida {
                            p { class: "vivo-sub t-dim", "MODO · {modo_texto}" }
                            p { class: "vivo-sub", "TU CAMPEÓN: {mi_champ}" }
                            div { class: "vivo-live-filas",
                                div { class: "vivo-live-grupo",
                                    span { class: "label", "ALIADOS {n_aliados}" }
                                    for (indice, nombre) in nombres_aliados.iter().enumerate() {
                                        span {
                                            class: "vivo-live-nombre",
                                            key: "{indice}",
                                            "{nombre}"
                                        }
                                    }
                                }
                                div { class: "vivo-live-grupo",
                                    span { class: "label", "ENEMIGOS {n_enemigos}" }
                                    for (indice, nombre) in nombres_enemigos.iter().enumerate() {
                                        span {
                                            class: "vivo-live-nombre",
                                            key: "{indice}",
                                            "{nombre}"
                                        }
                                    }
                                }
                            }
                        } else {
                            p { class: "vivo-sub t-dim",
                                "cuando entres a una partida, el espejo del cliente la muestra acá"
                            }
                            if let Some(chip) = live_chip(en_partida) {
                                div { class: "vivo-estado-chips",
                                    span { class: "vivo-chip-estado", "{chip}" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Fila de la lista de campeones de VIVO: retrato cuadrado de 40px +
/// nombre + tag de lane + WR (de la ficha embebida), emite `on_pick` con
/// el [`ChampionInfo`] elegido (el padre entra al modo manual).
///
/// Recibe TODO owned (`champ` clonado al render por el loop padre, `entry`
/// clonada del catálogo) para que el closure del click sea 'static sin
/// pedir prestado nada del render de [`VivoView`]: es el patrón de
/// `menu.rs` (clon eager al render, no por click; lección E0597).
///
/// Resolución del retrato (mecanismo EXACTO del menú): `champ_art` con el
/// `ddragon_id` de la entrada; sin PNG propio, cae al loading screen de
/// Data Dragon ([`ChampionEntry::loading_url`]); sin entrada en el
/// catálogo, al placeholder del LCU (`ChampionInfo::image_url`).
///
/// Lane y WR salen de la ficha embebida via [`lane_wr_de_ficha`] con el
/// slug de la entrada; sin ficha (o sin entry) la fila muestra "—" en
/// ambos, sin inventar datos. El WR se colorea por CSS con
/// [`clase_wr`]; como props no cambian entre renders, el lookup de la
/// ficha corre una vez por fila (cache `OnceLock` en `vs_guide_data`).
#[component]
fn VivoFila(
    champ: ChampionInfo,
    entry: Option<ChampionEntry>,
    on_pick: EventHandler<ChampionInfo>,
) -> Element {
    // Ficha embebida: lane + WR por slug; sin entry (o sin ficha) → "—".
    let (lane, wr) = entry
        .as_ref()
        .map(|entrada| lane_wr_de_ficha(&entrada.slug))
        .unwrap_or_default();
    let lane_texto = lane.unwrap_or_else(|| String::from("—"));
    let wr_texto = wr.map(wr_label).unwrap_or_else(|| String::from("—"));
    let wr_clase = wr.map(clase_wr).unwrap_or_default();

    rsx! {
        button {
            key: "{champ.id}",
            class: "vivo-fila",
            r#type: "button",
            "aria-label": format!("Elegir {} (modo manual)", champ.name),
            onclick: move |_| on_pick.call(champ.clone()),
            span { class: "vivo-fila-retrato",
                {match entry.as_ref().and_then(|entrada| champ_art(&entrada.ddragon_id)) {
                    Some(art) => rsx! { img { src: art, alt: "{champ.name}" } },
                    None => match entry.as_ref() {
                        Some(entrada) => rsx! { img { src: entrada.loading_url(), alt: "{champ.name}" } },
                        None => rsx! { img { src: "{champ.image_url}", alt: "{champ.name}" } },
                    },
                }}
            }
            span { class: "vivo-fila-nombre", "{champ.name}" }
            span { class: "vivo-fila-lane", "{lane_texto}" }
            span { class: "vivo-fila-wr {wr_clase}", "{wr_texto}" }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Fixture de 4 campeones: 3 reales (en orden NO alfabético) +
    /// "Kaisá" sintética (Data Dragon no trae nombres con tilde hoy; sirve
    /// para probar la búsqueda acento-insensitive).
    fn champs_fixture() -> Vec<ChampionInfo> {
        [
            (103, "Ahri", "https://ddragon.test/Ahri.png"),
            (84, "Akali", "https://ddragon.test/Akali.png"),
            (1, "Annie", "https://ddragon.test/Annie.png"),
            (999, "Kaisá", "https://ddragon.test/Kaisa.png"),
        ]
        .into_iter()
        .map(|(id, name, image_url)| ChampionInfo {
            id,
            name: name.to_string(),
            image_url: image_url.to_string(),
        })
        .collect()
    }

    /// Nombres de un resultado de filtro, en orden.
    fn nombres<'a>(res: &[&'a ChampionInfo]) -> Vec<&'a str> {
        res.iter().map(|champ| champ.name.as_str()).collect()
    }

    /// Búsqueda vacía (o solo espacios) → todos, en el orden de entrada.
    #[test]
    fn busqueda_vacia_devuelve_todos() {
        let champs = champs_fixture();
        assert_eq!(filtra_campeones(&champs, "").len(), 4);
        assert_eq!(nombres(&filtra_campeones(&champs, "  ")).len(), 4);
    }

    /// Filtro por nombre case-insensitive; sin match → vacío (nunca panic).
    #[test]
    fn busqueda_case_insensitive() {
        let champs = champs_fixture();
        assert_eq!(nombres(&filtra_campeones(&champs, "AN")), vec!["Annie"]);
        assert_eq!(nombres(&filtra_campeones(&champs, "ak")), vec!["Akali"]);
        assert!(filtra_campeones(&champs, "zzz").is_empty());
    }

    /// La búsqueda ignora tildes en ambos lados (nombre y query).
    #[test]
    fn busqueda_sin_acentos() {
        let champs = champs_fixture();
        assert_eq!(
            nombres(&filtra_campeones(&champs, "kaisa")),
            vec!["Kaisá"]
        );
        assert_eq!(
            nombres(&filtra_campeones(&champs, "KÁISÁ")),
            vec!["Kaisá"]
        );
    }

    /// Catálogo → ChampionInfo ordenado alfabético por nombre (desempate
    /// por id) con nombre/icono tomados del catálogo.
    #[test]
    fn infos_ordenadas_alfabetico_por_nombre() {
        let by_id: HashMap<i64, ChampionEntry> = [
            ("Annie", "annie", "Annie", 1),
            ("Ahri", "ahri", "Ahri", 103),
            ("Akali", "akali", "Akali", 84),
        ]
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
        let catalogo = ChampionCatalog {
            version: String::from("16.21.1"),
            by_id,
        };

        let infos = infos_ordenadas(&catalogo);
        let orden: Vec<&str> = infos.iter().map(|info| info.name.as_str()).collect();
        assert_eq!(orden, vec!["Ahri", "Akali", "Annie"]);

        let ahri = &infos[0];
        assert_eq!(ahri.id, 103);
        assert_eq!(ahri.image_url, "https://ddragon.test/Ahri.png");
    }

    /// Texto extra del punto de estado: solo con cliente ya detectado
    /// (WaitingForChampSelect); sin cliente ni en champ select real.
    #[test]
    fn estado_vivo_texto_extra() {
        assert_eq!(
            estado_vivo(&AppState::WaitingForChampSelect),
            Some("detecté el cliente, entrando…")
        );
        assert_eq!(estado_vivo(&AppState::Disconnected), None);
        assert_eq!(estado_vivo(&AppState::InChampSelect(None)), None);
    }

    /// Título del panel de estado: ACTIVO con champ select real, ESPERANDO
    /// en el resto.
    #[test]
    fn estado_titulo_segun_champ_select() {
        assert_eq!(estado_titulo(true), "CHAMP SELECT ACTIVO");
        assert_eq!(estado_titulo(false), "ESPERANDO CHAMP SELECT");
    }

    /// Tag del encabezado: LIVE con champ select real, ESPERANDO en el
    /// resto.
    #[test]
    fn estado_tag_segun_champ_select() {
        assert_eq!(estado_tag(true), "ESTADO · LIVE");
        assert_eq!(estado_tag(false), "ESTADO · ESPERANDO");
    }

    /// Etiquetas cortas de lane (convención del prototipo); desconocida →
    /// mayúsculas tal cual.
    #[test]
    fn lane_corto_usa_las_etiquetas_del_prototipo() {
        assert_eq!(lane_corto("top"), "TOP");
        assert_eq!(lane_corto("jungle"), "JGL");
        assert_eq!(lane_corto("middle"), "MID");
        assert_eq!(lane_corto("bottom"), "BOT");
        assert_eq!(lane_corto("utility"), "SUP");
        assert_eq!(lane_corto("support"), "SUP");
        assert_eq!(lane_corto("aram"), "ARAM");
    }

    /// Lane + WR de la ficha embebida: annie tiene ficha (lane "middle" →
    /// "MID", wr 51.82) y swain también (lane "utility" → "SUP",
    /// wr 50.05); sin ficha ("campeon-inexistente") o slug vacío →
    /// (None, None), sin inventar datos.
    #[test]
    fn lane_wr_de_ficha_lee_la_ficha_embebida() {
        let (lane, wr) = lane_wr_de_ficha("annie");
        assert_eq!(lane.as_deref(), Some("MID"));
        let wr = wr.expect("annie tiene wr en su ficha");
        assert!((wr - 51.82).abs() < 1e-9);

        let (lane, wr) = lane_wr_de_ficha("swain");
        assert_eq!(lane.as_deref(), Some("SUP"));
        assert_eq!(wr, Some(50.05));

        assert_eq!(lane_wr_de_ficha("campeon-inexistente"), (None, None));
        assert_eq!(lane_wr_de_ficha(""), (None, None));
    }

    /// WR formateado con `%`, recortando ceros colgantes.
    #[test]
    fn wr_label_recorta_ceros_colgantes() {
        assert_eq!(wr_label(51.82), "51.82%");
        assert_eq!(wr_label(51.2), "51.2%");
        assert_eq!(wr_label(50.0), "50%");
    }

    /// Clase de color del WR: verde > 50, rojo < 50, neutro en 50 exacto.
    #[test]
    fn clase_wr_verde_rojo_neutro() {
        assert_eq!(clase_wr(51.82), "vivo-fila-wr--verde");
        assert_eq!(clase_wr(50.01), "vivo-fila-wr--verde");
        assert_eq!(clase_wr(48.1), "vivo-fila-wr--rojo");
        assert_eq!(clase_wr(50.0), "");
    }

    /// Etiquetas visibles de las dos pestañas del header.
    #[test]
    fn tab_label_cubre_las_dos_pestanas() {
        assert_eq!(tab_label(VivoTab::ChampSelect), "CHAMP SELECT");
        assert_eq!(tab_label(VivoTab::PartidaVivo), "PARTIDA EN VIVO");
    }

    /// La pestaña activa lleva `vivo-tab--on`; la otra queda apagada.
    #[test]
    fn tab_clase_marca_solo_la_activa() {
        assert_eq!(
            tab_clase(VivoTab::ChampSelect, VivoTab::ChampSelect),
            "vivo-tab vivo-tab--on"
        );
        assert_eq!(
            tab_clase(VivoTab::PartidaVivo, VivoTab::ChampSelect),
            "vivo-tab"
        );
        assert_eq!(
            tab_clase(VivoTab::PartidaVivo, VivoTab::PartidaVivo),
            "vivo-tab vivo-tab--on"
        );
    }

    /// La vista arranca en CHAMP SELECT (default de la señal de pestaña).
    #[test]
    fn vivo_tab_default_es_champ_select() {
        assert_eq!(VivoTab::default(), VivoTab::ChampSelect);
    }

    /// Textos del panel live según hay partida (in_game + LiveGame) o no:
    /// título, chip y color del dot.
    #[test]
    fn live_estado_segun_partida() {
        assert_eq!(live_titulo(true), "PARTIDA EN VIVO");
        assert_eq!(live_titulo(false), "SIN PARTIDA EN VIVO");
        assert_eq!(live_chip(true), None);
        assert_eq!(live_chip(false), Some("ESPERANDO PARTIDA"));
        assert_eq!(live_dot_color(true), "#86c39c");
        assert_eq!(live_dot_color(false), "var(--silver)");
    }

    /// Nombres de aliados/enemigos: `LiveGame` trae ids (sin nombres), así
    /// que la resolución sale del catálogo; id desconocido o catálogo
    /// ausente → "—" (mismo largo que el equipo, sin inventar datos).
    #[test]
    fn nombres_equipo_resuelve_contra_el_catalogo() {
        let mut by_id = HashMap::new();
        by_id.insert(
            34,
            ChampionEntry {
                name: String::from("Akali"),
                image_url: String::from("https://ddragon.test/Akali.png"),
                slug: String::from("akali"),
                ddragon_id: String::from("Akali"),
            },
        );
        let catalogo = ChampionCatalog {
            version: String::from("16.21.1"),
            by_id,
        };

        let equipo = vec![(34, Some(Position::Jungle)), (999, None)];
        assert_eq!(
            nombres_equipo(&equipo, Some(&catalogo)),
            vec![String::from("Akali"), String::from("—")]
        );
        assert_eq!(
            nombres_equipo(&equipo, None),
            vec![String::from("—"), String::from("—")]
        );
        assert!(nombres_equipo(&[], Some(&catalogo)).is_empty());
    }
}
