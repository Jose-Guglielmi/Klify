//! Adapters de estadísticas de runas desde fuentes públicas (FASE 9).
//!
//! Dos fuentes, con fallback:
//!
//! - **Lolalytics** (primaria, [`lolalytics`]): dos páginas por consulta
//!   (más jugada y, si difiere, mejor winrate) para
//!   `campeón + lane + parche`, tier `platinum_plus`, cola ranked, región
//!   `all`. Formato verificado contra league-lean (`lib/sources/lolalytics.js`)
//!   y contra el endpoint real en vivo (parche 16.20).
//! - **U.GG** (fallback, [`ugg`]): una página (su recomendada plat+) para
//!   `campeón + posición + parche`. Se consulta solo si Lolalytics falló
//!   (red, 4xx/5xx o payload no parseable). Formato verificado contra
//!   league-lean (`lib/sources/ugg.js`); ver la nota de Cloudflare en la
//!   doc de [`ugg`].
//!
//! [`fetch_recommended_pages`] orquesta el fallback y reporta qué fuente
//! sirvió; si ambas fallan, [`SourceError::Both`] trae las dos causas.
//!
//! Normalización: ambas fuentes producen [`RunePageStats`], la forma única
//! que consumirán scoring y candidatas (fase siguiente). Los estilos
//! (árboles) se resuelven con [`resolve_style_id`]: primero el `tree_id` de
//! nuestra propia [`RuneCatalog`] de Data Dragon mirando los perk ids de la
//! página; si el catálogo no está o ningún id es conocido, cae al índice
//! `page.pri`/`page.sec` de Lolalytics vía [`STYLE_BY_INDEX`] (igual que el
//! `resolveTreeFromPerks` de league-lean) y como último recurso a `8000`.
//!
//! **Cache de memoria** ([`PAGES_CACHE`]): una consulta por
//! `(champion_id, posición, parche)` por corrida de la app, para no golpear
//! los endpoints en cada re-render o re-entrada al champ select. Sin cache
//! en disco por ahora (pendiente: las respuestas son por parche, un JSON
//! por `(champ, posición, parche)` con TTL = fin de parche sería suficiente).
//!
//! [`run_recommender`] (FASE 10) es la integración en la app: una tarea de
//! por vida que, al entrar a champ select con pick y con parche/catálogos
//! disponibles, descarga (con cache) las páginas y publica un
//! [`RecommendationsState`] en su señal — además de **loguearlas**, que es
//! como nació esta integración en FASE 9. La UI
//! ([`crate::ui::recommendations`]) renderiza ese estado debajo del campeón
//! elegido.
//!
//! Decisión de estructura: submódulos `lolalytics`/`ugg` bajo `source/`
//! (convención `mod.rs` del proyecto); `mod.rs` concentra los tipos
//! normalizados, la resolución de árboles, el cache y el orquestador.
//!
//! FASE 11a agrega tres submódulos de datos de análisis (misma base de
//! URL/mega_url y patrón de cache): [`counters`] (matchups `ep=counter`),
//! [`items`] (build `ep=build-itemset` + sugerencias vs comp como cruce con
//! tabla declarativa) y [`skills`] (skill order + summoners, hoy
//! `Unavailable`: ninguna fuente pública accesible los expone; ver su doc).
//!
//! Variantes de estilo: con el catálogo de runas disponible, a las páginas
//! reales se les completa el chip de estilo ([`estilo_de_keystone`],
//! conocimiento del juego, no stats) y se les agregan hasta dos páginas
//! **sintetizadas** ([`expandir_con_variantes`]): el mismo set de la página
//! más jugada cambiando solo el keystone por otro de su propio árbol, sin
//! métricas propias (la UI las distingue con `RunePageStats::sintetizada`).

use std::collections::HashMap;
use std::fmt;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use dioxus::prelude::{ReadableExt, Signal, WritableExt};
use serde::{Deserialize, Serialize};
use tokio::time::sleep;

use crate::data_dragon::models::ChampionCatalog;
use crate::data_dragon::runes::{RuneCatalog, RuneDef};
use crate::lcu::models::{AppState, MatchContext, Position};
use crate::stats::aggregation::patch_of;

pub mod analysis;
pub mod counters;
pub mod items;
pub mod lolalytics;
pub mod player_tips;
pub mod skills;
pub mod translate;
pub mod ugg;

// FASE 11b-2a: re-exports del analizador para que la app y la UI importen
// desde `stats::source` como el resto de los estados.
pub use analysis::{
    analysis_counters, analysis_detail, analysis_items, run_champ_analysis, AnalysisState,
};

/// Ids de árbol de maestrías por índice `page.pri`/`page.sec` de Lolalytics
/// (0 Precisión, 1 Dominación, 2 Brujería, 3 Inspiración, 4 Valor). Solo
/// entra en juego cuando el catálogo de runas no puede resolver el árbol
/// desde los propios perk ids.
pub const STYLE_BY_INDEX: [i64; 5] = [8000, 8100, 8200, 8300, 8400];

/// Nombres de los stat shards (runas de stats), verificado EN VIVO contra
/// `GET https://127.0.0.1:{puerto}/lol-perks/v1/perks` del LCU (2026-10-17,
/// cliente 16.20 de LA2): los ids 5001-5013 son los `kStatMod` del juego.
/// No existen 5004, 5006 ni 5009 (el juego los saltea). Los nombres van en
/// español corto para la UI del champ select; el payload real trae
/// "Health Scaling", "Adaptive Force", etc.
pub const SHARD_NAMES: &[(i64, &str)] = &[
    (5001, "Vida (escala)"),
    (5002, "Armadura"),
    (5003, "Res. mágica"),
    (5005, "Vel. de ataque"),
    (5007, "Celeridad"),
    (5008, "Fuerza adaptativa"),
    (5010, "Vel. de movimiento"),
    (5011, "Vida"),
    (5012, "Resistencias (escala)"),
    (5013, "Tenacidad"),
];

/// Nombre visible de un shard (puro, testeado): tabla [`SHARD_NAMES`]
/// primero, `#id` como fallback honesto para un id desconocido.
#[must_use]
pub fn shard_name(id: i64) -> String {
    SHARD_NAMES
        .iter()
        .find(|&&(shard_id, _)| shard_id == id)
        .map_or_else(|| format!("#{id}"), |&(_, name)| name.to_string())
}

/// Fuente de la que proviene una página de runas.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SourceKind {
    /// Lolalytics (primaria).
    Lolalytics,
    /// U.GG (fallback).
    Ugg,
}

impl fmt::Display for SourceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Lolalytics => write!(f, "Lolalytics"),
            Self::Ugg => write!(f, "U.GG"),
        }
    }
}

/// Errores de los adapters de fuentes. Cada variante conserva la causa como
/// texto (los adapters envuelven errores de `reqwest` y de parseo con la
/// URL involucrada); [`SourceError::Both`] acumula ambas cuando el fallback
/// también falló.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceError {
    /// Falló la consulta/parseo contra Lolalytics.
    Lolalytics(String),
    /// Falló la consulta/parseo contra U.GG.
    Ugg(String),
    /// Fallaron ambas fuentes; trae la causa de cada una.
    Both {
        /// Cause reportada por el adapter de Lolalytics.
        lolalytics: String,
        /// Causa reportada por el adapter de U.GG.
        ugg: String,
    },
}

impl fmt::Display for SourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Lolalytics(cause) => write!(f, "Lolalytics: {cause}"),
            Self::Ugg(cause) => write!(f, "U.GG: {cause}"),
            Self::Both { lolalytics, ugg } => write!(
                f,
                "ambas fuentes fallaron — Lolalytics: {lolalytics}; U.GG: {ugg}"
            ),
        }
    }
}

