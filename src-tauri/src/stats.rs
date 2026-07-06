//! Lot 4 : rapport de séries (tomes manquants) et statistiques du tableau
//! de bord. Tout est calculé depuis l'index SQLite — instantané, jamais de
//! balayage des YAML.

use crate::index::Index;
use crate::model::FieldType;
use crate::store::Library;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Serialize)]
pub struct SerieReport {
    pub id: String,
    pub nom: String,
    pub terminee: bool,
    /// Tomes possédés, triés.
    pub possedes: Vec<i64>,
    /// Tomes en wishlist.
    pub souhaites: Vec<i64>,
    /// Trous entre 1 et le plus grand tome possédé.
    pub manquants: Vec<i64>,
}

pub fn series_report(
    lib: &Library,
    idx: &Index,
    collection: &str,
) -> Result<Vec<SerieReport>, String> {
    let registry = lib.load_series(collection)?;
    let mut possedes: BTreeMap<String, BTreeSet<i64>> = BTreeMap::new();
    let mut souhaites: BTreeMap<String, BTreeSet<i64>> = BTreeMap::new();
    for (serie_id, tome, statut) in idx.series_rows(collection)? {
        let Some(tome) = tome else { continue };
        let target = if statut == "souhaite" { &mut souhaites } else { &mut possedes };
        target.entry(serie_id).or_default().insert(tome);
    }
    Ok(registry
        .into_iter()
        .map(|serie| {
            let owned = possedes.remove(&serie.id).unwrap_or_default();
            let wished: Vec<i64> =
                souhaites.remove(&serie.id).unwrap_or_default().into_iter().collect();
            let max = owned.iter().max().copied().unwrap_or(0);
            let manquants: Vec<i64> =
                (1..=max).filter(|t| !owned.contains(t)).collect();
            SerieReport {
                id: serie.id,
                nom: serie.nom,
                terminee: serie.terminee,
                possedes: owned.into_iter().collect(),
                souhaites: wished,
                manquants,
            }
        })
        .collect())
}

// ---------------------------------------------------------------------------
// Tableau de bord
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct CollectionCard {
    pub slug: String,
    pub name: String,
    pub possede: u64,
    pub souhaite: u64,
}

#[derive(Debug, Serialize)]
pub struct GenreCount {
    pub collection: String,
    pub genre: String,
    pub count: u64,
}

#[derive(Debug, Serialize)]
pub struct YearCount {
    pub collection: String,
    pub annee: i64,
    pub count: u64,
}

#[derive(Debug, Serialize)]
pub struct IncompleteSerie {
    pub collection: String,
    pub collection_name: String,
    pub nom: String,
    pub manquants: Vec<i64>,
}

#[derive(Debug, Serialize)]
pub struct DashboardStats {
    pub collections: Vec<CollectionCard>,
    pub total_possede: u64,
    pub total_souhaite: u64,
    pub genres: Vec<GenreCount>,
    pub annees: Vec<YearCount>,
    pub series_incompletes: Vec<IncompleteSerie>,
    pub series_incompletes_total: usize,
}

