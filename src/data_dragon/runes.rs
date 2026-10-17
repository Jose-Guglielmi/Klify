//! Modelos de runas de Data Dragon (`runesReforged.json`), base del futuro
//! recomendador de runas (RF-05 del spec).
//!
//! - `GET /cdn/{version}/data/{DATA_LOCALE}/runesReforged.json` → array de
//!   [`RuneTreeDto`] (un por árbol de maestrías: Dominación, Precisión, ...).
//!   Cada árbol trae `id` numérico (`8100`), `key` string (`"Domination"`),
//!   `name`, `icon` y `slots`, donde cada slot es `{ "runes": [...] }`.
//! - Cada runa trae `id` numérico (el mismo que llega como `perkId` en
//!   Match-V5), `key`, `name` e `icon`.
//!
//! [`RuneCatalog`] precomputa, además del árbol completo, un índice
//! `id → RuneDef` con la posición dentro del árbol (`tree_id`, `slot_index`)
//! y la condición de keystone: por convención de ddragon, **toda** runa de la
//! fila 0 de un árbol es keystone (cada árbol ofrece 3-5 keystones en esa
//! fila y runas menores en las filas siguientes). Así el recomendador futuro
//! resuelve cualquier `perkId` en O(1) sin red.
//!
//! Módulo aparte de `models.rs` (campeones) para mantener cada fuente de
//! ddragon con sus DTOs, su dominio y sus tests en un solo lugar.
//!
//! Los campos que no declaramos (`shortDesc`, `longDesc`, ...) se ignoran al
//! deserializar, y los declarados llevan `default`: un payload parcial o con
//! strings vacíos no debe tumbar la carga del catálogo completo.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::data_dragon::models::{CDN_BASE_URL, DATA_LOCALE};

/// URL del JSON de runas (`runesReforged.json`) para una versión.
#[must_use]
pub fn runes_reforged_url(version: &str) -> String {
    format!("{CDN_BASE_URL}/{version}/data/{DATA_LOCALE}/runesReforged.json")
}

/// Base de las imágenes de runas en la CDN.
///
/// NÓTESE: a diferencia de los campeones, los iconos de runas viven bajo
/// `/cdn/img/` **sin versión** — es la convención de ddragon para las
/// `perk-images` (verificado contra `runesReforged.json` real: las rutas
/// `perk-images/...` resuelven en `cdn/img/...` sin prefijo de versión).
pub const RUNE_IMAGE_BASE_URL: &str = "https://ddragon.leagueoflegends.com/cdn/img";

/// URL absoluta del icono de una runa a partir de su ruta relativa
/// (el campo `icon` de `runesReforged.json`, ej.
/// `"perk-images/Styles/Domination/Electrocute/Electrocute.png"`).
#[must_use]
pub fn rune_icon_url(icon_path: &str) -> String {
    format!("{RUNE_IMAGE_BASE_URL}/{icon_path}")
}

/// Runa dentro de `runesReforged.json` (subconjunto; `longDesc` se ignora).
///
/// FASE 11b-2a: `shortDesc` entra al modelo para las descripciones de runas
/// en la UI del champ select. Llega con HTML liviano de tooltips del cliente
/// (`<lol-uikit-tooltipped-keyword>`, `<font>`, ...); se limpia al render
/// con [`crate::ui::format::strip_html`] — el catálogo conserva el texto
/// crudo de la fuente.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RuneDto {
    /// Id numérico (el `perkId` de Match-V5). 0 = malformado/ausente.
    #[serde(default)]
    pub id: i64,
    /// Clave ddragon (ej. `"PressTheAttack"`).
    #[serde(default)]
    pub key: String,
    /// Nombre visible (ej. `"Press the Attack"`).
    #[serde(default)]
    pub name: String,
    /// Ruta relativa del icono (resolver con [`rune_icon_url`]).
    #[serde(default)]
    pub icon: String,
    /// Descripción corta (con HTML liviano; limpiar con `strip_html`).
    #[serde(rename = "shortDesc", default)]
    pub short_desc: String,
}

/// Fila de un árbol: `slots[n].runes` en el payload.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RuneSlotDto {
    /// Runas ofrecidas en esta fila (3 habitualmente, 3-5 en la fila 0).
    #[serde(default)]
    pub runes: Vec<RuneDto>,
}

