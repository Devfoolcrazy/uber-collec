import { useEffect, useState } from "react";
import { api, FieldDef } from "../api";

interface Props {
  collection: string;
  /** Champ liste de textes agrégé (scénaristes, artistes, réalisateurs…). */
  field: FieldDef;
  /** Ouvre l'onglet Objets avec cette personne en recherche. */
  onOpenPerson: (name: string) => void;
}

/** Onglet « personnes » générique : toutes les valeurs d'un champ avec leur
 *  nombre d'œuvres, filtrables, cliquables. */
export default function PersonsPanel({ collection, field, onOpenPerson }: Props) {
  const [values, setValues] = useState<[string, number][]>([]);
  const [filter, setFilter] = useState("");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setFilter("");
    api
      .fieldValues(collection, field.key)
      .then(setValues)
      .catch((e) => setError(String(e)));
  }, [collection, field.key]);

  const normalized = filter
    .toLowerCase()
    .normalize("NFD")
    .replace(/[̀-ͯ]/g, "");
  const shown = normalized
    ? values.filter(([name]) =>
        name
          .toLowerCase()
          .normalize("NFD")
          .replace(/[̀-ͯ]/g, "")
          .includes(normalized),
      )
    : values;
  const max = values[0]?.[1] ?? 1;

  return (
    <div className="persons-panel">
      <div className="series-toolbar">
        <div className="labels-filters">
          <input
            type="search"
            placeholder={`Filtrer ${values.length} noms…`}
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
          />
          <span className="muted">
            {shown.length !== values.length ? `${shown.length} / ` : ""}
            {values.length} {field.label.toLowerCase()}
          </span>
        </div>
      </div>

      {error && <p className="error">{error}</p>}

      <div className="persons-grid">
        {shown.map(([name, count]) => (
          <button
            key={name}
            className="person-row"
            title="Voir les œuvres"
            onClick={() => onOpenPerson(name)}
          >
            <span className="person-name">{name}</span>
            <span className="person-bar">
              <span className="person-fill" style={{ width: `${(count / max) * 100}%` }} />
            </span>
            <span className="person-count mono">{count}</span>
          </button>
        ))}
      </div>
      {shown.length === 0 && <p className="empty">Aucun nom.</p>}
    </div>
  );
}
