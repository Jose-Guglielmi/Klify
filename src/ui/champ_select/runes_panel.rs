//! Tab RUNAS del champ select (FASE 11b-2a · aplicación en 11b-2c).
//!
//! Renderiza el [`RecommendationsState`] del recomendador como **bandas
//! horizontales** (hasta 4) apiladas en `cs-runes`, una tarjeta ancha y
//! baja (`cs-page cs-page--banda`) por página:
//!
//! - `Loading` → "Consultando estadísticas de runas…".
//! - `Unavailable` → motivo + botón REINTENTAR (incrementa el contador
//!   `retry` que limpia el anti-reintento de [`run_recommender`]).
//! - `Ready` → la banda 0 conserva el tratamiento rojo (`cs-page--best`,
//!   badge "MEJOR PICK"); las páginas sintetizadas
//!   ([`RunePageStats::sintetizada`]) llevan `cs-page--variante`, badge
//!   gris "VARIANTE" y métricas "— SIN DATOS" (son variantes del mismo
//!   set, sin muestra propia).
//!
//! Cada banda es un flex row de segmentos (`cs-banda-seg`): keystone
//! (icono 64px + badges + chip de estilo + label), árbol principal (3
//! runas 40px), secundario (2), shards con nombre ([`shard_label`]),
//! métricas (WR grande + muestra) y el botón "APLICAR AL CLIENTE" (lo
//! traen todas las bandas): consulta el inventario LCU
//! ([`crate::lcu::actions`]) y crea (o reemplaza) la página "Klify ·
//! {champ}". Con el inventario lleno abre el modal de elección
//! ([`ModalInventario`]) para decidir qué página borrar.
//!
//! El reparto keystone/principales/secundarias es [`split_page`] (puro,
//! testeado en [`crate::ui::recommendations`]); el color del chip de
//! estilo es [`clase_chip`] (puro, testeado acá) y la orquestación pura
//! del flujo es [`siguiente_paso`] (idem). El resto de la aplicación es
//! async/UI.

use dioxus::prelude::*;

use crate::data_dragon::runes::RuneCatalog;
use crate::lcu::actions::{
    borrar_pagina_runas, consultar_inventario_runas, crear_pagina_runas, InventarioRunas,
    LcuPageSummary, ResultadoCreacion,
};
use crate::lcu::events::es_placeholder;
use crate::stats::source::shard_name;
use crate::stats::source::{RecommendationsState, RunePageStats};
use crate::ui::format::format_thousands;
use crate::ui::recommendations::{format_winrate, split_page, RuneRow, TreeInfo};

/// Nombre visible de un shard para la banda (puro, testeado): nombre de la
/// tabla [`SHARD_NAMES`] o `#id`.
#[must_use]
pub fn shard_label(id: i64) -> String {
    shard_name(id)
}

/// Clase de color del chip de estilo según su texto (puro, testeado):
/// rojo para daño/pelea, verde para sosten/defensa, azul para
/// movilidad/control y gris como fallback. Sin distinguir mayúsculas y
/// con precedencia en el orden de las categorías (gana la primera).
#[must_use]
fn clase_chip(estilo: &str) -> &'static str {
    let estilo = estilo.to_uppercase();
    let rojo = ["DAÑO", "PELEA", "COMBATE", "ALL"];
    let verde = ["SOSTEN", "AGUANTE", "PROTECCION", "DPS"];
    let azul = ["MOVILIDAD", "ROAM", "UTILIDAD", "CONTROL", "POKE"];

    if rojo.iter().any(|clave| estilo.contains(*clave)) {
        "cs-chip--rojo"
    } else if verde.iter().any(|clave| estilo.contains(*clave)) {
        "cs-chip--verde"
    } else if azul.iter().any(|clave| estilo.contains(*clave)) {
        "cs-chip--azul"
    } else {
        "cs-chip--gris"
    }
}

