//! Import CSV générique : assistant de mapping colonnes → champs de schéma,
//! nettoyage, création automatique des séries et enrichissement des listes
//! de genres. Réutilisable pour tout CSV, pas seulement l'import initial.

use crate::index::Index;
use crate::model::{derive_code, unaccent, FieldType, Item, Schema, SelectOption, Series, Statut};
use crate::store::Library;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Cibles spéciales d'une colonne, en plus des clés de champs du schéma.
pub const TARGET_IGNORE: &str = "__ignore";
pub const TARGET_SERIE: &str = "__serie";
pub const TARGET_TOME: &str = "__tome";

#[derive(Debug, Serialize)]
pub struct CsvPreview {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub total_rows: usize,
}

#[derive(Debug, Deserialize)]
pub struct ColumnMapping {
    pub column: String,
    /// Clé de champ du schéma, ou `__ignore` / `__serie` / `__tome`.
    pub target: String,
    /// Transformation optionnelle : `nom_prenom` (« Le Lay, Delphine » →
    /// « Delphine Le Lay »).
    #[serde(default)]
    pub transform: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ImportOptions {
    /// Ne pas réimporter un objet déjà présent (même EAN, ou même
    /// titre + série + tome).
    #[serde(default = "yes")]
    pub skip_duplicates: bool,
    /// Ligne dont la série porte le même nom que le titre, sans tome →
    /// one-shot (pas de série créée).
    #[serde(default = "yes")]
    pub oneshot_if_serie_equals_titre: bool,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Default, Serialize)]
pub struct ImportReport {
    pub total_rows: usize,
    pub imported: usize,
    pub skipped_duplicates: usize,
    pub series_created: usize,
    pub genres_added: Vec<String>,
    pub errors: Vec<String>,
}

/// Minuscules sans accents, pour comparer des libellés.
fn normalize(s: &str) -> String {
    s.trim().chars().map(unaccent).collect::<String>().to_lowercase()
}

fn slugify(s: &str) -> String {
    let mut slug = String::new();
    for c in normalize(s).chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c);
        } else if !slug.ends_with('-') && !slug.is_empty() {
            slug.push('-');
        }
    }
    slug.trim_end_matches('-').to_string()
}

/// « Le Lay, Delphine » → « Delphine Le Lay » ; sans virgule, inchangé.
fn nom_prenom(s: &str) -> String {
    match s.split_once(',') {
        Some((nom, prenom)) if !prenom.trim().is_empty() => {
            format!("{} {}", prenom.trim(), nom.trim())
        }
        _ => s.trim().to_string(),
    }
}

fn open_reader(path: &str) -> Result<csv::Reader<std::fs::File>, String> {
    csv::ReaderBuilder::new()
        .flexible(true)
        .from_path(path)
        .map_err(|e| format!("lecture du CSV : {e}"))
}

pub fn preview_csv(path: &str) -> Result<CsvPreview, String> {
    let mut reader = open_reader(path)?;
    let headers: Vec<String> = reader
        .headers()
        .map_err(|e| e.to_string())?
        .iter()
        .map(|h| h.trim_start_matches('\u{feff}').to_string()) // BOM Excel
        .collect();
    let mut rows = Vec::new();
    let mut total_rows = 0usize;
    for record in reader.records() {
        let record = record.map_err(|e| e.to_string())?;
        if rows.len() < 5 {
            rows.push(record.iter().map(str::to_string).collect());
        }
        total_rows += 1;
    }
    Ok(CsvPreview { headers, rows, total_rows })
}

/// Clé de détection de doublon d'un objet : EAN si présent, sinon
/// titre + série + tome normalisés.
fn dedup_keys(
    schema: &Schema,
    fields: &BTreeMap<String, serde_json::Value>,
) -> (Option<String>, String) {
    let ean = fields
        .get("ean")
        .or_else(|| fields.get("isbn"))
        .and_then(|v| v.as_str())
        .map(|s| s.replace([' ', '-'], ""))
        .filter(|s| !s.is_empty());
    let titre = schema
        .title_field()
        .and_then(|f| fields.get(&f.key))
        .and_then(|v| v.as_str())
        .map(normalize)
        .unwrap_or_default();
    let (serie, tome) = schema
        .fields
        .iter()
        .find(|f| f.field_type == FieldType::SeriesRef)
        .and_then(|f| fields.get(&f.key))
        .map(|v| {
            (
                v.get("id").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                v.get("tome").and_then(|x| x.as_i64()).unwrap_or(-1),
            )
        })
        .unwrap_or_default();
    (ean, format!("{titre}|{serie}|{tome}"))
}

