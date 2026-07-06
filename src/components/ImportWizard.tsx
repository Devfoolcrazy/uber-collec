import { useEffect, useMemo, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  api,
  ColumnMapping,
  CsvPreview,
  FieldDef,
  ImportReport,
  Schema,
} from "../api";

interface Props {
  collection: string;
  schema: Schema;
  onDone: (report: ImportReport) => void;
  onCancel: () => void;
}

const IGNORE = "__ignore";
const SERIE = "__serie";
const TOME = "__tome";

function normalize(s: string): string {
  return s
    .toLowerCase()
    .normalize("NFD")
    .replace(/[̀-ͯ]/g, "")
    .trim();
}

/** Propose une cible pour une colonne CSV d'après son intitulé. */
function suggestMapping(header: string, schema: Schema): ColumnMapping {
  const h = normalize(header);
  const hasSeries = schema.fields.some((f) => f.type === "series_ref");

  if (hasSeries && ["serie", "saga", "cycle"].includes(h)) {
    return { column: header, target: SERIE };
  }
  if (hasSeries && ["tome", "n° tome", "numero", "volume"].includes(h)) {
    return { column: header, target: TOME };
  }

  const mappable = schema.fields.filter(
    (f) => f.type !== "image" && f.type !== "series_ref",
  );
  const withTransform = (f: FieldDef): ColumnMapping => ({
    column: header,
    target: f.key,
    transform:
      f.type === "text[]" &&
      ["auteur", "scenariste", "dessinateur", "realisateur", "artiste"].some((k) =>
        normalize(f.key).includes(k),
      )
        ? "nom_prenom"
        : undefined,
  });

  // Correspondance exacte clé ou libellé.
  for (const f of mappable) {
    if (normalize(f.key) === h || normalize(f.label) === h) return withTransform(f);
  }
  // Alias fréquents.
  const aliases: Record<string, string[]> = {
    ean: ["ean", "isbn", "code-barres", "code barres", "ean / isbn"],
    isbn: ["isbn", "ean"],
    collection_editeur: ["collection", "collection editeur"],
    date_parution: ["date parution", "date de parution", "parution"],
    date_sortie: ["date sortie", "date de sortie", "sortie", "annee"],
  };
  for (const f of mappable) {
    if ((aliases[f.key] ?? []).includes(h)) return withTransform(f);
  }
  // Libellé contenant l'intitulé (ex : « Scenariste » vs « Scénariste(s) »).
  for (const f of mappable) {
    if (normalize(f.label).includes(h) || normalize(f.key).includes(h)) {
      return withTransform(f);
    }
  }
  return { column: header, target: IGNORE };
}