impl std::error::Error for SourceError {}

/// Una página de runas recomendada, normalizada entre fuentes.
///
/// Deriva `Serialize`/`Deserialize` para las cachés futuras (hoy la cache es
/// solo en memoria y no persiste).
///
/// Nota sobre `wins` en Lolalytics: el endpoint reporta `wr` (porcentaje) y
/// `n` (partidas), no victorias; se derivan como `wins = round(n * wr / 100)`
/// para que ambas fuentes entreguen la misma forma.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunePageStats {
    /// Fuente que reportó la página.
    pub source: SourceKind,
    /// Etiqueta legible (ej. `"Lolalytics · más jugada"`), lista para UI.
    pub label: String,
    /// Partidas de la muestra.
    pub games: u64,
    /// Victorias de la muestra (Lolalytics: derivadas de `n`/`wr`, ver la
    /// nota de la estructura).
    pub wins: u64,
    /// Winrate en porcentaje (0-100).
    pub winrate_pct: f64,
    /// Id del árbol de maestrías principal (resuelto con catálogo/índice).
    pub primary_style_id: i64,
    /// Id del árbol de maestrías secundario.
    pub sub_style_id: i64,
    /// Ids de runas: principales primero, secundarias después, SIN shards.
    pub perk_ids: Vec<i64>,
    /// Runas de stats (shards): `set.mod` de Lolalytics o `pd[8][2]` de U.GG.
    pub stat_shards: Vec<i64>,
    /// Keystone de la página: primer perk del árbol principal que el
    /// catálogo marca como keystone; sin catálogo, la primera runa del
    /// árbol principal (ambas fuentes listan las runas en orden de fila).
    pub keystone_id: Option<i64>,
    /// Chip de estilo según el keystone (conocimiento del juego, no stats):
    /// `Some("DAÑO EXPLOSIVO")`, etc. `None` = keystone fuera de la tabla
    /// de [`estilo_de_keystone`] (o página todavía sin procesar).
    pub estilo: Option<String>,
    /// `true` = página sintetizada (variante de estilo de
    /// [`expandir_con_variantes`]): no trae métricas propias —viaja con
    /// `games`/`wins`/`winrate_pct` en 0 y la UI muestra "—".
    pub sintetizada: bool,
}

impl RunePageStats {
    /// Configuración de la página sin métricas ni etiqueta: la identidad
    /// que se usa para deduplicar pick/win de Lolalytics cuando el endpoint
    /// devuelve el mismo set dos veces.
    pub(crate) fn config_key(&self) -> (i64, i64, Vec<i64>, Vec<i64>) {
        (
            self.primary_style_id,
            self.sub_style_id,
            self.perk_ids.clone(),
            self.stat_shards.clone(),
        )
    }
}

/// Resuelve el id de árbol de una página (principal o secundaria):
///
/// 1. Con catálogo: el primer `perk_id` presente en `runes.by_id` aporta su
///    `tree_id` (lo más robusto: los ids son la verdad, no la posición).
/// 2. Sin hit de catálogo (catálogo ausente, id desconocido): el índice de
///    la página vía [`STYLE_BY_INDEX`].
/// 3. Índice fuera de rango: `8000` (Precisión), valor benigno que nunca
///    debería alcanzarse con payloads válidos.
#[must_use]
pub fn resolve_style_id(perk_ids: &[i64], page_index: usize, runes: Option<&RuneCatalog>) -> i64 {
    if let Some(runes) = runes {
        for perk_id in perk_ids {
            if let Some(rune) = runes.get(*perk_id) {
                return i64::from(rune.tree_id);
            }
        }
    }
    STYLE_BY_INDEX
        .get(page_index)
        .copied()
        .unwrap_or(STYLE_BY_INDEX[0])
}

/// Keystone de una página a partir de las runas de su árbol principal: el
/// primer id que el catálogo marca como keystone; sin catálogo (o sin hit),
/// la primera runa de la lista — ambas fuentes listan en orden de fila y la
/// fila 0 es la del keystone. Lista vacía → `None`.
pub(crate) fn keystone_of(primary_perks: &[i64], runes: Option<&RuneCatalog>) -> Option<i64> {
    if let Some(runes) = runes {
        for perk_id in primary_perks {
            if runes.get(*perk_id).is_some_and(|rune| rune.is_keystone) {
                return Some(*perk_id);
            }
        }
    }
    primary_perks.first().copied()
}

/// Chip de estilo de un keystone (conocimiento del juego, no stats): la
/// categoría corta que etiqueta a cada página junto a su keystone. Ids
/// estables de Data Dragon; un id fuera de la tabla (keystone nuevo sin
/// clasificar, runa menor, 0/ausente) no tiene chip → `None`.
#[must_use]
pub fn estilo_de_keystone(id: i64) -> Option<&'static str> {
    // Clasificación de juego, no de datos: no consulta el catálogo.
    const ESTILOS_POR_KEYSTONE: &[(i64, &str)] = &[
        (8112, "DAÑO EXPLOSIVO"),   // Electrocute
        (8128, "DAÑO ESCALADO"),    // Dark Harvest
        (9923, "COMBATE CORTO"),    // Hail of Blades
        (8124, "ROAM/MOVILIDAD"),   // Predator
        (8005, "COMBATE CORTO"),    // Press the Attack
        (8010, "PELEA PROLONGADA"), // Conqueror
        (8008, "DPS PROLONGADO"),   // Lethal Tempo
        (8021, "SOSTEN DE LINEA"),  // Fleet Footwork
        (8214, "POKE/SOSTEN"),      // Summon Aery
        (8229, "POKE"),             // Arcane Comet
        (8230, "MOVILIDAD"),        // Phase Rush
        (8369, "DAÑO EXPLOSIVO"),   // First Strike
        (8360, "UTILIDAD"),         // Unsealed Spellbook
        (8351, "CONTROL"),          // Glacial Augment
        (8437, "AGUANTE"),          // Grasp of the Undying
        (8435, "AGUANTE/SOSTEN"),   // Aftershock
        (8439, "PROTECCION"),       // Guardian
    ];
    ESTILOS_POR_KEYSTONE
        .iter()
        .find(|&&(keystone, _)| keystone == id)
        .map(|&(_, estilo)| estilo)
}

