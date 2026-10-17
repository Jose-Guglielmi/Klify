//! Modelos de Data Dragon.
//!
//! Tipos serde de las respuestas públicas de Data Dragon y el catálogo
//! resuelto que consume la app:
//!
//! - `GET /api/versions.json` → array de `String`; la primera entrada es la
//!   versión más reciente.
//! - `GET /cdn/{version}/data/{DATA_LOCALE}/champion.json` → [`ChampionListResponse`],
//!   cuyo campo `data` mapea id-string (ej. `"Ahri"`) a [`ChampionDto`], que
//!   trae el id numérico en `key` (el mismo que llega como `championId` del
//!   LCU) y la metadata de imagen.
//!
//! [`ChampionCatalog`] precomputa el mapa `id numérico → ChampionEntry` con
//! las URLs de imagen ya armadas, de modo que resolver cualquier championId
//! (el del jugador local hoy, o el de aliados/enemigos en fases futuras) sea
//! un lookup O(1) sin red.
//!
//! Los campos que no declaramos se ignoran al deserializar, y los declarados
//! llevan `default` siempre que tiene sentido: un payload parcial no debe
//! tumbar la carga del catálogo completo.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::lcu::events::champion_placeholder;
use crate::lcu::models::ChampionInfo;

/// Base de la CDN pública de Data Dragon.
pub const CDN_BASE_URL: &str = "https://ddragon.leagueoflegends.com/cdn";

/// Locale de Data Dragon para TODA la data visible (catálogo, runas,
/// detalles de campeón, ítems): español latinoamericano, la región del
/// usuario (LAS). Verificado en vivo 2026-10-17: `es_AR` sirve el parche
/// completo ("Electrocutar — Alcanzar a un campeón…").
pub const DATA_LOCALE: &str = "es_AR";

/// URL del JSON de campeones (`champion.json`) para una versión.
#[must_use]
pub fn champion_json_url(version: &str) -> String {
    format!("{CDN_BASE_URL}/{version}/data/{DATA_LOCALE}/champion.json")
}

/// URL de la imagen cuadrada de un campeón para una versión.
///
/// `image_full` es el campo `image.full` del JSON (ej. `"Ahri.png"`).
#[must_use]
pub fn champion_image_url(version: &str, image_full: &str) -> String {
    format!("{CDN_BASE_URL}/{version}/img/champion/{image_full}")
}

/// Respuesta de `champion.json` (subconjunto: `type`, `format` y `version`
/// de nivel superior se ignoran).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ChampionListResponse {
    /// Mapa id-string → metadata de campeón. `default` para que un payload
    /// sin `data` produzca un catálogo vacío (placeholders) y no un error.
    #[serde(default)]
    pub data: HashMap<String, ChampionDto>,
}

/// Metadata de un campeón dentro de `champion.json`.
///
/// Campos de gameplay (`title`, `info`, `stats`, `tags`, ...) se ignoran.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ChampionDto {
    /// Id string (`"Ahri"`). No siempre coincide con `name` (Wukong tiene id
    /// `"MonkeyKing"`); es útil como clave de otras APIs de Data Dragon.
    #[serde(default)]
    pub id: String,
    /// Id numérico como string (`"103"`): el que matchea `championId` del LCU.
    #[serde(default)]
    pub key: String,
    /// Nombre visible (`"Ahri"`).
    #[serde(default)]
    pub name: String,
    /// Metadata de imagen; solo usamos `full`.
    #[serde(default)]
    pub image: ChampionImageDto,
}

/// Metadata de imagen de un campeón (subconjunto: sprite y coordenadas se
/// ignoran).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ChampionImageDto {
    /// Nombre del archivo en la CDN (ej. `"Ahri.png"`).
    #[serde(default)]
    pub full: String,
}

