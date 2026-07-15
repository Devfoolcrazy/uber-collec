//! Enrichissement de masse : complète les fiches d'une collection depuis la
//! BNF (couvertures, champs manquants), en tâche de fond, à un rythme
//! volontairement très lent pour respecter l'API publique.
//!
//! Rejouable : les fiches ayant déjà une couverture sont sautées sans appel
//! réseau — on peut interrompre et relancer sans dégât.

use crate::hydrate;
use crate::model::FieldType;
use crate::store::Library;
use serde::Serialize;
use std::sync::Mutex;
use tauri::Manager;

/// Pause après CHAQUE requête réseau (recherche BNF, couverture).
const REQUEST_DELAY_MS: u64 = 4_000;

#[derive(Debug, Default, Clone, Serialize)]
pub struct EnrichProgress {
    pub running: bool,
    pub done: bool,
    pub cancel_requested: bool,
    pub collection: String,
    /// Fiches à examiner au total.
    pub total: usize,
    pub processed: usize,
    /// Fiches où au moins un champ a été complété.
    pub enriched: usize,
    /// Couvertures téléchargées.
    pub covers: usize,
    /// Déjà complètes (couverture présente) — sautées sans appel réseau.
    pub skipped: usize,
    /// Sans EAN/ISBN : impossible à rapprocher automatiquement.
    pub no_ean: usize,
    /// EAN inconnu de la BNF.
    pub not_found: usize,
    pub errors: usize,
    pub last_error: Option<String>,
    /// Titre en cours de traitement (affichage).
    pub current: String,
}

pub type SharedProgress = Mutex<EnrichProgress>;

fn is_empty_value(v: Option<&serde_json::Value>) -> bool {
    match v {
        None | Some(serde_json::Value::Null) => true,
        Some(serde_json::Value::String(s)) => s.trim().is_empty(),
        Some(serde_json::Value::Array(a)) => a.is_empty(),
        _ => false,
    }
}

async fn pause() {
    tokio::time::sleep(std::time::Duration::from_millis(REQUEST_DELAY_MS)).await;
}

/// Boucle d'enrichissement. Tourne dans une tâche de fond ; l'état applicatif
/// n'est verrouillé que pour les lectures/écritures disque, jamais pendant
/// les appels réseau.
pub async fn run(app: tauri::AppHandle, collection: String) {
    let result = run_inner(&app, &collection).await;
    let summary = {
        let progress = app.state::<SharedProgress>();
        let mut p = progress.lock().unwrap();
        p.running = false;
        p.done = true;
        p.current = String::new();
        if let Err(e) = result {
            p.errors += 1;
            p.last_error = Some(e);
        }
        (p.enriched, p.covers)
    };
    if summary.0 > 0 || summary.1 > 0 {
        crate::sync::auto_commit(
            &app,
            format!(
                "Enrichissement {collection} : {} fiches, {} couvertures",
                summary.0, summary.1
            ),
        );
    }
}

