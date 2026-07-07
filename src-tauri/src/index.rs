//! Index SQLite **jetable** : accélère recherche, tri et stats sur ~20 000
//! objets. Reconstructible à tout moment depuis les YAML ; n'est jamais la
//! source de vérité et ne vit pas dans le dépôt de données.

use crate::model::{FieldType, Item, Schema, Series, Statut};
use crate::store::Library;
use rusqlite::{params, Connection};
use serde::Serialize;
use std::path::Path;

/// Version du schéma SQLite : l'index étant jetable, tout changement de
/// structure se gère en le recréant de zéro (puis reconstruction).
const INDEX_VERSION: i64 = 4;

/// Filtres structurés de recherche, combinables avec le plein texte.
#[derive(Debug, Default, serde::Deserialize)]
pub struct SearchFilters {
    pub statut: Option<String>,
    pub genre: Option<String>,
    pub annee: Option<i64>,
    /// Identifiant de série (slug du registre).
    pub serie: Option<String>,
}

pub struct Index {
    conn: Connection,
}

/// Ligne d'index renvoyée au front pour les listes.
#[derive(Debug, Serialize)]
pub struct IndexedItem {
    pub collection: String,
    pub id: String,
    pub titre: String,
    pub cote: Option<String>,
    pub statut: String,
    pub emplacement: Option<String>,
    pub date_ajout: String,
    pub serie_nom: Option<String>,
    pub serie_tome: Option<i64>,
    pub annee: Option<i64>,
    pub data: serde_json::Value,
}

