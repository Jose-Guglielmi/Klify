//! Vista PERFIL (FASE 11b-1).
//!
//! Según el prototipo aprobado (ui-preview.html, iteración 9):
//!
//! - **Fondo splash personalizable**: se lee `ProfileSettings` una vez al
//!   montar (`settings_path()`); default `Kayn_1`. El botón "CAMBIAR FONDO"
//!   abre un modal picker que carga el detalle de Kayn de Data Dragon
//!   (spawn async con cliente HTTP propio) y lista sus skins; click →
//!   `save_settings` + actualización del fondo en pantalla.
//! - **Hero**: icono de invocador, Riot ID grande + `#tag` en
//!   `--primary-bright`, chips (plataforma, nivel, ranked solo, "EN
//!   LÍNEA") y stats HUD (WR ranked / partidas / KDA).
//! - **TOP CHAMPS**: hasta 3 cards (icono + nombre + maestría + puntos +
//!   WR/partidas del historial reciente). Hover borde rojo.
//! - **HISTORIAL RECIENTE**: filas (resultado, champ, KDA, CS, duración,
//!   "hace X") que abren el modal de detalle
//!   ([`crate::ui::match_detail`]); el fetch usa el handle global del
//!   cliente LCU.
//! - **Buscador de invocadores** (arriba del perfil propio): input + botón
//!   que consulta `GET /lol-summoner/v1/summoners?name=` del LCU con el
//!   handle global del cliente (el nombre viaja percent-encoded a mano,
//!   sin crate extra). El resultado es una card compacta (icono de Data
//!   Dragon, Riot ID + `#tag`, nivel); sin resultado → aviso claro.
//!
//! Etiquetas honestas: el agregado del hero es "ÚLTIMAS {n} PARTIDAS"
//! (n = `profile.recent.len()`), no la temporada completa del servidor.
//!
//! Toda la derivación de datos para el render es pura y testeada:
//! [`rank_label`], [`hero_hud`], [`TopChampCard::list`], [`RecentRow::list`],
//! [`champ_label`], [`champ_icon`], [`skin_label`], [`url_busqueda_summoner`]
//! y [`ResultadoBusqueda::new`].

use std::time::{SystemTime, UNIX_EPOCH};

use dioxus::prelude::*;
use irelia::requests::Error as IreliaError;

use crate::data_dragon::models::ChampionCatalog;
use crate::data_dragon::runes::RuneCatalog;
use crate::lcu::models::{AppState, SummonerInfo};
use crate::profile::client_handle::{client_handle, LcuRestClient};
use crate::profile::lcu_fetch::fetch_match_detail;
use crate::profile::models::{MatchSummary, ProfileSummary, RankedInfo};
use crate::profile::settings::{
    load_settings, save_settings, settings_path, ProfileSettings,
};
use crate::ui::format::{
    background_splash_url, format_duration_mmss, format_kda, format_percent, format_queue_name,
    format_ratio, format_thousands, initials, profile_icon_url, DEFAULT_BACKGROUND_SPLASH,
};
use crate::ui::match_detail::{champ_icon, champ_label, MatchDetailModal, MatchModalState};
use crate::ui::shell::{KlifyFooter, KlifyHeader};

/// Campeón del picker de fondo (spec 11b-1: Kayn, 10 skins).
const PICKER_DD_ID: &str = "Kayn";

/// Endpoint de búsqueda por nombre del LCU (el nombre viaja como query
/// string percent-encoded).
const SUMMONER_SEARCH_URI: &str = "/lol-summoner/v1/summoners?name=";

/// Una opción del picker de splash: clave de settings, etiqueta y URL.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkinOption {
    /// Clave de settings (`"Kayn_1"`) que se persiste.
    pub key: String,
    /// Etiqueta visible ("Clásico" para la base).
    pub label: String,
    /// URL del splash en la CDN.
    pub url: String,
}

/// Estado del modal picker de fondo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PickerState {
    /// Cerrado (no renderiza).
    Closed,
    /// Cargando las skins de Kayn.
    Loading,
    /// Skins listas para elegir.
    Ready {
        /// Opciones ordenadas por `num` (el orden del payload).
        skins: Vec<SkinOption>,
    },
    /// Falló la carga de skins o el guardado de la preferencia.
    Failed {
        /// Mensaje corto para la UI.
        message: String,
    },
}

/// Etiqueta visible de una skin: la base de Data Dragon llega como
/// `"default"` (puro, testeado).
#[must_use]
pub fn skin_label(name: &str) -> String {
    if name.trim().eq_ignore_ascii_case("default") {
        String::from("Clásico")
    } else {
        name.trim().to_string()
    }
}

/// Etiqueta del chip de ranked solo (pura, testeada): `"PLATINUM II · 75
/// LP"` con los datos que haya; sin tier → `"SIN RANK"`.
#[must_use]
pub fn rank_label(rank: Option<&RankedInfo>) -> String {
    let Some(rank) = rank else {
        return String::from("SIN RANK");
    };
    let Some(tier) = rank.tier.as_deref().map(str::trim).filter(|t| !t.is_empty())
    else {
        return String::from("SIN RANK");
    };

    let mut label = tier.to_uppercase();
    if let Some(division) = rank.division.as_deref().map(str::trim).filter(|d| !d.is_empty()) {
        label.push(' ');
        label.push_str(&division.to_uppercase());
    }
    if let Some(lp) = rank.league_points {
        label.push_str(&format!(" · {lp} LP"));
    }
    label
}

