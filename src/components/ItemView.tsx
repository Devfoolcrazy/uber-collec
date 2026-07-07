import { confirm } from "@tauri-apps/plugin-dialog";
import { api, coverSrc, FieldDef, Item, Schema, Series } from "../api";

interface Props {
  collection: string;
  schema: Schema;
  item: Item;
  seriesList: Series[];
  libraryPath: string | null | undefined;
  /** Mode consultation pure (iOS) : aucune action de modification. */
  readOnly?: boolean;
  onEdit: () => void;
  /** Présent si la collection a une source d'hydratation. */
  onEnrich?: () => void;
  onClose: () => void;
  onDeleted: () => void;
}

/** Fiche en mode consultation : lecture seule, cote mise en avant pour
 *  l'étiquetage, champs vides masqués. */
export default function ItemView({
  collection,
  schema,
  item,
  seriesList,
  libraryPath,
  readOnly,
  onEdit,
  onEnrich,
  onClose,
  onDeleted,
}: Props) {
  const titleKey = schema.fields.find((f) => f.type === "text" && f.required)?.key;
  const title = (titleKey && (item[titleKey] as string)) || item.id;
  const imageKey = schema.fields.find((f) => f.type === "image")?.key;
  const coverRel = imageKey ? (item[imageKey] as string | undefined) : undefined;

  async function remove() {
    const ok = await confirm(`Supprimer définitivement « ${title} » (${item.id}) ?`, {
      title: "Suppression",
      kind: "warning",
    });
    if (!ok) return;
    await api.deleteItem(collection, item.id);
    onDeleted();
  }

  function renderValue(def: FieldDef): React.ReactNode | null {
    const value = item[def.key];
    if (value === undefined || value === null || value === "") return null;
    switch (def.type) {
      case "text":
      case "url":
      case "date":
        return String(value);
      case "longtext":
        return <p className="view-longtext">{String(value)}</p>;
      case "text[]":
      case "tags":
        return Array.isArray(value) ? (value as string[]).join(" ; ") : String(value);
      case "number":
        return String(value);
      case "rating":
        return `${value} / ${def.max ?? 5}`;
      case "boolean":
        return value ? "Oui" : "Non";
      case "select":
        return String(value);
      case "series_ref": {
        const sv = value as { id?: string; tome?: number };
        if (!sv.id) return null;
        const serie = seriesList.find((s) => s.id === sv.id);
        const nom = serie?.nom ?? sv.id;
        return (
          <>
            {nom}
            {sv.tome != null && <span className="muted"> · Tome {sv.tome}</span>}
            {serie?.terminee && <span className="muted"> · série terminée</span>}
          </>
        );
      }
      case "image":
        return null; // lot 3
      default:
        return String(value);
    }
  }

  const allRows = schema.fields
    .filter((f) => !(f.type === "text" && f.key === titleKey))
    .map((def) => ({ def, node: renderValue(def) }))
    .filter((r) => r.node !== null);
  // Les textes longs (synopsis…) se lisent en paragraphes, pas en grille.
  const gridRows = allRows.filter((r) => r.def.type !== "longtext");
  const longRows = allRows.filter((r) => r.def.type === "longtext");
  const hasCover = Boolean(coverRel && libraryPath);

  return (
    <div className="item-form item-view">
      <div className="view-hero">
        {hasCover && (
          <div className="view-cover-col">
            <img className="view-cover" src={coverSrc(libraryPath!, coverRel!)} alt={title} />
            {item.cote && <span className="cote-badge cote-big">{item.cote}</span>}
          </div>
        )}
        <div className="view-main">
          <h2>{title}</h2>
          <p className="view-meta muted">
            {item.id} · ajouté le {item.date_ajout}
            {item.statut === "souhaite" && " · ★ wishlist"}
            {item.emplacement && ` · 📍 ${item.emplacement}`}
          </p>
          {!hasCover && item.cote && (
            <p>
              <span className="cote-badge cote-big">{item.cote}</span>
            </p>
          )}
          <div className="view-grid">
            {gridRows.map(({ def, node }) => (
              <FieldRow key={def.key} label={def.label}>
                {node}
              </FieldRow>
            ))}
          </div>
          {longRows.map(({ def, node }) => (
            <section key={def.key} className="view-long">
              <h4>{def.label}</h4>
              {node}
            </section>
          ))}
        </div>
      </div>

      <footer>
        {!readOnly && (
          <button className="primary" onClick={onEdit}>
            Modifier
          </button>
        )}
        {!readOnly && onEnrich && (
          <button onClick={onEnrich}>Compléter depuis les bases</button>
        )}
        <button onClick={onClose}>Fermer</button>
        {!readOnly && (
          <button className="danger" onClick={remove}>
            Supprimer
          </button>
        )}
      </footer>
    </div>
  );
}

function FieldRow({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <>
      <label>{label}</label>
      <div>{children}</div>
    </>
  );
}
