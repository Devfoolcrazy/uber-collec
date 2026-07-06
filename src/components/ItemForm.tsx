import { useEffect, useState } from "react";
import { confirm, open } from "@tauri-apps/plugin-dialog";
import { api, coverSrc, FieldDef, FieldValues, Item, Schema, Series, Statut } from "../api";

interface Props {
  collection: string;
  schema: Schema;
  /** Objet en édition, ou null pour une création. */
  item: Item | null;
  /** Pré-remplissage (création depuis un candidat d'hydratation). */
  initialFields?: FieldValues | null;
  libraryPath?: string | null;
  onSaved: (saved: Item, created: boolean) => void;
  onCancel: () => void;
  onDeleted: () => void;
}

interface SeriesValue {
  id?: string;
  tome?: number;
}

/** Formulaire d'objet entièrement généré depuis le schéma de la collection. */
export default function ItemForm({
  collection,
  schema,
  item,
  initialFields,
  libraryPath,
  onSaved,
  onCancel,
  onDeleted,
}: Props) {
  const [fields, setFields] = useState<FieldValues>({});
  const [statut, setStatut] = useState<Statut>("possede");
  const [emplacement, setEmplacement] = useState("");
  const [seriesList, setSeriesList] = useState<Series[]>([]);
  const [newSeriesFor, setNewSeriesFor] = useState<string | null>(null);
  const [newSeriesName, setNewSeriesName] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  const hasSeriesField = schema.fields.some((f) => f.type === "series_ref");

  useEffect(() => {
    if (item) {
      const { id: _id, cote: _cote, statut: s, emplacement: e, date_ajout: _d, ...rest } = item;
      setFields(rest as FieldValues);
      setStatut(s);
      setEmplacement(e ?? "");
    } else {
      setFields(initialFields ?? {});
      setStatut("possede");
      setEmplacement("");
    }
    setError(null);
    setNewSeriesFor(null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [item, collection]);

  useEffect(() => {
    if (hasSeriesField) {
      api.listSeries(collection).then(setSeriesList).catch(() => setSeriesList([]));
    }
  }, [collection, hasSeriesField]);

  const set = (key: string, value: unknown) =>
    setFields((f) => {
      const next = { ...f };
      if (value === "" || value === undefined || value === null) delete next[key];
      else next[key] = value;
      return next;
    });

  async function save() {
    setSaving(true);
    setError(null);
    try {
      let saved: Item;
      if (item) {
        saved = await api.updateItem(collection, item.id, statut, emplacement.trim() || null, fields);
      } else {
        saved = await api.createItem(collection, statut, fields);
      }
      onSaved(saved, !item);
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  }

  async function remove() {
    if (!item) return;
    // window.confirm ne fonctionne pas dans une WebView Tauri : dialogue natif.
    const ok = await confirm(`Supprimer définitivement ${item.id} ?`, {
      title: "Suppression",
      kind: "warning",
    });
    if (!ok) return;
    try {
      await api.deleteItem(collection, item.id);
      onDeleted();
    } catch (e) {
      setError(String(e));
    }
  }

  async function createSeries(fieldKey: string) {
    const nom = newSeriesName.trim();
    if (!nom) return;
    const id = nom
      .toLowerCase()
      .normalize("NFD")
      .replace(/[̀-ͯ]/g, "")
      .replace(/[^a-z0-9]+/g, "-")
      .replace(/^-|-$/g, "");
    try {
      await api.upsertSeries(collection, { id, nom, terminee: false });
      setSeriesList(await api.listSeries(collection));
      const current = (fields[fieldKey] ?? {}) as SeriesValue;
      set(fieldKey, { ...current, id });
      setNewSeriesFor(null);
      setNewSeriesName("");
    } catch (e) {
      setError(String(e));
    }
  }

  function renderField(def: FieldDef) {
    const value = fields[def.key];
    switch (def.type) {
      case "text":
      case "url":
        return (
          <input
            type="text"
            value={(value as string) ?? ""}
            onChange={(e) => set(def.key, e.target.value)}
          />
        );
      case "longtext":
        return (
          <textarea
            rows={4}
            value={(value as string) ?? ""}
            onChange={(e) => set(def.key, e.target.value)}
          />
        );
      case "text[]":
      case "tags":
        return (
          <input
            type="text"
            placeholder="valeurs séparées par ;"
            value={Array.isArray(value) ? (value as string[]).join(" ; ") : ""}
            onChange={(e) =>
              set(
                def.key,
                e.target.value
                  .split(";")
                  .map((s) => s.trim())
                  .filter(Boolean),
              )
            }
          />
        );
      case "number":
        return (
          <input
            type="number"
            value={(value as number) ?? ""}
            onChange={(e) => set(def.key, e.target.value === "" ? "" : Number(e.target.value))}
          />
        );
      case "rating":
        return (
          <input
            type="number"
            min={0}
            max={def.max ?? 5}
            value={(value as number) ?? ""}
            onChange={(e) => set(def.key, e.target.value === "" ? "" : Number(e.target.value))}
          />
        );
      case "date":
        return (
          <input
            type="date"
            value={(value as string) ?? ""}
            onChange={(e) => set(def.key, e.target.value)}
          />
        );
      case "boolean":
        return (
          <input
            type="checkbox"
            checked={Boolean(value)}
            onChange={(e) => set(def.key, e.target.checked)}
          />
        );
      case "select":
        return (
          <select value={(value as string) ?? ""} onChange={(e) => set(def.key, e.target.value)}>
            <option value="">—</option>
            {(def.options ?? []).map((o) => (
              <option key={o.value} value={o.value}>
                {o.value}
              </option>
            ))}
          </select>
        );
      case "series_ref": {
        const sv = (value ?? {}) as SeriesValue;
        if (newSeriesFor === def.key) {
          return (
            <div className="series-field">
              <input
                type="text"
                autoFocus
                placeholder="Nom de la nouvelle série"
                value={newSeriesName}
                onChange={(e) => setNewSeriesName(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") void createSeries(def.key);
                  if (e.key === "Escape") setNewSeriesFor(null);
                }}
              />
              <button type="button" className="primary" onClick={() => createSeries(def.key)}>
                Créer
              </button>
              <button type="button" onClick={() => setNewSeriesFor(null)}>
                Annuler
              </button>
            </div>
          );
        }
        return (
          <div className="series-field">
            <select
              value={sv.id ?? ""}
              onChange={(e) => {
                const id = e.target.value;
                if (!id) set(def.key, "");
                else set(def.key, { ...sv, id });
              }}
            >
              <option value="">One-shot / aucune</option>
              {seriesList.map((s) => (
                <option key={s.id} value={s.id}>
                  {s.nom}
                  {s.terminee ? " ✓" : ""}
                </option>
              ))}
            </select>
            <input
              type="number"
              className="tome"
              placeholder="Tome"
              min={0}
              disabled={!sv.id}
              value={sv.tome ?? ""}
              onChange={(e) =>
                set(def.key, {
                  ...sv,
                  tome: e.target.value === "" ? undefined : Number(e.target.value),
                })
              }
            />
            <button
              type="button"
              className="ghost"
              onClick={() => {
                setNewSeriesName("");
                setNewSeriesFor(def.key);
              }}
            >
              + série
            </button>
          </div>
        );
      }
      case "image": {
        const rel = value as string | undefined;
        const pickImage = async () => {
          if (!item) return;
          const file = await open({
            title: "Choisir une image",
            filters: [{ name: "Images", extensions: ["jpg", "jpeg", "png", "webp", "gif"] }],
          });
          if (typeof file !== "string") return;
          try {
            const newRel = await api.setCoverFromFile(collection, item.id, file);
            set(def.key, newRel);
          } catch (e) {
            setError(String(e));
          }
        };
        return (
          <div className="image-field">
            {rel && libraryPath ? (
              <img className="form-cover" src={coverSrc(libraryPath, rel)} alt="" />
            ) : (
              <span className="muted">
                Récupérée via « Scanner » / « Compléter depuis les bases »
              </span>
            )}
            {item && (
              <button type="button" className="ghost" onClick={pickImage}>
                {rel ? "Remplacer l'image…" : "Choisir une image…"}
              </button>
            )}
          </div>
        );
      }
      default:
        return <span className="muted">type non géré : {def.type}</span>;
    }
  }

  return (
    <div className="item-form">
      <header>
        <h2>{item ? `${item.id}` : `Nouvel objet — ${schema.name}`}</h2>
        {item?.cote && <span className="cote-badge">{item.cote}</span>}
      </header>

      <div className="form-grid">
        <label>Statut</label>
        <select value={statut} onChange={(e) => setStatut(e.target.value as Statut)}>
          <option value="possede">Possédé</option>
          <option value="souhaite">Souhaité (wishlist)</option>
        </select>

        <label>Emplacement</label>
        <input
          type="text"
          placeholder="ex : Salon / Étagère B / Rangée 3"
          value={emplacement}
          onChange={(e) => setEmplacement(e.target.value)}
        />

        {schema.fields.map((def) => (
          <FieldRow key={def.key} def={def}>
            {renderField(def)}
          </FieldRow>
        ))}
      </div>

      {error && <p className="error">{error}</p>}

      <footer>
        <button className="primary" onClick={save} disabled={saving}>
          {saving ? "Enregistrement…" : "Enregistrer"}
        </button>
        <button onClick={onCancel}>Annuler</button>
        {item && (
          <button className="danger" onClick={remove}>
            Supprimer
          </button>
        )}
      </footer>
    </div>
  );
}

function FieldRow({ def, children }: { def: FieldDef; children: React.ReactNode }) {
  return (
    <>
      <label>
        {def.label}
        {def.required && <span className="required">*</span>}
      </label>
      <div>{children}</div>
    </>
  );
}