/// Árbol de runas dentro de `runesReforged.json`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RuneTreeDto {
    /// Id numérico del árbol (`8000` Precisión, `8100` Dominación, `8200`
    /// Brujería, `8300` Inspiración, `8400` Valor).
    #[serde(default)]
    pub id: i32,
    /// Clave ddragon del árbol (ej. `"Domination"`).
    #[serde(default)]
    pub key: String,
    /// Nombre visible (ej. `"Dominación"`/`"Domination"` según locale).
    #[serde(default)]
    pub name: String,
    /// Ruta relativa del icono del árbol.
    #[serde(default)]
    pub icon: String,
    /// Filas del árbol; la fila 0 contiene los keystones.
    #[serde(default)]
    pub slots: Vec<RuneSlotDto>,
}

/// Definición completa de una runa, ya posicionada dentro de su árbol.
///
/// Es lo que devuelve el lookup `by_id` (y lo que mapeará los `perkId` de
/// Match-V5 en fases futuras).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RuneDef {
    /// Id numérico (el `perkId` de Match-V5).
    pub id: i64,
    /// Clave ddragon (ej. `"PressTheAttack"`).
    pub key: String,
    /// Nombre visible (ej. `"Press the Attack"`).
    pub name: String,
    /// Ruta relativa del icono; resolver con [`RuneCatalog::icon_url`].
    pub icon_path: String,
    /// Id del árbol al que pertenece la runa.
    pub tree_id: i32,
    /// Índice de la fila dentro del árbol (0 = fila de keystones).
    pub slot_index: usize,
    /// Keystone: runa de la fila 0 de su árbol (regla ddragon).
    pub is_keystone: bool,
    /// Descripción corta con HTML liviano de la fuente (`shortDesc`).
    /// FASE 11b-2a; `default` para que catálogos serializados antes de esta
    /// fase sigan deserializando.
    #[serde(default)]
    pub short_desc: String,
}

/// Fila ya resuelta de un árbol del catálogo.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RuneSlot {
    /// Runas de la fila, en el orden del payload.
    pub runes: Vec<RuneDef>,
}

/// Árbol ya resuelto del catálogo.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RuneTree {
    /// Id numérico del árbol.
    pub id: i32,
    /// Clave ddragon del árbol (ej. `"Domination"`).
    pub key: String,
    /// Nombre visible.
    pub name: String,
    /// Ruta relativa del icono del árbol.
    pub icon_path: String,
    /// Filas del árbol; la fila 0 contiene los keystones.
    pub slots: Vec<RuneSlot>,
}

/// Catálogo completo de runas para una versión de Data Dragon.
///
/// Pensado para vivir en una señal `Signal<Option<RuneCatalog>>`: mientras
/// sea `None` (descarga pendiente o fallida) el recomendador futuro no tiene
/// datos y la app funciona igual; al llegar, cualquier `perkId` se resuelve
/// en O(1) vía [`RuneCatalog::get`].
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RuneCatalog {
    /// Versión de Data Dragon de la que proviene (ej. `"16.20.1"`).
    pub version: String,
    /// Árboles completos, en el orden del payload.
    pub trees: Vec<RuneTree>,
    /// Índice por id numérico de runa (el `perkId` de Match-V5).
    pub by_id: HashMap<i64, RuneDef>,
}

impl RuneCatalog {
    /// Construye el catálogo a partir de `runesReforged.json` ya parseado.
    ///
    /// - `version` es la versión vigente (del cliente o la más reciente de
    ///   `versions.json`); es la fuente de la URL de descarga y queda
    ///   registrada en el catálogo.
    /// - Anota en cada runa su `tree_id`, `slot_index` e `is_keystone`
    ///   (fila 0, regla ddragon).
    /// - Las runas sin id válido (ausente, `null` o 0) se descartan en lugar
    ///   de invalidar el catálogo completo. Ids duplicados: gana el último.
    #[must_use]
    pub fn build(version: String, trees: Vec<RuneTreeDto>) -> Self {
        let mut catalog = Self {
            version,
            trees: Vec::with_capacity(trees.len()),
            by_id: HashMap::new(),
        };

        for tree in trees {
            let mut slots = Vec::with_capacity(tree.slots.len());
            for (slot_index, slot) in tree.slots.into_iter().enumerate() {
                let runes: Vec<RuneDef> = slot
                    .runes
                    .into_iter()
                    .filter(|rune| rune.id > 0)
                    .map(|rune| RuneDef {
                        id: rune.id,
                        key: rune.key,
                        name: rune.name,
                        icon_path: rune.icon,
                        tree_id: tree.id,
                        slot_index,
                        is_keystone: slot_index == 0,
                        short_desc: rune.short_desc,
                    })
                    .collect();

                for rune in &runes {
                    catalog.by_id.insert(rune.id, rune.clone());
                }
                slots.push(RuneSlot { runes });
            }

            catalog.trees.push(RuneTree {
                id: tree.id,
                key: tree.key,
                name: tree.name,
                icon_path: tree.icon,
                slots,
            });
        }

        catalog
    }

