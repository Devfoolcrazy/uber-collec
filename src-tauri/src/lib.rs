mod defaults;
mod enrich;
mod hydrate;
mod import;
mod index;
mod mobile;
mod model;
mod stats;
mod store;
mod sync;

use index::{Index, IndexedItem};
use model::{Item, Schema, Series, Statut};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;
use store::Library;
use tauri::{AppHandle, Manager, State};

/// État global : bibliothèque ouverte + index associé.
#[derive(Default)]
pub(crate) struct AppState {
    pub(crate) library: Option<Library>,
    pub(crate) index: Option<Index>,
}

type SharedState<'a> = State<'a, Mutex<AppState>>;

#[derive(Debug, Default, Serialize, Deserialize)]
struct Config {
    library_path: Option<String>,
    /// Mode mobile : dépôt GitHub source de l'instantané (« owner/nom »).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    github_repo: Option<String>,
    /// Token GitHub en lecture (mode mobile uniquement).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    github_token: Option<String>,
    /// Clé d'API TMDB (adaptateur DVD).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tmdb_api_key: Option<String>,
    /// Token personnel Discogs (seconde source CD).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    discogs_token: Option<String>,
}

fn config_path(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_config_dir()
        .map(|d| d.join("config.json"))
        .map_err(|e| e.to_string())
}

/// (clé TMDB, token Discogs) depuis la configuration.
pub(crate) fn api_keys_from_config(app: &AppHandle) -> (Option<String>, Option<String>) {
    match load_config(app) {
        Ok(c) => (c.tmdb_api_key, c.discogs_token),
        Err(_) => (None, None),
    }
}

fn load_config(app: &AppHandle) -> Result<Config, String> {
    let path = config_path(app)?;
    if !path.exists() {
        return Ok(Config::default());
    }
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    serde_json::from_str(&text).map_err(|e| e.to_string())
}

fn save_config(app: &AppHandle, config: &Config) -> Result<(), String> {
    let path = config_path(app)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, serde_json::to_string_pretty(config).unwrap())
        .map_err(|e| e.to_string())
}

/// Ouvre la bibliothèque et son index ; reconstruit l'index si celui-ci ne
/// correspond pas à cette bibliothèque (index jetable).
fn attach_library(app: &AppHandle, state: &SharedState, path: &str) -> Result<(), String> {
    let library = Library::open(path)?;
    // Autorise le protocole asset:// sur la bibliothèque, pour afficher les
    // couvertures directement depuis le disque.
    app.asset_protocol_scope()
        .allow_directory(&library.root, true)
        .map_err(|e| e.to_string())?;
    let index_path = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("index.sqlite");
    let mut index = Index::open(&index_path)?;
    if index.meta("library_path").as_deref() != Some(path) {
        index.rebuild(&library)?;
        index.set_meta("library_path", path)?;
    }
    let mut guard = state.lock().unwrap();
    guard.library = Some(library);
    guard.index = Some(index);
    let mut config = load_config(app)?;
    config.library_path = Some(path.to_string());
    save_config(app, &config)
}

/// Racine de la bibliothèque ouverte (pour les commandes async : on extrait
/// le chemin puis on relâche le verrou avant tout travail long).
fn lib_root(state: &State<'_, Mutex<AppState>>) -> Result<std::path::PathBuf, String> {
    let guard = state.lock().unwrap();
    guard
        .library
        .as_ref()
        .map(|l| l.root.clone())
        .ok_or_else(|| "aucune bibliothèque ouverte".into())
}

/// Exécute `f` avec la bibliothèque et l'index ouverts.
fn with_state<T>(
    state: &SharedState,
    f: impl FnOnce(&Library, &mut Index) -> Result<T, String>,
) -> Result<T, String> {
    let mut guard = state.lock().unwrap();
    let AppState { library, index } = &mut *guard;
    match (library, index) {
        (Some(lib), Some(idx)) => f(lib, idx),
        _ => Err("aucune bibliothèque ouverte".into()),
    }
}

// ---------------------------------------------------------------------------
// Commandes : bibliothèque
// ---------------------------------------------------------------------------

#[tauri::command]
fn get_library_path(app: AppHandle, state: SharedState) -> Result<Option<String>, String> {
    // Réouverture automatique de la dernière bibliothèque au démarrage.
    let config = load_config(&app)?;
    if let Some(path) = &config.library_path {
        if state.lock().unwrap().library.is_none() {
            if let Err(e) = attach_library(&app, &state, path) {
                eprintln!("réouverture de {path} impossible : {e}");
                return Ok(None);
            }
        }
        return Ok(Some(path.clone()));
    }
    Ok(None)
}