/// Stats HUD del hero (puro, testeado): WR ranked, partidas y KDA medio.
/// Sin ranked → WR "—"; sin partidas → KDA "—".
#[derive(Clone, Debug, PartialEq)]
pub struct HeroHud {
    /// WR ranked formateado (`"58.1%"` o `"—"`).
    pub wr: String,
    /// Partidas del agregado traído.
    pub games: i32,
    /// KDA medio formateado (`"3.1"`) o `"—"` sin partidas.
    pub kda: String,
}

/// Etiqueta honesta del agregado: "ÚLTIMAS {n} PARTIDAS" (no "temporada").
#[must_use]
pub fn recent_span_label(profile: &ProfileSummary) -> String {
    format!("Últimas {} partidas", profile.recent.len())
}

#[must_use]
pub fn hero_hud(profile: &ProfileSummary) -> HeroHud {
    let wr = profile
        .rank
        .as_ref()
        .and_then(RankedInfo::winrate)
        .map(format_percent)
        .unwrap_or_else(|| String::from("—"));
    let kda = if profile.season.games > 0 {
        format_ratio(profile.season.avg_kda)
    } else {
        String::from("—")
    };
    HeroHud {
        wr,
        games: profile.season.games,
        kda,
    }
}

/// Card de top champ ya resuelta contra el catálogo.
#[derive(Clone, Debug, PartialEq)]
pub struct TopChampCard {
    /// Nombre visible (`"#id"` sin catálogo).
    pub name: String,
    /// URL del icono (`None` → tile placeholder).
    pub icon: Option<String>,
    /// Nivel de maestría.
    pub mastery_level: i64,
    /// Puntos con separador (`"132.460"`).
    pub points: String,
    /// Partidas de ese champ dentro del historial traído.
    pub games: i32,
    /// WR sobre esas partidas (`"58.2%"` o `"—"`).
    pub wr: String,
}

impl TopChampCard {
    /// Resuelve las cards de [`ProfileSummary::top_champs`] contra el
    /// catálogo (puro, testeado).
    #[must_use]
    pub fn list(profile: &ProfileSummary, catalog: Option<&ChampionCatalog>) -> Vec<Self> {
        profile
            .top_champs
            .iter()
            .map(|top| {
                let wr = if top.games > 0 {
                    format_percent(f64::from(top.wins) / f64::from(top.games))
                } else {
                    String::from("—")
                };
                Self {
                    name: champ_label(catalog, top.champion_id),
                    icon: champ_icon(catalog, top.champion_id),
                    mastery_level: top.mastery_level,
                    points: format_thousands(top.mastery_points),
                    games: top.games,
                    wr,
                }
            })
            .collect()
    }
}

/// Fila del historial ya formateada.
#[derive(Clone, Debug, PartialEq)]
pub struct RecentRow {
    /// Id de partida (para el modal de detalle).
    pub game_id: i64,
    /// Nombre visible del campeón.
    pub champ: String,
    /// URL del icono (`None` → tile placeholder).
    pub icon: Option<String>,
    /// Resultado (true = victoria).
    pub win: bool,
    /// `K/D/A`.
    pub kda: String,
    /// Ratio con un decimal.
    pub ratio: String,
    /// CS.
    pub cs: i32,
    /// Duración `mm:ss`.
    pub duration: String,
    /// Cola legible.
    pub queue: String,
    /// "hace X".
    pub ago: String,
}

impl RecentRow {
    /// Resuelve las filas de [`ProfileSummary::recent`] contra el catálogo
    /// y un `now_ms` inyectado (puro, testeado).
    #[must_use]
    pub fn list(
        profile: &ProfileSummary,
        catalog: Option<&ChampionCatalog>,
        now_ms: i64,
    ) -> Vec<Self> {
        profile
            .recent
            .iter()
            .map(|game: &MatchSummary| Self {
                game_id: game.game_id,
                champ: champ_label(catalog, game.champion_id),
                icon: champ_icon(catalog, game.champion_id),
                win: game.win,
                kda: format_kda(game.kills, game.deaths, game.assists),
                ratio: format_ratio(game.kda()),
                cs: game.cs,
                duration: format_duration_mmss(game.duration_secs),
                queue: format_queue_name(game.queue_id),
                ago: format_time_ago_for(game.creation_ms, now_ms),
            })
            .collect()
    }
}

/// "hace X" con re-export local para no exportar `format_time_ago` dos
/// veces (wrapper delgado: el default 0 cubre relojes sin epoch).
#[must_use]
fn format_time_ago_for(creation_ms: i64, now_ms: i64) -> String {
    crate::ui::format::format_time_ago(creation_ms, now_ms)
}

/// Millis actuales de pared (`0` si el reloj no tiene epoch: los tests no
/// dependen de acá).
#[must_use]
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|delta| delta.as_millis() as i64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Buscador de invocadores (panel arriba del perfil propio)
// ---------------------------------------------------------------------------

