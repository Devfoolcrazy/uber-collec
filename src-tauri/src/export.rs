//! Export d'une collection (avec recherche/filtres appliqués) vers CSV ou
//! JSON. Les en-têtes CSV reprennent les clés des champs du schéma : un
//! export se réimporte tel quel via l'assistant d'import.

use crate::index::{Index, IndexedItem, SearchFilters};
use crate::model::{FieldType, Schema};
use crate::store::Library;
use std::path::Path;

/// Colonnes système, avant les champs du schéma.
const SYSTEM_COLS: &[&str] = &["id", "cote", "statut", "emplacement", "date_ajout"];

fn cell(value: Option<&serde_json::Value>) -> String {
    match value {
        None | Some(serde_json::Value::Null) => String::new(),
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Array(a)) => a
            .iter()
            .map(|v| v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string()))
            .collect::<Vec<_>>()
            .join(" ; "),
        Some(serde_json::Value::Bool(b)) => if *b { "oui" } else { "" }.to_string(),
        Some(other) => other.to_string(),
    }
}

fn rows_for(
    lib: &Library,
    idx: &Index,
    collection: &str,
    query: &str,
    filters: &SearchFilters,
) -> Result<(Schema, Vec<IndexedItem>), String> {
    let schema = lib.load_schema(collection)?;
    let rows = idx.search(collection, query, filters, None, false, u32::MAX, 0)?;
    Ok((schema, rows))
}

pub fn export(
    lib: &Library,
    idx: &Index,
    collection: &str,
    path: &Path,
    query: &str,
    filters: &SearchFilters,
) -> Result<u64, String> {
    let (schema, rows) = rows_for(lib, idx, collection, query, filters)?;
    let json = path
        .extension()
        .is_some_and(|e| e.to_ascii_lowercase() == "json");
    if json {
        export_json(&schema, &rows, path)
    } else {
        export_csv(&schema, &rows, path)
    }
}

/// Champs du schéma exportés, dans l'ordre du schéma. Le champ série devient
/// deux colonnes lisibles et réimportables : `serie` (nom) et `tome`.
fn schema_columns(schema: &Schema) -> Vec<(String, FieldType)> {
    let mut cols = Vec::new();
    for f in &schema.fields {
        match f.field_type {
            FieldType::SeriesRef => {
                cols.push(("serie".to_string(), FieldType::SeriesRef));
                cols.push(("tome".to_string(), FieldType::Number));
            }
            _ => cols.push((f.key.clone(), f.field_type)),
        }
    }
    cols
}

fn export_csv(schema: &Schema, rows: &[IndexedItem], path: &Path) -> Result<u64, String> {
    let mut writer = csv::Writer::from_path(path).map_err(|e| e.to_string())?;
    let columns = schema_columns(schema);
    let header: Vec<&str> = SYSTEM_COLS
        .iter()
        .copied()
        .chain(columns.iter().map(|(k, _)| k.as_str()))
        .collect();
    writer.write_record(&header).map_err(|e| e.to_string())?;

    for row in rows {
        let mut record: Vec<String> = vec![
            row.id.clone(),
            row.cote.clone().unwrap_or_default(),
            row.statut.clone(),
            row.emplacement.clone().unwrap_or_default(),
            row.date_ajout.clone(),
        ];
        for (key, field_type) in &columns {
            let value = match (key.as_str(), field_type) {
                ("serie", FieldType::SeriesRef) => row.serie_nom.clone().unwrap_or_default(),
                ("tome", _) => row
                    .serie_tome
                    .map(|t| t.to_string())
                    .unwrap_or_default(),
                _ => cell(row.data.get(key)),
            };
            record.push(value);
        }
        writer.write_record(&record).map_err(|e| e.to_string())?;
    }
    writer.flush().map_err(|e| e.to_string())?;
    Ok(rows.len() as u64)
}