/// Entrada resuelta del catálogo: lo mínimo para mostrar un campeón.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChampionEntry {
    /// Nombre a mostrar (ej. `"Ahri"`).
    pub name: String,
    /// URL absoluta de la imagen en la CDN de Data Dragon.
    pub image_url: String,
    /// Slug para endpoints de estadísticas externos (Lolalytics): el `id`
    /// de Data Dragon en minúsculas, solo caracteres alfanuméricos ASCII
    /// (ver [`champion_slug`]). FASE 9; `default` para que catálogos
    /// serializados antes de este campo sigan deserializando.
    #[serde(default)]
    pub slug: String,
    /// Id de Data Dragon con su casing original (ej. `"MonkeyKing"`,
    /// `"Kaisa"`). Es la clave de los JSON por-campeón
    /// (`champion/{ChampionId}.json`) y de los iconos cuadrados: la CDN es
    /// case-sensitive y el slug en minúsculas da 403 (verificado en vivo:
    /// `img/champion/veigar.png` → 403, `img/champion/Veigar.png` → 200).
    /// FASE 11a; `default` por la misma razón que `slug`.
    #[serde(default)]
    pub ddragon_id: String,
}

impl ChampionEntry {
    /// URL del splash base del campeón (`{ddragon_id}_0.jpg`). Los splashes
    /// NO llevan versión en la CDN (verificado en vivo: `Veigar_0.jpg` →
    /// 200); es la imagen de las cards de equipo y del hero del champ
    /// select (FASE 11b-2a).
    ///
    /// Entradas viejas sin `ddragon_id` caen a `image_url` (icono cuadrado)
    /// en lugar de armar una URL con id vacío.
    #[must_use]
    pub fn splash_url(&self) -> String {
        if self.ddragon_id.is_empty() {
            return self.image_url.clone();
        }
        format!("{CDN_BASE_URL}/img/champion/splash/{}_0.jpg", self.ddragon_id)
    }

    /// URL del art vertical (loading screen) del campeón: la CDN sirve
    /// `img/champion/loading/{id}_0.jpg` (308×560, cuerpo casi completo).
    /// Es el arte del modo visual VS (FASE 12); cuando haya PNGs sin fondo
    /// propios, el origen se cambia acá sin tocar el layout.
    #[must_use]
    pub fn loading_url(&self) -> String {
        if self.ddragon_id.is_empty() {
            return self.image_url.clone();
        }
        format!("{CDN_BASE_URL}/img/champion/loading/{}_0.jpg", self.ddragon_id)
    }

    /// URL del icono cuadrado del campeón para una versión: la CDN sirve
    /// `img/champion/{id}.png` con el casing original del id de Data Dragon
    /// (ver nota del campo [`ChampionEntry::ddragon_id`]).
    ///
    /// Entradas viejas sin `ddragon_id` (catálogos serializados antes de
    /// FASE 11a) caen a `image_url`, que ya apunta al icono de la versión
    /// con que se cargó el catálogo.
    #[must_use]
    pub fn icon_url(&self, version: &str) -> String {
        if self.ddragon_id.is_empty() {
            return self.image_url.clone();
        }
        champion_image_url(version, &format!("{}.png", self.ddragon_id))
    }
}

/// URL del icono de un ítem para una versión: la CDN sirve
/// `img/item/{id}.png` (verificado en vivo: `3157.png`, `3140.png` → 200).
#[must_use]
pub fn item_icon_url(version: &str, item_id: i64) -> String {
    format!("{CDN_BASE_URL}/{version}/img/item/{item_id}.png")
}

/// Slug de un campeón para endpoints externos de estadísticas (Lolalytics):
/// el `id` de Data Dragon (ej. `"DrMundo"`) en minúsculas, conservando solo
/// caracteres alfanuméricos ASCII.
///
/// El `id` de ddragon ya viene sin espacios ni signos (`"DrMundo"`,
/// `"Kaisa"`, `"MonkeyKing"`), así que el filtro es cinturón y suspendores
/// frente a ids futuros con signos. Es el mismo formato que usa league-lean
/// (`formatChampionSlug`): lowercase sin apóstrofes, espacios ni puntos.
#[must_use]
pub fn champion_slug(id: &str) -> String {
    id.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Catálogo completo de campeones para una versión de Data Dragon.
///
/// Pensado para vivir en una señal `Signal<Option<ChampionCatalog>>`: mientras
/// sea `None` (descarga pendiente o fallida) la app funciona con
/// placeholders, y al llegar se usa para resolver cualquier championId.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ChampionCatalog {
    /// Versión de Data Dragon de la que proviene (ej. `"15.21.1"`).
    pub version: String,
    /// Índice por id numérico (el `championId` del LCU).
    pub by_id: HashMap<i64, ChampionEntry>,
}