/// Extiende las páginas reales con **variantes de estilo** (hasta 2): clones
/// de la página más jugada (índice 0) que cambian únicamente el keystone por
/// otro de su propio árbol primario. Pura y determinística: orden de
/// catálogo, sin red ni métricas.
///
/// - El chip de estilo ([`estilo_de_keystone`]) se completa para **todas**
///   las páginas reales a partir de su `keystone_id`.
/// - Keystones candidatos: fila 0 del árbol primario de la base, excluyendo
///   el keystone de la base, los de **todas** las páginas reales presentes
///   (dedupe por id) y los que no tengan estilo en la tabla.
/// - Orden: primero los de estilo distinto al de la base (diversidad),
///   después el resto; dentro de cada grupo, orden de catálogo.
/// - Cada variante: clon de la base con `perk_ids[0]` reemplazado, métricas
///   en cero (`games`/`wins`/`winrate_pct`), `sintetizada` en `true` y
///   etiqueta `Variante · {estilo}`; se agregan al final del vec, con las
///   reales al frente y en su orden original.
#[must_use]
pub fn expandir_con_variantes(
    mut pages: Vec<RunePageStats>,
    catalog: &RuneCatalog,
) -> Vec<RunePageStats> {
    // Chip de estilo de las reales: sale de la tabla pura, no del catálogo.
    for page in &mut pages {
        page.estilo = page
            .keystone_id
            .and_then(estilo_de_keystone)
            .map(String::from);
    }

    // La base es la primera página real (la más jugada de la fuente).
    let Some(base) = pages.first().cloned() else {
        return pages;
    };
    // Sin keystone conocido (o sin perks donde escribirlo) no hay variante
    // posible: el reemplazo es sobre `perk_ids[0]`, la fila del keystone.
    let Some(base_keystone) = base.keystone_id else {
        return pages;
    };
    if base.perk_ids.is_empty() {
        return pages;
    }
    let base_estilo = estilo_de_keystone(base_keystone);

    // Keystones ya presentes en páginas reales: no se repiten como variante.
    let usados: Vec<i64> = pages.iter().filter_map(|page| page.keystone_id).collect();

    // Árbol primario de la base y su fila 0 (los keystones del árbol).
    let Some(arbol) = catalog
        .trees
        .iter()
        .find(|tree| i64::from(tree.id) == base.primary_style_id)
    else {
        return pages;
    };
    let Some(keystones) = arbol.slots.first().map(|slot| &slot.runes) else {
        return pages;
    };

    let mut candidatos: Vec<&RuneDef> = keystones
        .iter()
        .filter(|rune| {
            rune.id != base_keystone
                && !usados.contains(&rune.id)
                && estilo_de_keystone(rune.id).is_some()
        })
        .collect();

    // Diversidad primero: estilo distinto al de la base encabeza; el resto
    // conserva el orden del catálogo (`sort_by_key` es estable).
    candidatos.sort_by_key(|rune| estilo_de_keystone(rune.id) == base_estilo);

    for candidato in candidatos.into_iter().take(2) {
        // Ya filtrado arriba: todo candidato tiene estilo en la tabla.
        let Some(estilo) = estilo_de_keystone(candidato.id) else {
            continue;
        };

        let mut variante = base.clone();
        variante.perk_ids[0] = candidato.id;
        variante.keystone_id = Some(candidato.id);
        variante.estilo = Some(String::from(estilo));
        variante.sintetizada = true;
        variante.games = 0;
        variante.wins = 0;
        variante.winrate_pct = 0.0;
        variante.label = format!("Variante · {estilo}");
        pages.push(variante);
    }

    pages
}

/// Lane de Lolalytics para una posición: `UTILITY` → `support`; sin
/// posición → `None` (el parámetro `lane` se omite en la URL).
pub(crate) fn lane_of(position: Option<Position>) -> Option<&'static str> {
    match position {
        None => None,
        Some(Position::Top) => Some("top"),
        Some(Position::Jungle) => Some("jungle"),
        Some(Position::Middle) => Some("middle"),
        Some(Position::Bottom) => Some("bottom"),
        Some(Position::Utility) => Some("support"),
    }
}

/// Arma la URL del mega-endpoint para un `ep` cualquiera con los common
/// params (`v`, `patch`, `c`, `tier`, `queue`, `region` y `lane` si hay
/// posición). Compartida por los adapters (rune, counter, build-itemset,
/// build-skill); FASE 11a.
pub(crate) fn mega_url(
    ep: &str,
    slug: &str,
    position: Option<Position>,
    patch: &str,
) -> String {
    let mut url = format!(
        "{}?ep={ep}&v=1&patch={patch}&c={slug}&tier={}&queue=ranked&region=all",
        lolalytics::LOLALYTICS_BASE,
        lolalytics::DEFAULT_TIER,
    );
    if let Some(lane) = lane_of(position) {
        url.push_str("&lane=");
        url.push_str(lane);
    }
    url
}

// ---------------------------------------------------------------------------
// Cache en memoria: una consulta por (champ, posición, parche) por corrida.
// ---------------------------------------------------------------------------

/// Clave de cache: campeón + posición + parche (el parche es parte de la
/// clave: los datos cambian con cada parche).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CacheKey {
    /// Id numérico del campeón.
    pub champion_id: i64,
    /// Posición pedida (`None` = sin rol / cualquier posición).
    pub position: Option<Position>,
    /// Parche consultado (`major.minor`).
    pub patch: String,
}

/// Mapa de cache en memoria.
pub type PageCache = HashMap<CacheKey, Vec<RunePageStats>>;

/// Cache global de la app (una entrada por consulta exitosa por corrida).
fn pages_cache() -> &'static Mutex<PageCache> {
    static PAGES_CACHE: OnceLock<Mutex<PageCache>> = OnceLock::new();
    PAGES_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Lookup puro sobre un mapa de cache (testeable sin el global).
#[must_use]
pub fn lookup_pages(cache: &PageCache, key: &CacheKey) -> Option<Vec<RunePageStats>> {
    cache.get(key).cloned()
}

/// Inserción pura sobre un mapa de cache (testeable sin el global).
pub fn insert_pages(cache: &mut PageCache, key: CacheKey, pages: Vec<RunePageStats>) {
    cache.insert(key, pages);
}

/// Lookup sobre la cache global. Cache envenenada (un panic ajeno mientras
/// se escribía) → se trata como miss y se re-consulta: nunca vale la pena
/// tumbar la app por la cache.
fn cached_lookup(key: &CacheKey) -> Option<Vec<RunePageStats>> {
    let cache = pages_cache().lock().ok()?;
    lookup_pages(&cache, key)
}

/// Inserción sobre la cache global (best-effort: un lock envenenado solo
/// pierde esta entrada).
fn cached_store(key: CacheKey, pages: Vec<RunePageStats>) {
    if let Ok(mut cache) = pages_cache().lock() {
        insert_pages(&mut cache, key, pages);
    }
}

// ---------------------------------------------------------------------------
// Orquestador con fallback.
// ---------------------------------------------------------------------------

/// Páginas recomendadas para un campeón: Lolalytics primero, U.GG si aquella
/// falla. Devuelve la fuente que sirvió la respuesta junto con las páginas
/// normalizadas. Si ambas fuentes fallan, [`SourceError::Both`] con las dos
/// causas. Sin cache: usar [`fetch_recommended_pages_cached`] desde la app.
pub async fn fetch_recommended_pages(
    client: &reqwest::Client,
    champion_slug: &str,
    champion_id: i64,
    position: Option<Position>,
    patch: &str,
    runes: Option<&RuneCatalog>,
) -> Result<(SourceKind, Vec<RunePageStats>), SourceError> {
    match lolalytics::fetch_rune_pages(client, champion_slug, position, patch, runes).await {
        Ok(pages) => {
            eprintln!(
                "klipy [source]: sirvió Lolalytics para {champion_slug} ({patch}): {} página(s)",
                pages.len()
            );
            Ok((SourceKind::Lolalytics, pages))
        }
        Err(lolalytics_error) => {
            eprintln!(
                "klipy [source]: aviso: Lolalytics falló para {champion_slug} ({patch}) ({lolalytics_error}); probando U.GG"
            );
            match ugg::fetch_rune_page(client, champion_id, position, patch, runes).await {
                Ok(pages) => {
                    eprintln!(
                        "klipy [source]: sirvió U.GG (fallback) para el campeón {champion_id}: {} página(s)",
                        pages.len()
                    );
                    Ok((SourceKind::Ugg, pages))
                }
                Err(ugg_error) => Err(combine_errors(lolalytics_error, ugg_error)),
            }
        }
    }
}

/// Combina las causas de ambas fuentes en [`SourceError::Both`] (puro,
/// testeado sin red).
fn combine_errors(lolalytics_error: SourceError, ugg_error: SourceError) -> SourceError {
    SourceError::Both {
        lolalytics: lolalytics_error.to_string(),
        ugg: ugg_error.to_string(),
    }
}