/// Estado de aplicación de nuestra página de runas al cliente.
///
/// Vive como [`Signal`] en cada [`RunePageCard`] (una copia por banda) y
/// alimenta el feedback de su botón.
#[derive(Debug, Clone, PartialEq)]
enum Aplicacion {
    /// Sin flujo en curso: botón "APLICAR AL CLIENTE".
    Inactiva,
    /// Consultando [`consultar_inventario_runas`].
    Consultando,
    /// Borrando/creando páginas en el cliente.
    Aplicando,
    /// La página "Klify · {champ}" quedó aplicada (re-click repite).
    Aplicada,
    /// Falló un paso; guarda el motivo para la línea de error.
    Error(String),
}

/// Próximo paso de la orquestación, decidido en base al inventario (puro).
/// El inventario lleno NO se calcula acá (el límite lo impone el servidor y
/// varía por cuenta): si el POST de creación responde
/// [`ResultadoCreacion::SinLugar`], [`spawn_aplicar`] abre el modal en
/// caliente con las páginas ya leídas.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Paso {
    /// Ya existe nuestra página (`nuestra: Some`): borrarla y recrearla.
    ReemplazarNuestra(i64),
    /// Sin página propia: crear directo (si el inventario está lleno el
    /// servidor lo dirá y se abre el modal de elección).
    CrearDirecto,
}

/// Dado el inventario del cliente, qué paso sigue para aplicar nuestra
/// página (puro, testeado). El caso `nuestra: Some` gana: reemplazar no
/// agrega páginas.
#[must_use]
fn siguiente_paso(inv: &InventarioRunas) -> Paso {
    match inv.nuestra {
        Some(id) => Paso::ReemplazarNuestra(id),
        None => Paso::CrearDirecto,
    }
}

/// Motivo corto para la línea de error del botón: primera línea del
/// mensaje, recortada (puro).
#[must_use]
fn motivo_corto(error: &str) -> String {
    const MAX: usize = 90;
    let primera = error.lines().next().unwrap_or(error);
    if primera.chars().count() <= MAX {
        primera.to_string()
    } else {
        let mut corta: String = primera.chars().take(MAX).collect();
        corta.push('…');
        corta
    }
}

/// Fila desacoplada de [`LcuPageSummary`] para las props del modal
/// (las props de Dioxus exigen `Clone + PartialEq`).
#[derive(Clone, PartialEq)]
struct PaginaInventario {
    id: i64,
    name: String,
    is_deletable: bool,
}

/// [`LcuPageSummary`] → fila del modal (puro).
#[must_use]
fn a_filas_modal(pages: &[LcuPageSummary]) -> Vec<PaginaInventario> {
    pages
        .iter()
        .map(|p| PaginaInventario {
            id: p.id,
            name: p.name.clone(),
            is_deletable: p.is_deletable,
        })
        .collect()
}