#[tauri::command]
fn create_library(app: AppHandle, state: SharedState, path: String) -> Result<(), String> {
    Library::create(&path)?;
    attach_library(&app, &state, &path)
}

#[tauri::command]
fn open_library(app: AppHandle, state: SharedState, path: String) -> Result<(), String> {
    attach_library(&app, &state, &path)
}

#[tauri::command]
fn rebuild_index(state: SharedState) -> Result<u64, String> {
    with_state(&state, |lib, idx| idx.rebuild(lib))
}

// ---------------------------------------------------------------------------
// Commandes : collections et schémas
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct CollectionInfo {
    slug: String,
    name: String,
    id_prefix: String,
    count: u64,
    wishlist_count: u64,
}

#[tauri::command]
fn list_collections(state: SharedState) -> Result<Vec<CollectionInfo>, String> {
    with_state(&state, |lib, idx| {
        lib.collections()?
            .into_iter()
            .map(|slug| {
                let schema = lib.load_schema(&slug)?;
                Ok(CollectionInfo {
                    count: idx.count(&slug, Some("possede"))?,
                    wishlist_count: idx.count(&slug, Some("souhaite"))?,
                    slug,
                    name: schema.name,
                    id_prefix: schema.id_prefix,
                })
            })
            .collect()
    })
}

#[tauri::command]
fn get_schema(state: SharedState, collection: String) -> Result<Schema, String> {
    with_state(&state, |lib, _| lib.load_schema(&collection))
}

#[tauri::command]
fn save_schema(
    app: AppHandle,
    state: SharedState,
    collection: String,
    schema: Schema,
) -> Result<(), String> {
    schema.validate()?;
    with_state(&state, |lib, _| lib.save_schema(&collection, &schema))?;
    sync::auto_commit(&app, format!("Schéma « {} » modifié", schema.name));
    Ok(())
}

#[tauri::command]
fn create_collection(
    app: AppHandle,
    state: SharedState,
    slug: String,
    schema: Schema,
) -> Result<(), String> {
    schema.validate()?;
    with_state(&state, |lib, _| lib.create_collection(&slug, &schema))?;
    sync::auto_commit(&app, format!("Nouvelle collection « {} »", schema.name));
    Ok(())
}

#[tauri::command]
fn delete_collection(
    app: AppHandle,
    state: SharedState,
    collection: String,
) -> Result<usize, String> {
    let count = with_state(&state, |lib, idx| {
        let count = lib.delete_collection(&collection)?;
        idx.remove_collection(&collection)?;
        Ok(count)
    })?;
    sync::auto_commit(&app, format!("Suppression de la collection {collection} ({count} objets)"));
    Ok(count)
}

// ---------------------------------------------------------------------------
// Commandes : objets
// ---------------------------------------------------------------------------

#[tauri::command]
fn count_items(
    state: SharedState,
    collection: String,
    query: Option<String>,
    filters: Option<index::SearchFilters>,
) -> Result<u64, String> {
    with_state(&state, |_, idx| {
        idx.count_search(
            &collection,
            query.as_deref().unwrap_or(""),
            &filters.unwrap_or_default(),
        )
    })
}

#[tauri::command]
fn list_years(state: SharedState, collection: String) -> Result<Vec<i64>, String> {
    with_state(&state, |_, idx| idx.list_years(&collection))
}

#[tauri::command]
fn regenerate_cotes(
    state: SharedState,
    collection: String,
) -> Result<Vec<store::CoteChange>, String> {
    with_state(&state, |lib, idx| {
        let changes = lib.regenerate_stale_cotes(&collection)?;
        if !changes.is_empty() {
            let schema = lib.load_schema(&collection)?;
            let series = lib.load_series(&collection)?;
            idx.bulk_begin()?;
            for change in &changes {
                let item = lib.load_item(&collection, &change.id)?;
                idx.upsert_item(&collection, &schema, &series, &item)?;
            }
            idx.bulk_commit()?;
        }
        Ok(changes)
    })
}

#[tauri::command]
fn search_items(
    state: SharedState,
    collection: String,
    query: Option<String>,
    filters: Option<index::SearchFilters>,
    sort: Option<String>,
    desc: Option<bool>,
    limit: Option<u32>,
    offset: Option<u32>,
) -> Result<Vec<IndexedItem>, String> {
    with_state(&state, |_, idx| {
        idx.search(
            &collection,
            query.as_deref().unwrap_or(""),
            &filters.unwrap_or_default(),
            sort.as_deref(),
            desc.unwrap_or(false),
            limit.unwrap_or(100),
            offset.unwrap_or(0),
        )
    })
}