/// Igual que [`fetch_recommended_pages`] pero consultando primero la cache
/// en memoria ([`cached_lookup`]/[`cached_store`]): una consulta de red por
/// `(champion_id, posición, parche)` por corrida de la app.
pub async fn fetch_recommended_pages_cached(
    client: &reqwest::Client,
    champion_slug: &str,
    champion_id: i64,
    position: Option<Position>,
    patch: &str,
    runes: Option<&RuneCatalog>,
) -> Result<(SourceKind, Vec<RunePageStats>), SourceError> {
    let key = CacheKey {
        champion_id,
        position,
        patch: patch.to_string(),
    };

    if let Some(pages) = cached_lookup(&key) {
        // La fuente de la primera página es la fuente de toda la consulta.
        let source = pages
            .first()
            .map(|page| page.source)
            .unwrap_or(SourceKind::Lolalytics);
        eprintln!(
            "klipy [source]: cache hit para {champion_slug} ({patch}): {} página(s) de {source}",
            pages.len()
        );
        return Ok((source, pages));
    }

    let (source, pages) =
        fetch_recommended_pages(client, champion_slug, champion_id, position, patch, runes)
            .await?;
    cached_store(key, pages.clone());
    Ok((source, pages))
}

// ---------------------------------------------------------------------------
// Estado de UI de recomendaciones (FASE 10) y tarea que lo publica.
// ---------------------------------------------------------------------------

/// Estado de las recomendaciones de runas para el campeón elegido, publicado
/// por [`run_recommender`] en su señal y consumido por
/// [`crate::ui::recommendations::RuneRecommendations`].
///
/// Sin `Eq` porque [`RunePageStats`] trae `f64` (`winrate_pct`); el
/// recommender compara con [`set_if_changed`] antes de publicar.
#[derive(Clone, Debug, PartialEq)]
pub enum RecommendationsState {
    /// Fuera de champ select o sin champ elegido: la UI no muestra nada.
    Idle,
    /// Consultando las fuentes (o esperando catálogo/parche) para el campeón.
    Loading {
        /// Id del campeón en curso.
        champion_id: i64,
    },
    /// Páginas recomendadas listas para mostrar.
    Ready {
        /// Id del campeón consultado.
        champion_id: i64,
        /// Páginas normalizadas (Lolalytics primero; U.GG si fue el fallback)
        /// y, si había catálogo de runas, hasta 2 variantes de estilo
        /// sintetizadas al final ([`expandir_con_variantes`]).
        pages: Vec<RunePageStats>,
    },
    /// Sin recomendaciones para este campeón (ambas fuentes fallaron).
    Unavailable {
        /// Id del campeón consultado.
        champion_id: i64,
        /// Motivo corto, listo para la UI (la causa detallada queda en el log).
        reason: String,
    },
}

/// Estado de UI para un champ select con pick que todavía no puede consultarse
/// (puro, testeado): sin catálogo de campeones no hay slug, sin parche no hay
/// URL — mientras tanto la UI muestra `Loading` del pick en curso. Fuera de
/// champ select (o sin pick) el estado es `Idle`.
#[must_use]
fn pending_state(state: &AppState) -> RecommendationsState {
    match state {
        AppState::InChampSelect(Some(champion)) => {
            RecommendationsState::Loading { champion_id: champion.id }
        }
        _ => RecommendationsState::Idle,
    }
}

/// Motivo corto para [`RecommendationsState::Unavailable`] a partir del error
/// del orquestador (puro, testeado): en la UI alcanza con saber qué fuente(s)
/// no respondieron; las causas detalladas quedan en el log del recommender.
#[must_use]
fn short_reason(error: &SourceError) -> String {
    match error {
        SourceError::Lolalytics(_) => String::from("Lolalytics no respondió"),
        SourceError::Ugg(_) => String::from("U.GG no respondió"),
        SourceError::Both { .. } => String::from("Lolalytics y U.GG no respondieron"),
    }
}

/// Publica `next` en la señal solo si difiere del valor vigente: el poll del
/// recommender corre cada [`POLL_INTERVAL`] y re-publicar el mismo estado
/// dispararía re-renders inútiles de la UI.
fn set_if_changed(recommendations: &mut Signal<RecommendationsState>, next: RecommendationsState) {
    if *recommendations.read() != next {
        recommendations.set(next);
    }
}

/// Pausa del poll del recomendador: reacciona a la entrada al champ
/// select en ~1.5 s sin martillar las señales.
const POLL_INTERVAL: Duration = Duration::from_millis(1500);
/// Límite para establecer conexión con los endpoints de estadísticas.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Límite total por request (las respuestas de Lolalytics pueden ser de
/// cientos de KB; 20 s es el mismo techo que Data Dragon).
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

/// Objetivo de consulta derivado de las señales, listo para el orquestador.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageTarget {
    /// Id numérico del campeón.
    pub champion_id: i64,
    /// Slug para Lolalytics (del catálogo).
    pub slug: String,
    /// Nombre visible, solo para logs.
    pub champion_name: String,
    /// Posición asignada al jugador local (`None` = sin rol).
    pub position: Option<Position>,
    /// Parche (`major.minor`) para ambas fuentes.
    pub patch: String,
}

/// Deriva el objetivo de consulta del estado actual (puro, testeado):
///
/// - Solo con `AppState::InChampSelect(Some(champ))` hay consulta.
/// - El slug sale del catálogo de campeones; sin catálogo (o sin entrada,
///   o slug vacío) aún no hay nada que consultar: la tarea reintenta suave.
/// - Parche: `client_version` del LCU primero ([`patch_of`]); si no, la
///   versión del catálogo. Sin ninguna, no hay consulta.
/// - Posición: la asignada al jugador local en el [`MatchContext`].
#[must_use]
pub fn preview_target(
    state: &AppState,
    context: &MatchContext,
    catalog: Option<&ChampionCatalog>,
    client_version: Option<&str>,
) -> Option<PageTarget> {
    let champion = match state {
        AppState::InChampSelect(Some(champion)) => champion,
        _ => return None,
    };

    let entry = catalog.and_then(|catalog| catalog.get(champion.id))?;
    if entry.slug.is_empty() {
        return None;
    }

    let patch = client_version
        .map(patch_of)
        .filter(|patch| !patch.is_empty())
        .or_else(|| {
            catalog
                .map(|catalog| patch_of(&catalog.version))
                .filter(|patch| !patch.is_empty())
        })?;

    Some(PageTarget {
        champion_id: champion.id,
        slug: entry.slug.clone(),
        champion_name: champion.name.clone(),
        position: context.my_position,
        patch,
    })
}

/// Cliente HTTP con timeouts acotados para los endpoints de estadísticas
/// (sin ellos un request colgado dejaría la tarea esperando para siempre).
/// Lo comparten el recomendador y el analizador de champ select
/// ([`analysis`], FASE 11b-2a).
pub(crate) fn build_http_client() -> Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .build()
}

