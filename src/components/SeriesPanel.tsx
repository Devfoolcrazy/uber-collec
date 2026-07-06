import { useCallback, useEffect, useState } from "react";
import { api, Schema, SerieReport } from "../api";

interface Props {
  collection: string;
  schema: Schema;
  /** Consultation pure (iOS) : pas d'ajout wishlist ni d'édition. */
  readOnly?: boolean;
  onNotice: (message: string) => void;
  /** Appelé quand un objet a été créé (wishlist) : compteurs à rafraîchir. */
  onItemsChanged: () => void;
}

/** « 1, 2, 3, 5, 6, 9 » → « 1–3, 5–6, 9 » */
function ranges(tomes: number[]): string {
  if (tomes.length === 0) return "";
  const parts: string[] = [];
  let start = tomes[0];
  let prev = tomes[0];
  for (const t of tomes.slice(1).concat([Number.NaN])) {
    if (t === prev + 1) {
      prev = t;
      continue;
    }
    parts.push(start === prev ? `${start}` : `${start}–${prev}`);
    start = t;
    prev = t;
  }
  return parts.join(", ");
}

export default function SeriesPanel({
  collection,
  schema,
  readOnly,
  onNotice,
  onItemsChanged,
}: Props) {
  const [report, setReport] = useState<SerieReport[]>([]);
  const [incompleteOnly, setIncompleteOnly] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      setReport(await api.seriesReport(collection));
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, [collection]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  async function toggleTerminee(serie: SerieReport) {
    await api.upsertSeries(collection, {
      id: serie.id,
      nom: serie.nom,
      terminee: !serie.terminee,
    });
    await refresh();
  }

  async function addToWishlist(serie: SerieReport, tome: number) {
    const titleKey =
      schema.fields.find((f) => f.type === "text" && f.required)?.key ?? "titre";
    const serieKey = schema.fields.find((f) => f.type === "series_ref")?.key ?? "serie";
    try {
      await api.createItem(collection, "souhaite", {
        [titleKey]: `${serie.nom} — Tome ${tome}`,
        [serieKey]: { id: serie.id, tome },
      });
      onNotice(`« ${serie.nom} » Tome ${tome} ajouté à la wishlist`);
      onItemsChanged();
      await refresh();
    } catch (e) {
      setError(String(e));
    }
  }

  const shown = incompleteOnly ? report.filter((s) => s.manquants.length > 0) : report;
  const incompletes = report.filter((s) => s.manquants.length > 0).length;

  return (
    <div className="series-panel">
      <div className="series-toolbar">
        <span className="muted">
          {report.length} séries · {incompletes} incomplètes
        </span>
        <label>
          <input
            type="checkbox"
            checked={incompleteOnly}
            onChange={(e) => setIncompleteOnly(e.target.checked)}
          />
          Incomplètes seulement
        </label>
      </div>

      {error && <p className="error">{error}</p>}

      <table className="items series-table">
        <thead>
          <tr>
            <th>Série</th>
            <th>Possédés</th>
            <th>Manquants</th>
            <th className="col-terminee">Terminée</th>
          </tr>
        </thead>
        <tbody>
          {shown.map((s) => (
            <tr key={s.id}>
              <td>
                <strong>{s.nom}</strong>{" "}
                <span className="muted">({s.possedes.length})</span>
              </td>
              <td className="mono">{ranges(s.possedes) || "—"}</td>
              <td>
                {s.manquants.length === 0 ? (
                  <span className="complete">✓ complète</span>
                ) : (
                  <span className="gap-chips">
                    {s.manquants.map((t) =>
                      s.souhaites.includes(t) ? (
                        <span key={t} className="chip chip-wished" title="Déjà en wishlist">
                          T{t} ★
                        </span>
                      ) : readOnly ? (
                        <span key={t} className="chip">
                          T{t}
                        </span>
                      ) : (
                        <button
                          key={t}
                          className="chip"
                          title="Ajouter à la wishlist"
                          onClick={() => addToWishlist(s, t)}
                        >
                          T{t} +
                        </button>
                      ),
                    )}
                  </span>
                )}
              </td>
              <td className="col-terminee">
                <input
                  type="checkbox"
                  checked={s.terminee}
                  disabled={readOnly}
                  onChange={() => toggleTerminee(s)}
                />
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      {shown.length === 0 && <p className="empty">Aucune série.</p>}
    </div>
  );
}
