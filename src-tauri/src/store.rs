//! Couche de stockage : les fichiers YAML sont l'unique source de vérité.
//!
//! Arborescence d'une bibliothèque :
//! ```text
//! <racine>/collections/<slug>/_schema.yaml
//! <racine>/collections/<slug>/_series.yaml
//! <racine>/collections/<slug>/_counters.yaml
//! <racine>/collections/<slug>/<ID>.yaml
//! <racine>/images/<slug>/<ID>.webp
//! ```

use crate::defaults::DEFAULT_SCHEMAS;
use crate::model::{derive_code, Counters, Item, Schema, Series, Statut};
use std::fs;
use std::path::{Path, PathBuf};

pub struct Library {
    pub root: PathBuf,
}

/// Changement de cote issu d'une régénération (étiquette à refaire).
#[derive(Debug, serde::Serialize)]
pub struct CoteChange {
    pub id: String,
    pub old: Option<String>,
    pub new: String,
}

fn read_yaml<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    let text = fs::read_to_string(path)
        .map_err(|e| format!("lecture {} : {e}", path.display()))?;
    serde_yaml::from_str(&text).map_err(|e| format!("YAML invalide {} : {e}", path.display()))
}

/// Écriture atomique : fichier temporaire puis renommage, pour ne jamais
/// laisser un YAML tronqué en cas d'interruption.
fn write_yaml<T: serde::Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let text = serde_yaml::to_string(value).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("yaml.tmp");
    fs::write(&tmp, text).map_err(|e| format!("écriture {} : {e}", tmp.display()))?;
    fs::rename(&tmp, path).map_err(|e| format!("renommage {} : {e}", path.display()))
}

