//! Hydratation depuis les bases ouvertes : recherche par code-barres
//! (EAN/ISBN) ou par titre, candidats présentés à l'utilisateur pour
//! validation avant écriture.
//!
//! Adaptateurs actifs : BNF (prioritaire — meilleure couverture du fonds
//! français, images incluses), Google Books, OpenLibrary.
//! À venir : IGDB (clé Twitch), MusicBrainz/Discogs, TMDB.

use crate::model::Schema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Résultat brut d'une base ouverte, indépendant du schéma cible.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Candidate {
    pub source: String,
    pub titre: Option<String>,
    #[serde(default)]
    pub auteurs: Vec<String>,
    /// Dessinateurs/illustrateurs, quand la source les distingue (BNF).
    #[serde(default)]
    pub illustrateurs: Vec<String>,
    pub editeur: Option<String>,
    pub date_parution: Option<String>,
    pub ean: Option<String>,
    pub synopsis: Option<String>,
    pub cover_url: Option<String>,
    /// Score de pertinence de la source (MusicBrainz : 0-100), pour les
    /// rapprochements automatiques sans code-barres.
    #[serde(default)]
    pub score: Option<i64>,
    /// Genre proposé par la source (TMDB), rapproché du schéma sans jamais
    /// créer de nouvelle option.
    #[serde(default)]
    pub genre: Option<String>,
    /// Acteurs principaux (TMDB).
    #[serde(default)]
    pub acteurs: Vec<String>,
}

/// Catalogue des sources d'hydratation disponibles, présenté dans l'éditeur
/// de schéma. Les collections custom choisissent librement dedans.
#[derive(Debug, Serialize)]
pub struct SourceInfo {
    pub id: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    /// Clé API à configurer (🔑), le cas échéant.
    pub requires_key: Option<&'static str>,
    /// Champs de schéma que la source sait remplir (clés).
    pub fills: &'static [&'static str],
}

pub fn sources_catalog() -> Vec<SourceInfo> {
    vec![
        SourceInfo {
            id: "livres",
            label: "Livres — BNF, Google Books, OpenLibrary",
            description: "Recherche par ISBN (douchette) ou titre. Idéal pour tout ce qui a un ISBN : romans, guides, livres-jeux…",
            requires_key: None,
            fills: &["titre", "auteur", "editeur", "date_parution", "synopsis", "isbn", "ean"],
        },
        SourceInfo {
            id: "bd",
            label: "BD & mangas — BNF, Google Books, OpenLibrary",
            description: "Comme Livres, avec scénariste et dessinateur distingués (BNF).",
            requires_key: None,
            fills: &["titre", "scenariste", "dessinateur", "editeur", "date_parution", "synopsis", "ean"],
        },
        SourceInfo {
            id: "cd",
            label: "Musique — MusicBrainz, Discogs",
            description: "Recherche par code-barres ou artiste + album. Genres et pochettes via Discogs.",
            requires_key: Some("discogs"),
            fills: &["titre", "artiste", "label", "editeur", "genre", "date_sortie", "ean"],
        },
        SourceInfo {
            id: "dvd",
            label: "Films — TMDB",
            description: "Recherche par titre uniquement (pas de code-barres). Affiches, synopsis, réalisateur, acteurs, genre.",
            requires_key: Some("tmdb"),
            fills: &["titre", "realisateur", "acteurs", "genre", "date_sortie", "synopsis"],
        },
    ]
}

pub(crate) fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent("UberCollec/0.1 (gestionnaire de collection personnel)")
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())
}

/// Envoie une requête en réessayant une fois, après deux secondes, sur les
/// échecs passagers : erreur réseau, 429, 5xx. Les APIs publiques toussent
/// régulièrement — un seul retry absorbe l'essentiel sans les harceler.
async fn send_with_retry(req: reqwest::RequestBuilder) -> Result<reqwest::Response, String> {
    let second_try = req.try_clone();
    let first = req.send().await;
    let transient = match &first {
        Ok(r) => {
            r.status().is_server_error() || r.status() == reqwest::StatusCode::TOO_MANY_REQUESTS
        }
        Err(_) => true,
    };
    let Some(second_try) = second_try else {
        return first.map_err(|e| e.to_string());
    };
    if !transient {
        return first.map_err(|e| e.to_string());
    }
    tokio::time::sleep(std::time::Duration::from_millis(2000)).await;
    second_try.send().await.map_err(|e| e.to_string())
}

/// Une chaîne de 10 ou 13 chiffres est traitée comme un code-barres.
pub fn looks_like_barcode(query: &str) -> bool {
    let q: String = query.chars().filter(|c| !c.is_whitespace() && *c != '-').collect();
    (q.len() == 10 || q.len() == 13) && q.chars().all(|c| c.is_ascii_digit())
}

/// Résultat d'une recherche multi-sources : les candidats trouvés, plus les
/// sources indisponibles signalées séparément — « introuvable » et « en
/// panne » ne doivent jamais se confondre.
#[derive(Debug, Serialize)]
pub struct SearchOutcome {
    pub candidates: Vec<Candidate>,
    pub warnings: Vec<String>,
}

/// Agrège des résultats de sources parallèles : erreur uniquement si TOUTES
/// ont échoué ; sinon candidats + avertissements.
fn combine(results: Vec<Result<Vec<Candidate>, String>>) -> Result<SearchOutcome, String> {
    let total = results.len();
    let mut candidates = Vec::new();
    let mut warnings = Vec::new();
    for result in results {
        match result {
            Ok(mut list) => candidates.append(&mut list),
            Err(e) => warnings.push(e),
        }
    }
    if warnings.len() == total {
        return Err(warnings.join(" · "));
    }
    Ok(SearchOutcome { candidates, warnings })
}

