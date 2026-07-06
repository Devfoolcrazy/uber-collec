import { useEffect, useState } from "react";
import { api, DashboardStats } from "../api";

interface Props {
  /** Navigation vers l'onglet Séries d'une collection. */
  onOpenSeries: (collection: string) => void;
}

export default function Dashboard({ onOpenSeries }: Props) {
  const [stats, setStats] = useState<DashboardStats | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.dashboardStats().then(setStats).catch((e) => setError(String(e)));
  }, []);

  if (error) return <p className="error">{error}</p>;
  if (!stats) return <p className="empty">Calcul…</p>;

  // Une seule mesure par graphique → une seule teinte ; libellés en encre.
  const genres = stats.genres.slice(0, 12);
  const maxGenre = Math.max(1, ...genres.map((g) => g.count));
  const annees = stats.annees;
  const maxAnnee = Math.max(1, ...annees.map((a) => a.count));
  const minYear = annees[0]?.annee;
  const maxYear = annees[annees.length - 1]?.annee;

  return (
    <div className="dashboard">
      <div className="stat-tiles">
        <div className="tile tile-total">
          <span className="tile-value">{stats.total_possede}</span>
          <span className="tile-label">objets possédés</span>
        </div>
        <div className="tile">
          <span className="tile-value">{stats.total_souhaite}</span>
          <span className="tile-label">en wishlist</span>
        </div>
        <div className="tile">
          <span className="tile-value">{stats.series_incompletes_total}</span>
          <span className="tile-label">séries incomplètes</span>
        </div>
        {stats.collections
          .filter((c) => c.possede + c.souhaite > 0)
          .map((c) => (
            <div key={c.slug} className="tile">
              <span className="tile-value">{c.possede}</span>
              <span className="tile-label">
                {c.name}
                {c.souhaite > 0 && ` · ${c.souhaite} ★`}
              </span>
            </div>
          ))}
      </div>

      <div className="dash-columns">
        <section className="dash-section">
          <h3>Genres les plus représentés</h3>
          {genres.map((g) => (
            <div key={`${g.collection}-${g.genre}`} className="hbar-row">
              <span className="hbar-label" title={g.genre}>
                {g.genre}
              </span>
              <div className="hbar-track">
                <div className="hbar" style={{ width: `${(g.count / maxGenre) * 100}%` }} />
              </div>
              <span className="hbar-value">{g.count}</span>
            </div>
          ))}
          {genres.length === 0 && <p className="muted">Aucune donnée de genre.</p>}
        </section>

        <section className="dash-section">
          <h3>Séries incomplètes</h3>
          {stats.series_incompletes.slice(0, 12).map((s) => (
            <div
              key={`${s.collection}-${s.nom}`}
              className="gap-row"
              onClick={() => onOpenSeries(s.collection)}
              title="Ouvrir dans l'onglet Séries"
            >
              <span className="gap-nom">{s.nom}</span>
              <span className="muted">
                {s.manquants.length <= 6
                  ? s.manquants.map((t) => `T${t}`).join(", ")
                  : `${s.manquants.length} tomes manquants`}
              </span>
            </div>
          ))}
          {stats.series_incompletes.length === 0 && (
            <p className="muted">Toutes vos séries sont complètes 🎉</p>
          )}
        </section>
      </div>

      <section className="dash-section">
        <h3>
          Par année de parution{" "}
          {minYear !== undefined && (
            <span className="muted">
              ({minYear} → {maxYear})
            </span>
          )}
        </h3>
        <div className="year-histo">
          {annees.map((a) => (
            <div
              key={`${a.collection}-${a.annee}`}
              className="year-bar"
              style={{ height: `${Math.max(4, (a.count / maxAnnee) * 100)}%` }}
              title={`${a.annee} : ${a.count} objets`}
            />
          ))}
        </div>
      </section>
    </div>
  );
}