/// Tarea de por vida del recomendador (FASE 10): al entrar a champ select con
/// pick (y con catálogo + parche disponibles) descarga las páginas
/// recomendadas —con cache en memoria— y publica el estado en la señal
/// `recommendations` ([`RecommendationsState`]), además de **loguearlas**
/// (la validación en vivo de FASE 9 sigue). Transiciones:
///
/// - Champ select cerrado / sin pick / cliente desconectado →
///   [`RecommendationsState::Idle`]. Resetear el anti-reintento acá hace que
///   re-entrar con el mismo champ vuelva a pasar por `Loading`, aunque la
///   cache evita la consulta de red.
/// - Pick sin catálogo/parche todavía → [`RecommendationsState::Loading`]
///   ([`pending_state`]) hasta que los loaders de Data Dragon terminen.
/// - Target nuevo (champ, posición o parche distinto) → `Loading` + consulta.
/// - Éxito → [`RecommendationsState::Ready`]; fallo →
///   [`RecommendationsState::Unavailable`] con motivo corto
///   ([`short_reason`]). En ambos casos `last_done` evita martillar los
///   endpoints hasta que cambie el target.
/// - FASE 11b-2a: `retry` es un contador que la UI incrementa con el botón
///   REINTENTAR; al cambiar, `last_done` se limpia para que el próximo poll
///   re-consulte el target vigente (los fallos nunca se cachean, así que
///   reintentar es una consulta real).
/// - Con catálogo de runas disponible, las páginas del estado `Ready` llevan
///   el chip de estilo ([`estilo_de_keystone`]) y hasta 2 variantes de estilo
///   sintetizadas ([`expandir_con_variantes`]) al final del vec; sin
///   catálogo, solo las reales.
pub async fn run_recommender(
    mut recommendations: Signal<RecommendationsState>,
    app_state: Signal<AppState>,
    match_context: Signal<MatchContext>,
    catalog: Signal<Option<ChampionCatalog>>,
    runes: Signal<Option<RuneCatalog>>,
    client_version: Signal<Option<String>>,
    retry: Signal<u64>,
) {
    // Último objetivo ya procesado (hit o fallo): evita re-loguear/re-
    // consultar el mismo (champ, posición, parche) en cada poll.
    let mut last_done: Option<CacheKey> = None;
    // Último valor del contador de reintento visto (FASE 11b-2a).
    let mut last_retry: u64 = *retry.read();

    loop {
        // Snapshots cortos: ningún guard de señal cruza un `await`.
        let state: AppState = (*app_state.read()).clone();
        let context: MatchContext = (*match_context.read()).clone();
        let catalog_snapshot: Option<ChampionCatalog> = (*catalog.read()).clone();
        let runes_snapshot: Option<RuneCatalog> = (*runes.read()).clone();
        let version_snapshot: Option<String> = (*client_version.read()).clone();

        // REINTENTAR de la UI: el contador cambió → liberar el anti-reintento
        // para re-consultar el target vigente en este mismo ciclo.
        let retry_now = *retry.read();
        if retry_now != last_retry {
            last_retry = retry_now;
            last_done = None;
        }

        // Fuera de champ select (o sin pick): Idle. `last_done = None` libera
        // el anti-reintento para la próxima entrada al champ select.
        if !matches!(state, AppState::InChampSelect(Some(_))) {
            last_done = None;
            set_if_changed(&mut recommendations, RecommendationsState::Idle);
            sleep(POLL_INTERVAL).await;
            continue;
        }

        match preview_target(
            &state,
            &context,
            catalog_snapshot.as_ref(),
            version_snapshot.as_deref(),
        ) {
            None => {
                // Pick sin catálogo/parche todavía: Loading hasta que estén
                // (los loaders de Data Dragon reintentan por su cuenta).
                set_if_changed(&mut recommendations, pending_state(&state));
                sleep(POLL_INTERVAL).await;
            }
            Some(target) => {
                let key = CacheKey {
                    champion_id: target.champion_id,
                    position: target.position,
                    patch: target.patch.clone(),
                };
                if last_done.as_ref() == Some(&key) {
                    // Mismo target ya resuelto (Ready o Unavailable): la UI
                    // queda como está.
                    sleep(POLL_INTERVAL).await;
                    continue;
                }

                set_if_changed(
                    &mut recommendations,
                    RecommendationsState::Loading {
                        champion_id: target.champion_id,
                    },
                );

                let client = match build_http_client() {
                    Ok(client) => client,
                    Err(error) => {
                        eprintln!(
                            "klipy [source]: no se pudo crear el cliente HTTP: {error}"
                        );
                        set_if_changed(
                            &mut recommendations,
                            RecommendationsState::Unavailable {
                                champion_id: target.champion_id,
                                reason: String::from("no se pudo iniciar la consulta"),
                            },
                        );
                        last_done = Some(key);
                        sleep(POLL_INTERVAL).await;
                        continue;
                    }
                };

                match fetch_recommended_pages_cached(
                    &client,
                    &target.slug,
                    target.champion_id,
                    target.position,
                    &target.patch,
                    runes_snapshot.as_ref(),
                )
                .await
                {
                    Ok((source, pages)) => {
                        // Con catálogo de runas disponible: chip de estilo en
                        // las reales y hasta 2 variantes de estilo al final.
                        // Sin catálogo, las reales quedan solas (la feature es
                        // optativa según disponibilidad). La cache guarda solo
                        // las reales: la expansión es pura y determinística,
                        // así un catálogo que llega tarde también produce
                        // variantes en el próximo cache hit.
                        let pages = match runes_snapshot.as_ref() {
                            Some(runes) => expandir_con_variantes(pages, runes),
                            None => pages,
                        };
                        log_recommended_pages(
                            &target,
                            source,
                            &pages,
                            runes_snapshot.as_ref(),
                        );
                        set_if_changed(
                            &mut recommendations,
                            RecommendationsState::Ready {
                                champion_id: target.champion_id,
                                pages,
                            },
                        );
                    }
                    Err(error) => {
                        eprintln!(
                            "klipy [source]: aviso: sin páginas de runas para {} ({error}); se reintenta al cambiar de champ/posición/parche",
                            target.champion_name
                        );
                        set_if_changed(
                            &mut recommendations,
                            RecommendationsState::Unavailable {
                                champion_id: target.champion_id,
                                reason: short_reason(&error),
                            },
                        );
                    }
                }
                last_done = Some(key);
            }
        }
    }
}