pub async fn search(
    source: &str,
    query: &str,
    tmdb_key: Option<&str>,
    discogs_token: Option<&str>,
) -> Result<SearchOutcome, String> {
    match source {
        "bd" | "livres" => search_books(query).await,
        "cd" => {
            let client = client()?;
            let barcode = looks_like_barcode(query);
            match discogs_token {
                Some(token) => {
                    // Deux sources en parallèle : MusicBrainz (référence) et
                    // Discogs (genres, éditions, pochettes complémentaires).
                    let (mb, dg) = tokio::join!(
                        musicbrainz(&client, query, barcode),
                        discogs(&client, token, query, barcode)
                    );
                    combine(vec![mb, dg])
                }
                None => combine(vec![musicbrainz(&client, query, barcode).await]),
            }
        }
        "dvd" => {
            let key = tmdb_key.ok_or(
                "TMDB_KEY_MISSING : clé d'API TMDB non configurée (themoviedb.org → Paramètres → API)",
            )?;
            if looks_like_barcode(query) {
                return Err(
                    "TMDB ne connaît pas les codes-barres — recherchez par titre de film".into(),
                );
            }
            let client = client()?;
            combine(vec![tmdb(&client, key, query).await])
        }
        other => Err(format!(
            "hydratation pas encore disponible pour « {other} » (adaptateur à venir)"
        )),
    }
}

async fn search_books(query: &str) -> Result<SearchOutcome, String> {
    let client = client()?;
    let barcode = looks_like_barcode(query);
    let (bnf, gb, ol) = tokio::join!(
        bnf(&client, query, barcode),
        google_books(&client, query, barcode),
        openlibrary(&client, query, barcode)
    );
    // BNF d'abord : la référence pour le fonds français (BD, mangas).
    combine(vec![bnf, gb, ol])
}

fn s(v: &serde_json::Value) -> Option<String> {
    v.as_str().map(str::to_string).filter(|s| !s.trim().is_empty())
}

// ---------------------------------------------------------------------------
// MusicBrainz (CD audio) — pochettes via Cover Art Archive
// ---------------------------------------------------------------------------

/// Recherche de releases. Par code-barres, ou en texte libre
/// (« artiste album »). L'API impose 1 requête/seconde — respecté par
/// l'appelant (l'enrichissement de masse attend 4 s entre requêtes).
pub(crate) async fn musicbrainz(
    client: &reqwest::Client,
    query: &str,
    barcode: bool,
) -> Result<Vec<Candidate>, String> {
    let q = if barcode {
        format!("barcode:{}", query.replace(['-', ' '], ""))
    } else {
        query.to_string()
    };
    musicbrainz_query(client, &q).await
}

async fn musicbrainz_query(
    client: &reqwest::Client,
    q: &str,
) -> Result<Vec<Candidate>, String> {
    let resp: serde_json::Value = send_with_retry(
        client
        .get("https://musicbrainz.org/ws/2/release/")
        .query(&[("query", q), ("fmt", "json"), ("limit", "8")])
    )
    .await
        .map_err(|e| format!("MusicBrainz : {e}"))?
        .error_for_status()
        .map_err(|e| format!("MusicBrainz : {e}"))?
        .json()
        .await
        .map_err(|e| format!("MusicBrainz : {e}"))?;

    let empty = Vec::new();
    let releases = resp["releases"].as_array().unwrap_or(&empty);
    Ok(releases
        .iter()
        .filter_map(|r| {
            let titre = s(&r["title"])?;
            let mbid = s(&r["id"])?;
            let artistes: Vec<String> = r["artist-credit"]
                .as_array()
                .map(|a| a.iter().filter_map(|c| s(&c["name"])).collect())
                .unwrap_or_default();
            let label = r["label-info"]
                .as_array()
                .and_then(|l| l.first())
                .and_then(|l| s(&l["label"]["name"]));
            Some(Candidate {
                source: "MusicBrainz".into(),
                titre: Some(titre),
                auteurs: artistes,
                illustrateurs: Vec::new(),
                editeur: label,
                date_parution: s(&r["date"]),
                ean: s(&r["barcode"]),
                synopsis: None,
                cover_url: Some(format!("https://coverartarchive.org/release/{mbid}/front-500")),
                score: r["score"].as_i64(),
                genre: None,
                acteurs: Vec::new(),
            })
        })
        .collect())
}

// ---------------------------------------------------------------------------
// Discogs (CD) — genres fiables, éditions précises, pochettes
// ---------------------------------------------------------------------------

/// Genres Discogs (anglais) → libellés du schéma CD par défaut.
fn discogs_genre_fr(genre: &str) -> String {
    match genre {
        "Classical" => "Classique",
        "Electronic" => "Électro",
        "Hip Hop" => "Rap / Hip-hop",
        "Folk, World, & Country" => "Folk",
        "Stage & Screen" => "Bande originale",
        other => other, // Rock, Pop, Jazz, Blues… identiques
    }
    .to_string()
}