/// Tab RUNAS.
#[component]
pub(crate) fn RunesPanel(
    recommendations: ReadSignal<RecommendationsState>,
    runes: ReadSignal<Option<RuneCatalog>>,
    mut retry: Signal<u64>,
    champ_nombre: String,
    client_connected: bool,
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
                // Columna de bandas horizontales (el grid de 2 columnas
                // quedó en el panel vertical; el layout lo hace main.css).
                div { class: "cs-runes",
                    {pages.iter().take(4).enumerate().map(|(index, page)| rsx! {
                        RunePageCard {
                            // Remonta al cambiar el champ: resetea el estado
                            // de aplicación (no arrastrar "APLICADA ✓" vieja).
                            key: "{champ_nombre}-{index}",
                            page: page.clone(),
                            runes,
                            best: index == 0,
                            champ_nombre: champ_nombre.clone(),
                            client_connected,
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

/// Una banda de página de runas: tarjeta ancha y baja (`cs-page--banda`),
/// todo en un flex row de segmentos. `best` = banda 0: borde/glow rojos
/// (`cs-page--best`), badge "MEJOR PICK" y WR en rojo. Las páginas
/// sintetizadas ([`RunePageStats::sintetizada`]) llevan badge "VARIANTE" y
/// "— SIN DATOS" en métricas, pero conservan el botón "APLICAR AL CLIENTE"
/// (la página es válida: 6 perks reales). El catálogo llega como señal
/// para no clonarlo por banda en cada render.
#[component]
fn RunePageCard(
    page: RunePageStats,
    runes: ReadSignal<Option<RuneCatalog>>,
    best: bool,
    champ_nombre: String,
    client_connected: bool,
) -> Element {
    let split = split_page(&page, (*runes.read()).as_ref());
    let winrate = format_winrate(page.winrate_pct);
    let games = format_thousands(i64::try_from(page.games).unwrap_or(i64::MAX));

    // La keystone se usa dos veces (tile + nombre): clon barato de la fila.
    let keystone = split.keystone.clone();

    // Clases de la banda: base + destacada + variante sintética.
    let clase = format!(
        "cs-page cs-page--banda{}{}",
        if best { " cs-page--best" } else { "" },
        if page.sintetizada { " cs-page--variante" } else { "" },
    );

    // Estado de aplicación + páginas del modal (una copia por banda).
    let estado: Signal<Aplicacion> = use_signal(|| Aplicacion::Inactiva);
    let modal: Signal<Option<Vec<PaginaInventario>>> = use_signal(|| None);

    // Snapshot corto: el guard no cruza la construcción del rsx.
    let estado_snapshot = estado.read().clone();

    // Clones para el onclick: el closure `move` los consume y el rsx de
    // abajo (modal) todavía necesita `page` y `champ_nombre` intactos.
    let page_para_click = page.clone();
    let champ_para_click = champ_nombre.clone();

    rsx! {
        div { class: "{clase}",

            // SEG KEYSTONE · icono 64px + badges/chip + identidad de la página
            div { class: "cs-banda-seg",
                KeystoneTile { rune: keystone.clone() }
                div { class: "min-w-0",
                    div {
                        "style": "display: flex; flex-wrap: wrap; align-items: center; gap: 6px;",
                        {if best { rsx! {
                            span { class: "cs-badge cs-badge--best", "Mejor pick" }
                        } } else if page.sintetizada { rsx! {
                            span { class: "cs-badge cs-badge--variante", "Variante" }
                        } } else { rsx! {} }}
                        {page.estilo.as_deref().map(|estilo| rsx! {
                            span { class: "cs-chip {clase_chip(estilo)}", "{estilo}" }
                        })}
                    }
                    div { class: "label mt-1", "{page.label}" }
                    {if let Some(keystone) = &keystone { rsx! {
                        div { class: "cs-rune-name", "{keystone.name}" }
                    } } else { rsx! {} }}
                }
            }

            // SEG PRIMARIO · título chico + 3 runas menores en fila (40px)
            span { class: "cs-banda-divisor", "aria-hidden": "true" }
            SegmentoArbol { role: "Primario", tree: split.primary_tree, runes: split.primary }

            // SEG SECUNDARIO · título chico + 2 runas en fila
            span { class: "cs-banda-divisor", "aria-hidden": "true" }
            SegmentoArbol { role: "Secundario", tree: split.secondary_tree, runes: split.secondary }

            // SEG SHARDS · 3 pills en fila (sin shards no hay segmento)
            {if !page.stat_shards.is_empty() { rsx! {
                span { class: "cs-banda-divisor", "aria-hidden": "true" }
                div { class: "cs-banda-seg",
                    span { class: "cs-banda-seg-titulo", "Shards" }
                    div {
                        "style": "display: flex; flex-wrap: wrap; align-items: center; gap: 6px;",
                        {page.stat_shards.iter().map(|id| rsx! {
                            span { class: "cs-shard", "{shard_label(*id)}" }
                        })}
                    }
                }
            } } else { rsx! {} }}

            // SEG MÉTRICAS · WR grande + muestra (el pick rate no viaja en
            // RunePageStats; cuando alguna fuente lo publique, va acá).
            span { class: "cs-banda-divisor", "aria-hidden": "true" }
            div { class: "cs-banda-seg",
                span { class: "cs-banda-seg-titulo", "Métricas" }
                {if page.sintetizada { rsx! {
                    div { class: "cs-page-wr-row",
                        span { class: "num-display tnum cs-page-wr t-dim", "—" }
                        span { class: "label t-dim", "SIN DATOS" }
                    }
                    span { class: "cs-hint", "variante sintética del mismo set" }
                } } else { rsx! {
                    div { class: "cs-page-wr-row",
                        span { class: if best { "num-display tnum cs-page-wr t-bright" } else { "num-display tnum cs-page-wr t-on" },
                            "{winrate}%"
                        }
                        span { class: "label t-dim", "WR" }
                        span { class: "cell-value t-dim",
                            span { class: "hud-sep", "·" }
                            " {games} partidas"
                        }
                    }
                } }}
            }

            // SEG APLICAR · botón (todas las bandas lo traen): aplicar la
            // página al cliente de LoL.
            span { class: "cs-banda-divisor", "aria-hidden": "true" }
            div { class: "cs-banda-seg",
                if client_connected {
                    button {
                        class: if matches!(estado_snapshot, Aplicacion::Aplicada) {
                            "btn-primary cs-apply-ok"
                        } else {
                            "btn-primary"
                        },
                        "type": "button",
                        disabled: matches!(
                            estado_snapshot,
                            Aplicacion::Consultando | Aplicacion::Aplicando
                        ),
                        onclick: move |_| {
                            spawn_aplicar(
                                estado,
                                modal,
                                page_para_click.clone(),
                                champ_para_click.clone(),
                            );
                        },
                        {match &estado_snapshot {
                            Aplicacion::Aplicada => "APLICADA ✓",
                            Aplicacion::Consultando | Aplicacion::Aplicando => "APLICANDO…",
                            Aplicacion::Inactiva | Aplicacion::Error(_) => "APLICAR AL CLIENTE",
                        }}
                    }
                } else {
                    button { class: "btn-primary", disabled: true, "APLICAR AL CLIENTE" }
                }

                {if client_connected {
                    match &estado_snapshot {
                        Aplicacion::Error(e) => rsx! {
                            p { class: "cs-apply-err", "{motivo_corto(e)}" }
                        },
                        _ => rsx! {},
                    }
                } else {
                    rsx! {
                        span { class: "cs-soon",
                            span { class: "dot-pulse", "style": "width: 8px; height: 8px; border-radius: 9999px; background: var(--silver);" }
                            span { class: "label", "CLIENTE NO CONECTADO" }
                        }
                    }
                }}
            }

            {match modal() {
                Some(pages) => rsx! {
                    ModalInventario {
                        pages,
                        page: page.clone(),
                        champ_nombre: champ_nombre.clone(),
                        estado,
                        abrir: modal,
                    }
                },
                None => rsx! {},
            }}

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

/// Orquesta el click en "APLICAR AL CLIENTE": consulta el inventario y
/// resuelve el [`Paso`] (reemplazo directo, creación directa o apertura del
/// modal de elección). Patrón `spawn_match_fetch` de [`crate::ui::profile`]:
/// función libre con signals, así el handler `onclick` es `Fn` y el botón
/// vuelve a funcionar tras `Aplicada` (re-click repite el flujo).
fn spawn_aplicar(
    mut estado: Signal<Aplicacion>,
    mut modal: Signal<Option<Vec<PaginaInventario>>>,
    page: RunePageStats,
    champ_nombre: String,
) {
    // Anti doble-click: ignora si ya hay un flujo en curso.
    if matches!(*estado.read(), Aplicacion::Consultando | Aplicacion::Aplicando) {
        return;
    }
    // Guard de placeholder: si el nombre es "Campeón {id}" el catálogo aún
    // no resolvió el campeón y crearíamos una página mal nombrada.
    if es_placeholder(&champ_nombre) {
        estado.set(Aplicacion::Error(String::from(
            "el catálogo todavía está resolviendo el campeón; esperá un segundo y probá de nuevo",
        )));
        return;
    }
    estado.set(Aplicacion::Consultando);
    spawn(async move {
        match consultar_inventario_runas(&champ_nombre).await {
            Err(e) => estado.set(Aplicacion::Error(e)),
            Ok(inv) => match siguiente_paso(&inv) {
                Paso::ReemplazarNuestra(id) => {
                    estado.set(Aplicacion::Aplicando);
                    match aplicar_reemplazando(&page, &champ_nombre, id).await {
                        Ok(()) => estado.set(Aplicacion::Aplicada),
                        Err(e) => estado.set(Aplicacion::Error(e)),
                    }
                }
                Paso::CrearDirecto => {
                    estado.set(Aplicacion::Aplicando);
                    match crear_pagina_runas(&page, &champ_nombre).await {
                        Ok(ResultadoCreacion::Creada) => estado.set(Aplicacion::Aplicada),
                        Ok(ResultadoCreacion::SinLugar) => {
                            // El servidor rechazó la creación (inventario
                            // lleno): el usuario elige qué página borrar.
                            // El botón vuelve a Inactiva con el modal abierto.
                            estado.set(Aplicacion::Inactiva);
                            modal.set(Some(a_filas_modal(&inv.pages)));
                        }
                        Err(e) => estado.set(Aplicacion::Error(e)),
                    }
                }
            },
        }
    });
}

/// Borra la página `id` y crea la nuestra; corta con el motivo del primer
/// paso que falle. Tras borrar hay un hueco libre, así que `SinLugar` acá
/// sería una carrera del servidor: se reporta como error descriptivo.
async fn aplicar_reemplazando(
    page: &RunePageStats,
    champ_nombre: &str,
    id: i64,
) -> Result<(), String> {
    borrar_pagina_runas(id).await?;
    match crear_pagina_runas(page, champ_nombre).await {
        Ok(ResultadoCreacion::Creada) => Ok(()),
        Ok(ResultadoCreacion::SinLugar) => Err(String::from(
            "el cliente no permitió crear la página aun tras liberar el hueco: inventario lleno",
        )),
        Err(e) => Err(e),
    }
}

/// Confirma la elección del modal: borra la página elegida y crea la
/// nuestra. Si falla el borrado, el modal queda abierto con el motivo; si
/// falla la creación (con nuestra página ya borrada), cierra y muestra el
/// error en el botón de la banda.
fn spawn_confirmar_eleccion(
    mut estado: Signal<Aplicacion>,
    mut abrir: Signal<Option<Vec<PaginaInventario>>>,
    mut error_modal: Signal<Option<String>>,
    mut ocupado: Signal<bool>,
    page: RunePageStats,
    champ_nombre: String,
    id: i64,
) {
    ocupado.set(true);
    error_modal.set(None);
    spawn(async move {
        match borrar_pagina_runas(id).await {
            Err(e) => {
                ocupado.set(false);
                error_modal.set(Some(e));
            }
            Ok(()) => {
                abrir.set(None);
                estado.set(Aplicacion::Aplicando);
                match crear_pagina_runas(&page, &champ_nombre).await {
                    Ok(ResultadoCreacion::Creada) => estado.set(Aplicacion::Aplicada),
                    Ok(ResultadoCreacion::SinLugar) => estado.set(Aplicacion::Error(
                        String::from(
                            "página borrada, pero el cliente no dejó crear la nuestra: sigue lleno el inventario",
                        ),
                    )),
                    Err(e) => estado.set(Aplicacion::Error(format!(
                        "Página borrada, pero no se pudo crear la nuestra: {e}"
                    ))),
                }
            }
        }
    });
}

/// Modal "INVENTARIO DE RUNAS LLENO": lista las páginas del cliente para
/// elegir cuál borrar (selección única) y crear la de Klify. Items NO
/// borrables (`isDeletable: false`, presets del juego) deshabilitados con
/// tag RIOT; tag KLIFY para las nuestras.
/// Esc y click fuera cierran (salvo con un borrado en curso).
#[component]
fn ModalInventario(
    pages: Vec<PaginaInventario>,
    page: RunePageStats,
    champ_nombre: String,
    mut estado: Signal<Aplicacion>,
    mut abrir: Signal<Option<Vec<PaginaInventario>>>,
) -> Element {
    let mut seleccion: Signal<Option<i64>> = use_signal(|| None);
    let error_modal: Signal<Option<String>> = use_signal(|| None);
    let ocupado: Signal<bool> = use_signal(|| false);

    let hay_seleccion = seleccion().is_some();
    let ocupado_val = ocupado();
    let error_val = error_modal();

    rsx! {
        div {
            class: "kl-modal-backdrop",
            tabindex: "-1",
            autofocus: true,
            onkeydown: move |e| {
                if e.key() == Key::Escape && !ocupado() {
                    abrir.set(None);
                }
            },
            onclick: move |_| {
                if !ocupado() {
                    abrir.set(None);
                }
            },

            div {
                class: "kl-modal",
                role: "dialog",
                "aria-modal": "true",
                onclick: move |e| e.stop_propagation(),

                h3 { class: "kl-modal-title", "INVENTARIO DE RUNAS LLENO" }
                p { class: "cs-hint", "Elegí qué página borrar para crear la de Klify:" }

                if let Some(err) = error_val {
                    p { class: "cs-unavailable-reason", "{err}" }
                }

                div { class: "kl-modal-list",
                    {pages.iter().map(|p| {
                        let id = p.id;
                        let nombre = p.name.clone();
                        let es_nuestra = p.name.starts_with("Klify ·");
                        let solo_lectura = !p.is_deletable;
                        let sel = seleccion() == Some(id);
                        rsx! {
                            button {
                                key: "{id}",
                                "type": "button",
                                class: if solo_lectura {
                                    "kl-modal-item kl-modal-item--ro"
                                } else if sel {
                                    "kl-modal-item kl-modal-item--sel"
                                } else if es_nuestra {
                                    "kl-modal-item kl-modal-item--ours"
                                } else {
                                    "kl-modal-item"
                                },
                                disabled: solo_lectura || ocupado_val,
                                onclick: move |_| seleccion.set(Some(id)),
                                span { class: "kl-modal-item-name", "{nombre}" }
                                if solo_lectura {
                                    span { class: "label", "RIOT" }
                                } else if es_nuestra {
                                    span { class: "label", "KLIFY" }
                                }
                            }
                        }
                    })}
                }

                div { class: "kl-modal-actions",
                    button {
                        class: "btn-primary",
                        "type": "button",
                        disabled: !hay_seleccion || ocupado_val,
                        onclick: move |_| {
                            let Some(id) = seleccion() else { return };
                            spawn_confirmar_eleccion(
                                estado,
                                abrir,
                                error_modal,
                                ocupado,
                                page.clone(),
                                champ_nombre.clone(),
                                id,
                            );
                        },
                        "BORRAR Y APLICAR"
                    }
                    button {
                        class: "btn-ghost-sm",
                        "type": "button",
                        disabled: ocupado_val,
                        onclick: move |_| abrir.set(None),
                        "CANCELAR"
                    }
                }
            }
        }
    }
}

/// Tile de keystone 64px con anillo rojo: icono del catálogo o `#` sin
/// él. El nombre va como tooltip (`title`): la banda no muestra la
/// descripción.
#[component]
fn KeystoneTile(rune: Option<RuneRow>) -> Element {
    let (icono, nombre) = match rune {
        Some(rune) => (rune.icon, rune.name),
        None => (None, String::from("Keystone")),
    };
    rsx! {
        div { class: "tile tile--keystone", "style": "width: 64px; height: 64px; flex-shrink: 0;", title: "{nombre}",
            {match icono {
                Some(icon) => rsx! {
                    img { src: "{icon}", alt: "{nombre}" }
                },
                None => rsx! {
                    span { class: "cs-qmark", "#" }
                },
            }}
        }
    }
}

/// Fila horizontal de iconos de runa 40px para la banda (sin nombre
/// visible: va como tooltip); `#` como marcador sin catálogo.
#[component]
fn FilaRunas(runes: Vec<RuneRow>) -> Element {
    rsx! {
        div { "style": "display: flex; align-items: center; gap: 6px; flex-wrap: wrap;",
            {runes.iter().map(|rune| rsx! {
                div { key: "{rune.id}", class: "tile", "style": "width: 40px; height: 40px; flex-shrink: 0;", title: "{rune.name}",
                    {match &rune.icon {
                        Some(icon) => rsx! {
                            img { src: "{icon}", alt: "{rune.name}" }
                        },
                        None => rsx! {
                            span { class: "cs-qmark", "#" }
                        },
                    }}
                }
            })}
        }
    }
}

/// Segmento de árbol de la banda: título chico ("PRIMARIO · Precision")
/// + fila horizontal de iconos 40px.
#[component]
fn SegmentoArbol(role: &'static str, tree: TreeInfo, runes: Vec<RuneRow>) -> Element {
    rsx! {
        div { class: "cs-banda-seg",
            span { class: "cs-banda-seg-titulo",
                "{role} "
                span { class: "hud-sep", "·" }
                " {tree.name}"
            }
            FilaRunas { runes }
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

    /// `clase_chip` mapea el texto del estilo a su clase de color: rojo
    /// (daño/pelea/combate/all), verde (sosten/aguante/proteccion/dps),
    /// azul (movilidad/roam/utilidad/control/poke) y gris como fallback,
    /// sin distinguir mayúsculas.
    #[test]
    fn clase_chip_mapea_categorias_con_fallback_gris() {
        // Rojo.
        assert_eq!(clase_chip("DAÑO EXPLOSIVO"), "cs-chip--rojo");
        assert_eq!(clase_chip("Pelea constante"), "cs-chip--rojo");
        assert_eq!(clase_chip("combate"), "cs-chip--rojo");
        assert_eq!(clase_chip("ALL-IN"), "cs-chip--rojo");
        // Verde.
        assert_eq!(clase_chip("SOSTEN"), "cs-chip--verde");
        assert_eq!(clase_chip("AGUANTE Y ESCUDO"), "cs-chip--verde");
        assert_eq!(clase_chip("proteccion"), "cs-chip--verde");
        assert_eq!(clase_chip("DPS SOSTENIDO"), "cs-chip--verde");
        // Azul.
        assert_eq!(clase_chip("MOVILIDAD"), "cs-chip--azul");
        assert_eq!(clase_chip("roam"), "cs-chip--azul");
        assert_eq!(clase_chip("UTILIDAD"), "cs-chip--azul");
        assert_eq!(clase_chip("CONTROL DE MASAS"), "cs-chip--azul");
        assert_eq!(clase_chip("POKE"), "cs-chip--azul");
        // Precedencia: la primera categoría del orden se lleva el chip.
        assert_eq!(clase_chip("DAÑO SOSTENIDO"), "cs-chip--rojo");
        // Fallback.
        assert_eq!(clase_chip("ESTILO RARO"), "cs-chip--gris");
        assert_eq!(clase_chip(""), "cs-chip--gris");
    }

    /// Con nuestra página presente (`nuestra: Some`) el paso es reemplazarla
    /// (borrar + crear), sea cual sea el resto del inventario.
    #[test]
    fn siguiente_paso_reemplaza_la_nuestra() {
        let inv = InventarioRunas {
            pages: vec![LcuPageSummary {
                id: 7,
                name: String::from("Klify · Ahri"),
                is_deletable: true,
            }],
            nuestra: Some(7),
        };
        assert_eq!(siguiente_paso(&inv), Paso::ReemplazarNuestra(7));
    }

    /// Sin página nuestra: crear directo (el inventario lleno lo detecta el
    /// servidor y se abre el modal en caliente, no acá).
    #[test]
    fn siguiente_paso_crea_directo_sin_nuestra() {
        let inv = InventarioRunas {
            pages: vec![LcuPageSummary {
                id: 3,
                name: String::from("Página del usuario"),
                is_deletable: true,
            }],
            nuestra: None,
        };
        assert_eq!(siguiente_paso(&inv), Paso::CrearDirecto);
    }
}