impl Library {
    /// Ouvre une bibliothèque existante.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, String> {
        let root = root.into();
        if !root.join("collections").is_dir() {
            return Err(format!(
                "{} n'est pas une bibliothèque (dossier collections/ absent)",
                root.display()
            ));
        }
        Ok(Self { root })
    }

    /// Crée une bibliothèque neuve avec les cinq collections fixes.
    pub fn create(root: impl Into<PathBuf>) -> Result<Self, String> {
        let root = root.into();
        if root.join("collections").exists() {
            return Err("une bibliothèque existe déjà à cet emplacement".into());
        }
        for (slug, yaml) in DEFAULT_SCHEMAS {
            let schema: Schema = serde_yaml::from_str(yaml)
                .map_err(|e| format!("schéma par défaut {slug} : {e}"))?;
            let dir = root.join("collections").join(slug);
            fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            write_yaml(&dir.join("_schema.yaml"), &schema)?;
            fs::create_dir_all(root.join("images").join(slug)).map_err(|e| e.to_string())?;
        }
        Ok(Self { root })
    }

    fn coll_dir(&self, slug: &str) -> PathBuf {
        self.root.join("collections").join(slug)
    }

    fn item_path(&self, slug: &str, id: &str) -> PathBuf {
        self.coll_dir(slug).join(format!("{id}.yaml"))
    }

    /// Slugs des collections, triés par nom de dossier.
    pub fn collections(&self) -> Result<Vec<String>, String> {
        let mut slugs = Vec::new();
        let dir = self.root.join("collections");
        for entry in fs::read_dir(&dir).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            if entry.path().join("_schema.yaml").is_file() {
                slugs.push(entry.file_name().to_string_lossy().into_owned());
            }
        }
        slugs.sort();
        Ok(slugs)
    }

    pub fn load_schema(&self, slug: &str) -> Result<Schema, String> {
        read_yaml(&self.coll_dir(slug).join("_schema.yaml"))
    }

    pub fn save_schema(&self, slug: &str, schema: &Schema) -> Result<(), String> {
        write_yaml(&self.coll_dir(slug).join("_schema.yaml"), schema)
    }

    /// Crée une nouvelle collection (custom) à partir d'un schéma.
    pub fn create_collection(&self, slug: &str, schema: &Schema) -> Result<(), String> {
        if slug.is_empty()
            || !slug
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            return Err("slug invalide (minuscules, chiffres et tirets uniquement)".into());
        }
        let dir = self.coll_dir(slug);
        if dir.exists() {
            return Err(format!("la collection {slug} existe déjà"));
        }
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        fs::create_dir_all(self.root.join("images").join(slug)).map_err(|e| e.to_string())?;
        write_yaml(&dir.join("_schema.yaml"), schema)
    }

    /// Supprime une collection entière (fiches, schéma, séries, images).
    /// Récupérable via l'historique Git de la bibliothèque.
    pub fn delete_collection(&self, slug: &str) -> Result<usize, String> {
        let dir = self.coll_dir(slug);
        if !dir.join("_schema.yaml").is_file() {
            return Err(format!("la collection « {slug} » n'existe pas"));
        }
        let count = self.list_item_ids(slug)?.len();
        fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
        let images = self.root.join("images").join(slug);
        if images.exists() {
            fs::remove_dir_all(&images).map_err(|e| e.to_string())?;
        }
        Ok(count)
    }

    pub fn load_series(&self, slug: &str) -> Result<Vec<Series>, String> {
        let path = self.coll_dir(slug).join("_series.yaml");
        if !path.exists() {
            return Ok(Vec::new());
        }
        read_yaml(&path)
    }

    pub fn save_series(&self, slug: &str, series: &[Series]) -> Result<(), String> {
        write_yaml(&self.coll_dir(slug).join("_series.yaml"), &series.to_vec())
    }

    pub fn upsert_series(&self, slug: &str, one: Series) -> Result<(), String> {
        let mut all = self.load_series(slug)?;
        match all.iter_mut().find(|s| s.id == one.id) {
            Some(existing) => *existing = one,
            None => all.push(one),
        }
        all.sort_by(|a, b| a.nom.to_lowercase().cmp(&b.nom.to_lowercase()));
        self.save_series(slug, &all)
    }

    pub(crate) fn load_counters(&self, slug: &str) -> Result<Counters, String> {
        let path = self.coll_dir(slug).join("_counters.yaml");
        if path.exists() {
            read_yaml(&path)
        } else {
            // Reconstruction par balayage : robustesse si le fichier manque.
            self.rebuild_counters(slug)
        }
    }

    pub(crate) fn save_counters(&self, slug: &str, counters: &Counters) -> Result<(), String> {
        write_yaml(&self.coll_dir(slug).join("_counters.yaml"), counters)
    }

    /// Reconstruit les compteurs en balayant les fichiers objets.
    pub fn rebuild_counters(&self, slug: &str) -> Result<Counters, String> {
        let mut counters = Counters::default();
        for item in self.iter_items(slug)? {
            if let Some(seq) = item
                .id
                .rsplit('-')
                .next()
                .and_then(|s| s.parse::<u64>().ok())
            {
                counters.next_id = counters.next_id.max(seq);
            }
            if let Some(cote) = &item.cote {
                if let Some((prefix, seq)) = cote.rsplit_once('-') {
                    if let Ok(seq) = seq.parse::<u64>() {
                        let entry = counters.cotes.entry(prefix.to_string()).or_insert(0);
                        *entry = (*entry).max(seq);
                    }
                }
            }
        }
        Ok(counters)
    }

    pub fn list_item_ids(&self, slug: &str) -> Result<Vec<String>, String> {
        let mut ids = Vec::new();
        for entry in fs::read_dir(self.coll_dir(slug)).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if let Some(stem) = name.strip_suffix(".yaml") {
                if !stem.starts_with('_') {
                    ids.push(stem.to_string());
                }
            }
        }
        ids.sort();
        Ok(ids)
    }

    pub fn iter_items(&self, slug: &str) -> Result<Vec<Item>, String> {
        self.list_item_ids(slug)?
            .iter()
            .map(|id| self.load_item(slug, id))
            .collect()
    }

    pub fn load_item(&self, slug: &str, id: &str) -> Result<Item, String> {
        read_yaml(&self.item_path(slug, id))
    }

    pub fn save_item(&self, slug: &str, item: &Item) -> Result<(), String> {
        write_yaml(&self.item_path(slug, &item.id), item)
    }

    pub fn delete_item(&self, slug: &str, id: &str) -> Result<(), String> {
        fs::remove_file(self.item_path(slug, id)).map_err(|e| e.to_string())
    }

    /// Crée un objet : alloue l'ID interne, génère la cote si l'objet est
    /// possédé, écrit le fichier.
    pub fn create_item(
        &self,
        slug: &str,
        statut: Statut,
        fields: std::collections::BTreeMap<String, serde_json::Value>,
    ) -> Result<Item, String> {
        let schema = self.load_schema(slug)?;
        self.validate_fields(&schema, &fields)?;
        let mut counters = self.load_counters(slug)?;
        counters.next_id += 1;
        let id = format!("{}-{:05}", schema.id_prefix, counters.next_id);

        let mut item = Item {
            id,
            cote: None,
            statut,
            emplacement: None,
            date_ajout: chrono::Local::now().format("%Y-%m-%d").to_string(),
            fields,
        };
        if statut == Statut::Possede {
            item.cote = Some(Self::allocate_cote(&schema, &item, &mut counters));
        }
        self.save_counters(slug, &counters)?;
        self.save_item(slug, &item)?;
        Ok(item)
    }

    /// Met à jour un objet. Régénère la cote si nécessaire :
    /// - passage souhaité → possédé (première attribution) ;
    /// - année ou genre modifiés alors qu'une cote existe (étiquette à refaire).
    pub fn update_item(
        &self,
        slug: &str,
        id: &str,
        statut: Statut,
        emplacement: Option<String>,
        fields: std::collections::BTreeMap<String, serde_json::Value>,
    ) -> Result<Item, String> {
        let schema = self.load_schema(slug)?;
        self.validate_fields(&schema, &fields)?;
        let mut item = self.load_item(slug, id)?;
        item.statut = statut;
        item.emplacement = emplacement.filter(|e| !e.trim().is_empty());
        item.fields = fields;

        let expected_prefix = Self::cote_prefix(&schema, &item);
        let needs_new_cote = match (&item.cote, &expected_prefix, statut) {
            (_, _, Statut::Souhaite) => false,
            (None, Some(_), Statut::Possede) => true,
            (Some(current), Some(prefix), Statut::Possede) => {
                !current.starts_with(&format!("{prefix}-"))
            }
            _ => false,
        };
        if needs_new_cote {
            let mut counters = self.load_counters(slug)?;
            item.cote = Some(Self::allocate_cote(&schema, &item, &mut counters));
            self.save_counters(slug, &counters)?;
        }
        self.save_item(slug, &item)?;
        Ok(item)
    }

    fn validate_fields(
        &self,
        schema: &Schema,
        fields: &std::collections::BTreeMap<String, serde_json::Value>,
    ) -> Result<(), String> {
        for def in schema.fields.iter().filter(|f| f.required) {
            let missing = match fields.get(&def.key) {
                None | Some(serde_json::Value::Null) => true,
                Some(serde_json::Value::String(s)) => s.trim().is_empty(),
                Some(_) => false,
            };
            if missing {
                return Err(format!("le champ « {} » est obligatoire", def.label));
            }
        }
        Ok(())
    }

    /// Préfixe `AAAA-GENRE` de la cote, ou None si le schéma n'a pas de
    /// configuration de cote.
    fn cote_prefix(schema: &Schema, item: &Item) -> Option<String> {
        let config = schema.cote.as_ref()?;
        let year = item
            .fields
            .get(&config.year_field)
            .and_then(|v| v.as_str())
            .and_then(|s| {
                let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
                (digits.len() == 4).then_some(digits)
            })
            .unwrap_or_else(|| "0000".to_string());
        let genre_value = item
            .fields
            .get(&config.genre_field)
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let code = schema
            .field(&config.genre_field)
            .and_then(|f| f.options.iter().find(|o| o.value == genre_value))
            .map(|o| o.effective_code())
            .unwrap_or_else(|| derive_code(genre_value));
        Some(format!("{year}-{code}"))
    }

    /// Régénère la cote des objets possédés dont l'année ou le genre ne
    /// correspondent plus au préfixe de leur cote actuelle (après correction
    /// de données ou changement de codes dans le schéma). Renvoie les
    /// changements — la future liste « étiquettes à refaire ».
    pub fn regenerate_stale_cotes(&self, slug: &str) -> Result<Vec<CoteChange>, String> {
        let schema = self.load_schema(slug)?;
        let mut counters = self.load_counters(slug)?;
        let mut changes = Vec::new();
        for mut item in self.iter_items(slug)? {
            if item.statut != Statut::Possede {
                continue;
            }
            let Some(prefix) = Self::cote_prefix(&schema, &item) else { continue };
            let stale = item
                .cote
                .as_ref()
                .map(|c| !c.starts_with(&format!("{prefix}-")))
                .unwrap_or(true);
            if stale {
                let old = item.cote.clone();
                item.cote = Some(Self::allocate_cote(&schema, &item, &mut counters));
                self.save_item(slug, &item)?;
                changes.push(CoteChange {
                    id: item.id.clone(),
                    old,
                    new: item.cote.clone().unwrap(),
                });
            }
        }
        self.save_counters(slug, &counters)?;
        Ok(changes)
    }

    pub(crate) fn allocate_cote(schema: &Schema, item: &Item, counters: &mut Counters) -> String {
        let prefix = match Self::cote_prefix(schema, item) {
            Some(p) => p,
            // Pas de config de cote : la cote reprend l'ID interne.
            None => return item.id.clone(),
        };
        let seq = counters.cotes.entry(prefix.clone()).or_insert(0);
        *seq += 1;
        format!("{prefix}-{:04}", seq)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::BTreeMap;

    fn fields(pairs: &[(&str, serde_json::Value)]) -> BTreeMap<String, serde_json::Value> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    #[test]
    fn create_library_seeds_fixed_collections() {
        let dir = tempfile::tempdir().unwrap();
        let lib = Library::create(dir.path().join("biblio")).unwrap();
        assert_eq!(
            lib.collections().unwrap(),
            vec!["bd", "cd", "dvd", "jeux-video", "livres"]
        );
        let schema = lib.load_schema("bd").unwrap();
        assert_eq!(schema.id_prefix, "BD");
    }

    #[test]
    fn create_item_allocates_id_and_cote() {
        let dir = tempfile::tempdir().unwrap();
        let lib = Library::create(dir.path().join("biblio")).unwrap();
        let item = lib
            .create_item(
                "bd",
                Statut::Possede,
                fields(&[
                    ("titre", json!("Lastman T1")),
                    ("genre", json!("Science-Fiction")),
                    ("date_parution", json!("2013-03-20")),
                ]),
            )
            .unwrap();
        assert_eq!(item.id, "BD-00001");
        assert_eq!(item.cote.as_deref(), Some("2013-SF-0001"));

        // Deuxième objet même année/genre → séquence incrémentée.
        let item2 = lib
            .create_item(
                "bd",
                Statut::Possede,
                fields(&[
                    ("titre", json!("Lastman T2")),
                    ("genre", json!("Science-Fiction")),
                    ("date_parution", json!("2013-06-12")),
                ]),
            )
            .unwrap();
        assert_eq!(item2.id, "BD-00002");
        assert_eq!(item2.cote.as_deref(), Some("2013-SF-0002"));
    }

    #[test]
    fn wishlist_item_gets_cote_only_when_owned() {
        let dir = tempfile::tempdir().unwrap();
        let lib = Library::create(dir.path().join("biblio")).unwrap();
        let item = lib
            .create_item("bd", Statut::Souhaite, fields(&[("titre", json!("Akira T1"))]))
            .unwrap();
        assert!(item.cote.is_none());

        let owned = lib
            .update_item(
                "bd",
                &item.id,
                Statut::Possede,
                None,
                fields(&[
                    ("titre", json!("Akira T1")),
                    ("genre", json!("Mangas - Seinen")),
                    ("date_parution", json!("1990-03-01")),
                ]),
            )
            .unwrap();
        assert_eq!(owned.cote.as_deref(), Some("1990-SEINEN-0001"));
    }

    #[test]
    fn changing_genre_regenerates_cote() {
        let dir = tempfile::tempdir().unwrap();
        let lib = Library::create(dir.path().join("biblio")).unwrap();
        let item = lib
            .create_item(
                "bd",
                Statut::Possede,
                fields(&[
                    ("titre", json!("Universal War One")),
                    ("genre", json!("Fantasy")),
                    ("date_parution", json!("1998-01-01")),
                ]),
            )
            .unwrap();
        assert_eq!(item.cote.as_deref(), Some("1998-FANT-0001"));

        let updated = lib
            .update_item(
                "bd",
                &item.id,
                Statut::Possede,
                None,
                fields(&[
                    ("titre", json!("Universal War One")),
                    ("genre", json!("Science-Fiction")),
                    ("date_parution", json!("1998-01-01")),
                ]),
            )
            .unwrap();
        assert_eq!(updated.cote.as_deref(), Some("1998-SF-0001"));
        assert_eq!(updated.id, item.id, "l'ID interne ne change jamais");
    }

    #[test]
    fn unknown_year_falls_back_to_0000() {
        let dir = tempfile::tempdir().unwrap();
        let lib = Library::create(dir.path().join("biblio")).unwrap();
        let item = lib
            .create_item(
                "bd",
                Statut::Possede,
                fields(&[("titre", json!("Mystère")), ("genre", json!("Humour"))]),
            )
            .unwrap();
        assert_eq!(item.cote.as_deref(), Some("0000-HUM-0001"));
    }

    #[test]
    fn required_field_is_enforced() {
        let dir = tempfile::tempdir().unwrap();
        let lib = Library::create(dir.path().join("biblio")).unwrap();
        let err = lib
            .create_item("bd", Statut::Possede, fields(&[]))
            .unwrap_err();
        assert!(err.contains("obligatoire"), "{err}");
    }

    #[test]
    fn counters_rebuild_matches_reality() {
        let dir = tempfile::tempdir().unwrap();
        let lib = Library::create(dir.path().join("biblio")).unwrap();
        for i in 0..3 {
            lib.create_item(
                "bd",
                Statut::Possede,
                fields(&[
                    ("titre", json!(format!("Tome {i}"))),
                    ("genre", json!("Humour")),
                    ("date_parution", json!("2020-01-01")),
                ]),
            )
            .unwrap();
        }
        // Suppression du fichier de compteurs → reconstruction par balayage.
        std::fs::remove_file(lib.root.join("collections/bd/_counters.yaml")).unwrap();
        let item = lib
            .create_item(
                "bd",
                Statut::Possede,
                fields(&[
                    ("titre", json!("Tome 4")),
                    ("genre", json!("Humour")),
                    ("date_parution", json!("2020-01-01")),
                ]),
            )
            .unwrap();
        assert_eq!(item.id, "BD-00004");
        assert_eq!(item.cote.as_deref(), Some("2020-HUM-0004"));
    }

    #[test]
    fn regenerate_stale_cotes_after_code_change() {
        let dir = tempfile::tempdir().unwrap();
        let lib = Library::create(dir.path().join("biblio")).unwrap();
        let item = lib
            .create_item(
                "bd",
                Statut::Possede,
                fields(&[
                    ("titre", json!("Thorgal T1")),
                    ("genre", json!("Fantasy")),
                    ("date_parution", json!("1980-03-01")),
                ]),
            )
            .unwrap();
        assert_eq!(item.cote.as_deref(), Some("1980-FANT-0001"));

        // L'utilisateur change le code du genre Fantasy dans le schéma.
        let mut schema = lib.load_schema("bd").unwrap();
        let genre = schema.fields.iter_mut().find(|f| f.key == "genre").unwrap();
        genre.options.iter_mut().find(|o| o.value == "Fantasy").unwrap().code =
            Some("FSY".into());
        lib.save_schema("bd", &schema).unwrap();

        let changes = lib.regenerate_stale_cotes("bd").unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].old.as_deref(), Some("1980-FANT-0001"));
        assert_eq!(changes[0].new, "1980-FSY-0001");
        assert_eq!(
            lib.load_item("bd", &item.id).unwrap().cote.as_deref(),
            Some("1980-FSY-0001")
        );

        // Idempotent : plus rien à régénérer.
        assert!(lib.regenerate_stale_cotes("bd").unwrap().is_empty());
    }

    /// Maintenance : retire des fiches CD les champs issus du premier
    /// enrichissement fautif (couverture/label/ean — absents du CSV source).
    /// `REAL_LIB=… cargo test wipe_cd -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn wipe_cd_enrichment_real() {
        let root = std::env::var("REAL_LIB").expect("REAL_LIB non défini");
        let lib = Library::open(root).unwrap();
        let mut fiches = 0;
        for id in lib.list_item_ids("cd").unwrap() {
            let mut item = lib.load_item("cd", &id).unwrap();
            let mut removed = false;
            for key in ["couverture", "label", "ean"] {
                removed |= item.fields.remove(key).is_some();
            }
            if removed {
                lib.save_item("cd", &item).unwrap();
                fiches += 1;
            }
        }
        println!("{fiches} fiches CD nettoyées");
    }

    /// Maintenance sur bibliothèque réelle (chemin via REAL_LIB) :
    /// `REAL_LIB=… cargo test regen_real -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn regen_real_library() {
        let root = std::env::var("REAL_LIB").expect("REAL_LIB non défini");
        let lib = Library::open(root).unwrap();
        let changes = lib.regenerate_stale_cotes("bd").unwrap();
        println!("{} cotes régénérées", changes.len());
        for c in &changes {
            println!("  {} : {} -> {}", c.id, c.old.as_deref().unwrap_or("(aucune)"), c.new);
        }
    }

    #[test]
    fn series_registry_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let lib = Library::create(dir.path().join("biblio")).unwrap();
        lib.upsert_series(
            "bd",
            Series { id: "lastman".into(), nom: "Lastman".into(), terminee: true },
        )
        .unwrap();
        let all = lib.load_series("bd").unwrap();
        assert_eq!(all.len(), 1);
        assert!(all[0].terminee);
    }
}
