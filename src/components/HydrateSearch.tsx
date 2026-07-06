import { useEffect, useRef, useState } from "react";
import { api, Candidate } from "../api";

interface Props {
  collection: string;
  title: string;
  /** Requête lancée automatiquement à l'ouverture (EAN ou titre). */
  initialQuery?: string;
  pickLabel: string;
  onPick: (candidate: Candidate) => void;
  onCancel: () => void;
}

/** Recherche dans les bases ouvertes — au clavier ou à la douchette.
 *  Une douchette USB « tape » le code-barres suivi d'Entrée : le champ est
 *  toujours focalisé, chaque scan déclenche donc la recherche. */
export default function HydrateSearch({
  collection,
  title,
  initialQuery,
  pickLabel,
  onPick,
  onCancel,
}: Props) {
  const [query, setQuery] = useState(initialQuery ?? "");
  const [results, setResults] = useState<Candidate[] | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [tmdbKey, setTmdbKey] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);

  const needsTmdbKey = error?.includes("TMDB_KEY_MISSING") ?? false;

  async function saveTmdbKey() {
    if (!tmdbKey.trim()) return;
    try {
      await api.setApiKey("tmdb", tmdbKey.trim());
      setError(null);
      setTmdbKey("");
      if (query.trim()) void run(query);
    } catch (e) {
      setError(String(e));
    }
  }

  async function run(q: string) {
    if (!q.trim()) return;
    setLoading(true);
    setError(null);
    try {
      setResults(await api.hydrateSearch(collection, q));
    } catch (e) {
      setError(String(e));
      setResults(null);
    } finally {
      setLoading(false);
      inputRef.current?.focus();
      inputRef.current?.select();
    }
  }

  useEffect(() => {
    inputRef.current?.focus();
    if (initialQuery) void run(initialQuery);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <div className="item-form">
      <header>
        <h2>{title}</h2>
      </header>

      <div className="scan-bar">
        <input
          ref={inputRef}
          type="text"
          placeholder="Scannez un code-barres ou tapez un titre, puis Entrée"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") void run(query);
            if (e.key === "Escape") onCancel();
          }}
        />
        <button className="primary" onClick={() => run(query)} disabled={loading}>
          {loading ? "Recherche…" : "Rechercher"}
        </button>
        <button onClick={onCancel}>Fermer</button>
      </div>

      {needsTmdbKey ? (
        <div className="tmdb-setup">
          <p>
            Les DVD s'appuient sur TMDB, qui demande une clé d'API gratuite (une seule
            fois) : <strong>themoviedb.org → Paramètres → API → Créer</strong>, puis
            collez la « Clé d'API » ici.
          </p>
          <div className="sync-row">
            <input
              type="password"
              placeholder="clé d'API TMDB"
              value={tmdbKey}
              onChange={(e) => setTmdbKey(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") void saveTmdbKey();
              }}
            />
            <button className="primary" onClick={saveTmdbKey} disabled={!tmdbKey.trim()}>
              Enregistrer
            </button>
          </div>
        </div>
      ) : (
        error && <p className="error">{error}</p>
      )}

      {results !== null && results.length === 0 && (
        <p className="empty">Aucun résultat dans les bases interrogées.</p>
      )}

      {results && results.length > 0 && (
        <ul className="candidates">
          {results.map((c, i) => (
            <li key={i} className="candidate">
              {c.cover_url ? (
                <img
                  src={c.cover_url}
                  alt=""
                  loading="lazy"
                  onError={(e) => {
                    // Cover Art Archive renvoie 404 quand la pochette manque.
                    (e.target as HTMLImageElement).style.visibility = "hidden";
                  }}
                />
              ) : (
                <div className="no-cover">?</div>
              )}
              <div className="candidate-info">
                <strong>{c.titre ?? "(sans titre)"}</strong>
                <span className="muted">
                  {[
                    c.auteurs.join(", "),
                    c.editeur,
                    c.date_parution,
                    c.ean,
                  ]
                    .filter(Boolean)
                    .join(" · ")}
                </span>
                {c.synopsis && <span className="candidate-synopsis">{c.synopsis}</span>}
                <span className="candidate-source">{c.source}</span>
              </div>
              <button className="primary" onClick={() => onPick(c)}>
                {pickLabel}
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
