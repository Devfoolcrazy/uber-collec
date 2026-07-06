import { convertFileSrc, invoke } from "@tauri-apps/api/core";

export type FieldType =
  | "text"
  | "longtext"
  | "text[]"
  | "number"
  | "date"
  | "select"
  | "tags"
  | "boolean"
  | "rating"
  | "url"
  | "image"
  | "series_ref";

export interface SelectOption {
  value: string;
  code?: string;
}

export interface FieldDef {
  key: string;
  label: string;
  type: FieldType;
  required?: boolean;
  options?: SelectOption[];
  max?: number;
}

export interface Schema {
  name: string;
  id_prefix: string;
  source?: string;
  cote?: { year_field: string; genre_field: string };
  fields: FieldDef[];
}

export type Statut = "possede" | "souhaite";

export interface SearchFilters {
  statut?: Statut;
  genre?: string;
  annee?: number;
  serie?: string;
}

export type FieldValues = Record<string, unknown>;

export interface Item {
  id: string;
  cote?: string;
  statut: Statut;
  emplacement?: string;
  date_ajout: string;
  [key: string]: unknown;
}

export interface IndexedItem {
  collection: string;
  id: string;
  titre: string;
  cote: string | null;
  statut: Statut;
  emplacement: string | null;
  date_ajout: string;
  serie_nom: string | null;
  serie_tome: number | null;
  annee: number | null;
  data: FieldValues;
}

export interface CollectionInfo {
  slug: string;
  name: string;
  id_prefix: string;
  count: number;
  wishlist_count: number;
}

export interface Series {
  id: string;
  nom: string;
  terminee: boolean;
}

export interface SerieReport {
  id: string;
  nom: string;
  terminee: boolean;
  possedes: number[];
  souhaites: number[];
  manquants: number[];
}

export interface DashboardStats {
  collections: { slug: string; name: string; possede: number; souhaite: number }[];
  total_possede: number;
  total_souhaite: number;
  genres: { collection: string; genre: string; count: number }[];
  annees: { collection: string; annee: number; count: number }[];
  series_incompletes: {
    collection: string;
    collection_name: string;
    nom: string;
    manquants: number[];
  }[];
  series_incompletes_total: number;
}

export interface Candidate {
  source: string;
  titre: string | null;
  auteurs: string[];
  illustrateurs: string[];
  acteurs: string[];
  editeur: string | null;
  date_parution: string | null;
  ean: string | null;
  genre: string | null;
  synopsis: string | null;
  cover_url: string | null;
  score: number | null;
}

/** URL affichable d'une couverture stockée dans la bibliothèque. */
export function coverSrc(libraryPath: string, rel: string): string {
  return convertFileSrc(`${libraryPath}/${rel}`);
}

export interface SyncStatus {
  is_repo: boolean;
  remote: string | null;
  dirty: boolean;
  ahead: number;
  behind: number;
  last_commit: string | null;
}

export interface EnrichProgress {
  running: boolean;
  done: boolean;
  cancel_requested: boolean;
  collection: string;
  total: number;
  processed: number;
  enriched: number;
  covers: number;
  skipped: number;
  no_ean: number;
  not_found: number;
  errors: number;
  last_error: string | null;
  current: string;
}

export interface SourceInfo {
  id: string;
  label: string;
  description: string;
  requires_key: string | null;
  fills: string[];
}

export interface CsvPreview {
  headers: string[];
  rows: string[][];
  total_rows: number;
}

export interface ColumnMapping {
  column: string;
  target: string;
  transform?: string;
}

export interface ImportOptions {
  skip_duplicates: boolean;
  oneshot_if_serie_equals_titre: boolean;
}

export interface ImportReport {
  total_rows: number;
  imported: number;
  skipped_duplicates: number;
  series_created: number;
  genres_added: string[];
  errors: string[];
}

