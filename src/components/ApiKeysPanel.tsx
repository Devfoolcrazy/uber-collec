import { useEffect, useState } from "react";
import { api } from "../api";

interface Props {
  onDone: (message: string) => void;
  onCancel: () => void;
}

/** Saisie des clés d'API des sources externes. Les clés vivent dans la
 *  configuration locale de l'app, jamais dans la bibliothèque versionnée. */
export default function ApiKeysPanel({ onDone, onCancel }: Props) {
  const [status, setStatus] = useState<{ tmdb: boolean; discogs: boolean } | null>(null);
  const [tmdb, setTmdb] = useState("");
  const [discogs, setDiscogs] = useState("");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.apiKeysStatus().then(setStatus).catch(() => {});
  }, []);

  async function save() {
    setError(null);
    try {
      if (tmdb.trim()) await api.setApiKey("tmdb", tmdb.trim());
      if (discogs.trim()) await api.setApiKey("discogs", discogs.trim());
      onDone("Clés API enregistrées");
    } catch (e) {
      setError(String(e));
    }
  }

  return (
    <div className="item-form">
      <header>
        <h2>Clés API</h2>
      </header>

      <section className="sync-option">
        <h3>
          Discogs — CD audio{" "}
          {status?.discogs && <span className="complete">✓ configurée</span>}
        </h3>
        <p className="muted">
          Genres, éditions et pochettes complémentaires. Token personnel gratuit :
          discogs.com → Settings → Developers → Generate new token.
        </p>
        <input
          type="password"
          placeholder={status?.discogs ? "•••••• (remplacer)" : "token Discogs"}
          value={discogs}
          onChange={(e) => setDiscogs(e.target.value)}
        />
      </section>

      <section className="sync-option">
        <h3>
          TMDB — DVD / Blu-ray{" "}
          {status?.tmdb && <span className="complete">✓ configurée</span>}
        </h3>
        <p className="muted">
          Films, affiches, synopsis. Clé gratuite : themoviedb.org → Paramètres → API.
        </p>
        <input
          type="password"
          placeholder={status?.tmdb ? "•••••• (remplacer)" : "clé d'API TMDB"}
          value={tmdb}
          onChange={(e) => setTmdb(e.target.value)}
        />
      </section>

      {error && <p className="error">{error}</p>}

      <footer>
        <button className="primary" onClick={save} disabled={!tmdb.trim() && !discogs.trim()}>
          Enregistrer
        </button>
        <button onClick={onCancel}>Fermer</button>
      </footer>
    </div>
  );
}