pub(crate) async fn discogs(
    client: &reqwest::Client,
    token: &str,
    query: &str,
    barcode: bool,
) -> Result<Vec<Candidate>, String> {
    let mut params: Vec<(&str, &str)> =
        vec![("type", "release"), ("format", "CD"), ("per_page", "8")];
    let cleaned;
    if barcode {
        cleaned = query.replace(['-', ' '], "");
        params.push(("barcode", cleaned.as_str()));
    } else {
        params.push(("q", query));
    }
    let resp: serde_json::Value = send_with_retry(
        client
        .get("https://api.discogs.com/database/search")
        .header("Authorization", format!("Discogs token={}", token.trim()))
        .query(&params)
    )
    .await
        .map_err(|e| format!("Discogs : {e}"))?
        .error_for_status()
        .map_err(|e| {
            if e.status() == Some(reqwest::StatusCode::UNAUTHORIZED) {
                "Discogs : token refusé — vérifiez-le dans discogs.com → Settings → Developers".to_string()
            } else {
                format!("Discogs : {e}")
            }
        })?
        .json()
        .await
        .map_err(|e| format!("Discogs : {e}"))?;

    let empty = Vec::new();
    let results = resp["results"].as_array().unwrap_or(&empty);
    Ok(results
        .iter()
        .filter_map(|r| {
            // Discogs combine « Artiste - Titre » dans un seul champ.
            let full = s(&r["title"])?;
            let (artiste, titre) = match full.split_once(" - ") {
                Some((a, t)) => (a.trim().to_string(), t.trim().to_string()),
                None => (String::new(), full),
            };
            Some(Candidate {
                source: "Discogs".into(),
                titre: Some(titre),
                auteurs: if artiste.is_empty() { Vec::new() } else { vec![artiste] },
                illustrateurs: Vec::new(),
                editeur: r["label"].as_array().and_then(|l| l.first()).and_then(s),
                date_parution: r["year"]
                    .as_str()
                    .map(str::to_string)
                    .or_else(|| r["year"].as_i64().map(|y| y.to_string())),
                ean: barcode.then(|| query.replace(['-', ' '], "")),
                synopsis: None,
                cover_url: s(&r["cover_image"]).filter(|u| !u.contains("spacer.gif")),
                score: None,
                // Le style (précis) d'abord, sinon le genre (large).
                genre: r["style"]
                    .as_array()
                    .and_then(|x| x.first())
                    .and_then(s)
                    .or_else(|| r["genre"].as_array().and_then(|x| x.first()).and_then(s))
                    .map(|g| discogs_genre_fr(&g)),
                acteurs: Vec::new(),
            })
        })
        .collect())
}

/// Variante validée pour l'enrichissement automatique : artiste ET titre
/// doivent correspondre à la fiche.
pub(crate) async fn discogs_strict(
    client: &reqwest::Client,
    token: &str,
    artiste: &str,
    titre: &str,
) -> Result<Vec<Candidate>, String> {
    let candidates = discogs(client, token, &format!("{artiste} {titre}"), false).await?;
    Ok(candidates
        .into_iter()
        .filter(|c| {
            c.titre.as_deref().is_some_and(|t| same_thing(t, titre))
                && c.auteurs.iter().any(|a| same_thing(a, artiste))
        })
        .collect())
}

// ---------------------------------------------------------------------------
// TMDB (DVD / Blu-ray) — recherche par titre, affiches, langue française
// ---------------------------------------------------------------------------

/// Genres TMDB (identifiants stables) → libellés français.
fn tmdb_genre(id: i64) -> Option<&'static str> {
    Some(match id {
        28 => "Action",
        12 => "Aventure",
        16 => "Animation",
        35 => "Comédie",
        99 => "Documentaire",
        18 => "Drame",
        14 => "Fantastique",
        27 => "Horreur",
        878 => "Science-Fiction",
        53 => "Thriller",
        _ => return None,
    })
}

/// Requête TMDB avec clé v3 (`api_key`) ou jeton v4 (`Bearer eyJ…`).
async fn tmdb_get(
    client: &reqwest::Client,
    key: &str,
    path: &str,
    params: &[(&str, &str)],
) -> Result<serde_json::Value, String> {
    let url = format!("https://api.themoviedb.org/3/{path}");
    let mut req = client.get(&url).query(params);
    if key.starts_with("eyJ") {
        req = req.bearer_auth(key);
    } else {
        req = req.query(&[("api_key", key)]);
    }
    let resp = send_with_retry(req).await.map_err(|e| format!("TMDB : {e}"))?;
    if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err("TMDB : clé d'API refusée — vérifiez-la dans themoviedb.org → Paramètres → API".into());
    }
    resp.error_for_status()
        .map_err(|e| format!("TMDB : {e}"))?
        .json()
        .await
        .map_err(|e| format!("TMDB : {e}"))
}

pub(crate) async fn tmdb(
    client: &reqwest::Client,
    key: &str,
    query: &str,
) -> Result<Vec<Candidate>, String> {
    let resp = tmdb_get(
        client,
        key,
        "search/movie",
        &[("query", query), ("language", "fr-FR"), ("include_adult", "false")],
    )
    .await?;
    let empty = Vec::new();
    let results = resp["results"].as_array().unwrap_or(&empty);

    let mut out = Vec::new();
    // Réalisateur/acteurs demandent un appel crédits par film : top 5.
    for movie in results.iter().take(5) {
        let Some(titre) = s(&movie["title"]) else { continue };
        let id = movie["id"].as_i64().unwrap_or(0);
        let credits = tmdb_get(client, key, &format!("movie/{id}/credits"), &[])
            .await
            .unwrap_or(serde_json::Value::Null);
        let realisateurs: Vec<String> = credits["crew"]
            .as_array()
            .map(|crew| {
                crew.iter()
                    .filter(|m| m["job"] == "Director")
                    .filter_map(|m| s(&m["name"]))
                    .collect()
            })
            .unwrap_or_default();
        let acteurs: Vec<String> = credits["cast"]
            .as_array()
            .map(|cast| cast.iter().take(4).filter_map(|m| s(&m["name"])).collect())
            .unwrap_or_default();
        out.push(Candidate {
            source: "TMDB".into(),
            titre: Some(titre),
            auteurs: realisateurs,
            illustrateurs: Vec::new(),
            editeur: None,
            date_parution: s(&movie["release_date"]),
            ean: None,
            synopsis: s(&movie["overview"]),
            cover_url: s(&movie["poster_path"])
                .map(|p| format!("https://image.tmdb.org/t/p/w500{p}")),
            score: movie["vote_count"].as_i64(),
            genre: movie["genre_ids"]
                .as_array()
                .and_then(|ids| ids.iter().filter_map(|i| i.as_i64()).find_map(tmdb_genre))
                .map(str::to_string),
            acteurs,
        });
    }
    Ok(out)
}

