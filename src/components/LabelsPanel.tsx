import { useCallback, useEffect, useState } from "react";
import { confirm } from "@tauri-apps/plugin-dialog";
import { api, CollectionInfo, IndexedItem } from "../api";

interface Props {
  collections: CollectionInfo[];
  onNotice: (message: string) => void;
  /** Le compteur global de la barre latérale doit être rafraîchi. */
  onChanged: () => void;
}

/** Étiquettes à faire : les cotes à recopier sur la LetraTag, triées par
 *  collection puis cote — l'ordre d'une session d'étiquetage en rayon. */
export default function LabelsPanel({ collections, onNotice, onChanged }: Props) {
  const [filter, setFilter] = useState("");
  const [rows, setRows] = useState<IndexedItem[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [working, setWorking] = useState(false);

  const refresh = useCallback(async () => {
    try {
      setRows(await api.labelsTodo(filter || undefined));
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, [filter]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const collectionName = (slug: string) =>
    collections.find((c) => c.slug === slug)?.name ?? slug;

  async function markOne(row: IndexedItem) {
    try {
      await api.markLabeled([{ collection: row.collection, id: row.id }]);
      setRows((rs) => rs.filter((r) => !(r.collection === row.collection && r.id === row.id)));
      onChanged();
    } catch (e) {
      setError(String(e));
    }
  }

  async function markAllShown() {
    const ok = await confirm(
      `Pointer les ${rows.length} étiquettes affichées comme faites ?\n\n` +
        `À utiliser après une session d'étiquetage, ou pour solder l'existant ` +
        `que vous ne comptez pas étiqueter.`,
      { title: "Tout pointer", kind: "warning" },
    );
    if (!ok) return;
    setWorking(true);
    try {
      const n = await api.markLabeled(
        rows.map((r) => ({ collection: r.collection, id: r.id })),
      );
      onNotice(`${n} étiquettes pointées`);
      await refresh();
      onChanged();
    } catch (e) {
      setError(String(e));
    } finally {
      setWorking(false);
    }
  }

  return (
    <div className="labels-panel">
      <div className="series-toolbar">
        <div className="labels-filters">
          <select value={filter} onChange={(e) => setFilter(e.target.value)}>
            <option value="">Toutes les collections</option>
            {collections.map((c) => (
              <option key={c.slug} value={c.slug}>
                {c.name}
              </option>
            ))}
          </select>
          <span className="muted">{rows.length} étiquettes à faire</span>
        </div>
        {rows.length > 0 && (
          <button onClick={markAllShown} disabled={working}>
            ✓ Tout pointer ({rows.length})
          </button>
        )}
      </div>

      {error && <p className="error">{error}</p>}

      {rows.length === 0 ? (
        <p className="empty">Rien à étiqueter — tout est à jour. 🏷</p>
      ) : (
        <table className="items labels-table">
          <thead>
            <tr>
              <th className="col-cote">Cote à écrire</th>
              <th>Titre</th>
              <th>Collection</th>
              <th>Emplacement</th>
              <th className="col-done"></th>
            </tr>
          </thead>
          <tbody>
            {rows.map((row) => (
              <tr key={`${row.collection}/${row.id}`}>
                <td className="mono label-cote">{row.cote}</td>
                <td>
                  {row.titre}
                  {row.serie_nom && (
                    <span className="muted">
                      {" "}
                      — {row.serie_nom}
                      {row.serie_tome != null ? ` T${row.serie_tome}` : ""}
                    </span>
                  )}
                </td>
                <td className="muted">{collectionName(row.collection)}</td>
                <td className="muted">{row.emplacement ?? ""}</td>
                <td className="col-done">
                  <button className="ghost" title="Étiquette faite" onClick={() => markOne(row)}>
                    ✓ Faite
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}
