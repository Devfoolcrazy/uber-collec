import { useState } from "react";
import { api, SyncStatus } from "../api";

interface Props {
  status: SyncStatus;
  onDone: (message: string) => void;
  onCancel: () => void;
}

/** Configuration de la sauvegarde Git : initialisation du dépôt local puis
 *  création du dépôt GitHub via le CLI gh (déjà authentifié), ou liaison
 *  d'un dépôt existant par son URL SSH. */
export default function SyncPanel({ status, onDone, onCancel }: Props) {
  const [repoName, setRepoName] = useState("ma-collection");
  const [isPrivate, setIsPrivate] = useState(true);
  const [existingUrl, setExistingUrl] = useState("");
  const [working, setWorking] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function initOnly() {
    setWorking("Initialisation du dépôt et premier commit…");
    setError(null);
    try {
      await api.syncInit();
      onDone("Historique Git activé (local). Vous pourrez le publier sur GitHub plus tard.");
    } catch (e) {
      setError(String(e));
    } finally {
      setWorking(null);
    }
  }

  async function createOnGithub() {
    setWorking("Initialisation, création du dépôt GitHub et premier envoi (peut prendre une minute : ~20 Mo de couvertures)…");
    setError(null);
    try {
      if (!status.is_repo) await api.syncInit();
      const url = await api.syncCreateGithub(repoName.trim(), isPrivate);
      onDone(`Sauvegarde GitHub active : ${url}`);
    } catch (e) {
      setError(String(e));
    } finally {
      setWorking(null);
    }
  }

  async function linkExisting() {
    setWorking("Liaison du dépôt et premier envoi…");
    setError(null);
    try {
      if (!status.is_repo) await api.syncInit();
      await api.syncSetRemote(existingUrl.trim());
      onDone(`Sauvegarde GitHub active : ${existingUrl.trim()}`);
    } catch (e) {
      setError(String(e));
    } finally {
      setWorking(null);
    }
  }

  return (
    <div className="item-form sync-panel">
      <header>
        <h2>Sauvegarde Git / GitHub</h2>
      </header>

      <p>
        Chaque modification (ajout, édition, import, couvertures…) devient un commit
        dans l'historique de votre bibliothèque, puis est poussée automatiquement sur
        GitHub. Vous pouvez tout retrouver, tout historiser, et l'app iOS s'appuiera
        sur ce dépôt.
      </p>

      {!status.remote && (
        <>
          <section className="sync-option">
            <h3>Créer un dépôt sur votre compte GitHub (recommandé)</h3>
            <p className="muted">
              Via le CLI <code>gh</code> détecté et déjà connecté à votre compte.
            </p>
            <div className="sync-row">
              <input
                type="text"
                value={repoName}
                onChange={(e) => setRepoName(e.target.value)}
                placeholder="nom-du-depot"
              />
              <label className="inline-label">
                <input
                  type="checkbox"
                  checked={isPrivate}
                  onChange={(e) => setIsPrivate(e.target.checked)}
                />
                Privé
              </label>
              <button className="primary" onClick={createOnGithub} disabled={!!working}>
                Créer et pousser
              </button>
            </div>
          </section>

          <section className="sync-option">
            <h3>Ou lier un dépôt existant</h3>
            <div className="sync-row">
              <input
                type="text"
                value={existingUrl}
                onChange={(e) => setExistingUrl(e.target.value)}
                placeholder="git@github.com:Devfoolcrazy/ma-collection.git"
              />
              <button onClick={linkExisting} disabled={!existingUrl.trim() || !!working}>
                Lier et pousser
              </button>
            </div>
          </section>

          {!status.is_repo && (
            <section className="sync-option">
              <h3>Ou historique local seulement</h3>
              <button onClick={initOnly} disabled={!!working}>
                Activer Git sans GitHub
              </button>
            </section>
          )}
        </>
      )}

      {working && <p className="notice">{working}</p>}
      {error && <p className="error">{error}</p>}

      <footer>
        <button onClick={onCancel} disabled={!!working}>
          Fermer
        </button>
      </footer>
    </div>
  );
}