export const api = {
  getLibraryPath: () => invoke<string | null>("get_library_path"),
  createLibrary: (path: string) => invoke<void>("create_library", { path }),
  openLibrary: (path: string) => invoke<void>("open_library", { path }),
  rebuildIndex: () => invoke<number>("rebuild_index"),

  listCollections: () => invoke<CollectionInfo[]>("list_collections"),
  getSchema: (collection: string) => invoke<Schema>("get_schema", { collection }),
  saveSchema: (collection: string, schema: Schema) =>
    invoke<void>("save_schema", { collection, schema }),
  createCollection: (slug: string, schema: Schema) =>
    invoke<void>("create_collection", { slug, schema }),
  deleteCollection: (collection: string) =>
    invoke<number>("delete_collection", { collection }),
  regenerateCotes: (collection: string) =>
    invoke<{ id: string; old: string | null; new: string }[]>("regenerate_cotes", {
      collection,
    }),

  searchItems: (
    collection: string,
    query: string,
    filters: SearchFilters,
    sort: string | null = null,
    desc = false,
    limit = 200,
    offset = 0,
  ) =>
    invoke<IndexedItem[]>("search_items", {
      collection,
      query,
      filters,
      sort,
      desc,
      limit,
      offset,
    }),
  countItems: (collection: string, query: string, filters: SearchFilters) =>
    invoke<number>("count_items", { collection, query, filters }),
  listYears: (collection: string) => invoke<number[]>("list_years", { collection }),
  getItem: (collection: string, id: string) => invoke<Item>("get_item", { collection, id }),
  createItem: (collection: string, statut: Statut, fields: FieldValues) =>
    invoke<Item>("create_item", { collection, statut, fields }),
  updateItem: (
    collection: string,
    id: string,
    statut: Statut,
    emplacement: string | null,
    fields: FieldValues,
  ) => invoke<Item>("update_item", { collection, id, statut, emplacement, fields }),
  deleteItem: (collection: string, id: string) =>
    invoke<void>("delete_item", { collection, id }),

  previewCsv: (path: string) => invoke<CsvPreview>("preview_csv", { path }),
  importCsv: (
    collection: string,
    path: string,
    mappings: ColumnMapping[],
    options: ImportOptions,
  ) => invoke<ImportReport>("import_csv", { collection, path, mappings, options }),

  hydrateSearch: (collection: string, query: string) =>
    invoke<{ candidates: Candidate[]; warnings: string[] }>("hydrate_search", {
      collection,
      query,
    }),
  candidateFields: (collection: string, candidate: Candidate) =>
    invoke<FieldValues>("candidate_fields", { collection, candidate }),
  downloadCover: (collection: string, id: string, url: string) =>
    invoke<string>("download_cover", { collection, id, url }),
  setCoverFromFile: (collection: string, id: string, path: string) =>
    invoke<string>("set_cover_from_file", { collection, id, path }),
  setApiKey: (provider: "tmdb" | "discogs", key: string) =>
    invoke<void>("set_api_key", { provider, key }),
  apiKeysStatus: () => invoke<{ tmdb: boolean; discogs: boolean }>("api_keys_status"),
  hydrationSources: () => invoke<SourceInfo[]>("hydration_sources"),

  isMobile: () => invoke<boolean>("is_mobile"),
  getMobileConfig: () =>
    invoke<{ repo: string | null; has_token: boolean }>("get_mobile_config"),
  mobileSync: (repo?: string, token?: string) =>
    invoke<number>("mobile_sync", { repo: repo ?? null, token: token ?? null }),

  syncStatus: () => invoke<SyncStatus>("sync_status"),
  syncInit: () => invoke<SyncStatus>("sync_init"),
  syncCreateGithub: (name: string, isPrivate: boolean) =>
    invoke<string>("sync_create_github", { name, private: isPrivate }),
  syncSetRemote: (url: string) => invoke<void>("sync_set_remote", { url }),
  syncPush: () => invoke<void>("sync_push"),
  syncPull: () => invoke<boolean>("sync_pull"),

  enrichStart: (collection: string) => invoke<void>("enrich_start", { collection }),
  enrichStatus: () => invoke<EnrichProgress>("enrich_status"),
  enrichCancel: () => invoke<void>("enrich_cancel"),

  seriesReport: (collection: string) =>
    invoke<SerieReport[]>("series_report", { collection }),
  dashboardStats: () => invoke<DashboardStats>("dashboard_stats"),

  listSeries: (collection: string) => invoke<Series[]>("list_series", { collection }),
  upsertSeries: (collection: string, series: Series) =>
    invoke<void>("upsert_series", { collection, series }),
};
