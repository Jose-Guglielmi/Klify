//! Vista VIVO (tab del menú lateral): pantalla de espera del champ select
//! REAL, con el layout de dos columnas del prototipo aprobado.
//!
//! Bajo el header, `main-container` tiene:
//!
//! - **Fila de título**: "VIVO · CHAMP SELECT" (estilo itálica skew de
//!   `.h-sec`, chico) + a la derecha el tag sesgado `.kl-badge-offline`
//!   con "ESTADO · ESPERANDO" (o "ESTADO · LIVE" con champ select real,
//!   [`AppState::InChampSelect`]).
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
//! Con champ select real el usuario NUNCA llega acá: `derive_view`
//! (app.rs) pisa la navegación y manda al champ select. Esta vista es solo
//! el camino manual/espera.
//!
//! Pie: sin [`KlifyFooter`](crate::ui::shell::KlifyFooter) (esta vista no
//! recibe `source_served`/`version`); el pie es un `.cs-hint`.
//!
//! Clases: reusa `.view`, `.main-container`, `.zone-head`, `.h-sec`,
//! `.kl-badge-offline`, `.dot-pulse`, `.kl-menu-search`, `.empty`,
//! `.cs-hint`, `.label`, `.t-dim` y `.ml-auto` (existentes en
//! `assets/main.css`). Las nuevas `.vivo-*` (`.vivo-layout`,
//! `.vivo-estado-panel`, `.vivo-estado-titulo`, `.vivo-estado-chips`,
//! `.vivo-chip-estado` + `--on`, `.vivo-buscador-panel`,
//! `.vivo-buscador-titulo`, `.vivo-lista`, `.vivo-fila`,
//! `.vivo-fila-retrato`, `.vivo-fila-nombre`, `.vivo-fila-lane`,
//! `.vivo-fila-wr` + `--verde`/`--rojo`) las agrega la misión de CSS.
//!
//! Nota de rutas: [`ChampionCatalog`] vive en
//! `crate::data_dragon::models` y [`ChampionInfo`] en
//! `crate::lcu::models` (verificado contra `src/ui/menu.rs` y
//! `src/app.rs`; no existe un módulo `data_dragon::champions`).

use dioxus::prelude::*;

use crate::data_dragon::models::{ChampionCatalog, ChampionEntry};
use crate::lcu::models::{AppState, ChampionInfo};
use crate::ui::champ_select::champ_art::champ_art;
use crate::ui::champ_select::vs_guide_data::guide_for_slug;
use crate::ui::recommendations::format_winrate;
use crate::ui::shell::KlifyHeader;

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

/// Vista VIVO: espera del champ select real + buscador manual, en dos
/// columnas (`.vivo-layout`) bajo la fila de título.
///
/// # Props
///
/// - `state`: estado del cliente (tag ESTADO ·, dot, chips y texto extra
///   "detecté el cliente, entrando…").
/// - `catalog`: catálogo de Data Dragon (`None` → hint "Catálogo
///   cargando…"; sin búsqueda la lista muestra todo).
/// - `on_pick`: canal de salida con el campeón elegido (el padre entra al
///   modo manual con el mismo handler que usa el menú).
#[component]
pub fn VivoView(
    state: ReadSignal<AppState>,
    catalog: Signal<Option<ChampionCatalog>>,
    on_pick: EventHandler<ChampionInfo>,
) -> Element {
    let mut busqueda = use_signal(String::new);

    // Snapshots cortos: los guards no cruzan la construcción del rsx
    // (convención del proyecto).
    let state_snapshot: AppState = state.read().clone();
    let catalog_data = catalog.read().clone();
    let busqueda_texto = busqueda.read().clone();

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

    rsx! {
        div { class: "view",
            KlifyHeader { state }

            main { class: "main-container",

                // ===== FILA DE TÍTULO =====
                div { class: "zone-head",
                    h2 { class: "h-sec", style: "font-size: 17px;",
                        "VIVO · CHAMP SELECT"
                    }
                    span { class: "kl-badge-offline ml-auto",
                        span { "{tag_estado}" }
                    }
                }

                // ===== DOS COLUMNAS =====
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

                // ===== PIE (sin footer: esta vista no recibe
                // source_served/version) =====
                p { class: "cs-hint vivo-pie",
                    "Sin champ select real: con partida abierta, el champ select del cliente pisa esta vista automáticamente."
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
}