    /// Definición de una runa por id numérico (`perkId`), si existe.
    #[must_use]
    pub fn get(&self, id: i64) -> Option<&RuneDef> {
        self.by_id.get(&id)
    }

    /// URL absoluta del icono de una runa del catálogo.
    ///
    /// Los iconos de runas van bajo `/cdn/img/` sin versión (ver
    /// [`RUNE_IMAGE_BASE_URL`]).
    #[must_use]
    pub fn icon_url(&self, rune: &RuneDef) -> String {
        rune_icon_url(&rune.icon_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Fixture realista: subconjunto del `runesReforged.json` real (16.20.1)
    /// con 2 árboles y 3 filas en total. Dominación trae sus dos primeras
    /// filas; Precisión, solo la fila de keystones. Campos extra y textos
    /// reales (`shortDesc`/`longDesc` con HTML) deben ignorarse sin error.
    fn runes_reforged_fixture() -> serde_json::Value {
        json!([
            {
                "id": 8100,
                "key": "Domination",
                "icon": "perk-images/Styles/7200_Domination.png",
                "name": "Domination",
                "slots": [
                    {
                        "runes": [
                            {
                                "id": 8112,
                                "key": "Electrocute",
                                "icon": "perk-images/Styles/Domination/Electrocute/Electrocute.png",
                                "name": "Electrocute",
                                "shortDesc": "Hitting a champion with 3 separate attacks...",
                                "longDesc": "Hitting a champion with 3 separate attacks... <lol-uikit-tooltipped-keyword>...</lol-uikit-tooltipped-keyword>"
                            },
                            {
                                "id": 8128,
                                "key": "DarkHarvest",
                                "icon": "perk-images/Styles/Domination/DarkHarvest/DarkHarvest.png",
                                "name": "Dark Harvest",
                                "shortDesc": "Damaging a low health champion...",
                                "longDesc": "Damaging a Champion below 50% health..."
                            },
                            {
                                "id": 9923,
                                "key": "HailOfBlades",
                                "icon": "perk-images/Styles/Domination/HailOfBlades/HailOfBlades.png",
                                "name": "Hail of Blades",
                                "shortDesc": "Gain a large amount of Attack Speed...",
                                "longDesc": "Gain 90% Attack Speed..."
                            }
                        ]
                    },
                    {
                        "runes": [
                            {
                                "id": 8126,
                                "key": "CheapShot",
                                "icon": "perk-images/Styles/Domination/CheapShot/CheapShot.png",
                                "name": "Cheap Shot",
                                "shortDesc": "Deal bonus true damage...",
                                "longDesc": "Damaging champions with impaired movement..."
                            },
                            {
                                "id": 8139,
                                "key": "TasteOfBlood",
                                "icon": "perk-images/Styles/Domination/TasteOfBlood/GreenTerror_TasteOfBlood.png",
                                "name": "Taste of Blood",
                                "shortDesc": "Heal when you damage an enemy champion.",
                                "longDesc": "Heal when you damage an enemy champion."
                            }
                        ]
                    }
                ]
            },
            {
                "id": 8000,
                "key": "Precision",
                "icon": "perk-images/Styles/7201_Precision.png",
                "name": "Precision",
                "slots": [
                    {
                        "runes": [
                            {
                                "id": 8005,
                                "key": "PressTheAttack",
                                "icon": "perk-images/Styles/Precision/PressTheAttack/PressTheAttack.png",
                                "name": "Press the Attack",
                                "shortDesc": "Hitting an enemy champion 3 consecutive times...",
                                "longDesc": "Hitting an enemy champion with 3 consecutive basic attacks..."
                            },
                            {
                                "id": 8008,
                                "key": "LethalTempo",
                                "icon": "perk-images/Styles/Precision/LethalTempo/LethalTempoTemp.png",
                                "name": "Lethal Tempo",
                                "shortDesc": "Attacking an enemy champion grants Attack Speed...",
                                "longDesc": "Attacking an enemy champion grants [6% Melee || 4% Ranged]..."
                            }
                        ]
                    }
                ]
            }
        ])
    }

    fn catalogo_de_ejemplo() -> RuneCatalog {
        let trees: Vec<RuneTreeDto> =
            serde_json::from_value(runes_reforged_fixture()).expect("runesReforged.json válido");
        RuneCatalog::build(String::from("16.20.1"), trees)
    }

    /// Parsing del fixture realista: 2 árboles, 3 filas en total, claves y
    /// nombres intactos, campos extra (`shortDesc`/`longDesc`) ignorados.
    #[test]
    fn runes_reforged_parsea_arboles_slots_y_keystones() {
        let catalog = catalogo_de_ejemplo();

        assert_eq!(catalog.version, "16.20.1");
        assert_eq!(catalog.trees.len(), 2);

        let domination = &catalog.trees[0];
        assert_eq!(domination.id, 8100);
        assert_eq!(domination.key, "Domination");
        assert_eq!(domination.name, "Domination");
        assert_eq!(domination.icon_path, "perk-images/Styles/7200_Domination.png");
        assert_eq!(domination.slots.len(), 2);

        let precision = &catalog.trees[1];
        assert_eq!(precision.id, 8000);
        assert_eq!(precision.key, "Precision");
        // Total de filas del fixture: 2 (Dominación) + 1 (Precisión).
        let total_slots: usize = catalog.trees.iter().map(|t| t.slots.len()).sum();
        assert_eq!(total_slots, 3);
    }

    /// Regla ddragon: toda runa de la fila 0 es keystone; las demás no.
    #[test]
    fn keystones_son_las_runas_de_la_fila_0() {
        let catalog = catalogo_de_ejemplo();

        // Fila 0 de Dominación: los tres keystones reales del árbol.
        for rune_id in [8112, 8128, 9923] {
            let rune = catalog.get(rune_id).expect("runa de la fila 0");
            assert!(rune.is_keystone, "la runa {rune_id} debería ser keystone");
        }
        // Fila 1 de Dominación: runas menores.
        for rune_id in [8126, 8139] {
            let rune = catalog.get(rune_id).expect("runa de la fila 1");
            assert!(!rune.is_keystone, "la runa {rune_id} no debería ser keystone");
        }
        // Fila 0 de Precisión: también keystones (Press the Attack, Lethal Tempo).
        for rune_id in [8005, 8008] {
            let rune = catalog.get(rune_id).expect("keystone de Precisión");
            assert!(rune.is_keystone);
        }
    }

    /// El índice `by_id` resuelve cada runa con su árbol, su fila y su clave.
    #[test]
    fn by_id_indexa_cada_runa_con_su_arbol_y_fila() {
        let catalog = catalogo_de_ejemplo();

        assert_eq!(catalog.by_id.len(), 7);

        let electrocute = catalog.get(8112).expect("Electrocute en by_id");
        assert_eq!(electrocute.id, 8112);
        assert_eq!(electrocute.key, "Electrocute");
        assert_eq!(electrocute.name, "Electrocute");
        assert_eq!(electrocute.tree_id, 8100);
        assert_eq!(electrocute.slot_index, 0);

        let cheap_shot = catalog.get(8126).expect("Cheap Shot en by_id");
        assert_eq!(cheap_shot.tree_id, 8100);
        assert_eq!(cheap_shot.slot_index, 1);
        assert_eq!(cheap_shot.name, "Cheap Shot");

        let press_the_attack = catalog.get(8005).expect("Press the Attack en by_id");
        assert_eq!(press_the_attack.tree_id, 8000);
        assert_eq!(press_the_attack.slot_index, 0);

        assert!(catalog.get(9999).is_none());
    }

    /// Los iconos de runas van bajo `/cdn/img/`, SIN versión (convención
    /// ddragon para perk-images, distinta de la de campeones).
    #[test]
    fn icon_url_usa_cdn_img_sin_version() {
        let catalog = catalogo_de_ejemplo();
        let electrocute = catalog.get(8112).expect("Electrocute en by_id");

        assert_eq!(
            catalog.icon_url(electrocute),
            "https://ddragon.leagueoflegends.com/cdn/img/perk-images/Styles/Domination/Electrocute/Electrocute.png"
        );

        // El helper libre (ruta relativa → URL) coincide con el método.
        assert_eq!(
            rune_icon_url(electrocute.icon_path.as_str()),
            catalog.icon_url(electrocute)
        );
        assert_eq!(
            runes_reforged_url("16.20.1"),
            format!("https://ddragon.leagueoflegends.com/cdn/16.20.1/data/{DATA_LOCALE}/runesReforged.json")
        );
    }

    /// FASE 11b-2a: `shortDesc` entra al catálogo con su HTML crudo (la UI
    /// lo limpia con `strip_html`); sin el campo → string vacío.
    #[test]
    fn short_desc_entra_al_catalogo_y_tolera_su_ausencia() {
        let catalog = catalogo_de_ejemplo();

        let electrocute = catalog.get(8112).expect("Electrocute en by_id");
        assert_eq!(electrocute.short_desc, "Hitting a champion with 3 separate attacks...");

        // Runa sin shortDesc en el payload → vacío, sin error.
        let trees: Vec<RuneTreeDto> = serde_json::from_value(json!([
            {
                "id": 8100,
                "key": "Domination",
                "name": "Domination",
                "slots": [
                    { "runes": [ { "id": 8112, "key": "Electrocute", "icon": "a.png", "name": "Electrocute" } ] }
                ]
            }
        ]))
        .expect("payload válido");
        let sin_desc = RuneCatalog::build(String::from("16.20.1"), trees);
        assert_eq!(sin_desc.get(8112).expect("runa").short_desc, "");
    }

    /// Deserialización tolerante: una runa sin `shortDesc`/`longDesc` (o con
    /// strings vacíos) parsea igual; los campos que no declaramos se ignoran.
    #[test]
    fn deserializacion_tolerante_sin_descripciones_y_strings_vacios() {
        let trees: Vec<RuneTreeDto> = serde_json::from_value(json!([
            {
                "id": 8400,
                "key": "Resolve",
                "icon": "",
                "name": "",
                "slots": [
                    { "runes": [ { "id": 8437, "key": "GraspOfTheUndying", "icon": "perk-images/x.png", "name": "Grasp of the Undying" } ] },
                    { "runes": [] }
                ]
            }
        ]))
        .expect("payload tolerante");

        let catalog = RuneCatalog::build(String::from("16.20.1"), trees);

        let grasp = catalog.get(8437).expect("Grasp en by_id");
        assert_eq!(grasp.name, "Grasp of the Undying");
        assert!(grasp.is_keystone);
        // Strings vacíos se toleran (quedan vacíos, no rompen el catálogo).
        assert_eq!(catalog.trees[0].name, "");
        assert_eq!(catalog.trees[0].icon_path, "");
        // Fila vacía se conserva tal cual llegó.
        assert!(catalog.trees[0].slots[1].runes.is_empty());
    }

    /// Runas sin id válido (ausente o 0) se descartan: no entran al `by_id`
    /// ni a los árboles, y el resto del catálogo queda intacto.
    #[test]
    fn runa_sin_id_valido_se_descarta() {
        let trees: Vec<RuneTreeDto> = serde_json::from_value(json!([
            {
                "id": 8100,
                "key": "Domination",
                "name": "Domination",
                "slots": [
                    {
                        "runes": [
                            { "id": 8112, "key": "Electrocute", "icon": "a.png", "name": "Electrocute" },
                            { "key": "SinId", "icon": "b.png", "name": "Sin id" },
                            { "id": 0, "key": "IdCero", "icon": "c.png", "name": "Id cero" }
                        ]
                    }
                ]
            }
        ]))
        .expect("payload válido");

        let catalog = RuneCatalog::build(String::from("16.20.1"), trees);

        assert_eq!(catalog.by_id.len(), 1);
        assert!(catalog.get(8112).is_some());
        assert!(catalog.get(0).is_none());
        assert_eq!(catalog.trees[0].slots[0].runes.len(), 1);
    }
}
