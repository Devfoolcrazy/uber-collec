import React, { useCallback, useEffect, useState } from "react";
import { api, Schema, SerieReport } from "../api";

interface Props {
  collection: string;
  schema: Schema;
  /** Consultation pure (iOS) : pas d'ajout wishlist ni d'édition. */
  readOnly?: boolean;
  onNotice: (message: string) => void;
  /** Appelé quand un objet a été créé (wishlist) : compteurs à rafraîchir. */
  onItemsChanged: () => void;
  /** Ouvre l'onglet Objets filtré sur cette série. */
  onOpenSerie: (serieId: string) => void;
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
  onOpenSerie,
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

  async function removeFromWishlist(serie: SerieReport, tome: number) {
    try {
      await api.removeWishlistTome(collection, serie.id, tome);
      onNotice(`« ${serie.nom} » Tome ${tome} retiré de la wishlist`);
      onItemsChanged();
      await refresh();
    } catch (e) {
      setError(String(e));
    }
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

  const incompletes = report.filter((s) => s.manquants.length > 0).length;
  // Incomplètes d'abord (l'action), puis le reste — alphabétique dans chaque
  // groupe (l'API renvoie déjà trié par nom).
  const sorted = [
    ...report.filter((s) => s.manquants.length > 0),
    ...report.filter((s) => s.manquants.length === 0),
  ];
  const shown = incompleteOnly ? sorted.filter((s) => s.manquants.length > 0) : sorted;
  const firstCompleteIndex = shown.findIndex((s) => s.manquants.length === 0);

  /** Barre de progression : possédés / plus grand tome connu. */
  function Progress({ s }: { s: SerieReport }) {
    const max = Math.max(s.possedes[s.possedes.length - 1] ?? 0, s.possedes.length);
    if (max === 0) return <span className="muted">—</span>;
    const pct = Math.round((s.possedes.length / max) * 100);
    return (
      <span className="progress-cell">
        <span className="progress-track">
          <span className="progress-fill" style={{ width: `${pct}%` }} />
        </span>
        <span className="progress-text mono">
          {s.possedes.length}/{max}
        </span>
      </span>
    );
  }

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
            <th className="col-progress">Progression</th>
            <th className="col-tomes">Tomes</th>
            <th>Manquants</th>
            <th className="col-terminee">Terminée</th>
          </tr>
        </thead>
        <tbody>
          {shown.map((s, i) => (
            <React.Fragment key={s.id}>
              {i === firstCompleteIndex && i > 0 && (
                <tr className="series-divider">
                  <td colSpan={5}>complètes</td>
                </tr>
              )}
            <tr className={s.manquants.length === 0 ? "serie-complete" : ""}>
              <td>
                <button
                  className="serie-link"
                  title="Voir les albums de cette série"
                  onClick={() => onOpenSerie(s.id)}
                >
                  {s.nom}
                </button>
              </td>
              <td className="col-progress">
                <Progress s={s} />
              </td>
              <td className="mono col-tomes">{ranges(s.possedes) || "—"}</td>
              <td>
                {s.possedes.length === 0 ? (
                  <span className="muted">—</span>
                ) : s.manquants.length === 0 ? (
                  <span className="complete">✓</span>
                ) : (
                  <span className="gap-chips">
                    {s.manquants.map((t) =>
                      s.souhaites.includes(t) ? (
                        readOnly ? (
                          <span key={t} className="chip chip-wished" title="En wishlist">
                            T{t} ★
                          </span>
                        ) : (
                          <button
                            key={t}
                            className="chip chip-wished"
                            title="Retirer de la wishlist"
                            onClick={() => removeFromWishlist(s, t)}
                          >
                            T{t} ★
                          </button>
                        )
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
            </React.Fragment>
          ))}
        </tbody>
      </table>
      {shown.length === 0 && <p className="empty">Aucune série.</p>}
    </div>
  );
}