pub fn run_import(
    lib: &Library,
    index: &mut Index,
    collection: &str,
    path: &str,
    mappings: &[ColumnMapping],
    options: &ImportOptions,
) -> Result<ImportReport, String> {
    let mut schema = lib.load_schema(collection)?;
    let mut series = lib.load_series(collection)?;
    let mut counters = lib.load_counters(collection)?;
    let mut report = ImportReport::default();

    let serie_field_key = schema
        .fields
        .iter()
        .find(|f| f.field_type == FieldType::SeriesRef)
        .map(|f| f.key.clone());
    let titre_key = schema
        .title_field()
        .map(|f| f.key.clone())
        .ok_or("le schéma n'a pas de champ titre")?;

    // Doublons existants (l'import doit être rejouable sans dégât).
    let mut seen_ean: BTreeSet<String> = BTreeSet::new();
    let mut seen_triple: BTreeSet<String> = BTreeSet::new();
    if options.skip_duplicates {
        for item in lib.iter_items(collection)? {
            let (ean, triple) = dedup_keys(&schema, &item.fields);
            if let Some(e) = ean {
                seen_ean.insert(e);
            }
            seen_triple.insert(triple);
        }
    }

    // Index série : id → position, pour find-or-create.
    let mut series_by_id: HashMap<String, usize> =
        series.iter().enumerate().map(|(i, s)| (s.id.clone(), i)).collect();
    let mut schema_dirty = false;

    let mut reader = open_reader(path)?;
    let headers: Vec<String> = reader
        .headers()
        .map_err(|e| e.to_string())?
        .iter()
        .map(|h| h.trim_start_matches('\u{feff}').to_string()) // BOM Excel
        .collect();
    let column_index: HashMap<&str, usize> = headers
        .iter()
        .enumerate()
        .map(|(i, h)| (h.as_str(), i))
        .collect();
    for m in mappings {
        if m.target != TARGET_IGNORE && !column_index.contains_key(m.column.as_str()) {
            return Err(format!("colonne « {} » absente du CSV", m.column));
        }
    }

    index.bulk_begin()?;
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();

    for (row_num, record) in reader.records().enumerate() {
        let line = row_num + 2; // 1-indexé + ligne d'en-tête
        let record = match record {
            Ok(r) => r,
            Err(e) => {
                report.errors.push(format!("ligne {line} : {e}"));
                continue;
            }
        };
        report.total_rows += 1;

        let mut fields: BTreeMap<String, serde_json::Value> = BTreeMap::new();
        let mut serie_name: Option<String> = None;
        let mut serie_tome: Option<i64> = None;

        for m in mappings {
            if m.target == TARGET_IGNORE {
                continue;
            }
            let raw = record
                .get(column_index[m.column.as_str()])
                .unwrap_or("")
                .trim();
            if raw.is_empty() || raw == "<N/A>" {
                continue;
            }
            let value = match m.transform.as_deref() {
                Some("nom_prenom") => nom_prenom(raw),
                _ => raw.to_string(),
            };
            match m.target.as_str() {
                TARGET_SERIE => serie_name = Some(value),
                TARGET_TOME => match value.parse::<i64>() {
                    Ok(t) => serie_tome = Some(t),
                    Err(_) => report
                        .errors
                        .push(format!("ligne {line} : tome « {value} » non numérique, ignoré")),
                },
                key => {
                    let Some(def) = schema.field(key) else { continue };
                    if fields.contains_key(key) {
                        continue; // première colonne non vide gagnante
                    }
                    let json_value = match def.field_type {
                        FieldType::TextList | FieldType::Tags => {
                            serde_json::json!([value])
                        }
                        FieldType::Number | FieldType::Rating => {
                            match value.parse::<f64>() {
                                Ok(n) => serde_json::json!(n),
                                Err(_) => {
                                    report.errors.push(format!(
                                        "ligne {line} : « {value} » non numérique pour {key}, ignoré"
                                    ));
                                    continue;
                                }
                            }
                        }
                        FieldType::Boolean => {
                            serde_json::json!(matches!(
                                normalize(&value).as_str(),
                                "oui" | "true" | "1" | "x" | "vrai"
                            ))
                        }
                        FieldType::Select => {
                            // Rapprochement insensible aux accents/casse avec
                            // les valeurs connues ; sinon ajout au schéma avec
                            // un code de cote dérivé.
                            let canonical = def
                                .options
                                .iter()
                                .find(|o| normalize(&o.value) == normalize(&value))
                                .map(|o| o.value.clone());
                            match canonical {
                                Some(v) => serde_json::json!(v),
                                None => {
                                    let def_mut = schema
                                        .fields
                                        .iter_mut()
                                        .find(|f| f.key == key)
                                        .expect("champ vérifié ci-dessus");
                                    // Code unique parmi les options existantes,
                                    // sinon deux genres partageraient un code
                                    // de cote ambigu.
                                    let taken: BTreeSet<String> = def_mut
                                        .options
                                        .iter()
                                        .map(|o| o.effective_code())
                                        .collect();
                                    let mut code = derive_code(&value);
                                    let mut n = 2;
                                    while taken.contains(&code) {
                                        code = format!("{}{n}", derive_code(&value));
                                        n += 1;
                                    }
                                    def_mut.options.push(SelectOption {
                                        value: value.clone(),
                                        code: Some(code),
                                    });
                                    schema_dirty = true;
                                    report.genres_added.push(value.clone());
                                    serde_json::json!(value)
                                }
                            }
                        }
                        _ => serde_json::json!(value),
                    };
                    fields.insert(key.to_string(), json_value);
                }
            }
        }

        // Résolution série / one-shot.
        if let Some(name) = serie_name {
            let titre = fields.get(&titre_key).and_then(|v| v.as_str()).unwrap_or("");
            let is_oneshot = options.oneshot_if_serie_equals_titre
                && serie_tome.is_none()
                && normalize(titre) == normalize(&name);
            if !is_oneshot {
                if let Some(serie_key) = &serie_field_key {
                    let id = slugify(&name);
                    if !id.is_empty() {
                        if !series_by_id.contains_key(&id) {
                            series.push(Series { id: id.clone(), nom: name.clone(), terminee: false });
                            series_by_id.insert(id.clone(), series.len() - 1);
                            report.series_created += 1;
                        }
                        let mut obj = serde_json::json!({ "id": id });
                        if let Some(t) = serie_tome {
                            obj["tome"] = serde_json::json!(t);
                        }
                        fields.insert(serie_key.clone(), obj);
                    }
                }
            }
        }

        if fields
            .get(&titre_key)
            .and_then(|v| v.as_str())
            .map(|s| s.trim().is_empty())
            .unwrap_or(true)
        {
            report.errors.push(format!("ligne {line} : titre manquant, ligne ignorée"));
            continue;
        }

        if options.skip_duplicates {
            let (ean, triple) = dedup_keys(&schema, &fields);
            let dup = ean.as_ref().map(|e| seen_ean.contains(e)).unwrap_or(false)
                || seen_triple.contains(&triple);
            if dup {
                report.skipped_duplicates += 1;
                continue;
            }
            if let Some(e) = ean {
                seen_ean.insert(e);
            }
            seen_triple.insert(triple);
        }

        counters.next_id += 1;
        let mut item = Item {
            id: format!("{}-{:05}", schema.id_prefix, counters.next_id),
            cote: None,
            statut: Statut::Possede,
            emplacement: None,
            etiquette: None,
            date_ajout: today.clone(),
            fields,
        };
        item.cote = Some(Library::allocate_cote(&schema, &item, &mut counters));
        lib.save_item(collection, &item)?;
        index.upsert_item(collection, &schema, &series, &item)?;
        report.imported += 1;
    }

    index.bulk_commit()?;
    series.sort_by(|a, b| a.nom.to_lowercase().cmp(&b.nom.to_lowercase()));
    lib.save_series(collection, &series)?;
    lib.save_counters(collection, &counters)?;
    if schema_dirty {
        lib.save_schema(collection, &schema)?;
    }
    report.genres_added.sort();
    report.genres_added.dedup();
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const CSV: &str = "\
\"Serie\",\"Titre\",\"Tome\",\"ISBN\",\"Genre\",\"Scenariste\",\"Dessinateur\",\"Editeur\",\"Collection\",\"Date parution\",\"EAN\"
\"Lastman\",\"Lastman, Tome 1\",\"1\",\"\",\"Science-Fiction\",\"Balak\",\"Vivès, Bastien\",\"Casterman\",\"KSTR\",\"2013-03-20\",\"9782203064669\"
\"Lastman\",\"Lastman, Tome 2\",\"2\",\"\",\"Science-Fiction\",\"Balak\",\"Vivès, Bastien\",\"Casterman\",\"KSTR\",\"2013-06-12\",\"9782203069473\"
\"Le Rapport de Brodeck\",\"Le Rapport de Brodeck\",\"\",\"\",\"Western\",\"Larcenet, Manu\",\"Larcenet, Manu\",\"Dargaud\",\"<N/A>\",\"2015-04-03\",\"9782205073010\"
\"100 maisons\",\"100 maisons\",\"\",\"\",\"Erotique\",\"Le Lay, Delphine\",\"Horellou, Alexis\",\"Delcourt\",\"Encrages\",\"2015-02-04\",\"\"
";

    fn mappings() -> Vec<ColumnMapping> {
        let m = |column: &str, target: &str, transform: Option<&str>| ColumnMapping {
            column: column.into(),
            target: target.into(),
            transform: transform.map(String::from),
        };
        vec![
            m("Serie", TARGET_SERIE, None),
            m("Titre", "titre", None),
            m("Tome", TARGET_TOME, None),
            m("EAN", "ean", None),
            m("ISBN", "ean", None),
            m("Genre", "genre", None),
            m("Scenariste", "scenariste", Some("nom_prenom")),
            m("Dessinateur", "dessinateur", Some("nom_prenom")),
            m("Editeur", "editeur", None),
            m("Collection", "collection_editeur", None),
            m("Date parution", "date_parution", None),
        ]
    }

    fn setup() -> (tempfile::TempDir, Library, Index, String) {
        let dir = tempfile::tempdir().unwrap();
        let lib = Library::create(dir.path().join("biblio")).unwrap();
        let index = Index::open(&dir.path().join("index.sqlite")).unwrap();
        let csv_path = dir.path().join("import.csv");
        std::fs::File::create(&csv_path)
            .unwrap()
            .write_all(CSV.as_bytes())
            .unwrap();
        let path = csv_path.to_string_lossy().into_owned();
        (dir, lib, index, path)
    }

    fn options() -> ImportOptions {
        ImportOptions { skip_duplicates: true, oneshot_if_serie_equals_titre: true }
    }

    #[test]
    fn import_creates_items_series_and_genres() {
        let (_dir, lib, mut index, path) = setup();
        let report = run_import(&lib, &mut index, "bd", &path, &mappings(), &options()).unwrap();

        assert_eq!(report.total_rows, 4);
        assert_eq!(report.imported, 4);
        assert!(report.errors.is_empty(), "{:?}", report.errors);

        // Une seule série créée : Lastman. Les 2 one-shots (série == titre
        // sans tome) n'en créent pas.
        assert_eq!(report.series_created, 1);
        let series = lib.load_series("bd").unwrap();
        assert_eq!(series.len(), 1);
        assert_eq!(series[0].nom, "Lastman");

        // Genre inconnu ajouté au schéma avec code dérivé ; « Erotique » sans
        // accent rapproché de « Érotique » existant (pas de doublon).
        assert_eq!(report.genres_added, vec!["Western".to_string()]);
        let schema = lib.load_schema("bd").unwrap();
        let genre = schema.field("genre").unwrap();
        assert!(genre.options.iter().any(|o| o.value == "Western"));
        assert_eq!(genre.options.iter().filter(|o| normalize(&o.value).starts_with("erotique")).count(), 1);

        // Transformation Nom, Prénom et cote générée.
        let items = lib.iter_items("bd").unwrap();
        let t1 = items.iter().find(|i| i.fields["titre"] == serde_json::json!("Lastman, Tome 1")).unwrap();
        assert_eq!(t1.fields["dessinateur"], serde_json::json!(["Bastien Vivès"]));
        assert_eq!(t1.cote.as_deref(), Some("2013-SF-0001"));
        assert_eq!(t1.fields["serie"]["tome"], serde_json::json!(1));
        // Le one-shot « Erotique » est rapproché de la valeur canonique.
        let maisons = items.iter().find(|i| i.fields["titre"] == serde_json::json!("100 maisons")).unwrap();
        assert_eq!(maisons.fields["genre"], serde_json::json!("Érotique"));
        assert!(maisons.fields.get("serie").is_none());
        // <N/A> nettoyé.
        let brodeck = items.iter().find(|i| i.fields["titre"] == serde_json::json!("Le Rapport de Brodeck")).unwrap();
        assert!(brodeck.fields.get("collection_editeur").is_none());

        // Recherche fonctionnelle après import (index cohérent).
        let hits = index
            .search("bd", "lastman", &crate::index::SearchFilters::default(), None, false, 50, 0)
            .unwrap();
        assert_eq!(hits.len(), 2);
    }

    #[test]
    fn reimport_skips_all_duplicates() {
        let (_dir, lib, mut index, path) = setup();
        run_import(&lib, &mut index, "bd", &path, &mappings(), &options()).unwrap();
        let second = run_import(&lib, &mut index, "bd", &path, &mappings(), &options()).unwrap();
        assert_eq!(second.imported, 0);
        assert_eq!(second.skipped_duplicates, 4);
        assert_eq!(second.series_created, 0);
        assert_eq!(lib.iter_items("bd").unwrap().len(), 4);
    }

    #[test]
    fn missing_column_is_an_error() {
        let (_dir, lib, mut index, path) = setup();
        let bad = vec![ColumnMapping {
            column: "Inexistante".into(),
            target: "titre".into(),
            transform: None,
        }];
        let err = run_import(&lib, &mut index, "bd", &path, &bad, &options()).unwrap_err();
        assert!(err.contains("Inexistante"), "{err}");
    }

    /// Import réel : REAL_LIB + REAL_CSV définis.
    /// `REAL_LIB=… REAL_CSV=… cargo test real_import -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_import() {
        let root = std::env::var("REAL_LIB").expect("REAL_LIB non défini");
        let csv = std::env::var("REAL_CSV").expect("REAL_CSV non défini");
        let lib = Library::open(root).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut index = Index::open(&dir.path().join("index.sqlite")).unwrap();
        let report = run_import(&lib, &mut index, "bd", &csv, &mappings(), &options()).unwrap();
        println!(
            "importés: {} | doublons: {} | séries: {} | genres ajoutés: {:?} | erreurs: {}",
            report.imported,
            report.skipped_duplicates,
            report.series_created,
            report.genres_added,
            report.errors.len()
        );
    }

    /// Répétition générale sur le CSV réel (chemin via REAL_CSV), dans une
    /// bibliothèque temporaire. `cargo test dry_run -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn dry_run_on_real_csv() {
        let path = std::env::var("REAL_CSV").expect("REAL_CSV non défini");
        let dir = tempfile::tempdir().unwrap();
        let lib = Library::create(dir.path().join("biblio")).unwrap();
        let mut index = Index::open(&dir.path().join("index.sqlite")).unwrap();
        let start = std::time::Instant::now();
        let report =
            run_import(&lib, &mut index, "bd", &path, &mappings(), &options()).unwrap();
        println!("durée: {:?}", start.elapsed());
        println!(
            "lignes: {} | importés: {} | doublons: {} | séries: {} | genres ajoutés: {:?}",
            report.total_rows,
            report.imported,
            report.skipped_duplicates,
            report.series_created,
            report.genres_added
        );
        for e in &report.errors {
            println!("ERREUR {e}");
        }
        // Vérifications de cohérence sur données réelles.
        assert_eq!(report.total_rows, report.imported + report.skipped_duplicates);
        let hits = index
            .search("bd", "", &crate::index::SearchFilters::default(), None, false, 10_000, 0)
            .unwrap();
        assert_eq!(hits.len() as usize, report.imported);
    }

    #[test]
    fn genre_code_collisions_get_unique_codes() {
        let (dir, lib, mut index, _) = setup();
        let csv = "\
\"Titre\",\"Genre\"
\"Album A\",\"Aventures Historiques\"
\"Album B\",\"Aventures Fantastiques\"
";
        let path = dir.path().join("genres.csv");
        std::fs::File::create(&path).unwrap().write_all(csv.as_bytes()).unwrap();
        let maps = vec![
            ColumnMapping { column: "Titre".into(), target: "titre".into(), transform: None },
            ColumnMapping { column: "Genre".into(), target: "genre".into(), transform: None },
        ];
        run_import(&lib, &mut index, "bd", &path.to_string_lossy(), &maps, &options()).unwrap();

        let schema = lib.load_schema("bd").unwrap();
        let genre = schema.field("genre").unwrap();
        let codes: Vec<String> = genre.options.iter().map(|o| o.effective_code()).collect();
        let mut deduped = codes.clone();
        deduped.sort();
        deduped.dedup();
        assert_eq!(codes.len(), deduped.len(), "codes non uniques : {codes:?}");
    }

    #[test]
    fn helpers() {
        assert_eq!(nom_prenom("Le Lay, Delphine"), "Delphine Le Lay");
        assert_eq!(nom_prenom("Balak"), "Balak");
        assert_eq!(slugify("Dragon Ball Super"), "dragon-ball-super");
        assert_eq!(slugify("L'Épée d'Ardenois !"), "l-epee-d-ardenois");
    }
}
