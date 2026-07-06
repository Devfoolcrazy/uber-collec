import { useEffect, useState } from "react";
import { confirm } from "@tauri-apps/plugin-dialog";
import { api, FieldDef, FieldType, Schema, SourceInfo } from "../api";

interface Props {
  mode: "create" | "edit";
  /** Slug + schéma existants (édition). */
  slug?: string;
  schema?: Schema;
  /** Nombre d'objets de la collection (édition), pour la confirmation. */
  itemCount?: number;
  onSaved: (slug: string) => void;
  onDeleted?: () => void;
  onCancel: () => void;
}

const TYPE_LABELS: [FieldType, string][] = [
  ["text", "Texte"],
  ["longtext", "Texte long"],
  ["text[]", "Liste de textes (auteurs…)"],
  ["number", "Nombre"],
  ["date", "Date"],
  ["select", "Liste à choix (genre…)"],
  ["tags", "Étiquettes libres"],
  ["boolean", "Case à cocher"],
  ["rating", "Note"],
  ["url", "Lien"],
  ["image", "Image / couverture"],
  ["series_ref", "Série"],
];

const RESERVED = ["id", "cote", "statut", "emplacement", "date_ajout"];

interface EditableField {
  key: string;
  label: string;
  type: FieldType;
  required: boolean;
  /** Une option par ligne : « Valeur » ou « Valeur = CODE ». */
  optionsText: string;
  max?: number;
  /** Champ présent avant l'édition : clé et type verrouillés. */
  existing: boolean;
}

function slugKey(label: string, taken: string[]): string {
  let base = label
    .toLowerCase()
    .normalize("NFD")
    .replace(/[̀-ͯ]/g, "")
    .replace(/[^a-z0-9]+/g, "_")
    .replace(/^_+|_+$/g, "");
  if (!base) base = "champ";
  if (RESERVED.includes(base)) base = `${base}_2`;
  let key = base;
  let n = 2;
  while (taken.includes(key)) key = `${base}_${n++}`;
  return key;
}

function toEditable(f: FieldDef): EditableField {
  return {
    key: f.key,
    label: f.label,
    type: f.type,
    required: f.required ?? false,
    optionsText: (f.options ?? [])
      .map((o) => (o.code ? `${o.value} = ${o.code}` : o.value))
      .join("\n"),
    max: f.max,
    existing: true,
  };
}

