//! Guías cualitativas "cómo ganarle" de campeones enemigos, embebidas en
//! el binario.
//!
//! Los datos viven en `design/data/{slug}.json` (curados a mano) y se
//! embeben con `include_str!` usando ruta absoluta vía `CARGO_MANIFEST_DIR`
//! (inmune a desde-dónde-se-compila), siguiendo el patrón de tabla estática
//! por slug de `champ_art`. El parseo a [`ChampionGuide`] corre una vez por
//! slug y queda cacheado en memoria.
//!
//! - `depth=basic` → los JSON pueden no traer `qualitative`: el campo es
//!   `Option` y falta sin error.
//! - Campos de más en JSONs futuros: serde ignora desconocidos por defecto
//!   (ej. `tier`, `fetchedAt`, `assets`, `strongAgainst`); campos de menos
//!   quedan cubiertos por los `#[serde(default)]`.
//! - En `counterTips` el `cid` del payload se ignora: alcanza con `name`.

use std::collections::HashMap;
use std::sync::OnceLock;

use serde::Deserialize;

/// Tabla estática slug → JSON crudo embebido. Para sumar un campeón a la
/// guía: agregar su entrada acá (y el JSON en `design/data/`); los slugs
/// disponibles se derivan de esta tabla, no hay segunda lista que mantener.
static EMBEDDED_GUIDES: &[(&str, &str)] = &[
    ("aatrox", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/aatrox.json"))),
    ("ahri", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/ahri.json"))),
    ("akali", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/akali.json"))),
    ("akshan", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/akshan.json"))),
    ("alistar", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/alistar.json"))),
    ("ambessa", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/ambessa.json"))),
    ("amumu", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/amumu.json"))),
    ("anivia", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/anivia.json"))),
    ("annie", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/annie.json"))),
    ("aphelios", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/aphelios.json"))),
    ("ashe", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/ashe.json"))),
    ("aurelionsol", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/aurelionsol.json"))),
    ("aurora", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/aurora.json"))),
    ("azir", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/azir.json"))),
    ("bard", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/bard.json"))),
    ("belveth", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/belveth.json"))),
    ("blitzcrank", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/blitzcrank.json"))),
    ("brand", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/brand.json"))),
    ("braum", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/braum.json"))),
    ("briar", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/briar.json"))),
    ("caitlyn", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/caitlyn.json"))),
    ("camille", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/camille.json"))),
    ("cassiopeia", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/cassiopeia.json"))),
    ("chogath", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/chogath.json"))),
    ("corki", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/corki.json"))),
    ("darius", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/darius.json"))),
    ("diana", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/diana.json"))),
    ("draven", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/draven.json"))),
    ("drmundo", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/drmundo.json"))),
    ("ekko", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/ekko.json"))),
    ("elise", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/elise.json"))),
    ("evelynn", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/evelynn.json"))),
    ("ezreal", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/ezreal.json"))),
    ("fiddlesticks", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/fiddlesticks.json"))),
    ("fiora", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/fiora.json"))),
    ("fizz", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/fizz.json"))),
    ("galio", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/galio.json"))),
    ("gangplank", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/gangplank.json"))),
    ("garen", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/garen.json"))),
    ("gnar", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/gnar.json"))),
    ("gragas", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/gragas.json"))),
    ("graves", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/graves.json"))),
    ("gwen", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/gwen.json"))),
    ("hecarim", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/hecarim.json"))),
    ("heimerdinger", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/heimerdinger.json"))),
    ("hwei", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/hwei.json"))),
    ("illaoi", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/illaoi.json"))),
    ("irelia", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/irelia.json"))),
    ("ivern", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/ivern.json"))),
    ("janna", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/janna.json"))),
    ("jarvaniv", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/jarvaniv.json"))),
    ("jax", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/jax.json"))),
    ("jayce", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/jayce.json"))),
    ("jhin", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/jhin.json"))),
    ("jinx", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/jinx.json"))),
    ("kaisa", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/kaisa.json"))),
    ("kalista", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/kalista.json"))),
    ("karma", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/karma.json"))),
    ("karthus", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/karthus.json"))),
    ("kassadin", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/kassadin.json"))),
    ("katarina", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/katarina.json"))),
    ("kayle", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/kayle.json"))),
    ("kayn", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/kayn.json"))),
    ("kennen", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/kennen.json"))),
    ("khazix", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/khazix.json"))),
    ("kindred", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/kindred.json"))),
    ("kled", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/kled.json"))),
    ("kogmaw", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/kogmaw.json"))),
    ("ksante", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/ksante.json"))),
    ("leblanc", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/leblanc.json"))),
    ("leesin", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/leesin.json"))),
    ("leona", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/leona.json"))),
    ("lillia", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/lillia.json"))),
    ("lissandra", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/lissandra.json"))),
    ("locke", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/locke.json"))),
    ("lucian", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/lucian.json"))),
    ("lulu", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/lulu.json"))),
    ("lux", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/lux.json"))),
    ("malphite", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/malphite.json"))),
    ("malzahar", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/malzahar.json"))),
    ("maokai", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/maokai.json"))),
    ("masteryi", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/masteryi.json"))),
    ("mel", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/mel.json"))),
    ("milio", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/milio.json"))),
    ("missfortune", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/missfortune.json"))),
    ("monkeyking", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/monkeyking.json"))),
    ("mordekaiser", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/mordekaiser.json"))),
    ("morgana", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/morgana.json"))),
    ("naafiri", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/naafiri.json"))),
    ("nami", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/nami.json"))),
    ("nasus", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/nasus.json"))),
    ("nautilus", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/nautilus.json"))),
    ("neeko", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/neeko.json"))),
    ("nidalee", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/nidalee.json"))),
    ("nilah", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/nilah.json"))),
    ("nocturne", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/nocturne.json"))),
    ("nunu", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/nunu.json"))),
    ("olaf", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/olaf.json"))),
    ("orianna", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/orianna.json"))),
    ("ornn", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/ornn.json"))),
    ("pantheon", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/pantheon.json"))),
    ("poppy", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/poppy.json"))),
    ("pyke", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/pyke.json"))),
    ("qiyana", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/qiyana.json"))),
    ("quinn", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/quinn.json"))),
    ("rakan", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/rakan.json"))),
    ("rammus", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/rammus.json"))),
    ("reksai", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/reksai.json"))),
    ("rell", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/rell.json"))),
    ("renata", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/renata.json"))),
    ("renekton", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/renekton.json"))),
    ("rengar", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/rengar.json"))),
    ("riven", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/riven.json"))),
    ("rumble", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/rumble.json"))),
    ("ryze", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/ryze.json"))),
    ("samira", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/samira.json"))),
    ("sejuani", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/sejuani.json"))),
    ("senna", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/senna.json"))),
    ("seraphine", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/seraphine.json"))),
    ("sett", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/sett.json"))),
    ("shaco", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/shaco.json"))),
    ("shen", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/shen.json"))),
    ("shyvana", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/shyvana.json"))),
    ("singed", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/singed.json"))),
    ("sion", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/sion.json"))),
    ("sivir", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/sivir.json"))),
    ("skarner", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/skarner.json"))),
    ("smolder", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/smolder.json"))),
    ("sona", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/sona.json"))),
    ("soraka", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/soraka.json"))),
    ("swain", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/swain.json"))),
    ("sylas", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/sylas.json"))),
    ("syndra", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/syndra.json"))),
    ("tahmkench", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/tahmkench.json"))),
    ("taliyah", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/taliyah.json"))),
    ("talon", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/talon.json"))),
    ("taric", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/taric.json"))),
    ("teemo", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/teemo.json"))),
    ("thresh", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/thresh.json"))),
    ("tristana", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/tristana.json"))),
    ("trundle", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/trundle.json"))),
    ("tryndamere", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/tryndamere.json"))),
    ("twistedfate", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/twistedfate.json"))),
    ("twitch", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/twitch.json"))),
    ("udyr", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/udyr.json"))),
    ("urgot", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/urgot.json"))),
    ("varus", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/varus.json"))),
    ("vayne", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/vayne.json"))),
    ("veigar", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/veigar.json"))),
    ("velkoz", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/velkoz.json"))),
    ("vex", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/vex.json"))),
    ("vi", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/vi.json"))),
    ("viego", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/viego.json"))),
    ("viktor", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/viktor.json"))),
    ("vladimir", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/vladimir.json"))),
    ("volibear", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/volibear.json"))),
    ("warwick", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/warwick.json"))),
    ("xayah", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/xayah.json"))),
    ("xerath", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/xerath.json"))),
    ("xinzhao", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/xinzhao.json"))),
    ("yasuo", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/yasuo.json"))),
    ("yone", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/yone.json"))),
    ("yorick", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/yorick.json"))),
    ("yunara", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/yunara.json"))),
    ("yuumi", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/yuumi.json"))),
    ("zaahen", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/zaahen.json"))),
    ("zac", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/zac.json"))),
    ("zed", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/zed.json"))),
    ("zeri", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/zeri.json"))),
    ("ziggs", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/ziggs.json"))),
    ("zilean", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/zilean.json"))),
    ("zoe", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/zoe.json"))),
    ("zyra", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/design/data/zyra.json"))),
];

/// Guía completa de un campeón enemigo: quién es, sus números globales y
/// (si el depth lo incluye) el contenido cualitativo para jugarle.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChampionGuide {
    /// Identidad del campeón (nombre, ids, parche de los datos).
    pub meta: GuideMeta,
    /// Números globales (wr/pr/br) de la muestra analizada.
    pub stats: GuideStats,
    /// Contenido cualitativo; ausente en guías `depth=basic`.
    #[serde(default)]
    pub qualitative: Option<GuideQualitative>,
}

/// Metadatos del campeón de la guía.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GuideMeta {
    /// Slug interno (id ddragon en minúsculas), ej. `"akali"`.
    pub slug: String,
    /// Id de Data Dragon con mayúsculas, ej. `"Akali"`.
    pub ddragon_id: String,
    /// Nombre visible, ej. `"Akali"`.
    pub name: String,
    /// Título localizado al español, ej. `"la Asesina Furtiva"`.
    pub title_es: String,
    /// Id numérico del campeón (`key` de Data Dragon).
    pub cid: i64,
    /// Parche del que vienen los datos, ej. `"16.21"`.
    pub patch: String,
    /// Línea principal del campeón, si la hay (`"middle"`, `"top"`, ...).
    #[serde(default)]
    pub lane: Option<String>,
}

/// Números globales del campeón en la muestra analizada.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GuideStats {
    /// Winrate global, en porcentaje.
    pub wr: f64,
    /// Pickrate global, en porcentaje.
    pub pr: f64,
    /// Banrate global, en porcentaje.
    pub br: f64,
    /// Partidas analizadas detrás de estos números.
    pub analysed: i64,
    /// Distribución de líneas en porcentajes (`"middle" → 70.2`, ...).
    #[serde(default)]
    pub lane_dist: HashMap<String, f64>,
}

/// Contenido cualitativo de la guía (depth completo): fortalezas,
/// debilidades y consejos por arquetipo y por counter.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GuideQualitative {
    /// Para qué brilla el campeón.
    #[serde(default)]
    pub strengths: Vec<String>,
    /// Contra qué sufre (la base de la guía "VS").
    #[serde(default)]
    pub weaknesses: Vec<String>,
    /// Ventanas de poder (`"Nivel 6"`, primer item, ...).
    #[serde(default)]
    pub power_spikes: Vec<String>,
    /// Cómo jugar ESTE campeón (combos, tips y plan por fases); ausente en
    /// JSONs viejos.
    #[serde(default)]
    pub playstyle: Option<GuidePlaystyle>,
    /// Cómo jugarle según el arquetipo de MI campeón.
    #[serde(default)]
    pub vs_archetypes: GuideArchetypes,
    /// Consejos concretos contra los counters listados.
    #[serde(default)]
    pub counter_tips: Vec<GuideCounterTip>,
    /// Fuentes de donde salió el contenido.
    #[serde(default)]
    pub sources: Vec<String>,
}

/// Cómo jugar ESTE campeón: combos, tips generales y plan por fases del
/// juego.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GuidePlaystyle {
    /// Combos concretos del campeón (secuencias de habilidades).
    #[serde(default)]
    pub combos: Vec<String>,
    /// Tips generales para ejecutar bien al campeón.
    #[serde(default)]
    pub tips: Vec<String>,
    /// Plan de juego por fase; ausente si el JSON no lo trae.
    #[serde(default)]
    pub plan: Option<GuidePlan>,
}

/// Plan de juego del campeón por fase de la partida.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GuidePlan {
    /// Qué hacer en early game.
    #[serde(default)]
    pub early: Option<String>,
    /// Qué hacer en mid game.
    #[serde(default)]
    pub mid: Option<String>,
    /// Qué hacer en late game.
    #[serde(default)]
    pub late: Option<String>,
}

/// Consejos por arquetipo (desde el lado del que lee la guía: cómo jugarle
/// SI VOS sos ese arquetipo).
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GuideArchetypes {
    /// Campeones de rango (poke desde afuera).
    #[serde(default)]
    pub range: Vec<String>,
    /// Tanques.
    #[serde(default)]
    pub tanks: Vec<String>,
    /// Asesinos.
    #[serde(default)]
    pub assassins: Vec<String>,
    /// Campeones con mucho crowd control.
    #[serde(default)]
    pub crowd_control: Vec<String>,
}

/// Consejos específicos contra un counter puntual del campeón enemigo.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GuideCounterTip {
    /// Nombre visible del counter, ej. `"Taliyah"`.
    pub name: String,
    /// Tips concretos para ese matchup.
    pub tips: Vec<String>,
}

/// Cache perezosa de guías ya parseadas: cada JSON embebido se parsea una
/// vez por corrida y el fallo (si lo hubiera) también queda cacheado, así
/// el aviso por stderr no se repite en cada lookup.
fn guides_cache() -> &'static HashMap<&'static str, Option<ChampionGuide>> {
    static GUIDES: OnceLock<HashMap<&'static str, Option<ChampionGuide>>> = OnceLock::new();
    GUIDES.get_or_init(|| {
        EMBEDDED_GUIDES
            .iter()
            .map(|(slug, json)| {
                let parsed = match serde_json::from_str::<ChampionGuide>(json) {
                    Ok(guide) => Some(guide),
                    Err(error) => {
                        eprintln!("klipy [guide-data]: JSON embebido de {slug} no parsea: {error}");
                        None
                    }
                };
                (*slug, parsed)
            })
            .collect()
    })
}

/// Guía del campeón enemigo por slug (id ddragon en minúsculas, ej.
/// `"akali"`). `None` si no hay JSON embebido para ese slug o si el parseo
/// falla (se avisa por stderr con prefijo `klipy [guide-data]:`).
#[must_use]
pub fn guide_for_slug(slug: &str) -> Option<ChampionGuide> {
    guides_cache().get(slug)?.clone()
}

/// Slugs lowercase de las fichas embebidas, orden alfabético.
// Queda pub como API de consulta del catálogo embebido.
#[allow(dead_code)]
#[must_use]
pub fn fichas_disponibles() -> &'static [&'static str] {
    static SLUGS: OnceLock<Vec<&'static str>> = OnceLock::new();
    SLUGS.get_or_init(|| EMBEDDED_GUIDES.iter().map(|(slug, _)| *slug).collect())
}

/// JSON de la ficha por slug lowercase (ej `"annie"`), None si no existe.
// Uso próximo: visor standalone de fichas (menú modo manual). No remover.
#[allow(dead_code)]
#[must_use]
pub fn ficha_json(slug: &str) -> Option<&'static str> {
    EMBEDDED_GUIDES
        .iter()
        .find(|(candidate, _)| *candidate == slug)
        .map(|(_, json)| *json)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Akali: guía completa con stats reales del JSON y depth cualitativo.
    #[test]
    fn guide_para_akali_parsea_con_stats_y_debilidades() {
        let guide = guide_for_slug("akali").expect("akali tiene guía embebida");

        assert_eq!(guide.meta.cid, 84);
        assert_eq!(guide.meta.ddragon_id, "Akali");
        assert_eq!(guide.meta.patch, "16.21");
        assert!(guide.stats.wr > 0.0, "el winrate debería ser positivo");
        assert!(guide.stats.analysed > 0);
        assert!(!guide.stats.lane_dist.is_empty());

        let qualitative = guide.qualitative.expect("akali es depth completo");
        assert!(!qualitative.weaknesses.is_empty());
    }

    /// Annie: mínimos del JSON embebido (cid 1).
    #[test]
    fn guide_para_annie_parsea() {
        let guide = guide_for_slug("annie").expect("annie tiene guía embebida");
        assert_eq!(guide.meta.cid, 1);
        assert_eq!(guide.meta.slug, "annie");
    }

    /// Slug sin JSON embebido → None, sin paniquear.
    #[test]
    fn slug_desconocido_da_none() {
        assert!(guide_for_slug("campeon-inexistente").is_none());
        assert!(guide_for_slug("").is_none());
    }

    /// `fichas_disponibles` lista todos los slugs embebidos, en sync con la
    /// tabla y en orden alfabético.
    #[test]
    fn fichas_disponibles_lista_todos_los_slugs_ordenados() {
        let slugs = fichas_disponibles();
        assert_eq!(slugs.len(), EMBEDDED_GUIDES.len());

        let esperados: Vec<&str> = EMBEDDED_GUIDES.iter().map(|(slug, _)| *slug).collect();
        assert_eq!(slugs, esperados, "los slugs deberían derivar de la tabla");

        let mut ordenados = slugs.to_vec();
        ordenados.sort_unstable();
        assert_eq!(slugs.to_vec(), ordenados, "deberían estar en orden alfabético");
    }

    /// `ficha_json` cubre todos los slugs publicados y da None para los de
    /// afuera.
    #[test]
    fn ficha_json_cubre_todos_los_slugs_y_none_para_desconocidos() {
        for slug in fichas_disponibles() {
            assert!(ficha_json(slug).is_some(), "{slug} debería tener JSON embebido");
        }
        assert!(ficha_json("campeon-inexistente").is_none());
        assert!(ficha_json("").is_none());
    }

    /// Las 12 fichas embebidas parsean como `ChampionGuide` y su `meta.slug`
    /// coincide con el de la tabla (smoke test de los JSON curados a mano).
    #[test]
    fn todas_las_fichas_embebidas_parsean_con_slug_consistente() {
        for slug in fichas_disponibles() {
            let guide = guide_for_slug(slug)
                .unwrap_or_else(|| panic!("la ficha {slug} debería parsear"));
            assert_eq!(guide.meta.slug, *slug);
        }
    }

    /// Fixture JSON mínimo con los campos obligatorios de la guía.
    fn fixture_json(qualitative: &str) -> String {
        format!(
            r#"{{
                "meta": {{
                    "slug": "zilean",
                    "ddragonId": "Zilean",
                    "name": "Zilean",
                    "titleEs": "el Cronomante",
                    "cid": 26,
                    "patch": "16.21"
                }},
                "stats": {{
                    "wr": 51.2,
                    "pr": 3.1,
                    "br": 0.4,
                    "analysed": 12000
                }},
                "qualitative": {qualitative}
            }}"#
        )
    }

    /// JSON con `qualitative.playstyle` → parsea combos, tips y plan completo.
    #[test]
    fn playstyle_con_plan_completo_parsea() {
        // Ojo: este string se inserta como VALOR en el `format!` de
        // `fixture_json`, así que las llaves van simples (el escape `{{`
        // solo aplica al template del macro, no a los argumentos).
        let json = fixture_json(
            r#"{
                "playstyle": {
                    "combos": ["E → Q → Q", "R sobre el carry"],
                    "tips": ["Guardar R para el all-in"],
                    "plan": {
                        "early": "Pokear con Q",
                        "mid": "Rotar con ult",
                        "late": "Peel del ADC"
                    }
                }
            }"#,
        );
        let guide = serde_json::from_str::<ChampionGuide>(&json)
            .expect("el fixture con playstyle debería parsear");

        let qualitative = guide.qualitative.expect("trae qualitative");
        let playstyle = qualitative.playstyle.expect("trae playstyle");
        assert_eq!(playstyle.combos, vec!["E → Q → Q", "R sobre el carry"]);
        assert_eq!(playstyle.tips, vec!["Guardar R para el all-in"]);
        let plan = playstyle.plan.expect("trae plan");
        assert_eq!(plan.early.as_deref(), Some("Pokear con Q"));
        assert_eq!(plan.mid.as_deref(), Some("Rotar con ult"));
        assert_eq!(plan.late.as_deref(), Some("Peel del ADC"));
    }

    /// JSON sin `playstyle` (y sin el resto de qualitative) → Option en
    /// None, sin error: los JSON viejos siguen parseando.
    #[test]
    fn playstyle_ausente_da_none_sin_error() {
        let json = fixture_json(r#"{ "strengths": ["Ult que salva"] }"#);
        let guide = serde_json::from_str::<ChampionGuide>(&json)
            .expect("el fixture sin playstyle debería parsear");

        let qualitative = guide.qualitative.expect("trae qualitative");
        assert!(qualitative.playstyle.is_none());
        assert_eq!(qualitative.strengths, vec!["Ult que salva"]);
    }
}