export default function ImportWizard({ collection, schema, onDone, onCancel }: Props) {
  const [path, setPath] = useState<string | null>(null);
  const [preview, setPreview] = useState<CsvPreview | null>(null);
  const [mappings, setMappings] = useState<ColumnMapping[]>([]);
  const [skipDuplicates, setSkipDuplicates] = useState(true);
  const [oneshotRule, setOneshotRule] = useState(true);
  const [running, setRunning] = useState(false);
  const [report, setReport] = useState<ImportReport | null>(null);
  const [error, setError] = useState<string | null>(null);

  const hasSeries = schema.fields.some((f) => f.type === "series_ref");
  const targets = useMemo(() => {
    const fields = schema.fields.filter(
      (f) => f.type !== "image" && f.type !== "series_ref",
    );
    return [
      { value: IGNORE, label: "— Ignorer —" },
      ...(hasSeries
        ? [
            { value: SERIE, label: "Série (nom)" },
            { value: TOME, label: "N° de tome" },
          ]
        : []),
      ...fields.map((f) => ({ value: f.key, label: f.label })),
    ];
  }, [schema, hasSeries]);

  async function pickFile() {
    const selected = await open({
      title: "Fichier CSV à importer",
      filters: [{ name: "CSV", extensions: ["csv"] }],
    });
    if (typeof selected !== "string") {
      onCancel();
      return;
    }
    try {
      const p = await api.previewCsv(selected);
      setPath(selected);
      setPreview(p);
      setMappings(p.headers.map((h) => suggestMapping(h, schema)));
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }

  useEffect(() => {
    void pickFile();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  function setTarget(column: string, target: string) {
    setMappings((ms) =>
      ms.map((m) =>
        m.column === column
          ? { ...m, target, transform: target === m.target ? m.transform : undefined }
          : m,
      ),
    );
  }

  function setTransform(column: string, on: boolean) {
    setMappings((ms) =>
      ms.map((m) =>
        m.column === column ? { ...m, transform: on ? "nom_prenom" : undefined } : m,
      ),
    );
  }

  async function run() {
    if (!path) return;
    setRunning(true);
    setError(null);
    try {
      // Si plusieurs colonnes visent le même champ, celle dont l'intitulé
      // correspond exactement au champ passe en premier (première valeur
      // non vide gagnante côté backend).
      const ordered = [...mappings].sort((a, b) => {
        if (a.target !== b.target) return 0;
        const exactA = normalize(a.column) === normalize(a.target) ? 0 : 1;
        const exactB = normalize(b.column) === normalize(b.target) ? 0 : 1;
        return exactA - exactB;
      });
      const r = await api.importCsv(collection, path, ordered, {
        skip_duplicates: skipDuplicates,
        oneshot_if_serie_equals_titre: oneshotRule,
      });
      setReport(r);
    } catch (e) {
      setError(String(e));
    } finally {
      setRunning(false);
    }
  }

  if (report) {
    return (
      <div className="item-form">
        <header>
          <h2>Import terminé</h2>
        </header>
        <ul className="report">
          <li>
            <strong>{report.imported}</strong> objets importés sur {report.total_rows}{" "}
            lignes
          </li>
          {report.skipped_duplicates > 0 && (
            <li>{report.skipped_duplicates} doublons ignorés</li>
          )}
          {report.series_created > 0 && <li>{report.series_created} séries créées</li>}
          {report.genres_added.length > 0 && (
            <li>
              Genres ajoutés au schéma (code de cote dérivé, modifiable) :{" "}
              {report.genres_added.join(", ")}
            </li>
          )}
          {report.errors.length > 0 && (
            <li>
              <details>
                <summary>{report.errors.length} avertissements</summary>
                <ul>
                  {report.errors.map((e, i) => (
                    <li key={i}>{e}</li>
                  ))}
                </ul>
              </details>
            </li>
          )}
        </ul>
        <footer>
          <button className="primary" onClick={() => onDone(report)}>
            Voir la collection
          </button>
        </footer>
      </div>
    );
  }

  if (!preview) {
    return (
      <div className="item-form">
        <header>
          <h2>Import CSV — {schema.name}</h2>
        </header>
        {error ? <p className="error">{error}</p> : <p>Sélection du fichier…</p>}
        <footer>
          <button onClick={pickFile}>Choisir un fichier</button>
          <button onClick={onCancel}>Annuler</button>
        </footer>
      </div>
    );
  }

  return (
    <div className="item-form import-wizard">
      <header>
        <h2>Import CSV — {schema.name}</h2>
        <span className="muted">
          {preview.total_rows} lignes · {path?.split("/").pop()}
        </span>
      </header>

      <table className="mapping">
        <thead>
          <tr>
            <th>Colonne CSV</th>
            <th>Exemple</th>
            <th>Champ cible</th>
            <th>Nom, Prénom → Prénom Nom</th>
          </tr>
        </thead>
        <tbody>
          {preview.headers.map((h, col) => {
            const m = mappings.find((x) => x.column === h)!;
            const targetField = schema.fields.find((f) => f.key === m.target);
            // Réservé aux champs multi-valeurs (auteurs) : sur un titre,
            // cette transformation corromprait « Lastman, Tome 1 ».
            const canTransform = targetField?.type === "text[]";
            return (
              <tr key={h}>
                <td>
                  <strong>{h}</strong>
                </td>
                <td className="muted sample">
                  {preview.rows[0]?.[col] || preview.rows[1]?.[col] || ""}
                </td>
                <td>
                  <select value={m.target} onChange={(e) => setTarget(h, e.target.value)}>
                    {targets.map((t) => (
                      <option key={t.value} value={t.value}>
                        {t.label}
                      </option>
                    ))}
                  </select>
                </td>
                <td className="center">
                  {canTransform && (
                    <input
                      type="checkbox"
                      checked={m.transform === "nom_prenom"}
                      onChange={(e) => setTransform(h, e.target.checked)}
                    />
                  )}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>

      <div className="import-options">
        <label>
          <input
            type="checkbox"
            checked={skipDuplicates}
            onChange={(e) => setSkipDuplicates(e.target.checked)}
          />
          Ignorer les doublons (même EAN, ou même titre + série + tome)
        </label>
        {hasSeries && (
          <label>
            <input
              type="checkbox"
              checked={oneshotRule}
              onChange={(e) => setOneshotRule(e.target.checked)}
            />
            Série identique au titre et sans tome → one-shot (pas de série créée)
          </label>
        )}
      </div>

      {error && <p className="error">{error}</p>}

      <footer>
        <button className="primary" onClick={run} disabled={running}>
          {running ? "Import en cours…" : `Importer ${preview.total_rows} lignes`}
        </button>
        <button onClick={onCancel} disabled={running}>
          Annuler
        </button>
      </footer>
    </div>
  );
}