/// Log multi-línea del resultado de una consulta (la validación visible de
/// esta fase): una línea de encabezado y una por página.
fn log_recommended_pages(
    target: &PageTarget,
    source: SourceKind,
    pages: &[RunePageStats],
    runes: Option<&RuneCatalog>,
) {
    let lane = lane_of(target.position).unwrap_or("sin rol");
    eprintln!(
        "klipy [source]: páginas recomendadas para {} ({lane}, parche {}): fuente {}, {} página(s)",
        target.champion_name,
        target.patch,
        source,
        pages.len()
    );

    for page in pages {
        let keystone = match page.keystone_id {
            None => String::from("—"),
            Some(id) => match runes.and_then(|runes| runes.get(id)) {
                Some(rune) => format!("{id} ({})", rune.name),
                None => format!("{id} (sin catálogo)"),
            },
        };
        let estilo = page.estilo.as_deref().unwrap_or("—");
        eprintln!(
            "klipy [source]:   · {} [fuente {}] wr {:.2}% en {} partidas · keystone {} · estilo {} · estilos pri {} / sec {} · {} perks + {} shards{}",
            page.label,
            page.source,
            page.winrate_pct,
            page.games,
            keystone,
            estilo,
            page.primary_style_id,
            page.sub_style_id,
            page.perk_ids.len(),
            page.stat_shards.len(),
            if page.sintetizada { " · variante sintetizada" } else { "" }
        );
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::data_dragon::models::ChampionEntry;
    use crate::data_dragon::runes::{RuneDef, RuneSlot, RuneTree};

    /// Catálogo de runas mínimo: Dominación (8100) con Electrocute (8112,
    /// keystone) y Precisión (8000) con Press the Attack (8005, keystone).
    fn catalogo_de_runas() -> RuneCatalog {
        let mut by_id = HashMap::new();
        for (id, key, name, tree, slot, keystone) in [
            (8112_i64, "Electrocute", "Electrocute", 8100_i32, 0_usize, true),
            (8005, "PressTheAttack", "Press the Attack", 8000, 0, true),
            (8139, "TasteOfBlood", "Taste of Blood", 8100, 1, false),
        ] {
            by_id.insert(
                id,
                RuneDef {
                    id,
                    key: String::from(key),
                    name: String::from(name),
                    icon_path: String::new(),
                    tree_id: tree,
                    slot_index: slot,
                    is_keystone: keystone,
                    short_desc: String::new(),
                },
            );
        }
        RuneCatalog {
            version: String::from("16.20.1"),
            trees: vec![RuneTree::default()],
            by_id,
        }
    }

    /// `resolve_style_id` prioriza el árbol real del primer perk conocido
    /// aunque el índice de la página diga otra cosa.
    #[test]
    fn resolve_style_id_con_catalogo_gana_sobre_el_indice() {
        let runes = catalogo_de_runas();

        // Electrocute es Dominación (8100) aunque el índice diga 3 (Inspiración).
        assert_eq!(
            resolve_style_id(&[8112, 8005], 3, Some(&runes)),
            8100
        );
        // El primer id desconocido se salta; el segundo resuelve el árbol.
        assert_eq!(resolve_style_id(&[999999, 8005], 1, Some(&runes)), 8000);
        // La periferia (runas menores) también resuelve por catálogo.
        assert_eq!(resolve_style_id(&[8139], 0, Some(&runes)), 8100);
    }

    /// Sin catálogo (o con ids desconocidos) cae al índice de la página.
    #[test]
    fn resolve_style_id_sin_catalogo_o_con_id_desconocido_cae_al_indice() {
        let runes = catalogo_de_runas();

        assert_eq!(resolve_style_id(&[8112], 4, None), 8400);
        assert_eq!(resolve_style_id(&[999999], 2, Some(&runes)), 8200);
        assert_eq!(resolve_style_id(&[], 0, None), 8000);
    }

    /// Índice fuera de rango → 8000 (el último recurso benigno).
    #[test]
    fn resolve_style_id_con_indice_fuera_de_rango_devuelve_8000() {
        assert_eq!(resolve_style_id(&[], 9, None), 8000);
        assert_eq!(resolve_style_id(&[], 5, None), 8000);
    }

    /// Keystone: primer perk del primario que el catálogo marca como
    /// keystone; sin catálogo, la primera runa; vacío → `None`.
    #[test]
    fn keystone_of_resuelve_con_y_sin_catalogo() {
        let runes = catalogo_de_runas();
        let pri = [8139, 8112];

        // Con catálogo: 8139 es runa menor, 8112 es el keystone.
        assert_eq!(keystone_of(&pri, Some(&runes)), Some(8112));
        // Sin catálogo: la primera runa del primario (orden de fila).
        assert_eq!(keystone_of(&pri, None), Some(8139));
        assert_eq!(keystone_of(&[], Some(&runes)), None);
    }

    /// Cache pura: miss → put → hit, y la inserción pisa la entrada previa.
    #[test]
    fn cache_pura_guarda_y_pisa_por_clave() {
        let mut cache = PageCache::new();
        let key = CacheKey {
            champion_id: 498,
            position: Some(Position::Bottom),
            patch: String::from("16.20"),
        };
        assert!(lookup_pages(&cache, &key).is_none());

        let page = RunePageStats {
            source: SourceKind::Ugg,
            label: String::from("U.GG · plat+ recomendada"),
            games: 100,
            wins: 52,
            winrate_pct: 52.0,
            primary_style_id: 8100,
            sub_style_id: 8200,
            perk_ids: vec![8112],
            stat_shards: vec![5008],
            keystone_id: Some(8112),
            estilo: None,
            sintetizada: false,
        };
        insert_pages(&mut cache, key.clone(), vec![page.clone()]);
        assert_eq!(lookup_pages(&cache, &key), Some(vec![page.clone()]));

        let otra = RunePageStats {
            label: String::from("otra"),
            ..page
        };
        insert_pages(&mut cache, key.clone(), vec![otra.clone()]);
        assert_eq!(lookup_pages(&cache, &key), Some(vec![otra]));
    }

    /// La clave de cache distingue campeón, posición y parche.
    #[test]
    fn cache_key_distingue_campeon_posicion_y_parche() {
        let base = CacheKey {
            champion_id: 498,
            position: Some(Position::Bottom),
            patch: String::from("16.20"),
        };

        let otra_pos = CacheKey {
            position: Some(Position::Middle),
            ..base.clone()
        };
        let sin_pos = CacheKey {
            position: None,
            ..base.clone()
        };
        let otro_parche = CacheKey {
            patch: String::from("16.19"),
            ..base.clone()
        };
        let otro_champ = CacheKey {
            champion_id: 103,
            ..base.clone()
        };

        assert_ne!(base, otra_pos);
        assert_ne!(base, sin_pos);
        assert_ne!(base, otro_parche);
        assert_ne!(base, otro_champ);
    }

    /// `lane_of`: mapeo Lolalytics (UTILITY → support) y `None` → omitir.
    #[test]
    fn lane_of_mapea_posiciones_a_lanes_de_lolalytics() {
        assert_eq!(lane_of(Some(Position::Top)), Some("top"));
        assert_eq!(lane_of(Some(Position::Jungle)), Some("jungle"));
        assert_eq!(lane_of(Some(Position::Middle)), Some("middle"));
        assert_eq!(lane_of(Some(Position::Bottom)), Some("bottom"));
        assert_eq!(lane_of(Some(Position::Utility)), Some("support"));
        assert_eq!(lane_of(None), None);
    }

    /// El fallo de ambas fuentes se combina en `Both` con las dos causas.
    #[test]
    fn combine_errors_acumula_ambas_causas() {
        let error = combine_errors(
            SourceError::Lolalytics(String::from("timeout")),
            SourceError::Ugg(String::from("403")),
        );

        assert_eq!(
            error,
            SourceError::Both {
                lolalytics: String::from("Lolalytics: timeout"),
                ugg: String::from("U.GG: 403"),
            }
        );
        assert_eq!(
            error.to_string(),
            "ambas fuentes fallaron — Lolalytics: Lolalytics: timeout; U.GG: U.GG: 403"
        );
    }

    /// `RunePageStats` hace roundtrip serde (formato camelCase estable para
    /// las cachés futuras) y `SourceKind` serializa como variantes simples.
    #[test]
    fn rune_page_stats_hace_roundtrip_serde() {
        let page = RunePageStats {
            source: SourceKind::Lolalytics,
            label: String::from("Lolalytics · más jugada"),
            games: 559_936,
            wins: 285_903,
            winrate_pct: 51.06,
            primary_style_id: 8000,
            sub_style_id: 8300,
            perk_ids: vec![8008, 8009, 9103, 8014, 8345, 8304],
            stat_shards: vec![5005, 5008, 5001],
            keystone_id: Some(8008),
            estilo: Some(String::from("DPS PROLONGADO")),
            sintetizada: false,
        };

        let json_text = serde_json::to_string(&page).expect("serializa");
        assert!(json_text.contains("\"source\":\"Lolalytics\""));
        assert!(json_text.contains("\"primaryStyleId\":8000"));
        assert!(json_text.contains("\"winratePct\":51.06"));
        assert!(json_text.contains("\"estilo\":\"DPS PROLONGADO\""));
        assert!(json_text.contains("\"sintetizada\":false"));

        let back: RunePageStats = serde_json::from_str(&json_text).expect("deserializa");
        assert_eq!(back, page);
    }

    /// Catálogo de campeones mínimo para `preview_target`.
    fn catalogo_de_campeones() -> ChampionCatalog {
        let mut by_id = HashMap::new();
        by_id.insert(
            498,
            ChampionEntry {
                name: String::from("Kai'Sa"),
                image_url: String::from("https://ddragon.leagueoflegends.com/cdn/x.png"),
                slug: String::from("kaisa"),
                ddragon_id: String::from("Kaisa"),
            },
        );
        ChampionCatalog {
            version: String::from("16.20.1"),
            by_id,
        }
    }

    fn champ_kaisa() -> crate::lcu::models::ChampionInfo {
        crate::lcu::models::ChampionInfo {
            id: 498,
            name: String::from("Kai'Sa"),
            image_url: String::from("https://ddragon.leagueoflegends.com/cdn/x.png"),
        }
    }

    /// Caso nominal: champ elegido + catálogo + versión de cliente → target
    /// con slug, parche `major.minor` y posición del contexto.
    #[test]
    fn preview_target_arma_el_objetivo_con_pick_y_catalogo() {
        let context = MatchContext {
            my_position: Some(Position::Bottom),
            ..MatchContext::default()
        };

        let target = preview_target(
            &AppState::InChampSelect(Some(champ_kaisa())),
            &context,
            Some(&catalogo_de_campeones()),
            Some("16.20.8248524"),
        )
        .expect("target");

        assert_eq!(target.champion_id, 498);
        assert_eq!(target.slug, "kaisa");
        assert_eq!(target.patch, "16.20");
        assert_eq!(target.position, Some(Position::Bottom));
        assert_eq!(target.champion_name, "Kai'Sa");
    }

    /// Fuera de champ select, sin catálogo, con slug vacío o sin parche no
    /// hay consulta (la tarea espera y reintenta suave).
    #[test]
    fn preview_target_exige_champ_catalogo_y_parche() {
        let context = MatchContext::default();
        let catalog = catalogo_de_campeones();

        // Fuera de champ select.
        assert_eq!(
            preview_target(
                &AppState::Disconnected,
                &context,
                Some(&catalog),
                Some("16.20.1")
            ),
            None
        );
        assert_eq!(
            preview_target(&AppState::InChampSelect(None), &context, Some(&catalog), Some("16.20.1")),
            None
        );
        // Sin catálogo no hay slug.
        assert_eq!(
            preview_target(
                &AppState::InChampSelect(Some(champ_kaisa())),
                &context,
                None,
                Some("16.20.1")
            ),
            None
        );
        // Champ fuera del catálogo.
        let mut desconocido = champ_kaisa();
        desconocido.id = 999;
        assert_eq!(
            preview_target(
                &AppState::InChampSelect(Some(desconocido)),
                &context,
                Some(&catalog),
                Some("16.20.1")
            ),
            None
        );
        // Sin parche en ninguna fuente.
        let mut sin_slug = catalogo_de_campeones();
        sin_slug.by_id.get_mut(&498).expect("entrada").slug.clear();
        assert_eq!(
            preview_target(
                &AppState::InChampSelect(Some(champ_kaisa())),
                &context,
                Some(&sin_slug),
                Some("16.20.1")
            ),
            None
        );
        assert_eq!(
            preview_target(
                &AppState::InChampSelect(Some(champ_kaisa())),
                &context,
                Some(&sin_slug),
                None
            ),
            None
        );
    }

    /// El parche del cliente precede a la versión del catálogo; sin
    /// cliente, se deriva del catálogo (`16.20.1` → `16.20`).
    #[test]
    fn preview_target_prefiere_el_parche_del_cliente() {
        let context = MatchContext::default();
        let catalog = catalogo_de_campeones();

        let con_cliente = preview_target(
            &AppState::InChampSelect(Some(champ_kaisa())),
            &context,
            Some(&catalog),
            Some("16.19.1234"),
        )
        .expect("target");
        assert_eq!(con_cliente.patch, "16.19");

        let sin_cliente = preview_target(
            &AppState::InChampSelect(Some(champ_kaisa())),
            &context,
            Some(&catalog),
            None,
        )
        .expect("target");
        assert_eq!(sin_cliente.patch, "16.20");
    }

    /// `patch_of` recorta tanto la versión completa del cliente LCU (ya
    /// limpia, ej. `"16.20.8248524"`) como la versión del catálogo
    /// (`"16.20.1"`) al `major.minor` que piden ambas fuentes.
    #[test]
    fn patch_of_recorta_client_version_a_major_minor() {
        assert_eq!(patch_of("16.20.8248524"), "16.20");
        assert_eq!(patch_of("16.20.1"), "16.20");
    }

    /// FASE 10: `pending_state` — con pick → `Loading` de ese champ; fuera de
    /// champ select (o sin pick) → `Idle` (transición de cierre de sesión).
    #[test]
    fn pending_state_es_loading_con_pick_e_idle_sin_el() {
        assert_eq!(
            pending_state(&AppState::Disconnected),
            RecommendationsState::Idle
        );
        assert_eq!(
            pending_state(&AppState::WaitingForChampSelect),
            RecommendationsState::Idle
        );
        assert_eq!(
            pending_state(&AppState::InChampSelect(None)),
            RecommendationsState::Idle
        );
        assert_eq!(
            pending_state(&AppState::InChampSelect(Some(champ_kaisa()))),
            RecommendationsState::Loading { champion_id: 498 }
        );
    }

    /// FASE 10: `short_reason` — el motivo de la UI nombra la(s) fuente(s)
    /// caída(s) sin arrastrar las causas detalladas (esas quedan en el log).
    #[test]
    fn short_reason_resume_la_fuente_caida() {
        assert_eq!(
            short_reason(&SourceError::Lolalytics(String::from("timeout de conexión"))),
            "Lolalytics no respondió"
        );
        assert_eq!(
            short_reason(&SourceError::Ugg(String::from("403 de Cloudflare"))),
            "U.GG no respondió"
        );
        assert_eq!(
            short_reason(&combine_errors(
                SourceError::Lolalytics(String::from("timeout")),
                SourceError::Ugg(String::from("403")),
            )),
            "Lolalytics y U.GG no respondieron"
        );
    }

    /// FASE 11b-2a: `shard_name` resuelve la tabla verificada en vivo contra
    /// `/lol-perks/v1/perks` y cae a `#id` para ids desconocidos (5004/5006/
    /// 5009 no existen en el juego).
    #[test]
    fn shard_name_resuelve_la_tabla_y_cae_a_id() {
        assert_eq!(shard_name(5001), "Vida (escala)");
        assert_eq!(shard_name(5005), "Vel. de ataque");
        assert_eq!(shard_name(5007), "Celeridad");
        assert_eq!(shard_name(5008), "Fuerza adaptativa");
        assert_eq!(shard_name(5010), "Vel. de movimiento");
        assert_eq!(shard_name(5011), "Vida");
        assert_eq!(shard_name(5012), "Resistencias (escala)");
        assert_eq!(shard_name(5013), "Tenacidad");
        assert_eq!(shard_name(5004), "#5004");
        assert_eq!(shard_name(5006), "#5006");
        assert_eq!(shard_name(5009), "#5009");
        assert_eq!(shard_name(9999), "#9999");
    }

    // -------------------------------------------------------------------------
    // Variantes de estilo: `estilo_de_keystone` + `expandir_con_variantes`.
    // -------------------------------------------------------------------------

    /// Runa de fixture con la forma que el catálogo le da a cada entrada.
    fn runa(id: i64, key: &str, tree_id: i32, slot: usize, keystone: bool) -> RuneDef {
        RuneDef {
            id,
            key: String::from(key),
            name: String::from(key),
            icon_path: String::new(),
            tree_id,
            slot_index: slot,
            is_keystone: keystone,
            short_desc: String::new(),
        }
    }

    /// Catálogo con árboles completos (fila 0 = keystones) para las
    /// variantes: Dominación con sus tres keystones reales (8112, 8128,
    /// 9923) y Precisión con PTA (8005).
    fn catalogo_con_arboles() -> RuneCatalog {
        let dominacion = RuneTree {
            id: 8100,
            key: String::from("Domination"),
            name: String::from("Domination"),
            icon_path: String::new(),
            slots: vec![
                RuneSlot {
                    runes: vec![
                        runa(8112, "Electrocute", 8100, 0, true),
                        runa(8128, "DarkHarvest", 8100, 0, true),
                        runa(9923, "HailOfBlades", 8100, 0, true),
                    ],
                },
                RuneSlot {
                    runes: vec![runa(8139, "TasteOfBlood", 8100, 1, false)],
                },
            ],
        };
        let precision = RuneTree {
            id: 8000,
            key: String::from("Precision"),
            name: String::from("Precision"),
            icon_path: String::new(),
            slots: vec![RuneSlot {
                runes: vec![runa(8005, "PressTheAttack", 8000, 0, true)],
            }],
        };

        let mut by_id = HashMap::new();
        for tree in [&dominacion, &precision] {
            for slot in &tree.slots {
                for rune in &slot.runes {
                    by_id.insert(rune.id, rune.clone());
                }
            }
        }

        RuneCatalog {
            version: String::from("16.20.1"),
            trees: vec![dominacion, precision],
            by_id,
        }
    }

    /// Página real de ejemplo: Lolalytics, 6 perks (keystone primero), árbol
    /// primario de Dominación.
    fn pagina_real(keystone: i64) -> RunePageStats {
        RunePageStats {
            source: SourceKind::Lolalytics,
            label: String::from("Lolalytics · más jugada"),
            games: 100,
            wins: 52,
            winrate_pct: 52.0,
            primary_style_id: 8100,
            sub_style_id: 8000,
            perk_ids: vec![keystone, 8126, 8139, 8135, 8106, 8138],
            stat_shards: vec![5008, 5008, 5001],
            keystone_id: Some(keystone),
            estilo: None,
            sintetizada: false,
        }
    }

    /// `estilo_de_keystone`: la tabla pura resuelve los ids conocidos y un
    /// id fuera de ella no tiene chip.
    #[test]
    fn estilo_de_keystone_resuelve_la_tabla_y_desconocido_es_none() {
        assert_eq!(estilo_de_keystone(8112), Some("DAÑO EXPLOSIVO"));
        assert_eq!(estilo_de_keystone(8010), Some("PELEA PROLONGADA"));
        assert_eq!(estilo_de_keystone(8437), Some("AGUANTE"));
        assert_eq!(estilo_de_keystone(9999), None);
        assert_eq!(estilo_de_keystone(0), None);
    }

    /// `expandir_con_variantes`: la base 8112 (Dominación) genera 2 variantes
    /// con keystones del mismo árbol, métricas en cero, el resto del set
    /// intacto y estilo distinto al de la base.
    #[test]
    fn expandir_con_variantes_genera_dos_variantes_de_estilo() {
        let catalog = catalogo_con_arboles();
        let base = pagina_real(8112);
        let pages = expandir_con_variantes(vec![base.clone()], &catalog);

        // 1 real + 2 variantes.
        assert_eq!(pages.len(), 3);

        // La real sigue al frente, intacta salvo el chip de estilo.
        let real = &pages[0];
        assert!(!real.sintetizada);
        assert_eq!(real.keystone_id, Some(8112));
        assert_eq!(real.games, 100);
        assert_eq!(real.estilo.as_deref(), Some("DAÑO EXPLOSIVO"));

        // Variantes en orden de catálogo (ambas con estilo distinto al de la
        // base, así que el orden del árbol manda): 8128 y 9923.
        let esperadas = [(8128, "DAÑO ESCALADO"), (9923, "COMBATE CORTO")];
        for (offset, esperada) in esperadas.iter().enumerate() {
            let (keystone, estilo) = *esperada;
            let variante = &pages[1 + offset];

            assert!(variante.sintetizada);
            assert_eq!(variante.games, 0);
            assert_eq!(variante.wins, 0);
            assert_eq!(variante.winrate_pct, 0.0);
            assert_eq!(variante.label, format!("Variante · {estilo}"));
            assert_eq!(variante.estilo.as_deref(), Some(estilo));
            assert_ne!(variante.estilo.as_deref(), real.estilo.as_deref());
            assert_eq!(variante.keystone_id, Some(keystone));
            assert_eq!(variante.perk_ids.len(), 6);
            assert_eq!(variante.perk_ids[0], keystone);
            // El resto del set viaja igual que la base, y también shards,
            // estilos y fuente.
            assert_eq!(variante.perk_ids[1..], base.perk_ids[1..]);
            assert_eq!(variante.stat_shards, base.stat_shards);
            assert_eq!(variante.primary_style_id, base.primary_style_id);
            assert_eq!(variante.sub_style_id, base.sub_style_id);
            assert_eq!(variante.source, base.source);
        }
    }

    /// Dedupe: un keystone ya usado por cualquier página real (8128 en la
    /// segunda) no sale como variante; la única posible es el restante.
    #[test]
    fn expandir_con_variantes_no_repite_keystones_de_las_reales() {
        let catalog = catalogo_con_arboles();
        let pages = expandir_con_variantes(
            vec![pagina_real(8112), pagina_real(8128)],
            &catalog,
        );

        // 2 reales + 1 sola variante (9923; 8112 y 8128 están tomados).
        assert_eq!(pages.len(), 3);
        assert!(!pages[0].sintetizada && !pages[1].sintetizada);
        assert_eq!(pages[1].keystone_id, Some(8128));
        assert_eq!(pages[1].estilo.as_deref(), Some("DAÑO ESCALADO"));

        let variante = &pages[2];
        assert!(variante.sintetizada);
        assert_eq!(variante.keystone_id, Some(9923));
        assert_eq!(variante.estilo.as_deref(), Some("COMBATE CORTO"));
    }

    /// Sin páginas no hay nada que expandir: vec vacío.
    #[test]
    fn expandir_con_variantes_sin_paginas_devuelve_vacio() {
        let catalog = catalogo_con_arboles();
        assert!(expandir_con_variantes(Vec::new(), &catalog).is_empty());
    }

    /// Si el árbol primario de la base no está en el catálogo, no hay
    /// candidatos: solo las reales, con el chip de estilo igual completado
    /// (viene de la tabla pura, no del árbol).
    #[test]
    fn expandir_con_variantes_sin_arbol_en_el_catalogo_no_genera_variantes() {
        let catalog = catalogo_con_arboles();
        let mut base = pagina_real(8112);
        base.primary_style_id = 8300; // Inspiración: ausente del fixture.

        let pages = expandir_con_variantes(vec![base], &catalog);

        assert_eq!(pages.len(), 1);
        assert!(!pages[0].sintetizada);
        assert_eq!(pages[0].estilo.as_deref(), Some("DAÑO EXPLOSIVO"));
    }

    /// Base sin keystone conocido: no hay dónde anclar la variante, quedan
    /// solo las reales (sin chip, el id no está en la tabla).
    #[test]
    fn expandir_con_variantes_base_sin_keystone_no_genera_variantes() {
        let catalog = catalogo_con_arboles();
        let mut base = pagina_real(8112);
        base.keystone_id = None;

        let pages = expandir_con_variantes(vec![base], &catalog);

        assert_eq!(pages.len(), 1);
        assert!(!pages[0].sintetizada);
        assert_eq!(pages[0].estilo, None);
    }
}