async fn run_inner(app: &tauri::AppHandle, collection: &str) -> Result<(), String> {
    let state = app.state::<Mutex<crate::AppState>>();
    let progress = app.state::<SharedProgress>();

    // Photographie initiale : racine, schéma, liste des fiches.
    let (root, mut schema, ids) = {
        let guard = state.lock().unwrap();
        let lib = guard.library.as_ref().ok_or("aucune bibliothèque ouverte")?;
        let schema = lib.load_schema(collection)?;
        let ids = lib.list_item_ids(collection)?;
        (lib.root.clone(), schema, ids)
    };
    let mut schema_dirty = false;
    let source = schema.source.clone().unwrap_or_default();
    let keys = crate::api_keys_from_config(app);
    if source == "dvd" && keys.tmdb.is_none() {
        return Err("clé d'API TMDB non configurée — lancez une recherche « Scanner » sur un DVD pour la saisir".into());
    }
    let lib = Library::open(&root)?;
    let image_key = schema
        .fields
        .iter()
        .find(|f| f.field_type == FieldType::Image)
        .map(|f| f.key.clone())
        .ok_or("le schéma n'a pas de champ image")?;
    let title_key = schema
        .title_field()
        .map(|f| f.key.clone())
        .unwrap_or_default();
    let synopsis_key =
        hydrate::find_target(&schema, &["synopsis", "resume", "description"]).map(|f| f.key);

    {
        let mut p = progress.lock().unwrap();
        p.total = ids.len();
    }

    let client = hydrate::client()?;

    for id in ids {
        if progress.lock().unwrap().cancel_requested {
            break;
        }

        let mut item = match lib.load_item(collection, &id) {
            Ok(i) => i,
            Err(e) => {
                let mut p = progress.lock().unwrap();
                p.processed += 1;
                p.errors += 1;
                p.last_error = Some(e);
                continue;
            }
        };
        let titre = item
            .fields
            .get(&title_key)
            .and_then(|v| v.as_str())
            .unwrap_or(&id)
            .to_string();
        {
            let mut p = progress.lock().unwrap();
            p.current = titre.clone();
        }

        // Rien à combler (couverture présente, et genre présent quand le
        // schéma en attend un) → aucun appel réseau. C'est ce critère qui
        // rend l'enrichissement rejouable pour combler les genres a
        // posteriori (Discogs) sans retélécharger les pochettes.
        let genre_key = schema.cote.as_ref().map(|c| c.genre_field.clone());
        let cover_missing = is_empty_value(item.fields.get(&image_key));
        let genre_missing = genre_key
            .as_ref()
            .map(|k| is_empty_value(item.fields.get(k)))
            .unwrap_or(false);
        // Synopsis : comblé pour les livres/BD avec clé Google Books (seule
        // source de synopsis en masse) et pour les DVD (TMDB le fournit).
        let synopsis_missing = match &synopsis_key {
            Some(k) if source == "dvd" => is_empty_value(item.fields.get(k)),
            Some(k) if (source == "bd" || source == "livres") && keys.gbooks.is_some() => {
                is_empty_value(item.fields.get(k))
            }
            _ => false,
        };
        // Nature de l'œuvre (Film / Série TV) : TMDB la fournit — comblée
        // pour les DVD quand le schéma a un sélecteur type/nature.
        let type_missing = source == "dvd"
            && hydrate::find_target(&schema, &["type", "nature"])
                .map(|f| is_empty_value(item.fields.get(&f.key)))
                .unwrap_or(false);
        if !cover_missing && !genre_missing && !synopsis_missing && !type_missing {
            let mut p = progress.lock().unwrap();
            p.processed += 1;
            p.skipped += 1;
            continue;
        }

        // Clé de rapprochement : EAN/ISBN si présent, sinon (CD uniquement)
        // artiste + titre avec seuil de confiance MusicBrainz.
        let ean = item
            .fields
            .get("ean")
            .or_else(|| item.fields.get("isbn"))
            .and_then(|v| v.as_str())
            .map(|s| s.chars().filter(|c| c.is_ascii_digit()).collect::<String>())
            .filter(|s| s.len() == 10 || s.len() == 13);

        let candidates = match (source.as_str(), &ean) {
            ("dvd", _) => {
                // TMDB ignore les codes-barres : titre + année, strictement.
                let annee = item
                    .fields
                    .get("date_sortie")
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.get(..4))
                    .and_then(|y| y.parse::<i64>().ok());
                if titre.is_empty() {
                    let mut p = progress.lock().unwrap();
                    p.processed += 1;
                    p.no_ean += 1;
                    continue;
                }
                let r =
                    hydrate::tmdb_strict(&client, keys.tmdb.as_deref().unwrap(), &titre, annee)
                        .await;
                pause().await;
                r.map(|mut list| {
                    list.truncate(1);
                    list
                })
            }
            ("cd", _) => {
                let artiste = item
                    .fields
                    .get("artiste")
                    .and_then(|v| v.as_array())
                    .and_then(|a| a.first())
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                // MusicBrainz (par EAN ou validation stricte artiste/titre),
                // complété par Discogs (genres, pochettes de secours).
                let mb = match &ean {
                    Some(ean) => hydrate::musicbrainz(&client, ean, true).await,
                    None if !artiste.is_empty() && !titre.is_empty() => {
                        hydrate::musicbrainz_strict(&client, &artiste, &titre).await
                    }
                    None => Ok(Vec::new()),
                };
                pause().await;
                let dg = match (&keys.discogs, artiste.is_empty() || titre.is_empty()) {
                    (Some(token), false) => {
                        let r = hydrate::discogs_strict(&client, token, &artiste, &titre).await;
                        pause().await;
                        r
                    }
                    _ => Ok(Vec::new()),
                };
                // iTunes : sans clé — pochettes haute résolution et genres.
                let it = match &ean {
                    Some(ean) => {
                        let r = hydrate::itunes(&client, ean, true).await;
                        pause().await;
                        r
                    }
                    None if !artiste.is_empty() && !titre.is_empty() => {
                        let r = hydrate::itunes_strict(&client, &artiste, &titre).await;
                        pause().await;
                        r
                    }
                    None => Ok(Vec::new()),
                };
                // Une source en panne n'annule pas les autres.
                let mut merged = Vec::new();
                let mut failures = Vec::new();
                for result in [mb, dg, it] {
                    match result {
                        Ok(mut list) => {
                            list.truncate(1);
                            merged.append(&mut list);
                        }
                        Err(e) => failures.push(e),
                    }
                }
                if merged.is_empty() && ean.is_none() && (artiste.is_empty() || titre.is_empty()) {
                    let mut p = progress.lock().unwrap();
                    p.processed += 1;
                    p.no_ean += 1;
                    continue;
                }
                if merged.is_empty() && failures.len() == 3 {
                    Err(failures.join(" · "))
                } else {
                    Ok(merged)
                }
            }
            (_, Some(ean)) => {
                // BNF (référence), puis Google Books si clé (synopsis).
                let bnf_r = hydrate::bnf(&client, ean, true).await;
                pause().await;
                let gb_r = if keys.gbooks.is_some() {
                    let r =
                        hydrate::google_books(&client, ean, true, keys.gbooks.as_deref()).await;
                    pause().await;
                    r
                } else {
                    Ok(Vec::new())
                };
                let mut merged = Vec::new();
                let mut failures = Vec::new();
                for result in [bnf_r, gb_r] {
                    match result {
                        Ok(mut list) => {
                            list.truncate(1);
                            merged.append(&mut list);
                        }
                        Err(e) => failures.push(e),
                    }
                }
                if merged.is_empty() && !failures.is_empty() {
                    Err(failures.join(" · "))
                } else {
                    Ok(merged)
                }
            }
            (_, None) => {
                let mut p = progress.lock().unwrap();
                p.processed += 1;
                p.no_ean += 1;
                continue;
            }
        };
        let candidates = match candidates {
            Ok(list) => list,
            Err(e) => {
                let mut p = progress.lock().unwrap();
                p.processed += 1;
                p.errors += 1;
                p.last_error = Some(format!("{titre} : {e}"));
                continue;
            }
        };
        if candidates.is_empty() {
            let mut p = progress.lock().unwrap();
            p.processed += 1;
            p.not_found += 1;
            continue;
        }

        // Genre inconnu proposé par une source (Discogs…) : ajouté au schéma
        // avec un code unique, comme à l'import — sinon la cote reste AUTRE.
        if genre_missing {
            if let Some(genre) = candidates.iter().find_map(|c| c.genre.as_deref()) {
                if let Some((_, _, added)) = hydrate::ensure_genre(&mut schema, genre) {
                    if added {
                        schema_dirty = true;
                    }
                }
            }
        }

        // Fusionne les champs de tous les candidats validés : premier
        // non-vide gagnant (MusicBrainz d'abord, puis Discogs).
        let mut changed = false;
        for candidate in &candidates {
            for (key, value) in hydrate::candidate_to_fields(&schema, candidate) {
                if is_empty_value(item.fields.get(&key)) {
                    item.fields.insert(key, value);
                    changed = true;
                }
            }
        }

        // Couverture : essaie chaque source dans l'ordre jusqu'au succès
        // (Cover Art Archive renvoie 404 quand la pochette manque), avec
        // OpenLibrary par ISBN en filet pour les livres/BD.
        let mut got_cover = false;
        let mut cover_urls: Vec<String> = if cover_missing {
            candidates.iter().filter_map(|c| c.cover_url.clone()).collect()
        } else {
            Vec::new()
        };
        if cover_missing && matches!(source.as_str(), "bd" | "livres") {
            if let Some(ean) = &ean {
                cover_urls.push(format!(
                    "https://covers.openlibrary.org/b/isbn/{ean}-L.jpg?default=false"
                ));
            }
        }
        for url in cover_urls {
            match hydrate::fetch_cover_webp(&url).await {
                Ok(bytes) => {
                    let rel = format!("images/{collection}/{id}.webp");
                    let path = root.join(&rel);
                    if let Some(parent) = path.parent() {
                        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                    }
                    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
                    item.fields.insert(image_key.clone(), serde_json::json!(rel));
                    changed = true;
                    got_cover = true;
                }
                Err(e) => {
                    let mut p = progress.lock().unwrap();
                    p.last_error = Some(format!("{titre} : {e}"));
                }
            }
            pause().await;
            if got_cover {
                break;
            }
        }

        if changed {
            // Écriture + réindexation sous verrou, réseau terminé.
            let guard = state.lock().unwrap();
            let crate::AppState { library, index } = &*guard;
            if let (Some(applib), Some(idx)) = (library, index) {
                applib.save_item(collection, &item)?;
                let series = applib.load_series(collection)?;
                idx.upsert_item(collection, &schema, &series, &item)?;
            } else {
                return Err("bibliothèque fermée pendant l'enrichissement".into());
            }
        }

        let mut p = progress.lock().unwrap();
        p.processed += 1;
        if changed {
            p.enriched += 1;
        }
        if got_cover {
            p.covers += 1;
        }
    }

    // Genres ajoutés en cours de route : le schéma enrichi est persisté.
    if schema_dirty {
        let guard = state.lock().unwrap();
        if let Some(lib) = &guard.library {
            lib.save_schema(collection, &schema)?;
        }
    }
    Ok(())
}