#[tauri::command]
fn get_item(state: SharedState, collection: String, id: String) -> Result<Item, String> {
    with_state(&state, |lib, _| lib.load_item(&collection, &id))
}

/// Libellé d'un objet pour les messages de commit.
fn item_label(schema: &Schema, item: &Item) -> String {
    schema
        .title_field()
        .and_then(|f| item.fields.get(&f.key))
        .and_then(|v| v.as_str())
        .map(|t| format!("{} — {}", item.id, t))
        .unwrap_or_else(|| item.id.clone())
}

#[tauri::command]
fn create_item(
    app: AppHandle,
    state: SharedState,
    collection: String,
    statut: Statut,
    fields: BTreeMap<String, serde_json::Value>,
) -> Result<Item, String> {
    let item = with_state(&state, |lib, idx| {
        let item = lib.create_item(&collection, statut, fields)?;
        let schema = lib.load_schema(&collection)?;
        let series = lib.load_series(&collection)?;
        idx.upsert_item(&collection, &schema, &series, &item)?;
        Ok((item, schema))
    })?;
    sync::auto_commit(&app, format!("Ajout {}", item_label(&item.1, &item.0)));
    Ok(item.0)
}

#[tauri::command]
fn update_item(
    app: AppHandle,
    state: SharedState,
    collection: String,
    id: String,
    statut: Statut,
    emplacement: Option<String>,
    fields: BTreeMap<String, serde_json::Value>,
) -> Result<Item, String> {
    let (item, schema) = with_state(&state, |lib, idx| {
        let item = lib.update_item(&collection, &id, statut, emplacement, fields)?;
        let schema = lib.load_schema(&collection)?;
        let series = lib.load_series(&collection)?;
        idx.upsert_item(&collection, &schema, &series, &item)?;
        Ok((item, schema))
    })?;
    sync::auto_commit(&app, format!("Modification {}", item_label(&schema, &item)));
    Ok(item)
}

#[tauri::command]
fn delete_item(
    app: AppHandle,
    state: SharedState,
    collection: String,
    id: String,
) -> Result<(), String> {
    with_state(&state, |lib, idx| {
        lib.delete_item(&collection, &id)?;
        idx.remove_item(&collection, &id)
    })?;
    sync::auto_commit(&app, format!("Suppression {id}"));
    Ok(())
}

// ---------------------------------------------------------------------------
// Commandes : mode mobile (iOS — consultation seule)
// ---------------------------------------------------------------------------

#[tauri::command]
fn is_mobile() -> bool {
    cfg!(any(target_os = "ios", target_os = "android"))
}

#[derive(Serialize)]
struct MobileConfig {
    repo: Option<String>,
    has_token: bool,
}

#[tauri::command]
fn get_mobile_config(app: AppHandle) -> Result<MobileConfig, String> {
    let config = load_config(&app)?;
    Ok(MobileConfig {
        repo: config.github_repo,
        has_token: config.github_token.is_some(),
    })
}

/// Télécharge l'instantané GitHub, l'installe comme bibliothèque et
/// reconstruit l'index. `repo`/`token` absents → réutilise la config.
#[tauri::command]
async fn mobile_sync(
    app: AppHandle,
    state: State<'_, Mutex<AppState>>,
    repo: Option<String>,
    token: Option<String>,
) -> Result<u64, String> {
    let mut config = load_config(&app)?;
    if let Some(r) = repo {
        config.github_repo = Some(r.trim().to_string());
    }
    if let Some(t) = token.filter(|t| !t.trim().is_empty()) {
        config.github_token = Some(t.trim().to_string());
    }
    let repo = config.github_repo.clone().ok_or("dépôt GitHub non configuré")?;
    let token = config.github_token.clone().ok_or("token GitHub non configuré")?;
    save_config(&app, &config)?;

    let dest = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("bibliotheque");
    mobile::fetch_snapshot(&repo, &token, &dest).await?;

    let path = dest.to_string_lossy().into_owned();
    attach_library(&app, &state, &path)?;
    // Le contenu a changé sous le même chemin : reconstruction systématique.
    with_state(&state, |lib, idx| idx.rebuild(lib))
}

// ---------------------------------------------------------------------------
// Commandes : synchronisation Git
// ---------------------------------------------------------------------------

