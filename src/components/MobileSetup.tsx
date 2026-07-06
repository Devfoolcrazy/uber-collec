import { useEffect, useState } from "react";
import { api } from "../api";

interface Props {
  onDone: () => void;
}

/** Premier lancement iOS : configuration du dépôt GitHub source et
 *  téléchargement de l'instantané de la collection. */
export default function MobileSetup({ onDone }: Props) {
  const [repo, setRepo] = useState("");
  const [token, setToken] = useState("");
  const [working, setWorking] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api
      .getMobileConfig()
      .then((c) => {
        if (c.repo) setRepo(c.repo);
      })
      .catch(() => {});
  }, []);

  async function download() {
    setError(null);
    if (!repo.trim() || !repo.includes("/")) {
      setError("Indique le dépôt au format « propriétaire/nom » (ex : Devfoolcrazy/ma-collection)");
      return;
    }
    if (!token.trim()) {
      setError("Indique le token d'accès (github_pat_…)");
      return;
    }
    setWorking(true);
    try {
      await api.mobileSync(repo.trim(), token.trim());
      onDone();
    } catch (e) {
      setError(String(e));
    } finally {
      setWorking(false);
    }
  }

  return (
    <div className="welcome mobile-setup">
      <h1>Uber Collec</h1>
      <p>Consultez votre collection depuis le dépôt GitHub qui la sauvegarde.</p>

      <label className="setup-field">
        Dépôt GitHub
        <input
          type="text"
          placeholder="Devfoolcrazy/ma-collection"
          value={repo}
          onChange={(e) => setRepo(e.target.value)}
          autoCapitalize="none"
          autoCorrect="off"
        />
      </label>

      <label className="setup-field">
        Token d'accès (lecture seule)
        <input
          type="password"
          placeholder="github_pat_…"
          value={token}
          onChange={(e) => setToken(e.target.value)}
        />
      </label>
      <p className="muted setup-hint">
        À créer une fois sur github.com → Settings → Developer settings →
        Fine-grained tokens : accès « Contents : Read-only » limité à ce dépôt.
      </p>

      <button className="primary" onClick={download} disabled={working}>
        {working ? "Téléchargement…" : "Télécharger ma collection"}
      </button>

      {error && <p className="error">{error}</p>}
    </div>
  );
}