fn export_json(schema: &Schema, rows: &[IndexedItem], path: &Path) -> Result<u64, String> {
    let serie_key = schema
        .fields
        .iter()
        .find(|f| f.field_type == FieldType::SeriesRef)
        .map(|f| f.key.clone());
    let objects: Vec<serde_json::Value> = rows
        .iter()
        .map(|row| {
            let mut obj = serde_json::Map::new();
            obj.insert("id".into(), serde_json::json!(row.id));
            obj.insert("cote".into(), serde_json::json!(row.cote));
            obj.insert("statut".into(), serde_json::json!(row.statut));
            obj.insert("emplacement".into(), serde_json::json!(row.emplacement));
            obj.insert("date_ajout".into(), serde_json::json!(row.date_ajout));
            for f in &schema.fields {
                if Some(&f.key) == serie_key.as_ref() {
                    obj.insert(
                        f.key.clone(),
                        serde_json::json!({
                            "nom": row.serie_nom,
                            "tome": row.serie_tome,
                        }),
                    );
                } else {
                    obj.insert(
                        f.key.clone(),
                        row.data.get(&f.key).cloned().unwrap_or(serde_json::Value::Null),
                    );
                }
            }
            serde_json::Value::Object(obj)
        })
        .collect();
    let text = serde_json::to_string_pretty(&objects).map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| e.to_string())?;
    Ok(rows.len() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Series, Statut};
    use serde_json::json;
    use std::collections::BTreeMap;

    fn setup() -> (tempfile::TempDir, Library, Index) {
        let dir = tempfile::tempdir().unwrap();
        let lib = Library::create(dir.path().join("b")).unwrap();
        let mut idx = Index::open(&dir.path().join("i.sqlite")).unwrap();
        lib.upsert_series(
            "bd",
            Series { id: "lastman".into(), nom: "Lastman".into(), terminee: false },
        )
        .unwrap();
        let mut fields = BTreeMap::new();
        fields.insert("titre".to_string(), json!("Lastman T1"));
        fields.insert("genre".to_string(), json!("Science-Fiction"));
        fields.insert("scenariste".to_string(), json!(["Balak", "Bastien Vivès"]));
        fields.insert("date_parution".to_string(), json!("2013-03-20"));
        fields.insert("serie".to_string(), json!({"id": "lastman", "tome": 1}));
        lib.create_item("bd", Statut::Possede, fields).unwrap();
        idx.rebuild(&lib).unwrap();
        (dir, lib, idx)
    }

    #[test]
    fn csv_export_roundtrips_headers_and_values() {
        let (dir, lib, idx) = setup();
        let path = dir.path().join("export.csv");
        let n = export(&lib, &idx, "bd", &path, "", &SearchFilters::default()).unwrap();
        assert_eq!(n, 1);

        let text = std::fs::read_to_string(&path).unwrap();
        let mut lines = text.lines();
        let header = lines.next().unwrap();
        assert!(header.starts_with("id,cote,statut,emplacement,date_ajout,titre"));
        assert!(header.contains(",serie,tome,"));
        let row = lines.next().unwrap();
        assert!(row.contains("BD-00001"));
        assert!(row.contains("2013-SF-0001"));
        assert!(row.contains("Balak ; Bastien Vivès"));
        assert!(row.contains("Lastman,1") || row.contains("Lastman,\"1\""));
    }

    #[test]
    fn json_export_is_structured() {
        let (dir, lib, idx) = setup();
        let path = dir.path().join("export.json");
        export(&lib, &idx, "bd", &path, "", &SearchFilters::default()).unwrap();
        let parsed: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let first = &parsed.as_array().unwrap()[0];
        assert_eq!(first["id"], json!("BD-00001"));
        assert_eq!(first["scenariste"], json!(["Balak", "Bastien Vivès"]));
        assert_eq!(first["serie"]["nom"], json!("Lastman"));
        assert_eq!(first["serie"]["tome"], json!(1));
    }
}