pub fn dashboard(lib: &Library, idx: &Index) -> Result<DashboardStats, String> {
    let mut stats = DashboardStats {
        collections: Vec::new(),
        total_possede: 0,
        total_souhaite: 0,
        genres: Vec::new(),
        annees: Vec::new(),
        series_incompletes: Vec::new(),
        series_incompletes_total: 0,
    };
    for slug in lib.collections()? {
        let schema = lib.load_schema(&slug)?;
        let possede = idx.count(&slug, Some("possede"))?;
        let souhaite = idx.count(&slug, Some("souhaite"))?;
        stats.total_possede += possede;
        stats.total_souhaite += souhaite;
        stats.collections.push(CollectionCard {
            slug: slug.clone(),
            name: schema.name.clone(),
            possede,
            souhaite,
        });

        for (genre, count) in idx.genre_distribution(&slug)? {
            stats.genres.push(GenreCount { collection: slug.clone(), genre, count });
        }
        for (annee, count) in idx.year_distribution(&slug)? {
            stats.annees.push(YearCount { collection: slug.clone(), annee, count });
        }

        if schema.fields.iter().any(|f| f.field_type == FieldType::SeriesRef) {
            let mut gaps: Vec<IncompleteSerie> = series_report(lib, idx, &slug)?
                .into_iter()
                .filter(|s| !s.manquants.is_empty())
                .map(|s| IncompleteSerie {
                    collection: slug.clone(),
                    collection_name: schema.name.clone(),
                    nom: s.nom,
                    manquants: s.manquants,
                })
                .collect();
            stats.series_incompletes_total += gaps.len();
            // Les plus trouées d'abord, pour la vitrine du dashboard.
            gaps.sort_by_key(|g| std::cmp::Reverse(g.manquants.len()));
            stats.series_incompletes.extend(gaps.into_iter().take(15));
        }
    }
    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Series, Statut};
    use serde_json::json;
    use std::collections::BTreeMap;

    fn add(lib: &Library, statut: Statut, titre: &str, serie: Option<(&str, i64)>) {
        let mut fields = BTreeMap::new();
        fields.insert("titre".to_string(), json!(titre));
        fields.insert("genre".to_string(), json!("Science-Fiction"));
        fields.insert("date_parution".to_string(), json!("2015-01-01"));
        if let Some((id, tome)) = serie {
            fields.insert("serie".to_string(), json!({ "id": id, "tome": tome }));
        }
        lib.create_item("bd", statut, fields).unwrap();
    }

    #[test]
    fn detects_missing_tomes() {
        let dir = tempfile::tempdir().unwrap();
        let lib = Library::create(dir.path().join("b")).unwrap();
        let mut idx = Index::open(&dir.path().join("i.sqlite")).unwrap();
        lib.upsert_series(
            "bd",
            Series { id: "uw1".into(), nom: "Universal War One".into(), terminee: true },
        )
        .unwrap();
        for t in [1, 2, 3, 5, 7] {
            add(&lib, Statut::Possede, &format!("UW1 T{t}"), Some(("uw1", t)));
        }
        add(&lib, Statut::Souhaite, "UW1 T4", Some(("uw1", 4)));
        idx.rebuild(&lib).unwrap();

        let report = series_report(&lib, &idx, "bd").unwrap();
        assert_eq!(report.len(), 1);
        let uw1 = &report[0];
        assert_eq!(uw1.possedes, vec![1, 2, 3, 5, 7]);
        assert_eq!(uw1.manquants, vec![4, 6]);
        assert_eq!(uw1.souhaites, vec![4]);
        assert!(uw1.terminee);
    }

    #[test]
    fn dashboard_aggregates() {
        let dir = tempfile::tempdir().unwrap();
        let lib = Library::create(dir.path().join("b")).unwrap();
        let mut idx = Index::open(&dir.path().join("i.sqlite")).unwrap();
        lib.upsert_series(
            "bd",
            Series { id: "s".into(), nom: "S".into(), terminee: false },
        )
        .unwrap();
        add(&lib, Statut::Possede, "T1", Some(("s", 1)));
        add(&lib, Statut::Possede, "T3", Some(("s", 3)));
        add(&lib, Statut::Souhaite, "Convoité", None);
        idx.rebuild(&lib).unwrap();

        let d = dashboard(&lib, &idx).unwrap();
        assert_eq!(d.total_possede, 2);
        assert_eq!(d.total_souhaite, 1);
        let bd = d.collections.iter().find(|c| c.slug == "bd").unwrap();
        assert_eq!(bd.possede, 2);
        assert_eq!(d.series_incompletes_total, 1);
        assert_eq!(d.series_incompletes[0].manquants, vec![2]);
        assert!(d.genres.iter().any(|g| g.genre == "Science-Fiction" && g.count == 2));
        assert!(d.annees.iter().any(|y| y.annee == 2015 && y.count == 2));
    }
}