export default function SchemaEditor({
  mode,
  slug,
  schema,
  itemCount,
  onSaved,
  onDeleted,
  onCancel,
}: Props) {
  const [name, setName] = useState(schema?.name ?? "");
  const [idPrefix, setIdPrefix] = useState(schema?.id_prefix ?? "");
  const [collSlug, setCollSlug] = useState(slug ?? "");
  const [source, setSource] = useState(schema?.source ?? "");
  const [fields, setFields] = useState<EditableField[]>(
    schema?.fields.map(toEditable) ?? [
      { key: "titre", label: "Titre", type: "text", required: true, optionsText: "", existing: false },
    ],
  );
  const [coteYear, setCoteYear] = useState(schema?.cote?.year_field ?? "");
  const [coteGenre, setCoteGenre] = useState(schema?.cote?.genre_field ?? "");
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [catalog, setCatalog] = useState<SourceInfo[]>([]);

  useEffect(() => {
    api.hydrationSources().then(setCatalog).catch(() => setCatalog([]));
  }, []);

  const selectedSource = catalog.find((s) => s.id === source);

  const set = (i: number, patch: Partial<EditableField>) =>
    setFields((fs) => fs.map((f, j) => (j === i ? { ...f, ...patch } : f)));

  const move = (i: number, delta: number) =>
    setFields((fs) => {
      const j = i + delta;
      if (j < 0 || j >= fs.length) return fs;
      const next = [...fs];
      [next[i], next[j]] = [next[j], next[i]];
      return next;
    });

  function addField() {
    setFields((fs) => [
      ...fs,
      { key: "", label: "", type: "text", required: false, optionsText: "", existing: false },
    ]);
  }

  function buildSchema(): Schema {
    const taken: string[] = [];
    const builtFields: FieldDef[] = fields.map((f) => {
      const key = f.existing ? f.key : f.key || slugKey(f.label, taken);
      taken.push(key);
      return {
        key,
        label: f.label.trim() || key,
        type: f.type,
        required: f.required,
        options:
          f.type === "select"
            ? f.optionsText
                .split("\n")
                .map((line) => line.trim())
                .filter(Boolean)
                .map((line) => {
                  const [value, code] = line.split("=").map((s) => s.trim());
                  return code ? { value, code } : { value };
                })
            : undefined,
        max: f.type === "rating" ? (f.max ?? 5) : undefined,
      };
    });
    return {
      name: name.trim(),
      id_prefix: idPrefix.trim().toUpperCase(),
      source: source || undefined,
      cote: coteYear && coteGenre ? { year_field: coteYear, genre_field: coteGenre } : undefined,
      fields: builtFields,
    };
  }

  async function save() {
    setSaving(true);
    setError(null);
    try {
      const built = buildSchema();
      if (mode === "create") {
        const s = collSlug
          .trim()
          .toLowerCase()
          .normalize("NFD")
          .replace(/[̀-ͯ]/g, "")
          .replace(/[^a-z0-9]+/g, "-")
          .replace(/^-+|-+$/g, "");
        await api.createCollection(s, built);
        onSaved(s);
      } else if (slug) {
        await api.saveSchema(slug, built);
        onSaved(slug);
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  }

  const dateFields = fields.filter((f) => f.type === "date");
  const selectFields = fields.filter((f) => f.type === "select");

  return (
    <div className="item-form schema-editor">
      <header>
        <h2>
          {mode === "create"
            ? "Nouvelle collection"
            : `Schéma de la collection « ${schema?.name} »`}
        </h2>
      </header>

      <div className="form-grid">
        <label>Nom</label>
        <input
          type="text"
          placeholder="ex : Vinyles"
          value={name}
          onChange={(e) => {
            setName(e.target.value);
            if (mode === "create") {
              setCollSlug(
                e.target.value
                  .toLowerCase()
                  .normalize("NFD")
                  .replace(/[̀-ͯ]/g, "")
                  .replace(/[^a-z0-9]+/g, "-")
                  .replace(/^-+|-+$/g, ""),
              );
              setIdPrefix(
                e.target.value
                  .normalize("NFD")
                  .replace(/[̀-ͯ]/g, "")
                  .replace(/[^a-zA-Z0-9]/g, "")
                  .toUpperCase()
                  .slice(0, 3),
              );
            }
          }}
        />

        <label>Préfixe d'ID</label>
        {mode === "create" ? (
          <input
            type="text"
            placeholder="ex : VIN (→ VIN-00001)"
            value={idPrefix}
            onChange={(e) => setIdPrefix(e.target.value.toUpperCase())}
          />
        ) : (
          <div className="muted">
            {idPrefix} <em>(figé : des objets portent déjà ces identifiants)</em>
          </div>
        )}

        <label>Hydratation</label>
        <div>
          <select value={source} onChange={(e) => setSource(e.target.value)}>
            <option value="">Aucune (saisie manuelle)</option>
            {catalog.map((s) => (
              <option key={s.id} value={s.id}>
                {s.label}
                {s.requires_key ? " · 🔑 clé requise" : ""}
              </option>
            ))}
          </select>
          {selectedSource && (
            <p className="muted source-hint">
              {selectedSource.description}
              <br />
              Champs remplis automatiquement (nommez vos champs ainsi) :{" "}
              <code>{selectedSource.fills.join(", ")}</code>
              {selectedSource.requires_key &&
                " — clé à saisir dans « 🔑 Clés API »."}
            </p>
          )}
        </div>
      </div>

      <h3 className="fields-title">Champs</h3>
      <table className="items schema-fields">
        <thead>
          <tr>
            <th>Libellé</th>
            <th>Type</th>
            <th className="center">Requis</th>
            <th>Détails</th>
            <th className="col-actions"></th>
          </tr>
        </thead>
        <tbody>
          {fields.map((f, i) => (
            <tr key={i}>
              <td>
                <input
                  type="text"
                  placeholder="ex : Artiste"
                  value={f.label}
                  onChange={(e) => set(i, { label: e.target.value })}
                />
                {f.existing && <div className="muted key-hint">{f.key}</div>}
              </td>
              <td>
                <select
                  value={f.type}
                  disabled={f.existing}
                  title={f.existing ? "Type figé : des données existent peut-être" : ""}
                  onChange={(e) => set(i, { type: e.target.value as FieldType })}
                >
                  {TYPE_LABELS.map(([t, label]) => (
                    <option key={t} value={t}>
                      {label}
                    </option>
                  ))}
                </select>
              </td>
              <td className="center">
                <input
                  type="checkbox"
                  checked={f.type === "image" ? false : f.required}
                  disabled={f.type === "image"}
                  title={
                    f.type === "image"
                      ? "Une image se remplit par hydratation, jamais à la main : « requis » bloquerait la saisie"
                      : ""
                  }
                  onChange={(e) => set(i, { required: e.target.checked })}
                />
              </td>
              <td>
                {f.type === "select" && (
                  <textarea
                    rows={3}
                    placeholder={"Une valeur par ligne, code de cote optionnel :\nRock = ROCK\nJazz = JAZZ"}
                    value={f.optionsText}
                    onChange={(e) => set(i, { optionsText: e.target.value })}
                  />
                )}
                {f.type === "rating" && (
                  <label className="inline-label">
                    Note max{" "}
                    <input
                      type="number"
                      min={2}
                      max={10}
                      value={f.max ?? 5}
                      onChange={(e) => set(i, { max: Number(e.target.value) })}
                    />
                  </label>
                )}
              </td>
              <td className="col-actions">
                <button className="ghost" title="Monter" onClick={() => move(i, -1)}>
                  ↑
                </button>
                <button className="ghost" title="Descendre" onClick={() => move(i, 1)}>
                  ↓
                </button>
                <button
                  className="ghost danger-ghost"
                  title="Supprimer le champ (les données déjà saisies restent dans les fichiers)"
                  onClick={() => setFields((fs) => fs.filter((_, j) => j !== i))}
                >
                  ✕
                </button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      <button className="ghost" onClick={addField}>
        + Ajouter un champ
      </button>

      <h3 className="fields-title">Cote d'étiquetage (AAAA-GENRE-NNNN)</h3>
      <div className="form-grid">
        <label>Champ année</label>
        <select value={coteYear} onChange={(e) => setCoteYear(e.target.value)}>
          <option value="">— pas de cote —</option>
          {dateFields.map((f) => (
            <option key={f.key || f.label} value={f.key || slugKey(f.label, [])}>
              {f.label || f.key}
            </option>
          ))}
        </select>
        <label>Champ genre</label>
        <select value={coteGenre} onChange={(e) => setCoteGenre(e.target.value)}>
          <option value="">— pas de cote —</option>
          {selectFields.map((f) => (
            <option key={f.key || f.label} value={f.key || slugKey(f.label, [])}>
              {f.label || f.key}
            </option>
          ))}
        </select>
      </div>

      {error && <p className="error">{error}</p>}

      <footer>
        <button className="primary" onClick={save} disabled={saving}>
          {mode === "create" ? "Créer la collection" : "Enregistrer le schéma"}
        </button>
        <button onClick={onCancel}>Annuler</button>
        {mode === "edit" && slug && onDeleted && (
          <button
            className="danger"
            onClick={async () => {
              const n = itemCount ?? 0;
              const first = await confirm(
                n > 0
                  ? `Supprimer la collection « ${schema?.name} » et ses ${n} objets (fiches, images, séries) ?`
                  : `Supprimer la collection vide « ${schema?.name} » ?`,
                { title: "Suppression de collection", kind: "warning" },
              );
              if (!first) return;
              if (n > 0) {
                const second = await confirm(
                  `Dernière confirmation : les ${n} objets seront retirés de la bibliothèque.\n\n` +
                    `La suppression est enregistrée dans l'historique Git — récupérable au besoin.`,
                  { title: "Vraiment supprimer ?", kind: "warning" },
                );
                if (!second) return;
              }
              try {
                await api.deleteCollection(slug);
                onDeleted();
              } catch (e) {
                setError(String(e));
              }
            }}
          >
            Supprimer la collection
          </button>
        )}
      </footer>
    </div>
  );
}