impl Index {
    pub fn open(path: &Path) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let conn = Connection::open(path).map_err(|e| e.to_string())?;
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        if version != INDEX_VERSION {
            conn.execute_batch(
                "DROP TABLE IF EXISTS meta;
                 DROP TABLE IF EXISTS items;
                 DROP TABLE IF EXISTS items_fts;",
            )
            .map_err(|e| e.to_string())?;
            conn.pragma_update(None, "user_version", INDEX_VERSION)
                .map_err(|e| e.to_string())?;
        }
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY, value TEXT);
             CREATE TABLE IF NOT EXISTS items(
                 collection TEXT NOT NULL,
                 id TEXT NOT NULL,
                 titre TEXT NOT NULL DEFAULT '',
                 cote TEXT,
                 statut TEXT NOT NULL,
                 emplacement TEXT,
                 date_ajout TEXT NOT NULL,
                 serie_id TEXT,
                 serie_nom TEXT,
                 serie_tome INTEGER,
                 genre TEXT,
                 annee INTEGER,
                 etiquette TEXT,
                 data TEXT NOT NULL,
                 PRIMARY KEY(collection, id)
             );
             CREATE INDEX IF NOT EXISTS idx_items_filters
                 ON items(collection, genre, annee, serie_id);
             CREATE VIRTUAL TABLE IF NOT EXISTS items_fts USING fts5(
                 collection UNINDEXED, id UNINDEXED, content
             );",
        )
        .map_err(|e| e.to_string())?;
        Ok(Self { conn })
    }

    /// Encapsule une rafale d'écritures dans une transaction (import de masse).
    pub fn bulk_begin(&self) -> Result<(), String> {
        self.conn
            .execute_batch("BEGIN IMMEDIATE")
            .map_err(|e| e.to_string())
    }

    pub fn bulk_commit(&self) -> Result<(), String> {
        self.conn.execute_batch("COMMIT").map_err(|e| e.to_string())
    }

    pub fn meta(&self, key: &str) -> Option<String> {
        self.conn
            .query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0))
            .ok()
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT INTO meta(key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                [key, value],
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    /// Série référencée par l'objet, résolue via le registre : (id, nom, tome).
    fn resolve_series(
        schema: &Schema,
        series: &[Series],
        item: &Item,
    ) -> (Option<String>, Option<String>, Option<i64>) {
        for def in &schema.fields {
            if def.field_type != FieldType::SeriesRef {
                continue;
            }
            let Some(value) = item.fields.get(&def.key) else { continue };
            let Some(id) = value.get("id").and_then(|v| v.as_str()) else { continue };
            let tome = value.get("tome").and_then(|v| v.as_i64());
            let nom = series
                .iter()
                .find(|s| s.id == id)
                .map(|s| s.nom.clone())
                .unwrap_or_else(|| id.to_string());
            return (Some(id.to_string()), Some(nom), tome);
        }
        (None, None, None)
    }

    /// Genre et année de production, extraits selon la config de cote du
    /// schéma (les mêmes champs qui composent la cote).
    fn resolve_genre_annee(schema: &Schema, item: &Item) -> (Option<String>, Option<i64>) {
        let Some(config) = &schema.cote else { return (None, None) };
        let genre = item
            .fields
            .get(&config.genre_field)
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let annee = item
            .fields
            .get(&config.year_field)
            .and_then(|v| v.as_str())
            .and_then(|s| {
                let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
                (digits.len() == 4).then(|| digits.parse::<i64>().ok()).flatten()
            });
        (genre, annee)
    }

    /// Texte cherchable d'un objet : concaténation des valeurs textuelles,
    /// nom de série résolu inclus.
    fn searchable_content(schema: &Schema, series: &[Series], item: &Item) -> String {
        let mut parts: Vec<String> = vec![item.id.clone()];
        if let Some(cote) = &item.cote {
            parts.push(cote.clone());
        }
        if let Some(e) = &item.emplacement {
            parts.push(e.clone());
        }
        let (_, serie_nom, serie_tome) = Self::resolve_series(schema, series, item);
        if let Some(nom) = serie_nom {
            parts.push(nom);
        }
        if let Some(tome) = serie_tome {
            parts.push(format!("T{tome}"));
        }
        for def in &schema.fields {
            let Some(value) = item.fields.get(&def.key) else { continue };
            match def.field_type {
                FieldType::Image | FieldType::SeriesRef => {}
                _ => match value {
                    serde_json::Value::String(s) => parts.push(s.clone()),
                    serde_json::Value::Array(arr) => parts.extend(
                        arr.iter()
                            .filter_map(|v| v.as_str().map(str::to_string)),
                    ),
                    serde_json::Value::Number(n) => parts.push(n.to_string()),
                    _ => {}
                },
            }
        }
        parts.join(" ")
    }

    fn title_of(schema: &Schema, item: &Item) -> String {
        schema
            .title_field()
            .and_then(|f| item.fields.get(&f.key))
            .and_then(|v| v.as_str())
            .unwrap_or("(sans titre)")
            .to_string()
    }

    pub fn upsert_item(
        &self,
        collection: &str,
        schema: &Schema,
        series: &[Series],
        item: &Item,
    ) -> Result<(), String> {
        let statut = match item.statut {
            Statut::Possede => "possede",
            Statut::Souhaite => "souhaite",
        };
        let data = serde_json::to_string(&item.fields).map_err(|e| e.to_string())?;
        let (serie_id, serie_nom, serie_tome) = Self::resolve_series(schema, series, item);
        let (genre, annee) = Self::resolve_genre_annee(schema, item);
        self.conn
            .execute(
                "INSERT INTO items(collection, id, titre, cote, statut, emplacement, date_ajout, serie_id, serie_nom, serie_tome, genre, annee, etiquette, data)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
                 ON CONFLICT(collection, id) DO UPDATE SET
                   titre = excluded.titre, cote = excluded.cote, statut = excluded.statut,
                   emplacement = excluded.emplacement, date_ajout = excluded.date_ajout,
                   serie_id = excluded.serie_id, serie_nom = excluded.serie_nom,
                   serie_tome = excluded.serie_tome, genre = excluded.genre,
                   annee = excluded.annee, etiquette = excluded.etiquette,
                   data = excluded.data",
                params![
                    collection,
                    item.id,
                    Self::title_of(schema, item),
                    item.cote,
                    statut,
                    item.emplacement,
                    item.date_ajout,
                    serie_id,
                    serie_nom,
                    serie_tome,
                    genre,
                    annee,
                    item.etiquette,
                    data
                ],
            )
            .map_err(|e| e.to_string())?;
        self.conn
            .execute(
                "DELETE FROM items_fts WHERE collection = ?1 AND id = ?2",
                params![collection, item.id],
            )
            .map_err(|e| e.to_string())?;
        self.conn
            .execute(
                "INSERT INTO items_fts(collection, id, content) VALUES (?1, ?2, ?3)",
                params![collection, item.id, Self::searchable_content(schema, series, item)],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn remove_collection(&self, collection: &str) -> Result<(), String> {
        self.conn
            .execute("DELETE FROM items WHERE collection = ?1", [collection])
            .map_err(|e| e.to_string())?;
        self.conn
            .execute("DELETE FROM items_fts WHERE collection = ?1", [collection])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn remove_item(&self, collection: &str, id: &str) -> Result<(), String> {
        self.conn
            .execute(
                "DELETE FROM items WHERE collection = ?1 AND id = ?2",
                params![collection, id],
            )
            .map_err(|e| e.to_string())?;
        self.conn
            .execute(
                "DELETE FROM items_fts WHERE collection = ?1 AND id = ?2",
                params![collection, id],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Vide et reconstruit l'index complet depuis les YAML.
    pub fn rebuild(&mut self, lib: &Library) -> Result<u64, String> {
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        tx.execute_batch("DELETE FROM items; DELETE FROM items_fts;")
            .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;

        let mut count = 0u64;
        for slug in lib.collections()? {
            let schema = lib.load_schema(&slug)?;
            let series = lib.load_series(&slug)?;
            for item in lib.iter_items(&slug)? {
                self.upsert_item(&slug, &schema, &series, &item)?;
                count += 1;
            }
        }
        Ok(count)
    }

    pub fn count(&self, collection: &str, statut: Option<&str>) -> Result<u64, String> {
        let (sql, has_statut) = match statut {
            Some(_) => ("SELECT COUNT(*) FROM items WHERE collection = ?1 AND statut = ?2", true),
            None => ("SELECT COUNT(*) FROM items WHERE collection = ?1", false),
        };
        let result = if has_statut {
            self.conn
                .query_row(sql, params![collection, statut.unwrap()], |r| r.get(0))
        } else {
            self.conn.query_row(sql, params![collection], |r| r.get(0))
        };
        result.map_err(|e| e.to_string())
    }

    /// Clauses SQL et paramètres nommés correspondant aux filtres.
    fn filter_clauses(
        filters: &SearchFilters,
    ) -> (String, Vec<(&'static str, Box<dyn rusqlite::ToSql>)>) {
        let mut clauses = String::new();
        let mut params: Vec<(&'static str, Box<dyn rusqlite::ToSql>)> = Vec::new();
        if let Some(s) = &filters.statut {
            clauses.push_str(" AND i.statut = :statut");
            params.push((":statut", Box::new(s.clone())));
        }
        if let Some(g) = &filters.genre {
            clauses.push_str(" AND i.genre = :genre");
            params.push((":genre", Box::new(g.clone())));
        }
        if let Some(a) = filters.annee {
            clauses.push_str(" AND i.annee = :annee");
            params.push((":annee", Box::new(a)));
        }
        if let Some(s) = &filters.serie {
            clauses.push_str(" AND i.serie_id = :serie");
            params.push((":serie", Box::new(s.clone())));
        }
        (clauses, params)
    }

    /// Nombre total de résultats pour une recherche (pour la pagination).
    pub fn count_search(
        &self,
        collection: &str,
        query: &str,
        filters: &SearchFilters,
    ) -> Result<u64, String> {
        let query = query.trim();
        let (clauses, mut params) = Self::filter_clauses(filters);
        let sql = if query.is_empty() {
            format!("SELECT COUNT(*) FROM items i WHERE i.collection = :collection{clauses}")
        } else {
            format!(
                "SELECT COUNT(*) FROM items_fts f
                 JOIN items i ON i.collection = f.collection AND i.id = f.id
                 WHERE f.collection = :collection AND items_fts MATCH :match{clauses}"
            )
        };
        let mut stmt = self.conn.prepare(&sql).map_err(|e| e.to_string())?;
        params.push((":collection", Box::new(collection.to_string())));
        if !query.is_empty() {
            params.push((":match", Box::new(Self::fts_query(query))));
        }
        let params_ref: Vec<(&str, &dyn rusqlite::ToSql)> =
            params.iter().map(|(k, v)| (*k, v.as_ref() as &dyn rusqlite::ToSql)).collect();
        stmt.query_row(params_ref.as_slice(), |r| r.get(0))
            .map_err(|e| e.to_string())
    }

    /// Fiches dont l'étiquette physique est à faire : possédées, cotées, et
    /// jamais étiquetées ou étiquetées sous une ancienne cote. Triées par
    /// collection puis cote — l'ordre d'une session d'étiquetage en rayon.
    pub fn labels_todo(&self, collection: Option<&str>) -> Result<Vec<IndexedItem>, String> {
        let clause = match collection {
            Some(_) => " AND i.collection = :collection",
            None => "",
        };
        let sql = format!(
            "SELECT i.collection, i.id, i.titre, i.cote, i.statut, i.emplacement, i.date_ajout, i.serie_nom, i.serie_tome, i.annee, i.data
             FROM items i
             WHERE i.statut = 'possede' AND i.cote IS NOT NULL
               AND (i.etiquette IS NULL OR i.etiquette != i.cote){clause}
             ORDER BY i.collection, i.cote"
        );
        let mut stmt = self.conn.prepare(&sql).map_err(|e| e.to_string())?;
        let mut params: Vec<(&str, Box<dyn rusqlite::ToSql>)> = Vec::new();
        if let Some(c) = collection {
            params.push((":collection", Box::new(c.to_string())));
        }
        let params_ref: Vec<(&str, &dyn rusqlite::ToSql)> =
            params.iter().map(|(k, v)| (*k, v.as_ref() as &dyn rusqlite::ToSql)).collect();
        let rows = stmt
            .query_map(params_ref.as_slice(), |row| {
                Ok(IndexedItem {
                    collection: row.get(0)?,
                    id: row.get(1)?,
                    titre: row.get(2)?,
                    cote: row.get(3)?,
                    statut: row.get(4)?,
                    emplacement: row.get(5)?,
                    date_ajout: row.get(6)?,
                    serie_nom: row.get(7)?,
                    serie_tome: row.get(8)?,
                    annee: row.get(9)?,
                    data: serde_json::from_str(&row.get::<_, String>(10)?)
                        .unwrap_or(serde_json::Value::Null),
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }

    /// Nombre total d'étiquettes à faire, toutes collections confondues.
    pub fn labels_todo_count(&self) -> Result<u64, String> {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM items
                 WHERE statut = 'possede' AND cote IS NOT NULL
                   AND (etiquette IS NULL OR etiquette != cote)",
                [],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())
    }

    /// Répartition des objets possédés par genre, décroissante.
    pub fn genre_distribution(&self, collection: &str) -> Result<Vec<(String, u64)>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT genre, COUNT(*) FROM items
                 WHERE collection = ?1 AND statut = 'possede' AND genre IS NOT NULL
                 GROUP BY genre ORDER BY COUNT(*) DESC",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([collection], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }

    /// Répartition des objets possédés par année de production, croissante.
    pub fn year_distribution(&self, collection: &str) -> Result<Vec<(i64, u64)>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT annee, COUNT(*) FROM items
                 WHERE collection = ?1 AND statut = 'possede' AND annee IS NOT NULL
                 GROUP BY annee ORDER BY annee",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([collection], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }

    /// Triplets (série, tome, statut) de tous les objets rattachés à une série.
    pub fn series_rows(
        &self,
        collection: &str,
    ) -> Result<Vec<(String, Option<i64>, String)>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT serie_id, serie_tome, statut FROM items
                 WHERE collection = ?1 AND serie_id IS NOT NULL",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([collection], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }

    /// Années de production présentes dans une collection, décroissantes.
    pub fn list_years(&self, collection: &str) -> Result<Vec<i64>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT DISTINCT annee FROM items
                 WHERE collection = ?1 AND annee IS NOT NULL ORDER BY annee DESC",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([collection], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }

    /// Clause ORDER BY pour une clé de tri (liste blanche — jamais de SQL
    /// venant du front). Les valeurs absentes vont toujours en fin de liste.
    fn order_clause(sort: Option<&str>, desc: bool, has_query: bool) -> String {
        let dir = if desc { "DESC" } else { "ASC" };
        match sort {
            Some("titre") => format!("i.titre COLLATE NOCASE {dir}"),
            Some("cote") => format!("i.cote IS NULL, i.cote {dir}"),
            Some("annee") => format!("i.annee IS NULL, i.annee {dir}, i.titre COLLATE NOCASE"),
            Some("serie") => format!(
                "i.serie_nom IS NULL, i.serie_nom COLLATE NOCASE {dir}, i.serie_tome {dir}"
            ),
            Some("date_ajout") => format!("i.date_ajout {dir}, i.id {dir}"),
            Some("emplacement") => {
                format!("i.emplacement IS NULL, i.emplacement COLLATE NOCASE {dir}")
            }
            _ if has_query => "rank".to_string(),
            _ => "i.serie_nom IS NULL, i.serie_nom COLLATE NOCASE, i.serie_tome, i.titre COLLATE NOCASE".to_string(),
        }
    }

    /// Recherche paginée. Sans tri explicite : série puis titre, ou
    /// pertinence quand une recherche plein texte est active.
    pub fn search(
        &self,
        collection: &str,
        query: &str,
        filters: &SearchFilters,
        sort: Option<&str>,
        desc: bool,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<IndexedItem>, String> {
        let query = query.trim();
        let (clauses, mut params) = Self::filter_clauses(filters);
        let order = Self::order_clause(sort, desc, !query.is_empty());
        const COLS: &str = "i.collection, i.id, i.titre, i.cote, i.statut, i.emplacement, i.date_ajout, i.serie_nom, i.serie_tome, i.annee, i.data";
        let sql = if query.is_empty() {
            format!(
                "SELECT {COLS}
                 FROM items i WHERE i.collection = :collection{clauses}
                 ORDER BY {order}
                 LIMIT :limit OFFSET :offset"
            )
        } else {
            format!(
                "SELECT {COLS}
                 FROM items_fts f
                 JOIN items i ON i.collection = f.collection AND i.id = f.id
                 WHERE f.collection = :collection AND items_fts MATCH :match{clauses}
                 ORDER BY {order} LIMIT :limit OFFSET :offset"
            )
        };
        let mut stmt = self.conn.prepare(&sql).map_err(|e| e.to_string())?;
        params.push((":collection", Box::new(collection.to_string())));
        params.push((":limit", Box::new(limit)));
        params.push((":offset", Box::new(offset)));
        if !query.is_empty() {
            params.push((":match", Box::new(Self::fts_query(query))));
        }
        let params_ref: Vec<(&str, &dyn rusqlite::ToSql)> =
            params.iter().map(|(k, v)| (*k, v.as_ref() as &dyn rusqlite::ToSql)).collect();
        let rows = stmt
            .query_map(params_ref.as_slice(), |row| {
                Ok(IndexedItem {
                    collection: row.get(0)?,
                    id: row.get(1)?,
                    titre: row.get(2)?,
                    cote: row.get(3)?,
                    statut: row.get(4)?,
                    emplacement: row.get(5)?,
                    date_ajout: row.get(6)?,
                    serie_nom: row.get(7)?,
                    serie_tome: row.get(8)?,
                    annee: row.get(9)?,
                    data: serde_json::from_str(&row.get::<_, String>(10)?)
                        .unwrap_or(serde_json::Value::Null),
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }

    /// Requête utilisateur → syntaxe FTS5 sûre : chaque mot devient un
    /// préfixe entre guillemets (`"last"* "man"*`).
    fn fts_query(raw: &str) -> String {
        raw.split_whitespace()
            .map(|tok| format!("\"{}\"*", tok.replace('"', "")))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Statut;
    use serde_json::json;
    use std::collections::BTreeMap;

    fn setup() -> (tempfile::TempDir, Library, Index) {
        let dir = tempfile::tempdir().unwrap();
        let lib = Library::create(dir.path().join("biblio")).unwrap();
        let index = Index::open(&dir.path().join("index.sqlite")).unwrap();
        (dir, lib, index)
    }

    fn bd(lib: &Library, titre: &str, genre: &str) -> crate::model::Item {
        let mut fields = BTreeMap::new();
        fields.insert("titre".to_string(), json!(titre));
        fields.insert("genre".to_string(), json!(genre));
        fields.insert("date_parution".to_string(), json!("2020-01-01"));
        lib.create_item("bd", Statut::Possede, fields).unwrap()
    }

    #[test]
    fn series_resolved_in_list_and_search() {
        let (_dir, lib, mut index) = setup();
        lib.upsert_series(
            "bd",
            crate::model::Series { id: "dragon-ball-super".into(), nom: "Dragon Ball Super".into(), terminee: false },
        )
        .unwrap();
        let mut fields = BTreeMap::new();
        fields.insert("titre".to_string(), json!("L'identité de Merus"));
        fields.insert("genre".to_string(), json!("Mangas - Shônen"));
        fields.insert("date_parution".to_string(), json!("2020-11-04"));
        fields.insert("serie".to_string(), json!({"id": "dragon-ball-super", "tome": 12}));
        lib.create_item("bd", Statut::Possede, fields).unwrap();
        index.rebuild(&lib).unwrap();

        // Le nom de série est résolu dans les lignes de liste…
        let rows = index.search("bd", "", &SearchFilters::default(), None, false, 50, 0).unwrap();
        assert_eq!(rows[0].serie_nom.as_deref(), Some("Dragon Ball Super"));
        assert_eq!(rows[0].serie_tome, Some(12));

        // …et cherchable en plein texte.
        let hits = index.search("bd", "dragon ball", &SearchFilters::default(), None, false, 50, 0).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].titre, "L'identité de Merus");
    }

    #[test]
    fn fts5_available_and_search_works() {
        let (_dir, lib, mut index) = setup();
        bd(&lib, "Lastman", "Science-Fiction");
        bd(&lib, "Astérix le Gaulois", "Humour");
        let n = index.rebuild(&lib).unwrap();
        assert_eq!(n, 2);

        // Recherche par préfixe, insensible à la casse.
        let hits = index.search("bd", "last", &SearchFilters::default(), None, false, 50, 0).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].titre, "Lastman");

        // Recherche par cote.
        let hits = index.search("bd", "2020-HUM", &SearchFilters::default(), None, false, 50, 0).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].titre, "Astérix le Gaulois");

        // Caractères spéciaux FTS → pas d'erreur de syntaxe.
        assert!(index.search("bd", "l'usine \"guillemets\" (x)", &SearchFilters::default(), None, false, 50, 0).is_ok());
    }

    #[test]
    fn upsert_and_remove_keep_index_consistent() {
        let (_dir, lib, mut index) = setup();
        let item = bd(&lib, "Akira", "Mangas - Seinen");
        index.rebuild(&lib).unwrap();
        assert_eq!(index.count("bd", None).unwrap(), 1);

        index.remove_item("bd", &item.id).unwrap();
        assert_eq!(index.count("bd", None).unwrap(), 0);
        assert!(index.search("bd", "akira", &SearchFilters::default(), None, false, 50, 0).unwrap().is_empty());
    }

    #[test]
    fn structured_filters() {
        let (_dir, lib, mut index) = setup();
        bd(&lib, "Lastman T1", "Science-Fiction"); // 2020
        bd(&lib, "Astérix", "Humour"); // 2020
        let mut fields = BTreeMap::new();
        fields.insert("titre".to_string(), json!("Universal War One"));
        fields.insert("genre".to_string(), json!("Science-Fiction"));
        fields.insert("date_parution".to_string(), json!("1998-05-01"));
        fields.insert("serie".to_string(), json!({"id": "uw1", "tome": 1}));
        lib.upsert_series(
            "bd",
            crate::model::Series { id: "uw1".into(), nom: "Universal War".into(), terminee: false },
        )
        .unwrap();
        lib.create_item("bd", Statut::Possede, fields).unwrap();
        index.rebuild(&lib).unwrap();

        let by_genre = SearchFilters { genre: Some("Science-Fiction".into()), ..Default::default() };
        assert_eq!(index.search("bd", "", &by_genre, None, false, 50, 0).unwrap().len(), 2);
        assert_eq!(index.count_search("bd", "", &by_genre).unwrap(), 2);

        let by_year = SearchFilters { annee: Some(1998), ..Default::default() };
        let hits = index.search("bd", "", &by_year, None, false, 50, 0).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].titre, "Universal War One");

        let by_serie = SearchFilters { serie: Some("uw1".into()), ..Default::default() };
        assert_eq!(index.search("bd", "", &by_serie, None, false, 50, 0).unwrap().len(), 1);

        // Combinable avec le plein texte.
        let hits = index.search("bd", "universal", &by_genre, None, false, 50, 0).unwrap();
        assert_eq!(hits.len(), 1);

        assert_eq!(index.list_years("bd").unwrap(), vec![2020, 1998]);

        // Tri par colonne : année croissante puis décroissante.
        let all = SearchFilters::default();
        let asc = index.search("bd", "", &all, Some("annee"), false, 50, 0).unwrap();
        assert_eq!(asc[0].titre, "Universal War One");
        let desc = index.search("bd", "", &all, Some("annee"), true, 50, 0).unwrap();
        assert_ne!(desc[0].titre, "Universal War One");
        // Clé de tri inconnue → pas d'injection, ordre par défaut.
        assert!(index.search("bd", "", &all, Some("evil; DROP"), false, 50, 0).is_ok());
    }

    #[test]
    fn labels_todo_lifecycle() {
        let (_dir, lib, mut index) = setup();
        let item = bd(&lib, "Lastman T1", "Science-Fiction");
        // Un souhaité (sans cote) ne doit jamais apparaître.
        let mut fields = BTreeMap::new();
        fields.insert("titre".to_string(), json!("Convoité"));
        lib.create_item("bd", Statut::Souhaite, fields).unwrap();
        index.rebuild(&lib).unwrap();

        // Fiche neuve → étiquette à faire.
        let todo = index.labels_todo(None).unwrap();
        assert_eq!(todo.len(), 1);
        assert_eq!(todo[0].id, item.id);
        assert_eq!(index.labels_todo_count().unwrap(), 1);

        // Pointée → disparaît.
        let labeled = lib.mark_labeled("bd", &item.id).unwrap();
        let schema = lib.load_schema("bd").unwrap();
        index.upsert_item("bd", &schema, &[], &labeled).unwrap();
        assert_eq!(index.labels_todo_count().unwrap(), 0);

        // Cote régénérée (changement de code du genre) → réapparaît.
        let mut schema = lib.load_schema("bd").unwrap();
        schema
            .fields
            .iter_mut()
            .find(|f| f.key == "genre")
            .unwrap()
            .options
            .iter_mut()
            .find(|o| o.value == "Science-Fiction")
            .unwrap()
            .code = Some("SCIFI".into());
        lib.save_schema("bd", &schema).unwrap();
        let changes = lib.regenerate_stale_cotes("bd").unwrap();
        assert_eq!(changes.len(), 1);
        index.rebuild(&lib).unwrap();
        assert_eq!(index.labels_todo_count().unwrap(), 1, "cote changée → étiquette à refaire");
    }

    #[test]
    fn statut_filter() {
        let (_dir, lib, mut index) = setup();
        bd(&lib, "Possédé", "Humour");
        let mut fields = BTreeMap::new();
        fields.insert("titre".to_string(), json!("Convoité"));
        lib.create_item("bd", Statut::Souhaite, fields).unwrap();
        index.rebuild(&lib).unwrap();

        let filters = SearchFilters { statut: Some("souhaite".into()), ..Default::default() };
        let wishlist = index.search("bd", "", &filters, None, false, 50, 0).unwrap();
        assert_eq!(wishlist.len(), 1);
        assert_eq!(wishlist[0].titre, "Convoité");
        assert!(wishlist[0].cote.is_none());
    }
}