/// Percent-encoding RFC 3986 para el query string: los unreserved pasan
/// tal cual y el resto va `%XX` (mismo criterio que `riot_api` y
/// `translate`; sin crate extra). Puro, testeado.
#[must_use]
fn encode_query_value(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// URI de búsqueda del LCU para un nombre: trim + percent-encoded
/// (`"PKT MidKing"` → `/lol-summoner/v1/summoners?name=PKT%20MidKing`).
/// Pura, testeada.
#[must_use]
pub fn url_busqueda_summoner(nombre: &str) -> String {
    format!(
        "{SUMMONER_SEARCH_URI}{}",
        encode_query_value(nombre.trim())
    )
}

/// Card compacta del invocador encontrado por el buscador, ya resuelta
/// contra la versión de Data Dragon disponible (pura, testeada).
#[derive(Clone, Debug, PartialEq)]
pub struct ResultadoBusqueda {
    /// Parte del Riot ID antes del `#` (con fallback amable si el payload
    /// no la trae).
    pub nombre: String,
    /// Parte después del `#` (`None` si el payload no la trae).
    pub tag: Option<String>,
    /// Nivel legible (`"NV 173"`); `None` si el payload no lo trae.
    pub nivel: Option<String>,
    /// URL del icono de perfil (`None` sin versión o sin id → la UI
    /// muestra [`Self::iniciales`]).
    pub icono: Option<String>,
    /// Iniciales para el tile de fallback.
    pub iniciales: String,
}

impl ResultadoBusqueda {
    /// Resuelve la card desde el payload del LCU y la versión del
    /// catálogo (pura, testeada).
    #[must_use]
    pub fn new(info: &SummonerInfo, version: Option<&str>) -> Self {
        let nombre = info
            .game_name
            .as_deref()
            .map(str::trim)
            .filter(|nombre| !nombre.is_empty())
            .unwrap_or("Invocador")
            .to_string();
        let tag = info
            .tag_line
            .as_deref()
            .map(str::trim)
            .filter(|tag| !tag.is_empty())
            .map(str::to_string);
        Self {
            iniciales: initials(&nombre),
            nombre,
            tag,
            nivel: info.summoner_level.map(|nivel| format!("NV {nivel}")),
            icono: profile_icon_url(version, info.profile_icon_id),
        }
    }
}

/// GET del invocador por nombre exacto al LCU. `Ok(None)` = no existe
/// (404); `Err` = el cliente falló (el caller arma el mensaje).
async fn fetch_summoner_by_name(
    client: &LcuRestClient,
    nombre: &str,
) -> Result<Option<SummonerInfo>, IreliaError> {
    let uri = url_busqueda_summoner(nombre);
    match client.get::<SummonerInfo>(&uri).await {
        Ok(info) => Ok(Some(info)),
        Err(IreliaError::Request(status)) if status.as_u16() == 404 => Ok(None),
        Err(error) => Err(error),
    }
}

/// Búsqueda on-demand con el handle global del LCU y publicación del
/// resultado (o del aviso). Función libre (no closure): los handlers
/// `onclick`/`onkeydown` son `Fn` y comparten la misma lógica. Sin
/// nombre → aviso sin pegarle al LCU; búsqueda ya en curso → no-op.
fn spawn_summoner_search(
    mut resultado: Signal<Option<SummonerInfo>>,
    mut error: Signal<Option<String>>,
    mut buscando: Signal<bool>,
    nombre: String,
) {
    let nombre = nombre.trim().to_string();
    if nombre.is_empty() {
        resultado.set(None);
        error.set(Some(String::from("Escribí un nombre para buscar.")));
        return;
    }
    if buscando() {
        return;
    }

    resultado.set(None);
    error.set(None);
    buscando.set(true);
    spawn(async move {
        let Some(client) = client_handle() else {
            buscando.set(false);
            error.set(Some(String::from("Cliente no conectado")));
            return;
        };
        match fetch_summoner_by_name(&client, &nombre).await {
            Ok(Some(info)) => resultado.set(Some(info)),
            Ok(None) => {
                error.set(Some(String::from(
                    "No encontré ese invocador en este cliente.",
                )));
            }
            Err(err) => {
                error.set(Some(format!("La búsqueda falló: {err}")));
            }
        }
        buscando.set(false);
    });
}

/// Carga las skins de Kayn para el picker (spawn del modal). Usa la versión
/// del catálogo si está; si no, la más reciente de `versions.json`.
async fn load_picker_skins(catalog_version: Option<String>) -> Result<Vec<SkinOption>, String> {
    let client = crate::data_dragon::http_client().map_err(|error| error.to_string())?;
    let version = match catalog_version
        .as_deref()
        .map(str::trim)
        .filter(|version| !version.is_empty())
    {
        Some(version) => version.to_string(),
        None => crate::data_dragon::latest_version(&client)
            .await
            .map_err(|error| error.to_string())?,
    };

    let detail = crate::data_dragon::champion_detail::load_champion_detail(
        &client,
        &version,
        PICKER_DD_ID,
    )
    .await
    .map_err(|error| error.to_string())?;

    Ok(detail
        .skins
        .iter()
        .map(|skin| SkinOption {
            key: format!("{PICKER_DD_ID}_{}", skin.num),
            label: skin_label(&skin.name),
            url: crate::data_dragon::champion_detail::skin_splash_url(PICKER_DD_ID, skin.num),
        })
        .collect())
}

/// Resuelve las skins del picker y publica el resultado (éxito con lista
/// no vacía, o fallo con mensaje).
fn spawn_load_picker_skins(
    mut picker: Signal<PickerState>,
    catalog_version: Option<String>,
) {
    picker.set(PickerState::Loading);
    spawn(async move {
        match load_picker_skins(catalog_version).await {
            Ok(skins) if skins.is_empty() => {
                picker.set(PickerState::Failed {
                    message: String::from("El detalle de Kayn no trajo skins."),
                });
            }
            Ok(skins) => {
                picker.set(PickerState::Ready { skins });
            }
            Err(message) => {
                picker.set(PickerState::Failed { message });
            }
        }
    });
}

/// Fetch del detalle de partida on-demand con el handle global del LCU y
/// publicación del estado del modal. Función libre (no closure): así los
/// handlers `onclick` son `Fn` y comparten la misma lógica el click en la
/// fila y el "Reintentar" del modal.
fn spawn_match_fetch(mut match_modal: Signal<MatchModalState>, game_id: i64) {
    match_modal.set(MatchModalState::Loading { game_id });
    spawn(async move {
        let Some(client) = client_handle() else {
            match_modal.set(MatchModalState::Failed {
                game_id,
                message: String::from("Cliente no conectado"),
            });
            return;
        };
        match fetch_match_detail(&client, game_id).await {
            Ok(detail) => match_modal.set(MatchModalState::Ready { game_id, detail }),
            Err(error) => match_modal.set(MatchModalState::Failed {
                game_id,
                message: error.to_string(),
            }),
        }
    });
}

/// Vista PERFIL.
#[component]
pub fn ProfileView(
    state: ReadSignal<AppState>,
    profile: ProfileSummary,
    summoner: Option<SummonerInfo>,
    platform: Option<String>,
    catalog: Option<ChampionCatalog>,
    runes: Option<RuneCatalog>,
    source_served: bool,
    /// Click en "VER FICHAS DE CAMPEONES": vuelve al menú de fichas
    /// (la app ahora inicia ahí y el Perfil se visita desde él).
    on_ver_fichas: EventHandler<()>,
) -> Element {
    // Clave de splash del fondo (settings). Empieza en el default: sin
    // flash aunque el archivo tarde en leerse.
    let mut background_key: Signal<String> =
        use_signal(|| DEFAULT_BACKGROUND_SPLASH.to_string());

    // Settings reales una vez al montar (IO chico, en tarea aparte).
    use_effect(move || {
        spawn(async move {
            let settings = load_settings(&settings_path());
            let key = settings
                .background_splash
                .filter(|key| !key.trim().is_empty())
                .unwrap_or_else(|| DEFAULT_BACKGROUND_SPLASH.to_string());
            background_key.set(key);
        });
    });

    // Picker de fondo.
    let mut picker: Signal<PickerState> = use_signal(|| PickerState::Closed);

    // Modal de detalle de partida.
    let mut match_modal: Signal<MatchModalState> = use_signal(|| MatchModalState::Closed);

    // Buscador de invocadores (panel arriba del perfil propio). Solo
    // `busqueda` se muta acá (oninput); el resto lo muta
    // `spawn_summoner_search` con sus propias copias.
    let mut busqueda: Signal<String> = use_signal(String::new);
    let resultado: Signal<Option<SummonerInfo>> = use_signal(|| None);
    let buscando: Signal<bool> = use_signal(|| false);
    let error: Signal<Option<String>> = use_signal(|| None);

    // Snapshots cortos: los guards no cruzan la construcción del rsx.
    let background_url = background_splash_url(Some(&*background_key.read()));
    let version = catalog.as_ref().map(|data| data.version.clone());
    // Copias de la versión para cada closure del picker (livianas; evitan
    // mover el mismo Option dos veces dentro del rsx).
    let picker_version_main = version.clone();
    let picker_version_retry = version.clone();

    let hero = hero_hud(&profile);
    let cards = TopChampCard::list(&profile, catalog.as_ref());
    let rows = RecentRow::list(&profile, catalog.as_ref(), now_ms());

    // Snapshot del resultado del buscador (liviano; evita guards en el
    // rsx y la card se resuelve igual que las de top champs).
    let buscado_card = resultado
        .read()
        .clone()
        .map(|info| ResultadoBusqueda::new(&info, version.as_deref()));

    let game_name = summoner
        .as_ref()
        .and_then(|summoner| summoner.game_name.clone())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| String::from("INVOCADOR"));
    let tag_line = summoner
        .as_ref()
        .and_then(|summoner| summoner.tag_line.clone())
        .unwrap_or_default();
    let level = summoner.as_ref().and_then(|summoner| summoner.summoner_level);
    let icon_id = summoner
        .as_ref()
        .and_then(|summoner| summoner.profile_icon_id);
    let my_puuid = summoner.as_ref().and_then(|summoner| summoner.puuid.clone());
    let icon_url = profile_icon_url(version.as_deref(), icon_id);
    let rank = rank_label(profile.rank.as_ref());
    let with_rank = profile.rank.as_ref().is_some_and(|rank| rank.tier.is_some());

    rsx! {
        div { class: "view",
            // Fondo splash personalizable + franjas
            div { class: "bg-splash", "aria-hidden": "true",
                img { src: "{background_url}", alt: "" }
                div { class: "stripes-hdr" }
            }

            KlifyHeader { state }

            main { class: "main-container",

                // ===== BUSCADOR DE INVOCADORES (arriba del perfil propio) =====
                section { class: "perfil-buscador",
                    div { class: "section-head",
                        span { class: "bar-skew", "style": "width: 32px;" }
                        span { class: "label label-md",
                            span { class: "sl", "//" }
                            " Buscar invocador"
                        }
                        span { class: "label fs-10 t-outline-var ml-auto", "Cliente LCU" }
                    }

                    div { class: "perfil-buscador-fila",
                        input {
                            class: "perfil-buscador-input",
                            "type": "text",
                            value: busqueda(),
                            placeholder: "Nombre de invocador…",
                            autocomplete: "off",
                            oninput: move |event: Event<FormData>| busqueda.set(event.value()),
                            onkeydown: move |event: Event<KeyboardData>| {
                                if event.key() == Key::Enter {
                                    let nombre = busqueda.read().clone();
                                    spawn_summoner_search(resultado, error, buscando, nombre);
                                }
                            },
                        }
                        button { class: "btn-primary", type: "button",
                            disabled: buscando(),
                            onclick: move |_| {
                                let nombre = busqueda.read().clone();
                                spawn_summoner_search(resultado, error, buscando, nombre);
                            },
                            if buscando() { "BUSCANDO…" } else { "BUSCAR" }
                        }
                    }

                    {match error.read().clone() {
                        Some(mensaje) => rsx! {
                            p { class: "label fs-9 t-error", "style": "margin-top: 8px;", "{mensaje}" }
                        },
                        None => rsx! {},
                    }}

                    {match buscado_card {
                        Some(card) => rsx! {
                            div { class: "perfil-resultado",
                                div { class: "tile", "style": "width: 48px; height: 48px; flex-shrink: 0;",
                                    {match &card.icono {
                                        Some(url) => rsx! { img { src: "{url}", alt: "{card.nombre}" } },
                                        None => rsx! {
                                            span { class: "num-display t-outline-var",
                                                "style": "display: flex; align-items: center; justify-content: center; height: 100%; font-size: 18px;",
                                                "{card.iniciales}"
                                            }
                                        },
                                    }}
                                }
                                div { "style": "min-width: 0; flex: 1;",
                                    div { "style": "display: flex; flex-wrap: wrap; align-items: baseline; gap: 2px 10px;",
                                        span {
                                            "style": "font-size: 16px; font-weight: 700; text-transform: uppercase; letter-spacing: 0.04em; color: var(--on-surface);",
                                            "{card.nombre}"
                                        }
                                        if let Some(tag) = &card.tag {
                                            span { class: "hero-tag", "#{tag}" }
                                        }
                                    }
                                }
                                if let Some(nivel) = &card.nivel {
                                    span { class: "chip chip--soft", "{nivel}" }
                                }
                            }
                        },
                        None => rsx! {},
                    }}
                }

                // Separador: buscador → perfil propio.
                hr { class: "divider" }

                // CAMBIAR FONDO · arriba a la derecha
                div { "style": "display: flex; justify-content: flex-end; padding-top: 20px;",
                    button { class: "btn-ghost-sm", type: "button",
                        onclick: move |_| {
                            let version = picker_version_main.clone();
                            spawn_load_picker_skins(picker, version)
                        },
                        "Cambiar fondo"
                    }
                }

                // ===== HERO DE PERFIL =====
                section { class: "hero-card",
                    div { class: "frame cut-tr-bl",
                        div { class: "cut-tr-bl-i bg-low hero-inner",
                            div { class: "tile", "style": "width: 96px; height: 96px;",
                                {match icon_url {
                                    Some(url) => rsx! { img { src: "{url}", alt: "Icono de invocador" } },
                                    None => rsx! {
                                        span { class: "num-display t-outline-var",
                                            "style": "display: flex; align-items: center; justify-content: center; height: 100%; font-size: 32px;",
                                            "?"
                                        }
                                    },
                                }}
                            }

                            div { "style": "min-width: 0; flex: 1;",
                                div { "style": "display: flex; flex-wrap: wrap; align-items: baseline; gap: 4px 12px;",
                                    h2 { class: "hero-name", "{game_name}" }
                                    if !tag_line.is_empty() {
                                        span { class: "hero-tag", "#{tag_line}" }
                                    }
                                }
                                div { class: "hero-chips",
                                    if let Some(platform) = platform.as_deref() {
                                        span { class: "chip chip--soft", "{platform.to_uppercase()}" }
                                    }
                                    if let Some(level) = level {
                                        span { class: "chip chip--soft", "Nivel {level}" }
                                    }
                                    span {
                                        class: if with_rank { "chip-rank" } else { "chip-rank chip-rank--empty" },
                                        if with_rank {
                                            span { class: "label fs-9", "Ranked Solo" }
                                            span { "style": "font-size: 12px; font-weight: 700; text-transform: uppercase; letter-spacing: 0.08em;", "{rank}" }
                                        } else {
                                            span { "style": "font-size: 12px; font-weight: 700; text-transform: uppercase; letter-spacing: 0.08em;", "Sin rank" }
                                        }
                                    }
                                    span { "style": "display: flex; align-items: center; gap: 8px; padding: 0 4px;",
                                        span { class: "dot-pulse", "style": "width: 8px; height: 8px; border-radius: 9999px; background: var(--primary);" }
                                        span { class: "label t-on", "En línea" }
                                    }
                                }
                            }

                            div { class: "stats-hud num-display",
                                span { class: "tnum t-on", "{hero.wr}" }
                                span { class: "label", "{recent_span_label(&profile)}" }
                                span { class: "hud-sep", "style": "font-size: 30px;", "/" }
                                span { class: "tnum t-dim", "{hero.games}" }
                                span { class: "label", "Partidas" }
                                span { class: "hud-sep", "style": "font-size: 30px;", "/" }
                                span { class: "tnum t-on", "{hero.kda}" }
                                span { class: "label", "KDA" }
                            }
                        }
                    }
                    span { class: "bracket", "aria-hidden": "true", i {} i {} i {} i {} }
                }

                // ===== TOP CHAMPS =====
                section { class: "section",
                    div { class: "section-head",
                        span { class: "bar-skew", "style": "width: 32px;" }
                        span { class: "label label-md",
                            span { class: "sl", "//" }
                            " Top Champs"
                        }
                        // Volver al menú de fichas (inicio de la app).
                        button { class: "btn-primary", type: "button",
                            "style": "margin-left: 16px; padding: 8px 18px; font-size: 13px;",
                            onclick: move |_| on_ver_fichas.call(()),
                            "VER FICHAS DE CAMPEONES"
                        }
                        span { class: "label fs-10 t-outline-var ml-auto",
                            "{recent_span_label(&profile)}"
                        }
                    }

                    div { class: "grid-3 mt-4",
                        for card in cards {
                            article { class: "champ-card",
                                div { class: "champ-card-head",
                                    div { class: "tile", "style": "width: 64px; height: 64px;",
                                        {match card.icon {
                                            Some(icon) => rsx! { img { src: "{icon}", alt: "{card.name}" } },
                                            None => rsx! {
                                                span { class: "num-display t-outline-var",
                                                    "style": "display: flex; align-items: center; justify-content: center; height: 100%; font-size: 22px;",
                                                    "?"
                                                }
                                            },
                                        }}
                                    }
                                    div { "style": "min-width: 0;",
                                        div { class: "champ-card-name", "{card.name}" }
                                        div { class: "label fs-10 mt-1", "Maestría {card.mastery_level}" }
                                    }
                                }
                                div { class: "champ-card-foot",
                                    span { class: "tnum t-dim", "style": "font-size: 13px; font-weight: 600;", "{card.points} pts" }
                                    span { class: "label fs-10 t-outline-var", "{card.games} partidas" }
                                }
                                div { class: "stat-line", "style": "margin-top: 8px;",
                                    span { class: "num-display tnum t-on", "style": "font-size: 26px; line-height: 1;", "{card.wr}" }
                                    span { class: "label fs-10", "WR" }
                                }
                            }
                        }
                    }
                }

                // ===== HISTORIAL RECIENTE =====
                section { class: "section",
                    div { class: "section-head",
                        span { class: "bar-skew", "style": "width: 32px;" }
                        span { class: "label label-md",
                            span { class: "sl", "//" }
                            " Historial reciente"
                        }
                        span { class: "label fs-10 t-outline-var ml-auto",
                            "Click para ver detalle"
                        }
                    }

                    div { class: "history-list",
                        for row in rows {
                            button { class: "match-row", type: "button",
                                onclick: move |_| spawn_match_fetch(match_modal, row.game_id),
                                span {
                                    class: if row.win { "chip-result chip-win" } else { "chip-result chip-loss" },
                                    if row.win { "Victoria" } else { "Derrota" }
                                }
                                div { class: "tile", "style": "width: 40px; height: 40px;",
                                    {match row.icon {
                                        Some(icon) => rsx! { img { src: "{icon}", alt: "{row.champ}" } },
                                        None => rsx! {
                                            span { class: "num-display t-outline-var",
                                                "style": "display: flex; align-items: center; justify-content: center; height: 100%; font-size: 14px;",
                                                "?"
                                            }
                                        },
                                    }}
                                }
                                div { class: "match-row-champ",
                                    div { "style": "font-size: 14px; font-weight: 600; text-transform: uppercase; letter-spacing: 0.04em; color: var(--on-surface);", "{row.champ}" }
                                    div { class: "label fs-9 mt-1", "{row.queue}" }
                                }
                                div { class: "match-row-kda num-display tnum",
                                    "{row.kda}"
                                }
                                div { class: "match-row-cell",
                                    div { class: "cell-value", "{row.ratio}" }
                                    div { class: "label fs-9", "KDA" }
                                }
                                div { class: "match-row-cell",
                                    div { class: "cell-value", "{row.cs}" }
                                    div { class: "label fs-9", "CS" }
                                }
                                div { class: "match-row-cell",
                                    div { class: "cell-value", "{row.duration}" }
                                    div { class: "label fs-9", "Duración" }
                                }
                                span { class: "tnum t-dim ml-auto", "style": "font-size: 12px; font-weight: 500;", "{row.ago}" }
                            }
                        }
                    }

                    p { class: "label fs-10 source-note t-outline-var",
                        "FUENTE: CLIENTE LCU · MATCH-HISTORY V1 · {recent_span_label(&profile)}"
                    }
                }
            }

            KlifyFooter { source_served, version }

            // ===== MODAL · PICKER DE FONDO =====
            {match picker.read().clone() {
                PickerState::Closed => rsx! {},
                PickerState::Loading => rsx! {
                    div { class: "modal-overlay",
                        onclick: move |_| picker.set(PickerState::Closed),
                        div { class: "modal-panel",
                            onclick: move |event: Event<MouseData>| event.stop_propagation(),
                            div { "style": "display: flex; align-items: center; gap: 16px; padding: 24px 0;",
                                div { class: "spinner" }
                                span { class: "label label-md t-dim pulse-soft", "Cargando skins de Kayn…" }
                            }
                        }
                    }
                },
                PickerState::Failed { message } => rsx! {
                    div { class: "modal-overlay",
                        onclick: move |_| picker.set(PickerState::Closed),
                        div { class: "modal-panel",
                            onclick: move |event: Event<MouseData>| event.stop_propagation(),
                            div { "style": "display: flex; align-items: center; gap: 12px;",
                                span { class: "bar-skew", "style": "width: 24px;" }
                                span { class: "label label-md",
                                    span { class: "sl", "//" }
                                    " Fondo de perfil"
                                }
                                button { class: "modal-x ml-auto", "aria-label": "Cerrar",
                                    onclick: move |_| picker.set(PickerState::Closed),
                                    "✕"
                                }
                            }
                            p { class: "label label-md t-error mt-4", "No se pudieron cargar las skins" }
                            p { class: "t-dim mt-2", "style": "font-size: 13px;", "{message}" }
                            button { class: "btn-ghost-sm mt-4",
                                onclick: move |_| {
                                    let version = picker_version_retry.clone();
                                    spawn_load_picker_skins(picker, version)
                                },
                                "Reintentar"
                            }
                        }
                    }
                },
                PickerState::Ready { skins } => rsx! {
                    div { class: "modal-overlay",
                        onclick: move |_| picker.set(PickerState::Closed),
                        div { class: "modal-panel",
                            onclick: move |event: Event<MouseData>| event.stop_propagation(),
                            div { "style": "display: flex; align-items: center; gap: 12px;",
                                span { class: "bar-skew", "style": "width: 24px;" }
                                span { class: "label label-md",
                                    span { class: "sl", "//" }
                                    " Fondo de perfil · Kayn"
                                }
                                button { class: "modal-x ml-auto", "aria-label": "Cerrar",
                                    onclick: move |_| picker.set(PickerState::Closed),
                                    "✕"
                                }
                            }
                            div { class: "picker-grid",
                                for skin in skins {
                                    button {
                                        class: if skin.key == *background_key.read() {
                                            "pick-tile pick-tile--active"
                                        } else {
                                            "pick-tile"
                                        },
                                        type: "button",
                                        "title": "{skin.label}",
                                        onclick: move |_| {
                                            let settings = ProfileSettings::with_background_splash(&skin.key);
                                            match save_settings(&settings_path(), &settings) {
                                                Ok(()) => {
                                                    background_key.set(skin.key.clone());
                                                    picker.set(PickerState::Closed);
                                                }
                                                Err(error) => {
                                                    picker.set(PickerState::Failed {
                                                        message: format!("No se pudo guardar la preferencia: {error}"),
                                                    });
                                                }
                                            }
                                        },
                                        img { src: "{skin.url}", alt: "{skin.label}" }
                                        span { class: "pick-tile-label", "{skin.label}" }
                                    }
                                }
                            }
                        }
                    }
                },
            }}

            // ===== MODAL · DETALLE DE PARTIDA =====
            MatchDetailModal {
                state: match_modal,
                catalog,
                runes,
                my_puuid,
                on_close: move |_| match_modal.set(MatchModalState::Closed),
                on_retry: move |game_id: i64| spawn_match_fetch(match_modal, game_id),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_dragon::models::ChampionListResponse;
    use serde_json::json;

    /// Catálogo chico con Ahri (103) y un champ sin nombre (7 → "#7").
    fn catalogo() -> ChampionCatalog {
        let response: ChampionListResponse = serde_json::from_value(json!({
            "data": {
                "Ahri": {
                    "id": "Ahri", "key": "103", "name": "Ahri",
                    "image": { "full": "Ahri.png" }
                }
            }
        }))
        .expect("champion.json válido");
        ChampionCatalog::build(String::from("16.20.1"), response)
    }

    fn perfil_de_ejemplo() -> ProfileSummary {
        ProfileSummary {
            rank: Some(RankedInfo {
                queue: String::from("RANKED_SOLO_5x5"),
                tier: Some(String::from("PLATINUM")),
                division: Some(String::from("II")),
                league_points: Some(75),
                wins: Some(30),
                losses: Some(20),
            }),
            top_champs: vec![
                crate::profile::models::TopChamp {
                    champion_id: 103,
                    mastery_level: 7,
                    mastery_points: 132_460,
                    games: 5,
                    wins: 3,
                },
                crate::profile::models::TopChamp {
                    champion_id: 7,
                    mastery_level: 4,
                    mastery_points: 61_730,
                    games: 0,
                    wins: 0,
                },
            ],
            recent: vec![
                MatchSummary {
                    game_id: 100,
                    champion_id: 103,
                    win: true,
                    kills: 6,
                    deaths: 5,
                    assists: 6,
                    duration_secs: 1_934,
                    queue_id: 400,
                    creation_ms: 1_792_187_581_712 - 2 * 86_400_000,
                    cs: 245,
                },
                MatchSummary {
                    game_id: 101,
                    champion_id: 7,
                    win: false,
                    kills: 4,
                    deaths: 6,
                    assists: 5,
                    duration_secs: 1_635,
                    queue_id: 420,
                    creation_ms: 1_792_187_581_712 - 5 * 3_600_000,
                    cs: 198,
                },
            ],
            season: crate::profile::models::SeasonStats {
                games: 2,
                wins: 1,
                avg_kda: 2.0,
            },
        }
    }

    /// Etiqueta de ranked: completa, sin división, sin LP y sin rank.
    #[test]
    fn etiqueta_de_rank() {
        let perfil = perfil_de_ejemplo();
        assert_eq!(
            rank_label(perfil.rank.as_ref()),
            "PLATINUM II · 75 LP"
        );

        let solo_tier = RankedInfo {
            queue: String::from("RANKED_SOLO_5x5"),
            tier: Some(String::from("gold")),
            division: None,
            league_points: None,
            wins: None,
            losses: None,
        };
        assert_eq!(rank_label(Some(&solo_tier)), "GOLD");

        let sin_rank = RankedInfo {
            tier: None,
            ..RankedInfo::default()
        };
        assert_eq!(rank_label(Some(&sin_rank)), "SIN RANK");
        assert_eq!(rank_label(None), "SIN RANK");
        assert_eq!(rank_label(Some(&RankedInfo::default())), "SIN RANK");
    }

    /// Stats HUD del hero: WR ranked, partidas del agregado y KDA; con
    /// rank `None` → WR "—" y sin partidas → KDA "—".
    #[test]
    fn hud_del_hero() {
        let perfil = perfil_de_ejemplo();
        assert_eq!(
            hero_hud(&perfil),
            HeroHud {
                wr: String::from("60.0%"),
                games: 2,
                kda: String::from("2.0"),
            }
        );

        let mut vacio = perfil.clone();
        vacio.rank = None;
        vacio.season.games = 0;
        vacio.season.avg_kda = 0.0;
        assert_eq!(
            hero_hud(&vacio),
            HeroHud {
                wr: String::from("—"),
                games: 0,
                kda: String::from("—"),
            }
        );
    }

    /// Etiqueta honesta del span de partidas.
    #[test]
    fn etiqueta_de_ultimas_partidas() {
        let perfil = perfil_de_ejemplo();
        assert_eq!(recent_span_label(&perfil), "Últimas 2 partidas");
    }

    /// Cards de top champs: nombre/icono del catálogo, puntos con miles y
    /// WR "—" sin partidas del champ.
    #[test]
    fn cards_de_top_champs() {
        let catalog = catalogo();
        let cards = TopChampCard::list(&perfil_de_ejemplo(), Some(&catalog));

        assert_eq!(cards.len(), 2);
        assert_eq!(cards[0].name, "Ahri");
        assert_eq!(
            cards[0].icon.as_deref(),
            Some("https://ddragon.leagueoflegends.com/cdn/16.20.1/img/champion/Ahri.png")
        );
        assert_eq!(cards[0].points, "132.460");
        assert_eq!(cards[0].wr, "60.0%");
        assert_eq!(cards[0].games, 5);

        // Champ fuera del catálogo → "#7", sin icono, WR "—" sin games.
        assert_eq!(cards[1].name, "#7");
        assert_eq!(cards[1].icon, None);
        assert_eq!(cards[1].wr, "—");

        // Sin catálogo: nombres en "#id" e iconos None.
        let sin_catalogo = TopChampCard::list(&perfil_de_ejemplo(), None);
        assert_eq!(sin_catalogo[0].name, "#103");
        assert_eq!(sin_catalogo[0].icon, None);
    }

    /// Filas del historial: champ, resultado, KDA/CS/duración/cola y
    /// "hace X" con `now_ms` inyectado.
    #[test]
    fn filas_del_historial() {
        let catalog = catalogo();
        let now = 1_792_187_581_712;
        let rows = RecentRow::list(&perfil_de_ejemplo(), Some(&catalog), now);

        assert_eq!(rows.len(), 2);

        assert_eq!(rows[0].game_id, 100);
        assert_eq!(rows[0].champ, "Ahri");
        assert!(rows[0].win);
        assert_eq!(rows[0].kda, "6/5/6");
        assert_eq!(rows[0].ratio, "2.4");
        assert_eq!(rows[0].cs, 245);
        assert_eq!(rows[0].duration, "32:14");
        assert_eq!(rows[0].queue, "Normal");
        assert_eq!(rows[0].ago, "hace 2 días");

        assert_eq!(rows[1].champ, "#7");
        assert!(!rows[1].win);
        assert_eq!(rows[1].queue, "Ranked Solo");
        assert_eq!(rows[1].duration, "27:15");
        assert_eq!(rows[1].ago, "hace 5 horas");
    }

    /// Etiqueta de skins: la base llega como "default".
    #[test]
    fn etiqueta_de_skin() {
        assert_eq!(skin_label("default"), "Clásico");
        assert_eq!(skin_label("Default"), "Clásico");
        assert_eq!(skin_label("  Odyssey Kayn  "), "Odyssey Kayn");
        assert_eq!(skin_label("Soulhunter Kayn"), "Soulhunter Kayn");
    }

    /// `now_ms` no viaja en cero en entornos normales (smoke).
    #[test]
    fn now_ms_es_de_epoch() {
        assert!(now_ms() > 1_600_000_000_000);
    }

    /// URI de búsqueda del LCU: trim + percent-encoding manual (espacios,
    /// `%` y no-ASCII); vacío → query vacío.
    #[test]
    fn url_de_busqueda_summoner() {
        assert_eq!(
            url_busqueda_summoner("PKT MidKing"),
            "/lol-summoner/v1/summoners?name=PKT%20MidKing"
        );
        // `%` se codifica para no romper el query string.
        assert_eq!(
            url_busqueda_summoner("100%real"),
            "/lol-summoner/v1/summoners?name=100%25real"
        );
        // No-ASCII (UTF-8 byte a byte) y trim de los bordes.
        assert_eq!(
            url_busqueda_summoner("  feña  "),
            "/lol-summoner/v1/summoners?name=fe%C3%B1a"
        );
        assert_eq!(
            url_busqueda_summoner("   "),
            "/lol-summoner/v1/summoners?name="
        );
    }

    /// Card del resultado del buscador: nombre + `#tag`, `NV` + nivel,
    /// icono con la versión del catálogo e iniciales cuando no hay
    /// versión; sin tag ni nivel → `None`.
    #[test]
    fn card_del_resultado_del_buscador() {
        let info: SummonerInfo = serde_json::from_value(json!({
            "puuid": "abc-123",
            "gameName": "PKT MidKing",
            "tagLine": "Akali",
            "profileIconId": 588,
            "summonerLevel": 173
        }))
        .expect("summoner parsea");

        let card = ResultadoBusqueda::new(&info, Some("16.20.1"));
        assert_eq!(card.nombre, "PKT MidKing");
        assert_eq!(card.tag.as_deref(), Some("Akali"));
        assert_eq!(card.nivel.as_deref(), Some("NV 173"));
        assert_eq!(
            card.icono.as_deref(),
            Some("https://ddragon.leagueoflegends.com/cdn/16.20.1/img/profileicon/588.png")
        );
        assert_eq!(card.iniciales, "PM");

        // Sin versión disponible: sin icono y con iniciales de fallback.
        let sin_version = ResultadoBusqueda::new(&info, None);
        assert_eq!(sin_version.icono, None);
        assert_eq!(sin_version.iniciales, "PM");

        // Sin tag ni nivel (payload mínimo): campos en `None`.
        let minimo: SummonerInfo =
            serde_json::from_value(json!({ "gameName": "solo" })).expect("mínimo parsea");
        let card_min = ResultadoBusqueda::new(&minimo, Some("16.20.1"));
        assert_eq!(card_min.nombre, "solo");
        assert_eq!(card_min.tag, None);
        assert_eq!(card_min.nivel, None);

        // Encoding del query: nombre con espacio viaja bien formado.
        assert_eq!(
            url_busqueda_summoner(&card_min.nombre),
            "/lol-summoner/v1/summoners?name=solo"
        );
    }
}