#[tauri::command]
async fn sync_status(state: State<'_, Mutex<AppState>>) -> Result<sync::SyncStatus, String> {
    let root = lib_root(&state)?;
    tauri::async_runtime::spawn_blocking(move || sync::status(&root))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn sync_init(state: State<'_, Mutex<AppState>>) -> Result<sync::SyncStatus, String> {
    let root = lib_root(&state)?;
    tauri::async_runtime::spawn_blocking(move || {
        let login = sync::gh_login(&root);
        sync::init(&root, login.as_deref())?;
        Ok(sync::status(&root))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn sync_create_github(
    state: State<'_, Mutex<AppState>>,
    name: String,
    private: bool,
) -> Result<String, String> {
    let root = lib_root(&state)?;
    tauri::async_runtime::spawn_blocking(move || sync::create_github_repo(&root, &name, private))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn sync_set_remote(state: State<'_, Mutex<AppState>>, url: String) -> Result<(), String> {
    let root = lib_root(&state)?;
    tauri::async_runtime::spawn_blocking(move || sync::set_remote(&root, &url))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn sync_push(state: State<'_, Mutex<AppState>>) -> Result<(), String> {
    let root = lib_root(&state)?;
    tauri::async_runtime::spawn_blocking(move || sync::push(&root))
        .await
        .map_err(|e| e.to_string())?
}

/// Pull fast-forward ; renvoie true si des changements sont arrivés (le
/// front doit alors demander une reconstruction d'index).
#[tauri::command]
async fn sync_pull(state: State<'_, Mutex<AppState>>) -> Result<bool, String> {
    let root = lib_root(&state)?;
    tauri::async_runtime::spawn_blocking(move || sync::pull(&root))
        .await
        .map_err(|e| e.to_string())?
}

// ---------------------------------------------------------------------------
// Commandes : hydratation
// ---------------------------------------------------------------------------

#[tauri::command]
async fn hydrate_search(
    app: AppHandle,
    state: State<'_, Mutex<AppState>>,
    collection: String,
    query: String,
) -> Result<Vec<hydrate::Candidate>, String> {
    // Le verrou ne doit jamais traverser un await : on extrait la source puis
    // on le relâche avant les appels réseau.
    let source = {
        let guard = state.lock().unwrap();
        let lib = guard.library.as_ref().ok_or("aucune bibliothèque ouverte")?;
        lib.load_schema(&collection)?
            .source
            .ok_or("cette collection n'a pas de source d'hydratation associée")?
    };
    let (tmdb_key, discogs_token) = api_keys_from_config(&app);
    hydrate::search(&source, query.trim(), tmdb_key.as_deref(), discogs_token.as_deref()).await
}

#[tauri::command]
fn set_api_key(app: AppHandle, provider: String, key: String) -> Result<(), String> {
    let mut config = load_config(&app)?;
    let value = Some(key.trim().to_string()).filter(|k| !k.is_empty());
    match provider.as_str() {
        "tmdb" => config.tmdb_api_key = value,
        "discogs" => config.discogs_token = value,
        other => return Err(format!("fournisseur inconnu : {other}")),
    }
    save_config(&app, &config)
}

#[derive(Serialize)]
struct ApiKeysStatus {
    tmdb: bool,
    discogs: bool,
}

#[tauri::command]
fn api_keys_status(app: AppHandle) -> Result<ApiKeysStatus, String> {
    let config = load_config(&app)?;
    Ok(ApiKeysStatus {
        tmdb: config.tmdb_api_key.is_some(),
        discogs: config.discogs_token.is_some(),
    })
}

#[tauri::command]
fn candidate_fields(
    state: SharedState,
    collection: String,
    candidate: hydrate::Candidate,
) -> Result<BTreeMap<String, serde_json::Value>, String> {
    with_state(&state, |lib, _| {
        let schema = lib.load_schema(&collection)?;
        Ok(hydrate::candidate_to_fields(&schema, &candidate))
    })
}

/// Télécharge une couverture, la convertit en WebP (≤ 400 px), l'enregistre
/// dans `images/<collection>/<id>.webp` et met la fiche à jour.
#[tauri::command]
async fn download_cover(
    app: AppHandle,
    state: State<'_, Mutex<AppState>>,
    collection: String,
    id: String,
    url: String,
) -> Result<String, String> {
    let root = {
        let guard = state.lock().unwrap();
        guard
            .library
            .as_ref()
            .ok_or("aucune bibliothèque ouverte")?
            .root
            .clone()
    };
    let webp_bytes = hydrate::fetch_cover_webp(&url).await?;
    let rel = format!("images/{collection}/{id}.webp");
    let path = root.join(&rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, webp_bytes).map_err(|e| e.to_string())?;

    let guard = state.lock().unwrap();
    let AppState { library, index } = &*guard;
    let (lib, idx) = match (library, index) {
        (Some(l), Some(i)) => (l, i),
        _ => return Err("aucune bibliothèque ouverte".into()),
    };
    let schema = lib.load_schema(&collection)?;
    let image_key = schema
        .fields
        .iter()
        .find(|f| f.field_type == model::FieldType::Image)
        .map(|f| f.key.clone())
        .ok_or("le schéma n'a pas de champ image")?;
    let mut item = lib.load_item(&collection, &id)?;
    item.fields.insert(image_key, serde_json::json!(rel));
    lib.save_item(&collection, &item)?;
    let series = lib.load_series(&collection)?;
    idx.upsert_item(&collection, &schema, &series, &item)?;
    drop(guard);
    sync::auto_commit(&app, format!("Couverture {id}"));
    Ok(rel)
}

// ---------------------------------------------------------------------------
// Commandes : enrichissement de masse
// ---------------------------------------------------------------------------

#[tauri::command]
fn enrich_start(app: AppHandle, collection: String) -> Result<(), String> {
    let progress = app.state::<enrich::SharedProgress>();
    {
        let mut p = progress.lock().unwrap();
        if p.running {
            return Err("un enrichissement est déjà en cours".into());
        }
        *p = enrich::EnrichProgress {
            running: true,
            collection: collection.clone(),
            ..Default::default()
        };
    }
    let handle = app.clone();
    tauri::async_runtime::spawn(enrich::run(handle, collection));
    Ok(())
}

#[tauri::command]
fn enrich_status(app: AppHandle) -> enrich::EnrichProgress {
    app.state::<enrich::SharedProgress>().lock().unwrap().clone()
}

#[tauri::command]
fn enrich_cancel(app: AppHandle) {
    app.state::<enrich::SharedProgress>().lock().unwrap().cancel_requested = true;
}

// ---------------------------------------------------------------------------
// Commandes : import CSV
// ---------------------------------------------------------------------------

#[tauri::command]
fn preview_csv(path: String) -> Result<import::CsvPreview, String> {
    import::preview_csv(&path)
}

#[tauri::command]
fn import_csv(
    app: AppHandle,
    state: SharedState,
    collection: String,
    path: String,
    mappings: Vec<import::ColumnMapping>,
    options: import::ImportOptions,
) -> Result<import::ImportReport, String> {
    let report = with_state(&state, |lib, idx| {
        import::run_import(lib, idx, &collection, &path, &mappings, &options)
    })?;
    if report.imported > 0 {
        sync::auto_commit(
            &app,
            format!("Import CSV : {} objets dans {collection}", report.imported),
        );
    }
    Ok(report)
}

// ---------------------------------------------------------------------------
// Commandes : séries et tableau de bord
// ---------------------------------------------------------------------------

#[tauri::command]
fn series_report(state: SharedState, collection: String) -> Result<Vec<stats::SerieReport>, String> {
    with_state(&state, |lib, idx| stats::series_report(lib, idx, &collection))
}

#[tauri::command]
fn dashboard_stats(state: SharedState) -> Result<stats::DashboardStats, String> {
    with_state(&state, |lib, idx| stats::dashboard(lib, idx))
}

#[tauri::command]
fn list_series(state: SharedState, collection: String) -> Result<Vec<Series>, String> {
    with_state(&state, |lib, _| lib.load_series(&collection))
}

#[tauri::command]
fn upsert_series(
    app: AppHandle,
    state: SharedState,
    collection: String,
    series: Series,
) -> Result<(), String> {
    let nom = series.nom.clone();
    with_state(&state, |lib, _| lib.upsert_series(&collection, series))?;
    sync::auto_commit(&app, format!("Série « {nom} »"));
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(Mutex::new(AppState::default()))
        .manage(Mutex::new(enrich::EnrichProgress::default()))
        .invoke_handler(tauri::generate_handler![
            get_library_path,
            create_library,
            open_library,
            rebuild_index,
            list_collections,
            get_schema,
            save_schema,
            create_collection,
            delete_collection,
            search_items,
            count_items,
            list_years,
            regenerate_cotes,
            get_item,
            create_item,
            update_item,
            delete_item,
            preview_csv,
            import_csv,
            is_mobile,
            get_mobile_config,
            mobile_sync,
            sync_status,
            sync_init,
            sync_create_github,
            sync_set_remote,
            sync_push,
            sync_pull,
            hydrate_search,
            candidate_fields,
            download_cover,
            set_api_key,
            api_keys_status,
            enrich_start,
            enrich_status,
            enrich_cancel,
            list_series,
            upsert_series,
            series_report,
            dashboard_stats,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