impl ChampionCatalog {
    /// Construye el catálogo a partir de la respuesta de `champion.json`.
    ///
    /// - `version` es la versión vigente según `versions.json` (fuente
    ///   autoritativa para las URLs de la CDN).
    /// - Las entradas con `key` no numérica (payload malformado) se ignoran
    ///   en lugar de invalidar el catálogo completo.
    #[must_use]
    pub fn build(version: String, response: ChampionListResponse) -> Self {
        let by_id = response
            .data
            .into_values()
            .filter_map(|dto| {
                let id = dto.key.parse::<i64>().ok()?;
                let entry = ChampionEntry {
                    name: dto.name,
                    image_url: champion_image_url(&version, &dto.image.full),
                    slug: champion_slug(&dto.id),
                    ddragon_id: dto.id,
                };
                Some((id, entry))
            })
            .collect();

        Self { version, by_id }
    }

    /// Entrada del catálogo para un id numérico, si existe.
    #[must_use]
    pub fn get(&self, id: i64) -> Option<&ChampionEntry> {
        self.by_id.get(&id)
    }

    /// Resuelve un id a [`ChampionInfo`]. Si el id no está en el catálogo
    /// (campeón nuevo, o `key` malformada), cae al placeholder de
    /// [`crate::lcu::events::champion_placeholder`].
    #[must_use]
    pub fn resolve(&self, id: i64) -> ChampionInfo {
        self.get(id).map_or_else(
            || champion_placeholder(id),
            |entry| ChampionInfo {
                id,
                name: entry.name.clone(),
                image_url: entry.image_url.clone(),
            },
        )
    }
}