/// Rapprochement automatique DVD : titre exact (normalisé) et année à ±1 an
/// quand la fiche en a une.
pub(crate) async fn tmdb_strict(
    client: &reqwest::Client,
    key: &str,
    titre: &str,
    annee: Option<i64>,
) -> Result<Vec<Candidate>, String> {
    let candidates = tmdb(client, key, titre).await?;
    Ok(candidates
        .into_iter()
        .filter(|c| {
            let titre_ok = c.titre.as_deref().is_some_and(|t| same_thing(t, titre));
            let annee_ok = match (annee, &c.date_parution) {
                (Some(a), Some(d)) => d
                    .get(..4)
                    .and_then(|y| y.parse::<i64>().ok())
                    .is_some_and(|y| (y - a).abs() <= 1),
                (Some(_), None) => false,
                (None, _) => true,
            };
            titre_ok && annee_ok
        })
        .collect())
}

/// Minuscules sans accents ni ponctuation, pour comparer artiste/titre.
fn norm(s: &str) -> String {
    s.chars()
        .map(crate::model::unaccent)
        .filter(|c| c.is_ascii_alphanumeric() || c.is_whitespace())
        .collect::<String>()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Deux libellés désignent-ils la même chose ? Égalité normalisée, ou
/// inclusion (« Ballbreaker » vs « Ballbreaker (remastered) »).
fn same_thing(a: &str, b: &str) -> bool {
    let (na, nb) = (norm(a), norm(b));
    if na.is_empty() || nb.is_empty() {
        return false;
    }
    na == nb || (na.len() >= 4 && nb.contains(&na)) || (nb.len() >= 4 && na.contains(&nb))
}

/// Rapprochement automatique CD sans code-barres : requête par champs
/// (artiste ET album), puis ne garde que les candidats dont l'artiste et le
/// titre correspondent vraiment — le score seul ment sur les requêtes libres.
pub(crate) async fn musicbrainz_strict(
    client: &reqwest::Client,
    artiste: &str,
    titre: &str,
) -> Result<Vec<Candidate>, String> {
    let esc = |s: &str| s.replace('"', "");
    let q = format!(
        "release:\"{}\" AND artist:\"{}\" AND status:official",
        esc(titre),
        esc(artiste)
    );
    let candidates = musicbrainz_query(client, &q).await?;
    Ok(candidates
        .into_iter()
        .filter(|c| {
            c.titre.as_deref().is_some_and(|t| same_thing(t, titre))
                && c.auteurs.iter().any(|a| same_thing(a, artiste))
        })
        .collect())
}

// ---------------------------------------------------------------------------
// BNF (SRU, Dublin Core) — la référence pour le fonds français
// ---------------------------------------------------------------------------

/// Valeurs de toutes les occurrences d'une balise dans un fragment XML.
fn xml_tags(xml: &str, tag: &str) -> Vec<String> {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find(&open) {
        let after = &rest[start..];
        let Some(gt) = after.find('>') else { break };
        let Some(end) = after.find(&close) else { break };
        if end > gt {
            out.push(xml_unescape(after[gt + 1..end].trim()));
        }
        rest = &after[end + close.len()..];
    }
    out
}

fn xml_unescape(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}

/// « Toriyama, Akira (1955-2024). Auteur du texte » → « Akira Toriyama ».
fn clean_bnf_name(raw: &str) -> String {
    let mut name = raw;
    if let Some(p) = name.find(" (") {
        name = &name[..p];
    } else if let Some(p) = name.rfind(". ") {
        // Suffixe de rôle (« . Illustrateur ») uniquement — pas les initiales.
        let role = &name[p + 2..];
        if role.chars().next().is_some_and(|c| c.is_uppercase()) && role.len() > 4 {
            name = &name[..p];
        }
    }
    let name = name.trim().trim_end_matches('.');
    match name.split_once(',') {
        Some((nom, prenom)) if !prenom.trim().is_empty() => {
            format!("{} {}", prenom.trim(), nom.trim())
        }
        _ => name.to_string(),
    }
}

pub(crate) async fn bnf(
    client: &reqwest::Client,
    query: &str,
    barcode: bool,
) -> Result<Vec<Candidate>, String> {
    let cql = if barcode {
        format!("bib.isbn all \"{query}\"")
    } else {
        format!("bib.title all \"{}\"", query.replace('"', ""))
    };
    let xml = send_with_retry(
        client
        .get("https://catalogue.bnf.fr/api/SRU")
        .query(&[
            ("version", "1.2"),
            ("operation", "searchRetrieve"),
            ("query", cql.as_str()),
            ("recordSchema", "dublincore"),
            ("maximumRecords", "8"),
        ])
    )
    .await
        .map_err(|e| format!("BNF : {e}"))?
        .error_for_status()
        .map_err(|e| format!("BNF : {e}"))?
        .text()
        .await
        .map_err(|e| format!("BNF : {e}"))?;

    let mut out = Vec::new();
    for record in xml.split("<srw:recordData>").skip(1) {
        let record = record.split("</srw:recordData>").next().unwrap_or("");
        let titre = xml_tags(record, "dc:title").into_iter().next().map(|t| {
            // « Titre / scénario X ; dessin Y » → « Titre »
            t.split(" / ").next().unwrap_or(&t).trim().to_string()
        });
        if titre.is_none() {
            continue;
        }
        let identifiers = xml_tags(record, "dc:identifier");
        let ark = identifiers
            .iter()
            .find_map(|i| i.find("ark:/").map(|p| i[p..].trim_end_matches('/').to_string()));
        let ean = identifiers.iter().find_map(|i| {
            i.strip_prefix("ISBN ")
                .map(|x| x.replace('-', "").trim().to_string())
                .filter(|x| looks_like_barcode(x))
        });
        let contributors = xml_tags(record, "dc:contributor");
        let illustrateurs: Vec<String> = contributors
            .iter()
            .filter(|c| c.contains("Illustrateur") || c.contains("Dessinateur"))
            .map(|c| clean_bnf_name(c))
            .collect();
        out.push(Candidate {
            source: "BNF".into(),
            titre,
            auteurs: xml_tags(record, "dc:creator")
                .iter()
                .map(|c| clean_bnf_name(c))
                .collect(),
            illustrateurs,
            editeur: xml_tags(record, "dc:publisher")
                .into_iter()
                .next()
                .map(|p| p.split(" (").next().unwrap_or(&p).trim().to_string()),
            date_parution: xml_tags(record, "dc:date").into_iter().next(),
            ean: ean.or_else(|| barcode.then(|| query.to_string())),
            synopsis: None,
            cover_url: ark.map(|a| {
                format!("https://catalogue.bnf.fr/couverture?appName=NE&idArk={a}&couverture=1")
            }),
            score: None,
            genre: None,
            acteurs: Vec::new(),
        });
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Google Books
// ---------------------------------------------------------------------------

async fn google_books(
    client: &reqwest::Client,
    query: &str,
    barcode: bool,
) -> Result<Vec<Candidate>, String> {
    let q = if barcode { format!("isbn:{query}") } else { query.to_string() };
    let url = "https://www.googleapis.com/books/v1/volumes";
    let resp: serde_json::Value = send_with_retry(
        client
        .get(url)
        .query(&[("q", q.as_str()), ("maxResults", "8"), ("langRestrict", "fr")])
    )
    .await
        .map_err(|e| format!("Google Books : {e}"))?
        .error_for_status()
        .map_err(|e| format!("Google Books : {e}"))?
        .json()
        .await
        .map_err(|e| format!("Google Books : {e}"))?;

    let empty = Vec::new();
    let items = resp["items"].as_array().unwrap_or(&empty);
    Ok(items
        .iter()
        .filter_map(|item| {
            let info = &item["volumeInfo"];
            let titre = s(&info["title"])?;
            let ean = info["industryIdentifiers"]
                .as_array()
                .and_then(|ids| {
                    ids.iter()
                        .find(|i| i["type"] == "ISBN_13")
                        .or_else(|| ids.iter().find(|i| i["type"] == "ISBN_10"))
                })
                .and_then(|i| s(&i["identifier"]));
            let cover_url = s(&info["imageLinks"]["thumbnail"])
                .map(|u| u.replace("http://", "https://"));
            Some(Candidate {
                source: "Google Books".into(),
                titre: Some(titre),
                illustrateurs: Vec::new(),
                auteurs: info["authors"]
                    .as_array()
                    .map(|a| a.iter().filter_map(s).collect())
                    .unwrap_or_default(),
                editeur: s(&info["publisher"]),
                date_parution: s(&info["publishedDate"]),
                ean,
                synopsis: s(&info["description"]),
                cover_url,
                score: None,
            genre: None,
            acteurs: Vec::new(),
            })
        })
        .collect())
}

// ---------------------------------------------------------------------------
// OpenLibrary
// ---------------------------------------------------------------------------

async fn openlibrary(
    client: &reqwest::Client,
    query: &str,
    barcode: bool,
) -> Result<Vec<Candidate>, String> {
    if barcode {
        let key = format!("ISBN:{query}");
        let resp: serde_json::Value = send_with_retry(
        client
            .get("https://openlibrary.org/api/books")
            .query(&[("bibkeys", key.as_str()), ("format", "json"), ("jscmd", "data")])
    )
    .await
            .map_err(|e| format!("OpenLibrary : {e}"))?
            .error_for_status()
            .map_err(|e| format!("OpenLibrary : {e}"))?
            .json()
            .await
            .map_err(|e| format!("OpenLibrary : {e}"))?;
        let Some(book) = resp.get(&key) else { return Ok(Vec::new()) };
        let names = |v: &serde_json::Value| -> Vec<String> {
            v.as_array()
                .map(|a| a.iter().filter_map(|x| s(&x["name"])).collect())
                .unwrap_or_default()
        };
        return Ok(vec![Candidate {
            source: "OpenLibrary".into(),
            titre: s(&book["title"]),
            illustrateurs: Vec::new(),
            auteurs: names(&book["authors"]),
            editeur: names(&book["publishers"]).into_iter().next(),
            date_parution: s(&book["publish_date"]),
            ean: Some(query.to_string()),
            synopsis: None,
            cover_url: s(&book["cover"]["large"]).or_else(|| s(&book["cover"]["medium"])),
            score: None,
            genre: None,
            acteurs: Vec::new(),
        }]);
    }

    let resp: serde_json::Value = send_with_retry(
        client
        .get("https://openlibrary.org/search.json")
        .query(&[("q", query), ("limit", "8")])
    )
    .await
        .map_err(|e| format!("OpenLibrary : {e}"))?
        .error_for_status()
        .map_err(|e| format!("OpenLibrary : {e}"))?
        .json()
        .await
        .map_err(|e| format!("OpenLibrary : {e}"))?;

    let empty = Vec::new();
    let docs = resp["docs"].as_array().unwrap_or(&empty);
    Ok(docs
        .iter()
        .filter_map(|doc| {
            let titre = s(&doc["title"])?;
            Some(Candidate {
                source: "OpenLibrary".into(),
                titre: Some(titre),
                illustrateurs: Vec::new(),
                auteurs: doc["author_name"]
                    .as_array()
                    .map(|a| a.iter().filter_map(s).collect())
                    .unwrap_or_default(),
                editeur: doc["publisher"].as_array().and_then(|p| p.first()).and_then(s),
                date_parution: doc["first_publish_year"].as_i64().map(|y| y.to_string()),
                ean: doc["isbn"].as_array().and_then(|i| {
                    i.iter().filter_map(s).find(|x| x.len() == 13)
                }),
                synopsis: None,
                cover_url: doc["cover_i"]
                    .as_i64()
                    .map(|id| format!("https://covers.openlibrary.org/b/id/{id}-L.jpg")),
                score: None,
            genre: None,
            acteurs: Vec::new(),
            })
        })
        .collect())
}

// ---------------------------------------------------------------------------
// Candidat → champs du schéma cible
// ---------------------------------------------------------------------------

/// Clé de champ réduite à ses lettres/chiffres : `auteur_s` → `auteurs`.
fn norm_key(key: &str) -> String {
    key.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_lowercase()
}

/// Premier champ du schéma correspondant à l'une des clés canoniques —
/// insensible aux underscores et au pluriel (`auteur_s` ≡ `auteurs` ≡
/// `auteur`), pour que les collections custom marchent sans nommage exact.
fn find_target(schema: &Schema, wanted: &[&str]) -> Option<crate::model::FieldDef> {
    for w in wanted {
        let nw = norm_key(w);
        for f in &schema.fields {
            let nf = norm_key(&f.key);
            if nf == nw || nf == format!("{nw}s") || format!("{nf}s") == nw {
                return Some(f.clone());
            }
        }
    }
    None
}

/// Rapproche un genre proposé des options du champ genre du schéma ; s'il
/// est inconnu, l'ajoute avec un code de cote unique dérivé (même politique
/// que l'import CSV). Renvoie (clé du champ, valeur canonique, ajouté ?).
pub(crate) fn ensure_genre(
    schema: &mut Schema,
    genre: &str,
) -> Option<(String, String, bool)> {
    let key = find_target(schema, &["genre", "style"])?.key;
    let def = schema.fields.iter_mut().find(|f| f.key == key)?;
    if def.field_type != crate::model::FieldType::Select {
        return None;
    }
    if let Some(option) = def.options.iter().find(|o| same_thing(&o.value, genre)) {
        return Some((key, option.value.clone(), false));
    }
    let taken: std::collections::BTreeSet<String> =
        def.options.iter().map(|o| o.effective_code()).collect();
    let mut code = crate::model::derive_code(genre);
    let mut n = 2;
    while taken.contains(&code) {
        code = format!("{}{n}", crate::model::derive_code(genre));
        n += 1;
    }
    def.options.push(crate::model::SelectOption {
        value: genre.to_string(),
        code: Some(code),
    });
    Some((key, genre.to_string(), true))
}

/// Liste de noms → valeur adaptée au type du champ cible (liste, ou texte
/// joint par « ; » si le champ est un simple texte).
fn names_value(field: &crate::model::FieldDef, names: &[String]) -> serde_json::Value {
    match field.field_type {
        crate::model::FieldType::TextList | crate::model::FieldType::Tags => {
            serde_json::json!(names)
        }
        _ => serde_json::json!(names.join(" ; ")),
    }
}

/// Convertit un candidat en champs, en ne gardant que ce qui a un champ
/// correspondant dans le schéma (correspondance tolérante, voir
/// [`find_target`]).
pub fn candidate_to_fields(
    schema: &Schema,
    c: &Candidate,
) -> BTreeMap<String, serde_json::Value> {
    let mut out = BTreeMap::new();

    if let Some(t) = &c.titre {
        if let Some(f) = find_target(schema, &["titre"]) {
            out.insert(f.key, serde_json::json!(t));
        }
    }
    if !c.auteurs.is_empty() {
        if let Some(f) = find_target(schema, &["scenariste", "auteur", "artiste", "realisateur"]) {
            out.insert(f.key.clone(), names_value(&f, &c.auteurs));
        }
    }
    if !c.illustrateurs.is_empty() {
        if let Some(f) = find_target(schema, &["dessinateur", "illustrateur"]) {
            out.insert(f.key.clone(), names_value(&f, &c.illustrateurs));
        }
    }
    if let Some(e) = &c.editeur {
        if let Some(f) = find_target(schema, &["editeur", "label"]) {
            out.insert(f.key, serde_json::json!(e));
        }
    }
    if let Some(d) = &c.date_parution {
        if let Some(f) =
            find_target(schema, &["date_parution", "date_sortie", "date", "annee", "parution"])
        {
            out.insert(f.key, serde_json::json!(d));
        }
    }
    if let Some(syn) = &c.synopsis {
        if let Some(f) = find_target(schema, &["synopsis", "resume", "description"]) {
            out.insert(f.key, serde_json::json!(syn));
        }
    }
    if let Some(ean) = &c.ean {
        if let Some(f) = find_target(schema, &["ean", "isbn", "code_barres"]) {
            out.insert(f.key, serde_json::json!(ean));
        }
    }
    if !c.acteurs.is_empty() {
        if let Some(f) = find_target(schema, &["acteurs"]) {
            out.insert(f.key.clone(), names_value(&f, &c.acteurs));
        }
    }
    // Genre : uniquement s'il correspond à une option existante du schéma —
    // l'hydratation ne crée jamais de nouvelle option, contrairement à
    // l'import CSV.
    if let Some(genre) = &c.genre {
        if let Some(def) = find_target(schema, &["genre", "style"]) {
            if let Some(option) = def.options.iter().find(|o| same_thing(&o.value, genre)) {
                out.insert(def.key.clone(), serde_json::json!(option.value));
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Couvertures : téléchargement → WebP redimensionné
// ---------------------------------------------------------------------------

/// Convertit une image (JPEG/PNG…) en WebP qualité 80, largeur max 400 px.
pub fn to_webp(bytes: &[u8], max_width: u32) -> Result<Vec<u8>, String> {
    let img = image::load_from_memory(bytes).map_err(|e| format!("image illisible : {e}"))?;
    let img = if img.width() > max_width {
        img.resize(max_width, 10_000, image::imageops::FilterType::Lanczos3)
    } else {
        img
    };
    let rgba = img.to_rgba8();
    let encoder = webp::Encoder::from_rgba(&rgba, rgba.width(), rgba.height());
    Ok(encoder.encode(80.0).to_vec())
}

pub async fn fetch_cover_webp(url: &str) -> Result<Vec<u8>, String> {
    let bytes = client()?
        .get(url)
        .send()
        .await
        .map_err(|e| format!("téléchargement couverture : {e}"))?
        .error_for_status()
        .map_err(|e| format!("téléchargement couverture : {e}"))?
        .bytes()
        .await
        .map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || to_webp(&bytes, 400))
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn barcode_detection() {
        assert!(looks_like_barcode("9782344044438"));
        assert!(looks_like_barcode("2-203-06466-9"));
        assert!(!looks_like_barcode("lastman"));
        assert!(!looks_like_barcode("12345"));
    }

    #[test]
    fn bnf_record_parsing() {
        let xml = r#"<srw:recordData><oai_dc:dc>
          <dc:identifier>http://catalogue.bnf.fr/ark:/12148/cb46657982q</dc:identifier>
          <dc:title>L'identité de Merus / scénario, Akira Toriyama ; dessin, Toyotaro</dc:title>
          <dc:creator>Toriyama, Akira (1955-2024). Auteur du texte</dc:creator>
          <dc:contributor>Toyotarō. Illustrateur</dc:contributor>
          <dc:publisher>Glénat (Grenoble)</dc:publisher>
          <dc:date>2020</dc:date>
          <dc:identifier>ISBN 9782344044438</dc:identifier>
        </oai_dc:dc></srw:recordData>"#;
        let record = xml.split("<srw:recordData>").nth(1).unwrap();
        assert_eq!(
            xml_tags(record, "dc:title"),
            vec!["L'identité de Merus / scénario, Akira Toriyama ; dessin, Toyotaro"]
        );
        assert_eq!(clean_bnf_name("Toriyama, Akira (1955-2024). Auteur du texte"), "Akira Toriyama");
        assert_eq!(clean_bnf_name("Toyotarō. Illustrateur"), "Toyotarō");
        assert_eq!(
            clean_bnf_name("Tolkien, John Ronald Reuel (1892-1973). Auteur du texte"),
            "John Ronald Reuel Tolkien"
        );
    }

    #[test]
    fn ensure_genre_matches_or_appends_with_unique_code() {
        let mut schema: Schema =
            serde_yaml::from_str(crate::defaults::DEFAULT_SCHEMAS.iter().find(|(s, _)| *s == "cd").unwrap().1)
                .unwrap();
        // Variante d'accent/graphie → option canonique, rien d'ajouté.
        let (key, value, added) = ensure_genre(&mut schema, "Electro").unwrap();
        assert_eq!((key.as_str(), value.as_str(), added), ("genre", "Électro", false));

        // Genre inconnu → ajouté avec code dérivé.
        let (_, value, added) = ensure_genre(&mut schema, "Italo-Disco").unwrap();
        assert!(added);
        assert_eq!(value, "Italo-Disco");
        let genre = schema.field("genre").unwrap();
        assert!(genre.options.iter().any(|o| o.value == "Italo-Disco"));

        // Redemander le même → rapproché, pas de doublon.
        let (_, _, added) = ensure_genre(&mut schema, "italo disco").unwrap();
        assert!(!added);
        assert_eq!(
            genre.options.len() + 1,
            schema.field("genre").unwrap().options.len() + 1
        );
    }

    #[test]
    fn candidate_maps_to_custom_schema_with_loose_keys() {
        // Collection custom type LDVELH : clés générées depuis les libellés
        // (« Auteur(s) » → auteur_s, champ texte simple).
        let schema: Schema = serde_yaml::from_str(
            "name: LDVELH\nid_prefix: LDV\nfields:\n\
             - { key: titre, label: Titre, type: text, required: true }\n\
             - { key: auteur_s, label: 'Auteur(s)', type: text }\n\
             - { key: date, label: Date, type: date }\n\
             - { key: isbn, label: ISBN, type: text }\n",
        )
        .unwrap();
        let c = Candidate {
            source: "BNF".into(),
            titre: Some("Le sorcier de la montagne de feu".into()),
            auteurs: vec!["Steve Jackson".into(), "Ian Livingstone".into()],
            illustrateurs: vec!["Vlado Krizan".into()],
            editeur: Some("Gallimard jeunesse".into()),
            date_parution: Some("2018".into()),
            ean: Some("9782075100649".into()),
            synopsis: None,
            cover_url: None,
            score: None,
            genre: None,
            acteurs: Vec::new(),
        };
        let fields = candidate_to_fields(&schema, &c);
        // auteur_s ≡ auteurs : rempli, joint en texte car champ simple.
        assert_eq!(
            fields["auteur_s"],
            serde_json::json!("Steve Jackson ; Ian Livingstone")
        );
        assert_eq!(fields["date"], serde_json::json!("2018"));
        assert_eq!(fields["isbn"], serde_json::json!("9782075100649"));
        // Pas de champ dessinateur ni éditeur → ignorés sans erreur.
        assert!(!fields.contains_key("dessinateur"));
    }

    #[test]
    fn candidate_maps_to_bd_schema() {
        let schema: Schema =
            serde_yaml::from_str(crate::defaults::DEFAULT_SCHEMAS[0].1).unwrap();
        let c = Candidate {
            source: "Google Books".into(),
            titre: Some("Lastman T1".into()),
            illustrateurs: Vec::new(),
            auteurs: vec!["Balak".into(), "Bastien Vivès".into()],
            editeur: Some("Casterman".into()),
            date_parution: Some("2013-03-20".into()),
            ean: Some("9782203064669".into()),
            synopsis: Some("Adieu l'école...".into()),
            cover_url: None,
            score: None,
            genre: None,
            acteurs: Vec::new(),
        };
        let fields = candidate_to_fields(&schema, &c);
        assert_eq!(fields["titre"], serde_json::json!("Lastman T1"));
        assert_eq!(fields["scenariste"], serde_json::json!(["Balak", "Bastien Vivès"]));
        assert_eq!(fields["ean"], serde_json::json!("9782203064669"));
        assert_eq!(fields["synopsis"], serde_json::json!("Adieu l'école..."));
        assert!(!fields.contains_key("auteur"));
    }

    #[test]
    fn webp_conversion_resizes() {
        // PNG 800×1200 générée en mémoire.
        let img = image::DynamicImage::new_rgb8(800, 1200);
        let mut png = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let webp_bytes = to_webp(&png, 400).unwrap();
        let back = image::load_from_memory(&webp_bytes).unwrap();
        assert_eq!(back.width(), 400);
        assert_eq!(back.height(), 600);
    }

    #[test]
    fn same_thing_matching() {
        assert!(same_thing("AC/DC", "ACDC"));
        assert!(same_thing("Adéle", "Adele"));
        assert!(same_thing("Ballbreaker", "Ballbreaker (Remastered)"));
        assert!(same_thing("Greatest Hits", "greatest hits"));
        assert!(!same_thing("Greatest Hits", "Greatest Hits Vol. 2") == false); // inclusion tolérée
        assert!(!same_thing("Abba", "Blondie"));
        assert!(!same_thing("21", "12"));
    }

    /// Cas piège réel : « Greatest hits » existe chez des dizaines d'artistes.
    /// `cargo test mb_strict_live -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn mb_strict_live() {
        let client = client().unwrap();
        let results = musicbrainz_strict(&client, "Abba", "Greatest hits").await.unwrap();
        println!("{} candidats validés", results.len());
        for c in results.iter().take(3) {
            println!("  {:?} — {:?} ({:?})", c.titre, c.auteurs, c.date_parution);
        }
        assert!(!results.is_empty());
        for c in &results {
            assert!(
                c.auteurs.iter().any(|a| same_thing(a, "Abba")),
                "artiste étranger accepté : {:?}",
                c.auteurs
            );
        }
    }

    /// Test réseau réel DVD (clé requise) :
    /// `TMDB_KEY=… cargo test tmdb_live -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn tmdb_live() {
        let key = std::env::var("TMDB_KEY").expect("TMDB_KEY non défini");
        let client = client().unwrap();
        let results = tmdb(&client, &key, "Le Cinquième Élément").await.unwrap();
        println!("{} candidats", results.len());
        for c in results.iter().take(3) {
            println!(
                "  {:?} ({:?}) — réal. {:?} — genre {:?} — affiche: {}",
                c.titre, c.date_parution, c.auteurs, c.genre, c.cover_url.is_some()
            );
        }
        assert!(!results.is_empty());
        let strict = tmdb_strict(&client, &key, "Le Cinquième Élément", Some(1997))
            .await
            .unwrap();
        assert!(!strict.is_empty(), "le film de 1997 doit passer le filtre strict");
    }

    /// Test réseau réel CD : `cargo test mb_live -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn mb_live() {
        let client = client().unwrap();
        // Recherche texte libre « artiste album » (cas de son CSV sans EAN).
        let results = musicbrainz(&client, "ACDC Ballbreaker", false).await.unwrap();
        println!("{} candidats", results.len());
        for c in results.iter().take(3) {
            println!(
                "  [{}] {:?} — {:?} ({:?}) score={:?} ean={:?}",
                c.source, c.titre, c.auteurs, c.date_parution, c.score, c.ean
            );
        }
        assert!(!results.is_empty());
        assert!(results[0].score.unwrap_or(0) >= 90, "meilleur score trop bas");

        // Pochette du meilleur candidat via Cover Art Archive.
        let cover = results[0].cover_url.clone().unwrap();
        match fetch_cover_webp(&cover).await {
            Ok(webp) => println!("pochette webp: {} octets", webp.len()),
            Err(e) => println!("pas de pochette pour ce release : {e}"),
        }
    }

    /// Test réseau réel : `cargo test hydrate_live -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn hydrate_live() {
        let by_ean = search("bd", "9782344044438", None, None).await.unwrap().candidates;
        println!("EAN → {} candidats", by_ean.len());
        for c in &by_ean {
            println!("  [{}] {:?} — {:?} ({:?}) cover: {}", c.source, c.titre, c.auteurs, c.date_parution, c.cover_url.is_some());
        }
        assert!(!by_ean.is_empty());

        let by_title = search("bd", "Lastman Balak", None, None).await.unwrap().candidates;
        println!("titre → {} candidats", by_title.len());
        assert!(!by_title.is_empty());

        let cover_url = by_ean.iter().find_map(|c| c.cover_url.clone()).unwrap();
        let webp = fetch_cover_webp(&cover_url).await.unwrap();
        println!("couverture webp: {} octets", webp.len());
        assert!(webp.len() > 1000);
    }
}
