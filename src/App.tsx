import { useCallback, useEffect, useRef, useState } from "react";
import { confirm, open, save } from "@tauri-apps/plugin-dialog";
import {
  api,
  Candidate,
  CollectionInfo,
  EnrichProgress,
  FieldValues,
  ImportReport,
  IndexedItem,
  Item,
  Schema,
  SearchFilters,
  Series,
  Statut,
} from "./api";
import { coverSrc } from "./api";
import ItemForm from "./components/ItemForm";
import ItemView from "./components/ItemView";
import ImportWizard from "./components/ImportWizard";
import HydrateSearch from "./components/HydrateSearch";
import SeriesPanel from "./components/SeriesPanel";
import Dashboard from "./components/Dashboard";
import SchemaEditor from "./components/SchemaEditor";
import SyncPanel from "./components/SyncPanel";
import MobileSetup from "./components/MobileSetup";
import ApiKeysPanel from "./components/ApiKeysPanel";
import LabelsPanel from "./components/LabelsPanel";
import { SyncStatus } from "./api";
import "./App.css";

type StatutFilter = Statut | "tous";

const PAGE_SIZE = 200;

export default function App() {
  const [mobile, setMobile] = useState(false);
  const [libraryPath, setLibraryPath] = useState<string | null | undefined>(undefined);
  const [collections, setCollections] = useState<CollectionInfo[]>([]);
  const [current, setCurrent] = useState<string | null>(null);
  const [schema, setSchema] = useState<Schema | null>(null);
  const [items, setItems] = useState<IndexedItem[]>([]);
  const [total, setTotal] = useState(0);
  const [loadingMore, setLoadingMore] = useState(false);
  const [query, setQuery] = useState("");
  const [statutFilter, setStatutFilter] = useState<StatutFilter>("tous");
  const [genreFilter, setGenreFilter] = useState("");
  const [anneeFilter, setAnneeFilter] = useState("");
  const [serieFilter, setSerieFilter] = useState("");
  const [years, setYears] = useState<number[]>([]);
  const [seriesList, setSeriesList] = useState<Series[]>([]);
  const [editing, setEditing] = useState<Item | null>(null);
  const [viewing, setViewing] = useState<Item | null>(null);
  const [showForm, setShowForm] = useState(false);
  const [showImport, setShowImport] = useState(false);
  const [scanMode, setScanMode] = useState(false);
  const [enrichMode, setEnrichMode] = useState(false);
  const [draftFields, setDraftFields] = useState<FieldValues | null>(null);
  const [draftCoverUrl, setDraftCoverUrl] = useState<string | null>(null);
  const [enrichProgress, setEnrichProgress] = useState<EnrichProgress | null>(null);
  const enrichTimer = useRef<ReturnType<typeof setInterval> | null>(null);
  const [sortKey, setSortKey] = useState<string | null>(null);
  const [sortDesc, setSortDesc] = useState(false);
  const [viewMode, setViewMode] = useState<"list" | "grid">(
    () => (localStorage.getItem("viewMode") as "list" | "grid") ?? "list",
  );
  const [theme, setTheme] = useState<"auto" | "light" | "dark">(
    () => (localStorage.getItem("theme") as "auto" | "light" | "dark") ?? "auto",
  );

  // Thème : « auto » suit macOS ; « clair »/« sombre » forcent via data-theme.
  useEffect(() => {
    if (theme === "auto") delete document.documentElement.dataset.theme;
    else document.documentElement.dataset.theme = theme;
    localStorage.setItem("theme", theme);
  }, [theme]);
  const [showDashboard, setShowDashboard] = useState(false);
  const [showLabels, setShowLabels] = useState(false);
  const [labelsCount, setLabelsCount] = useState(0);
  const [schemaEditor, setSchemaEditor] = useState<"create" | "edit" | null>(null);
  const [syncStatus, setSyncStatus] = useState<SyncStatus | null>(null);
  const [showSyncPanel, setShowSyncPanel] = useState(false);
  const [showApiKeys, setShowApiKeys] = useState(false);
  const [pushing, setPushing] = useState(false);
  const [colTab, setColTab] = useState<"objets" | "series">("objets");
  const pendingTab = useRef<"objets" | "series" | null>(null);

  function switchView(mode: "list" | "grid") {
    setViewMode(mode);
    localStorage.setItem("viewMode", mode);
  }
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  // Bannière de confirmation, effacée automatiquement.
  useEffect(() => {
    if (!notice) return;
    const t = setTimeout(() => setNotice(null), 5000);
    return () => clearTimeout(t);
  }, [notice]);

  async function startEnrich() {
    if (!current || !schema) return;
    const ok = await confirm(
      `Compléter automatiquement les fiches de « ${schema.name} » depuis la BNF ?\n\n` +
        `— Seules les fiches SANS couverture et AVEC un EAN sont traitées.\n` +
        `— Seuls les champs vides sont complétés, jamais d'écrasement.\n` +
        `— Rythme volontairement lent (~8 s par fiche) : comptez plusieurs heures.\n` +
        `— L'app doit rester ouverte et le Mac éveillé.\n\n` +
        `Interruptible et rejouable à tout moment.`,
      { title: "Enrichissement de masse", kind: "info" },
    );
    if (!ok) return;
    try {
      await api.enrichStart(current);
      setEnrichProgress(await api.enrichStatus());
      watchEnrich();
    } catch (e) {
      setError(String(e));
    }
  }

  const refreshCollections = useCallback(async () => {
    const cols = await api.listCollections();
    setCollections(cols);
    api.labelsCount().then(setLabelsCount).catch(() => {});
    return cols;
  }, []);

  const refreshSync = useCallback(async () => {
    try {
      setSyncStatus(await api.syncStatus());
    } catch {
      setSyncStatus(null);
    }
  }, []);

  // Statut git : au chargement, puis toutes les 60 s (les commits/push
  // automatiques tournent en fond).
  useEffect(() => {
    const t = setInterval(() => void refreshSync(), 60000);
    return () => clearInterval(t);
  }, [refreshSync]);

  // Démarrage : réouverture de la dernière bibliothèque.
  useEffect(() => {
    api.isMobile().then(setMobile).catch(() => {});
    api
      .getLibraryPath()
      .then(async (path) => {
        setLibraryPath(path);
        if (path) {
          const cols = await refreshCollections();
          if (cols.length > 0) setCurrent(cols[0].slug);
          // Pull au démarrage : d'autres machines (iOS…) ont pu écrire.
          await refreshSync();
          try {
            if (await api.syncPull()) {
              await api.rebuildIndex();
              await refreshCollections();
              setNotice("Données mises à jour depuis GitHub");
            }
          } catch {
            /* hors ligne : sans gravité */
          }
        }
      })
      .catch((e) => {
        setLibraryPath(null);
        setError(String(e));
      });
  }, [refreshCollections]);

  // Changement de collection → schéma + objets.
  useEffect(() => {
    if (!current) return;
    setShowForm(false);
    setShowImport(false);
    setScanMode(false);
    setEnrichMode(false);
    setEditing(null);
    setViewing(null);
    setGenreFilter("");
    setAnneeFilter("");
    setSerieFilter("");
    setSortKey(null);
    setSortDesc(false);
    setColTab(pendingTab.current ?? "objets");
    pendingTab.current = null;
    setSchemaEditor(null);
    api.getSchema(current).then(setSchema).catch((e) => setError(String(e)));
    api.listYears(current).then(setYears).catch(() => setYears([]));
    api.listSeries(current).then(setSeriesList).catch(() => setSeriesList([]));
  }, [current]);

  const filters: SearchFilters = {
    statut: statutFilter === "tous" ? undefined : statutFilter,
    genre: genreFilter || undefined,
    annee: anneeFilter ? Number(anneeFilter) : undefined,
    serie: serieFilter || undefined,
  };
  const filtersKey = JSON.stringify(filters);

  const refreshItems = useCallback(async () => {
    if (!current) return;
    try {
      const f: SearchFilters = JSON.parse(filtersKey);
      const [page, count] = await Promise.all([
        api.searchItems(current, query, f, sortKey, sortDesc, PAGE_SIZE, 0),
        api.countItems(current, query, f),
      ]);
      setItems(page);
      setTotal(count);
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, [current, query, filtersKey, sortKey, sortDesc]);

  async function loadMore() {
    if (!current || loadingMore) return;
    setLoadingMore(true);
    try {
      const page = await api.searchItems(
        current,
        query,
        filters,
        sortKey,
        sortDesc,
        PAGE_SIZE,
        items.length,
      );
      setItems((prev) => [...prev, ...page]);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoadingMore(false);
    }
  }

  useEffect(() => {
    void refreshItems();
  }, [refreshItems]);

  // Suivi de l'enrichissement de masse (toutes les 3 s tant qu'il tourne).
  const watchEnrich = useCallback(() => {
    if (enrichTimer.current) return;
    enrichTimer.current = setInterval(async () => {
      try {
        const p = await api.enrichStatus();
        setEnrichProgress(p);
        if (!p.running) {
          if (enrichTimer.current) clearInterval(enrichTimer.current);
          enrichTimer.current = null;
          if (p.done && p.processed > 0) {
            // Des genres ont pu être comblés : les cotes AUTRE se régénèrent.
            let cotesNote = "";
            if (p.collection) {
              try {
                const changes = await api.regenerateCotes(p.collection);
                if (changes.length > 0) cotesNote = `, ${changes.length} cotes régénérées`;
              } catch {
                /* non bloquant */
              }
            }
            setNotice(
              `Enrichissement terminé : ${p.covers} couvertures, ${p.enriched} fiches complétées` +
                (p.not_found ? `, ${p.not_found} introuvables` : "") +
                (p.no_ean ? `, ${p.no_ean} sans clé de recherche` : "") +
                cotesNote,
            );
            await Promise.all([refreshItems(), refreshCollections()]);
          }
        }
      } catch {
        /* backend indisponible ce tick : on réessaie au suivant */
      }
    }, 3000);
  }, [refreshItems, refreshCollections]);

  // Au démarrage : un enrichissement tourne peut-être déjà.
  useEffect(() => {
    api
      .enrichStatus()
      .then((p) => {
        if (p.running) {
          setEnrichProgress(p);
          watchEnrich();
        }
      })
      .catch(() => {});
  }, [watchEnrich]);

  async function pickFolder(create: boolean) {
    const path = await open({
      directory: true,
      title: create ? "Dossier de la nouvelle bibliothèque" : "Dossier de la bibliothèque",
    });
    if (typeof path !== "string") return;
    try {
      if (create) await api.createLibrary(path);
      else await api.openLibrary(path);
      setLibraryPath(path);
      // Remise à zéro complète de la navigation.
      setShowDashboard(false);
      setShowSyncPanel(false);
      setSchemaEditor(null);
      setViewing(null);
      setShowForm(false);
      setScanMode(false);
      setQuery("");
      const cols = await refreshCollections();
      setCurrent(cols.length > 0 ? cols[0].slug : null);
      setError(null);
      setNotice(`Bibliothèque ouverte : ${path.split("/").pop()}`);
      await refreshSync();
      try {
        if (await api.syncPull()) {
          await api.rebuildIndex();
          await refreshCollections();
          setNotice("Données mises à jour depuis GitHub");
        }
      } catch {
        /* hors ligne */
      }
    } catch (e) {
      setError(String(e));
    }
  }

  async function openItem(row: IndexedItem) {
    if (!current) return;
    try {
      setViewing(await api.getItem(current, row.id));
    } catch (e) {
      setError(String(e));
    }
  }

  function toggleSort(key: string) {
    if (sortKey === key) {
      setSortDesc((d) => !d);
    } else {
      setSortKey(key);
      setSortDesc(false);
    }
  }

  async function afterChange() {
    setShowForm(false);
    setEditing(null);
    setViewing(null);
    // Le commit automatique part en fond : on rafraîchit le statut après coup.
    setTimeout(() => void refreshSync(), 2500);
    try {
      await Promise.all([refreshItems(), refreshCollections()]);
      if (current) {
        api.listYears(current).then(setYears).catch(() => {});
        api.listSeries(current).then(setSeriesList).catch(() => {});
      }
    } catch (e) {
      setError(String(e));
    }
  }

  async function onSaved(saved: Item, created: boolean) {
    let coverNote = "";
    if (created && draftCoverUrl && current) {
      try {
        await api.downloadCover(current, saved.id, draftCoverUrl);
        coverNote = " + couverture";
      } catch {
        coverNote = " (couverture non récupérée)";
      }
    }
    setDraftFields(null);
    setDraftCoverUrl(null);
    setNotice(
      created
        ? `${saved.id} enregistré${saved.cote ? ` — cote ${saved.cote}` : ""}${coverNote}`
        : `${saved.id} mis à jour${saved.cote ? ` — cote ${saved.cote}` : ""}`,
    );
    const wasEditing = !created;
    await afterChange();
    if (wasEditing) setViewing(saved); // retour à la fiche après édition
  }

  /** Scan/recherche → candidat choisi → formulaire de création pré-rempli. */
  async function pickForCreate(candidate: Candidate) {
    if (!current) return;
    try {
      setDraftFields(await api.candidateFields(current, candidate));
      setDraftCoverUrl(candidate.cover_url);
      setEditing(null);
      setShowForm(true);
    } catch (e) {
      setError(String(e));
    }
  }

  /** Fiche existante → candidat choisi → complète les champs vides. */
  async function pickForEnrich(candidate: Candidate) {
    if (!current || !viewing) return;
    try {
      const fields = await api.candidateFields(current, candidate);
      const { id, cote: _c, statut, emplacement, date_ajout: _d, ...rest } = viewing;
      const merged: FieldValues = { ...rest };
      const added: string[] = [];
      for (const [k, v] of Object.entries(fields)) {
        const existing = merged[k];
        const empty =
          existing === undefined ||
          existing === null ||
          existing === "" ||
          (Array.isArray(existing) && existing.length === 0);
        if (empty) {
          merged[k] = v;
          added.push(k);
        }
      }
      const updated = await api.updateItem(current, id, statut, emplacement ?? null, merged);
      let coverNote = "";
      const imageKey = schema?.fields.find((f) => f.type === "image")?.key;
      if (candidate.cover_url && imageKey && !updated[imageKey]) {
        try {
          await api.downloadCover(current, id, candidate.cover_url);
          coverNote = added.length ? " + couverture" : "couverture récupérée";
        } catch {
          coverNote = " (couverture non récupérée)";
        }
      }
      setNotice(
        added.length || coverNote
          ? `Complété : ${added.join(", ")}${coverNote}`
          : "Rien à compléter — la fiche était déjà complète",
      );
      setEnrichMode(false);
      setViewing(await api.getItem(current, id));
      await refreshItems();
    } catch (e) {
      setError(String(e));
    }
  }

  if (libraryPath === undefined) return <div className="welcome">Chargement…</div>;

  if (!libraryPath && mobile) {
    return (
      <MobileSetup
        onDone={async () => {
          const path = await api.getLibraryPath();
          setLibraryPath(path);
          const cols = await refreshCollections();
          if (cols.length > 0) setCurrent(cols[0].slug);
        }}
      />
    );
  }

  if (!libraryPath) {
    return (
      <div className="welcome">
        <h1>Uber Collec</h1>
        <p>Gestionnaire de collections — vos données restent chez vous, en fichiers YAML.</p>
        <div className="welcome-actions">
          <button className="primary" onClick={() => pickFolder(true)}>
            Créer une bibliothèque
          </button>
          <button onClick={() => pickFolder(false)}>Ouvrir une bibliothèque existante</button>
        </div>
        {error && <p className="error">{error}</p>}
      </div>
    );
  }

  const currentInfo = collections.find((c) => c.slug === current);

  return (
    <div className="layout">
      <aside className="sidebar">
        <h1>Uber Collec</h1>
        <nav>
          <button
            className={showDashboard ? "nav-item active" : "nav-item"}
            onClick={() => {
              setShowLabels(false);
              setShowDashboard(true);
            }}
          >
            <span>📊 Tableau de bord</span>
          </button>
          {!mobile && (
            <button
              className={showLabels ? "nav-item active" : "nav-item"}
              onClick={() => {
                setShowDashboard(false);
                setShowLabels(true);
              }}
            >
              <span>🏷 Étiquettes</span>
              {labelsCount > 0 && <span className="count">{labelsCount}</span>}
            </button>
          )}
          {collections.map((c) => (
            <button
              key={c.slug}
              className={c.slug === current && !showDashboard ? "nav-item active" : "nav-item"}
              onClick={() => {
                setShowDashboard(false);
                setShowLabels(false);
                setCurrent(c.slug);
              }}
            >
              <span>{c.name}</span>
              <span className="count">
                {c.count}
                {c.wishlist_count > 0 && <em> +{c.wishlist_count}★</em>}
              </span>
            </button>
          ))}
        </nav>
        {!mobile && (
          <button
            className="nav-item new-collection"
            onClick={() => {
              setShowDashboard(false);
              setShowLabels(false);
              setSchemaEditor("create");
            }}
          >
            ＋ Nouvelle collection
          </button>
        )}
        {!mobile && (
          <div className="sync-widget">
            <button className="ghost" onClick={() => setShowApiKeys(true)}>
              🔑 Clés API…
            </button>
          </div>
        )}
        <div className="sync-widget" hidden={mobile}>
          {!syncStatus?.is_repo ? (
            <button className="ghost" onClick={() => setShowSyncPanel(true)}>
              ☁ Activer la sauvegarde Git…
            </button>
          ) : !syncStatus.remote ? (
            <button className="ghost" onClick={() => setShowSyncPanel(true)}>
              ☁ Git local · publier sur GitHub…
            </button>
          ) : syncStatus.ahead > 0 || syncStatus.dirty ? (
            <button
              className="ghost"
              disabled={pushing}
              title={syncStatus.last_commit ?? ""}
              onClick={async () => {
                setPushing(true);
                try {
                  await api.syncPush();
                  await refreshSync();
                } catch (e) {
                  setError(String(e));
                } finally {
                  setPushing(false);
                }
              }}
            >
              {pushing ? "☁ envoi…" : `☁ ↑${syncStatus.ahead || "…"} à pousser`}
            </button>
          ) : (
            <span className="sync-ok" title={syncStatus.last_commit ?? ""}>
              ☁ synchronisé
            </span>
          )}
        </div>
        <footer className="sidebar-footer">
          <button
            className="ghost theme-switch"
            title="Thème d'affichage"
            onClick={() =>
              setTheme((t) => (t === "auto" ? "dark" : t === "dark" ? "light" : "auto"))
            }
          >
            {theme === "auto" ? "◐ Thème auto" : theme === "dark" ? "🌙 Sombre" : "☀️ Clair"}
          </button>
          <span title={libraryPath}>{mobile ? "lecture seule" : libraryPath.split("/").pop()}</span>
          <button
            hidden={mobile}
            className="ghost switch-lib"
            title={
              enrichProgress?.running
                ? "Impossible pendant un enrichissement"
                : "Ouvrir une autre bibliothèque"
            }
            disabled={enrichProgress?.running}
            onClick={() => pickFolder(false)}
          >
            ⇄ Ouvrir une autre bibliothèque…
          </button>
        </footer>
      </aside>

      <main className="main">
        {showApiKeys ? (
          <ApiKeysPanel
            onDone={(message) => {
              setShowApiKeys(false);
              setNotice(message);
            }}
            onCancel={() => setShowApiKeys(false)}
          />
        ) : showLabels ? (
          <>
            {notice && <p className="notice">{notice}</p>}
            <LabelsPanel
              collections={collections}
              onNotice={setNotice}
              onChanged={() => {
                api.labelsCount().then(setLabelsCount).catch(() => {});
              }}
            />
          </>
        ) : showSyncPanel ? (
          <SyncPanel
            status={syncStatus ?? { is_repo: false, remote: null, dirty: false, ahead: 0, behind: 0, last_commit: null }}
            onDone={async (message) => {
              setShowSyncPanel(false);
              setNotice(message);
              await refreshSync();
            }}
            onCancel={() => setShowSyncPanel(false)}
          />
        ) : schemaEditor ? (
          <SchemaEditor
            mode={schemaEditor}
            slug={schemaEditor === "edit" ? (current ?? undefined) : undefined}
            schema={schemaEditor === "edit" ? (schema ?? undefined) : undefined}
            itemCount={
              schemaEditor === "edit"
                ? (collections.find((c) => c.slug === current)?.count ?? 0) +
                  (collections.find((c) => c.slug === current)?.wishlist_count ?? 0)
                : undefined
            }
            onDeleted={async () => {
              setSchemaEditor(null);
              setNotice(`Collection supprimée — récupérable dans l'historique Git`);
              const cols = await refreshCollections();
              setCurrent(cols.length > 0 ? cols[0].slug : null);
            }}
            onSaved={async (savedSlug) => {
              const editing = schemaEditor === "edit";
              setSchemaEditor(null);
              await refreshCollections();
              if (editing && current) {
                api.getSchema(current).then(setSchema).catch(() => {});
                try {
                  const changes = await api.regenerateCotes(current);
                  setNotice(
                    changes.length > 0
                      ? `Schéma enregistré — ${changes.length} cotes régénérées (étiquettes à refaire)`
                      : "Schéma enregistré",
                  );
                } catch {
                  setNotice("Schéma enregistré");
                }
                await refreshItems();
              } else {
                setNotice("Collection créée");
                setCurrent(savedSlug);
              }
            }}
            onCancel={() => setSchemaEditor(null)}
          />
        ) : showDashboard ? (
          <Dashboard
            onOpenSeries={(coll) => {
              setShowDashboard(false);
              if (coll === current) {
                setColTab("series");
              } else {
                pendingTab.current = "series";
                setCurrent(coll);
              }
            }}
          />
        ) : current && schema && schema.fields.some((f) => f.type === "series_ref") && colTab === "series" ? (
          <>
            <div className="tabs">
              <button className="tab" onClick={() => setColTab("objets")}>
                Objets
              </button>
              <button className="tab active">Séries</button>
            </div>
            {error && <p className="error">{error}</p>}
            {notice && <p className="notice">{notice}</p>}
            <SeriesPanel
              collection={current}
              schema={schema}
              readOnly={mobile}
              onNotice={setNotice}
              onItemsChanged={() => {
                void refreshItems();
                void refreshCollections();
              }}
            />
          </>
        ) : current && schema ? (
          <>
            {schema.fields.some((f) => f.type === "series_ref") && (
              <div className="tabs">
                <button className="tab active">Objets</button>
                <button className="tab" onClick={() => setColTab("series")}>
                  Séries
                </button>
              </div>
            )}
            <div className="toolbar">
              <input
                type="search"
                placeholder={`Rechercher dans ${schema.name}…`}
                value={query}
                onChange={(e) => setQuery(e.target.value)}
              />
              <div className="toolbar-controls">
              <select
                value={statutFilter}
                onChange={(e) => setStatutFilter(e.target.value as StatutFilter)}
              >
                <option value="tous">Tous</option>
                <option value="possede">Possédés</option>
                <option value="souhaite">Wishlist</option>
              </select>
              <div className="view-switch">
                <button
                  className={viewMode === "list" ? "active" : ""}
                  title="Vue liste"
                  onClick={() => switchView("list")}
                >
                  ☰
                </button>
                <button
                  className={viewMode === "grid" ? "active" : ""}
                  title="Vue grille"
                  onClick={() => switchView("grid")}
                >
                  ▦
                </button>
              </div>
              {mobile && (
                <button
                  title="Retélécharger la collection depuis GitHub"
                  onClick={async () => {
                    try {
                      setNotice("Actualisation depuis GitHub…");
                      const n = await api.mobileSync();
                      setNotice(`Collection actualisée (${n} objets)`);
                      await Promise.all([refreshItems(), refreshCollections()]);
                    } catch (e) {
                      setError(String(e));
                    }
                  }}
                >
                  ↻
                </button>
              )}
              {!mobile && schema.source && (
                <button
                  className="primary"
                  onClick={() => {
                    setShowForm(false);
                    setEditing(null);
                    setViewing(null);
                    setScanMode(true);
                  }}
                >
                  ⚡ Scanner
                </button>
              )}
              {!mobile && (
                <button
                  onClick={() => {
                    setShowForm(false);
                    setEditing(null);
                    setShowImport(true);
                  }}
                >
                  Importer CSV
                </button>
              )}
              {!mobile && (
                <button
                  title="Exporte la liste affichée (recherche et filtres appliqués)"
                  onClick={async () => {
                    if (!current) return;
                    const filtered =
                      query.trim() !== "" || Object.values(filters).some((v) => v !== undefined);
                    const dest = await save({
                      title: "Exporter la collection",
                      defaultPath: `${current}${filtered ? "-filtre" : ""}.csv`,
                      filters: [
                        { name: "CSV", extensions: ["csv"] },
                        { name: "JSON", extensions: ["json"] },
                      ],
                    });
                    if (!dest) return;
                    try {
                      const n = await api.exportCollection(current, dest, query, filters);
                      setNotice(`${n} objets exportés vers ${dest.split("/").pop()}`);
                    } catch (e) {
                      setError(String(e));
                    }
                  }}
                >
                  Exporter…
                </button>
              )}
              {!mobile && schema.source && (
                <button onClick={startEnrich} disabled={enrichProgress?.running}>
                  Enrichir tout
                </button>
              )}
              {!mobile && (
                <button title="Modifier le schéma de la collection" onClick={() => setSchemaEditor("edit")}>
                  ⚙
                </button>
              )}
              {!mobile && (
                <button
                  className="primary"
                  onClick={() => {
                    setEditing(null);
                    setShowForm(true);
                  }}
                >
                  + Ajouter
                </button>
              )}
              </div>
            </div>

            {(() => {
              const genreField = schema.cote
                ? schema.fields.find((f) => f.key === schema.cote!.genre_field)
                : undefined;
              const hasSeries = schema.fields.some((f) => f.type === "series_ref");
              if (!genreField && years.length === 0 && !hasSeries) return null;
              const active = genreFilter || anneeFilter || serieFilter;
              return (
                <div className="filter-bar">
                  {years.length > 0 && (
                    <select value={anneeFilter} onChange={(e) => setAnneeFilter(e.target.value)}>
                      <option value="">Toutes les années</option>
                      {years.map((y) => (
                        <option key={y} value={y}>
                          {y}
                        </option>
                      ))}
                    </select>
                  )}
                  {genreField && (
                    <select value={genreFilter} onChange={(e) => setGenreFilter(e.target.value)}>
                      <option value="">Tous les genres</option>
                      {(genreField.options ?? []).map((o) => (
                        <option key={o.value} value={o.value}>
                          {o.value}
                        </option>
                      ))}
                    </select>
                  )}
                  {hasSeries && seriesList.length > 0 && (
                    <select value={serieFilter} onChange={(e) => setSerieFilter(e.target.value)}>
                      <option value="">Toutes les séries</option>
                      {seriesList.map((s) => (
                        <option key={s.id} value={s.id}>
                          {s.nom}
                        </option>
                      ))}
                    </select>
                  )}
                  {active && (
                    <button
                      className="ghost"
                      onClick={() => {
                        setGenreFilter("");
                        setAnneeFilter("");
                        setSerieFilter("");
                      }}
                    >
                      Réinitialiser
                    </button>
                  )}
                </div>
              );
            })()}

            {error && <p className="error">{error}</p>}
            {notice && <p className="notice">{notice}</p>}
            {enrichProgress?.running && (
              <div className="enrich-banner">
                <div className="enrich-info">
                  <strong>
                    Enrichissement BNF — {enrichProgress.processed}/{enrichProgress.total}
                  </strong>
                  <span className="muted">
                    {enrichProgress.covers} couvertures · {enrichProgress.enriched} complétées
                    {enrichProgress.not_found > 0 && ` · ${enrichProgress.not_found} introuvables`}
                    {enrichProgress.current && ` · en cours : ${enrichProgress.current}`}
                  </span>
                  <progress value={enrichProgress.processed} max={enrichProgress.total || 1} />
                </div>
                <button
                  onClick={() => api.enrichCancel()}
                  disabled={enrichProgress.cancel_requested}
                >
                  {enrichProgress.cancel_requested ? "Arrêt en cours…" : "Arrêter"}
                </button>
              </div>
            )}

            {enrichMode && viewing ? (
              <HydrateSearch
                collection={current}
                title={`Compléter « ${String(
                  viewing[schema.fields.find((f) => f.type === "text" && f.required)?.key ?? ""] ?? viewing.id,
                )} » depuis les bases`}
                initialQuery={
                  viewing.ean || viewing.isbn
                    ? String(viewing.ean ?? viewing.isbn)
                    : [
                        Array.isArray(viewing.artiste) ? (viewing.artiste as string[])[0] : "",
                        String(viewing.titre ?? ""),
                      ]
                        .filter(Boolean)
                        .join(" ")
                }
                pickLabel="Compléter avec cette fiche"
                onPick={pickForEnrich}
                onCancel={() => setEnrichMode(false)}
              />
            ) : viewing && !showForm ? (
              <ItemView
                collection={current}
                schema={schema}
                item={viewing}
                seriesList={seriesList}
                libraryPath={libraryPath}
                readOnly={mobile}
                onEdit={() => {
                  setEditing(viewing);
                  setShowForm(true);
                }}
                onEnrich={!mobile && schema.source ? () => setEnrichMode(true) : undefined}
                onClose={() => setViewing(null)}
                onDeleted={afterChange}
              />
            ) : showImport ? (
              <ImportWizard
                collection={current}
                schema={schema}
                onDone={async (report: ImportReport) => {
                  setShowImport(false);
                  setNotice(
                    `Import : ${report.imported} objets, ${report.series_created} séries créées`,
                  );
                  // Le schéma a pu être enrichi (nouveaux genres).
                  api.getSchema(current).then(setSchema).catch(() => {});
                  await Promise.all([refreshItems(), refreshCollections()]);
                }}
                onCancel={() => setShowImport(false)}
              />
            ) : showForm ? (
              <ItemForm
                collection={current}
                schema={schema}
                item={editing}
                initialFields={draftFields}
                libraryPath={libraryPath}
                onSaved={onSaved}
                onDeleted={afterChange}
                onCancel={() => {
                  setShowForm(false);
                  setEditing(null);
                  setDraftFields(null);
                  setDraftCoverUrl(null);
                }}
              />
            ) : scanMode ? (
              <HydrateSearch
                collection={current}
                title={`Ajouter par scan ou recherche — ${schema.name}`}
                pickLabel="Créer la fiche"
                onPick={pickForCreate}
                onCancel={() => setScanMode(false)}
              />
            ) : viewMode !== "grid" && mobile ? (
              <>
                <ul className="mobile-list">
                  {items.map((row) => {
                    const imageKey = schema.fields.find((f) => f.type === "image")?.key;
                    const rel = imageKey ? (row.data[imageKey] as string | undefined) : undefined;
                    return (
                      <li key={row.id} onClick={() => openItem(row)}>
                        {rel && libraryPath ? (
                          <img src={coverSrc(libraryPath, rel)} alt="" loading="lazy" />
                        ) : (
                          <div className="mini-placeholder" />
                        )}
                        <div className="mobile-item-info">
                          <strong>
                            {row.statut === "souhaite" && <span className="wish">★ </span>}
                            {row.titre}
                          </strong>
                          <span className="muted">
                            {row.serie_nom
                              ? `${row.serie_nom}${row.serie_tome != null ? ` · T${row.serie_tome}` : ""}`
                              : ""}
                          </span>
                          {row.cote && <span className="mono mobile-cote">{row.cote}</span>}
                        </div>
                      </li>
                    );
                  })}
                </ul>
                {items.length === 0 && (
                  <p className="empty">{query ? "Aucun résultat." : "Collection vide."}</p>
                )}
                {items.length > 0 && (
                  <div className="list-footer">
                    <span className="muted">
                      {items.length} affichés sur {total}
                    </span>
                    {items.length < total && (
                      <button onClick={loadMore} disabled={loadingMore}>
                        {loadingMore
                          ? "Chargement…"
                          : `Afficher ${Math.min(PAGE_SIZE, total - items.length)} de plus`}
                      </button>
                    )}
                  </div>
                )}
              </>
            ) : viewMode === "grid" ? (
              <>
                <div className="grid-cards">
                  {items.map((row) => {
                    const imageKey = schema.fields.find((f) => f.type === "image")?.key;
                    const rel = imageKey ? (row.data[imageKey] as string | undefined) : undefined;
                    return (
                      <div key={row.id} className="card" onClick={() => openItem(row)}>
                        {rel && libraryPath ? (
                          <img src={coverSrc(libraryPath, rel)} alt={row.titre} loading="lazy" />
                        ) : (
                          <div className="card-placeholder">
                            <span>{row.titre}</span>
                          </div>
                        )}
                        <div className="card-caption">
                          <strong title={row.titre}>
                            {row.statut === "souhaite" && <span className="wish">★ </span>}
                            {row.titre}
                          </strong>
                          <span className="muted">
                            {row.serie_nom
                              ? `${row.serie_nom}${row.serie_tome != null ? ` · T${row.serie_tome}` : ""}`
                              : row.cote ?? ""}
                          </span>
                        </div>
                      </div>
                    );
                  })}
                </div>
                {items.length === 0 && (
                  <p className="empty">{query ? "Aucun résultat." : "Collection vide."}</p>
                )}
                {items.length > 0 && (
                  <div className="list-footer">
                    <span className="muted">
                      {items.length} affichés sur {total}
                    </span>
                    {items.length < total && (
                      <button onClick={loadMore} disabled={loadingMore}>
                        {loadingMore
                          ? "Chargement…"
                          : `Afficher ${Math.min(PAGE_SIZE, total - items.length)} de plus`}
                      </button>
                    )}
                  </div>
                )}
              </>
            ) : (
              <>
                <table className="items">
                  <thead>
                    <tr>
                      <SortableTh
                        label="Cote"
                        k="cote"
                        className="col-cote"
                        sortKey={sortKey}
                        sortDesc={sortDesc}
                        onSort={toggleSort}
                      />
                      <SortableTh label="Titre" k="titre" sortKey={sortKey} sortDesc={sortDesc} onSort={toggleSort} />
                      {schema.fields.some((f) => f.type === "series_ref") && (
                        <SortableTh label="Série" k="serie" sortKey={sortKey} sortDesc={sortDesc} onSort={toggleSort} />
                      )}
                      <SortableTh
                        label="Année"
                        k="annee"
                        className="col-annee"
                        sortKey={sortKey}
                        sortDesc={sortDesc}
                        onSort={toggleSort}
                      />
                      <th className="col-statut">Statut</th>
                      <SortableTh label="Emplacement" k="emplacement" sortKey={sortKey} sortDesc={sortDesc} onSort={toggleSort} />
                    </tr>
                  </thead>
                  <tbody>
                    {items.map((row) => (
                      <tr key={row.id} onClick={() => openItem(row)}>
                        <td className="col-cote mono">{row.cote ?? "—"}</td>
                        <td>{row.titre}</td>
                        {schema.fields.some((f) => f.type === "series_ref") && (
                          <td>
                            {row.serie_nom ? (
                              <>
                                {row.serie_nom}
                                {row.serie_tome != null && (
                                  <span className="muted"> · T{row.serie_tome}</span>
                                )}
                              </>
                            ) : (
                              <span className="muted">—</span>
                            )}
                          </td>
                        )}
                        <td className="col-annee">{row.annee ?? ""}</td>
                        <td className="col-statut">
                          {row.statut === "souhaite" ? (
                            <span className="wish">★ souhaité</span>
                          ) : (
                            "possédé"
                          )}
                        </td>
                        <td className="muted">{row.emplacement ?? ""}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
                {items.length === 0 && (
                  <p className="empty">
                    {query
                      ? "Aucun résultat."
                      : `Collection vide (${currentInfo?.count ?? 0} objets). Cliquez sur « + Ajouter ».`}
                  </p>
                )}
                {items.length > 0 && (
                  <div className="list-footer">
                    <span className="muted">
                      {items.length} affichés sur {total}
                    </span>
                    {items.length < total && (
                      <button onClick={loadMore} disabled={loadingMore}>
                        {loadingMore
                          ? "Chargement…"
                          : `Afficher ${Math.min(PAGE_SIZE, total - items.length)} de plus`}
                      </button>
                    )}
                  </div>
                )}
              </>
            )}
          </>
        ) : null}
      </main>
    </div>
  );
}

function SortableTh({
  label,
  k,
  sortKey,
  sortDesc,
  onSort,
  className,
}: {
  label: string;
  k: string;
  sortKey: string | null;
  sortDesc: boolean;
  onSort: (k: string) => void;
  className?: string;
}) {
  const active = sortKey === k;
  return (
    <th
      className={`sortable ${active ? "sorted" : ""} ${className ?? ""}`}
      onClick={() => onSort(k)}
      title="Trier par cette colonne"
    >
      {label}
      {active && <span className="sort-arrow">{sortDesc ? " ▼" : " ▲"}</span>}
    </th>
  );
}