/// Resuelve un championId contra un catálogo opcional.
///
/// `None` (catálogo todavía no cargado) o id ausente → placeholder. Con el
/// catálogo presente delega en [`ChampionCatalog::resolve`].
#[must_use]
pub fn resolve_champion(id: i64, catalog: Option<&ChampionCatalog>) -> ChampionInfo {
    catalog.map_or_else(|| champion_placeholder(id), |catalog| catalog.resolve(id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// `versions.json` reducido: array de strings, la primera es la reciente.
    #[test]
    fn versions_parsea_y_la_primera_es_la_mas_reciente() {
        let versions: Vec<String> =
            serde_json::from_value(json!(["15.21.1", "15.20.1", "15.19.1"]))
                .expect("versions.json válido");

        assert_eq!(versions.len(), 3);
        assert_eq!(versions.first().map(String::as_str), Some("15.21.1"));
    }

    /// `champion.json` de ejemplo con 3 campeones reales (incluye Wukong,
    /// cuyo id-string `"MonkeyKing"` difiere del `name`) y campos extra que
    /// deben ignorarse.
    fn champion_json() -> serde_json::Value {
        json!({
            "type": "champion",
            "format": "standaloneData",
            "version": "15.21.1",
            "data": {
                "Aatrox": {
                    "id": "Aatrox",
                    "key": "266",
                    "name": "Aatrox",
                    "title": "the Darkin Blade",
                    "blurb": "...",
                    "tags": ["Fighter"],
                    "partype": "Blood Well",
                    "image": {
                        "full": "Aatrox.png",
                        "sprite": "champion0.png",
                        "group": "champion",
                        "x": 0, "y": 0, "w": 48, "h": 48
                    }
                },
                "Ahri": {
                    "id": "Ahri",
                    "key": "103",
                    "name": "Ahri",
                    "title": "the Nine-Tailed Fox",
                    "image": { "full": "Ahri.png", "sprite": "champion1.png" }
                },
                "MonkeyKing": {
                    "id": "MonkeyKing",
                    "key": "62",
                    "name": "Wukong",
                    "title": "the Monkey King",
                    "image": { "full": "MonkeyKing.png", "sprite": "champion2.png" }
                }
            }
        })
    }

    fn catalogo_de_ejemplo() -> ChampionCatalog {
        let response: ChampionListResponse =
            serde_json::from_value(champion_json()).expect("champion.json válido");
        ChampionCatalog::build(String::from("15.21.1"), response)
    }

    #[test]
    fn champion_json_parsea_ignorando_campos_extra() {
        let response: ChampionListResponse =
            serde_json::from_value(champion_json()).expect("champion.json válido");

        assert_eq!(response.data.len(), 3);
        let ahri = response.data.get("Ahri").expect("Ahri en data");
        assert_eq!(ahri.key, "103");
        assert_eq!(ahri.name, "Ahri");
        assert_eq!(ahri.image.full, "Ahri.png");
    }

    #[test]
    fn build_indexa_por_id_numerico_del_key() {
        let catalog = catalogo_de_ejemplo();

        assert_eq!(catalog.version, "15.21.1");
        assert_eq!(catalog.by_id.len(), 3);
        assert_eq!(catalog.get(266).map(|e| e.name.as_str()), Some("Aatrox"));
        assert_eq!(catalog.get(103).map(|e| e.name.as_str()), Some("Ahri"));
        // El índice es por `key` numérico, no por id-string ni por `name`.
        assert_eq!(catalog.get(62).map(|e| e.name.as_str()), Some("Wukong"));
        assert!(catalog.get(999).is_none());
    }

    #[test]
    fn url_de_imagen_correcta() {
        let catalog = catalogo_de_ejemplo();
        let ahri = catalog.get(103).expect("Ahri en el catálogo");

        assert_eq!(
            ahri.image_url,
            "https://ddragon.leagueoflegends.com/cdn/15.21.1/img/champion/Ahri.png"
        );
    }

    #[test]
    fn entradas_con_key_no_numerica_se_ignoran() {
        let response: ChampionListResponse = serde_json::from_value(json!({
            "data": {
                "Ahri": { "id": "Ahri", "key": "103", "name": "Ahri", "image": { "full": "Ahri.png" } },
                "Roto": { "id": "Roto", "key": "no-es-un-numero", "name": "Roto", "image": { "full": "Roto.png" } }
            }
        }))
        .expect("payload válido");

        let catalog = ChampionCatalog::build(String::from("15.21.1"), response);

        assert_eq!(catalog.by_id.len(), 1);
        assert!(catalog.get(103).is_some());
    }

    #[test]
    fn resolve_existente_devuelve_info_real() {
        let catalog = catalogo_de_ejemplo();
        let info = catalog.resolve(103);

        assert_eq!(
            info,
            ChampionInfo {
                id: 103,
                name: String::from("Ahri"),
                image_url: String::from(
                    "https://ddragon.leagueoflegends.com/cdn/15.21.1/img/champion/Ahri.png"
                ),
            }
        );
    }

    #[test]
    fn resolve_desconocido_cae_al_placeholder() {
        let catalog = catalogo_de_ejemplo();

        assert_eq!(catalog.resolve(9999), champion_placeholder(9999));
    }

    #[test]
    fn resolve_champion_sin_catalogo_cae_al_placeholder() {
        assert_eq!(resolve_champion(412, None), champion_placeholder(412));
    }

    #[test]
    fn urls_de_la_cdn_tienen_el_formato_esperado() {
        assert_eq!(
            champion_json_url("15.21.1"),
            format!("https://ddragon.leagueoflegends.com/cdn/15.21.1/data/{DATA_LOCALE}/champion.json")
        );
        assert_eq!(
            champion_image_url("15.21.1", "MonkeyKing.png"),
            "https://ddragon.leagueoflegends.com/cdn/15.21.1/img/champion/MonkeyKing.png"
        );
    }

    /// FASE 9: el slug es el `id` de ddragon en minúsculas, filtrando todo
    /// carácter no alfanumérico. Los ids reales ya vienen limpios
    /// (`DrMundo`, `Kaisa`, `MonkeyKing`); los casos trampa cubren ids
    /// futuros con signos (el *nombre* visible de Kai'Sa / Dr. Mundo sí los
    /// trae, de ahí la regla).
    #[test]
    fn champion_slug_lowercases_y_filtra_no_alfanumericos() {
        assert_eq!(champion_slug("DrMundo"), "drmundo");
        assert_eq!(champion_slug("Ahri"), "ahri");
        assert_eq!(champion_slug("MonkeyKing"), "monkeyking");
        assert_eq!(champion_slug("Kai'Sa"), "kaisa");
        assert_eq!(champion_slug("Dr. Mundo"), "drmundo");
        assert_eq!(champion_slug("Nunu & Willump"), "nunuwillump");
        assert_eq!(champion_slug(""), "");
    }

    /// `build` llena el slug desde el `id` string de cada DTO.
    #[test]
    fn build_llena_el_slug_desde_el_id_ddragon() {
        let catalog = catalogo_de_ejemplo();

        assert_eq!(catalog.get(266).map(|e| e.slug.as_str()), Some("aatrox"));
        assert_eq!(catalog.get(103).map(|e| e.slug.as_str()), Some("ahri"));
        assert_eq!(
            catalog.get(62).map(|e| e.slug.as_str()),
            Some("monkeyking")
        );
    }

    /// FASE 11a: `build` conserva el casing original del id de ddragon en
    /// `ddragon_id` (la CDN es case-sensitive para iconos y para el JSON
    /// por-campeón).
    #[test]
    fn build_llena_el_ddragon_id_con_casing_original() {
        let catalog = catalogo_de_ejemplo();

        assert_eq!(
            catalog.get(62).map(|e| e.ddragon_id.as_str()),
            Some("MonkeyKing")
        );
        assert_eq!(
            catalog.get(103).map(|e| e.ddragon_id.as_str()),
            Some("Ahri")
        );
    }

    /// `icon_url` arma la URL con el casing original (verificado en vivo:
    /// `veigar.png` en minúsculas da 403, `Veigar.png` da 200).
    #[test]
    fn icon_url_usa_el_ddragon_id_con_casing_original() {
        let catalog = catalogo_de_ejemplo();
        let wukong = catalog.get(62).expect("Wukong en el catálogo");

        assert_eq!(
            wukong.icon_url("16.20.1"),
            "https://ddragon.leagueoflegends.com/cdn/16.20.1/img/champion/MonkeyKing.png"
        );
    }

    /// FASE 11b-2a: `splash_url` arma la URL del splash base sin versión
    /// (formato verificado en vivo contra la CDN); sin `ddragon_id` cae a
    /// `image_url`.
    #[test]
    fn splash_url_apunta_al_splash_base_sin_version() {
        let catalog = catalogo_de_ejemplo();

        let ahri = catalog.get(103).expect("Ahri en el catálogo");
        assert_eq!(
            ahri.splash_url(),
            "https://ddragon.leagueoflegends.com/cdn/img/champion/splash/Ahri_0.jpg"
        );

        let vieja = ChampionEntry {
            name: String::from("Vieja"),
            image_url: String::from("https://ddragon.leagueoflegends.com/cdn/15.21.1/img/champion/Vieja.png"),
            slug: String::from("vieja"),
            ddragon_id: String::new(),
        };
        assert_eq!(
            vieja.splash_url(),
            "https://ddragon.leagueoflegends.com/cdn/15.21.1/img/champion/Vieja.png"
        );
    }

    /// Entradas sin `ddragon_id` (catálogos previos a FASE 11a) caen a
    /// `image_url` en lugar de armar una URL con id vacío.
    #[test]
    fn icon_url_sin_ddragon_id_cae_a_image_url() {
        let entry = ChampionEntry {
            name: String::from("Vieja"),
            image_url: String::from("https://ddragon.leagueoflegends.com/cdn/15.21.1/img/champion/Vieja.png"),
            slug: String::from("vieja"),
            ddragon_id: String::new(),
        };

        assert_eq!(
            entry.icon_url("16.20.1"),
            "https://ddragon.leagueoflegends.com/cdn/15.21.1/img/champion/Vieja.png"
        );
    }

    /// URL del icono de ítem: `img/item/{id}.png` (formato verificado en
    /// vivo con 3157/3140/3020/6657).
    #[test]
    fn item_icon_url_tiene_el_formato_de_la_cdn() {
        assert_eq!(
            item_icon_url("16.20.1", 3157),
            "https://ddragon.leagueoflegends.com/cdn/16.20.1/img/item/3157.png"
        );
    }
}
