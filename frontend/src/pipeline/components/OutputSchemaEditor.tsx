import { useEffect, useRef, useState } from "react";
import { Plus, X } from "lucide-react";
import { IconButton, TextInput } from "../../shared/ui/kit";
import type { FrontmatterFieldDecl } from "../../shared/generated/FrontmatterFieldDecl";

const FIELD_TYPES = ["enum", "int", "string", "bool", "list"] as const;
type FieldType = (typeof FIELD_TYPES)[number];

interface FieldEntry {
  name: string;
  /** One of {@link FIELD_TYPES}, or a type nobody knows as the YAML spells it:
   *  kept as written, so an edit elsewhere never rewrites it (the lint names it). */
  type: string;
  allowed?: string[];
}

const known = (type: string): type is FieldType => (FIELD_TYPES as readonly string[]).includes(type);

function schemaToEntries(
  schema: Record<string, FrontmatterFieldDecl> | null | undefined,
): FieldEntry[] {
  if (!schema) return [];
  return Object.entries(schema).map(([name, decl]) => ({
    name,
    type: decl.type,
    allowed: decl.allowed ?? undefined,
  }));
}

function entriesToSchema(
  entries: FieldEntry[],
): Record<string, FrontmatterFieldDecl> | undefined {
  if (entries.length === 0) return undefined;
  const schema: Record<string, FrontmatterFieldDecl> = {};
  for (const e of entries) {
    schema[e.name] = {
      type: e.type,
      // An unknown type keeps its list too: a `type: Enum` typo must not lose
      // its values on an edit elsewhere. Picking a known type drops it (updateField).
      ...((e.type === "enum" || !known(e.type)) && e.allowed ? { allowed: e.allowed } : {}),
    };
  }
  return schema;
}

interface Props {
  schema: Record<string, FrontmatterFieldDecl> | null | undefined;
  onChange: (schema: Record<string, FrontmatterFieldDecl> | undefined) => void;
}

export default function OutputSchemaEditor({ schema, onChange }: Props) {
  const entries = schemaToEntries(schema);
  // The placeholder name of the field just added, selected once its row shows,
  // so typing the real name replaces it (`fieldverdict`, FP of CORE-227).
  const [justAdded, setJustAdded] = useState<string | null>(null);

  function update(newEntries: FieldEntry[]) {
    onChange(entriesToSchema(newEntries));
  }

  function addField() {
    let name = "field";
    let counter = 1;
    while (entries.some((e) => e.name === name)) {
      name = `field_${++counter}`;
    }
    update([...entries, { name, type: "string" }]);
    setJustAdded(name);
  }

  function removeField(index: number) {
    update(entries.filter((_, i) => i !== index));
  }

  function updateField(index: number, patch: Partial<FieldEntry>) {
    update(
      entries.map((e, i) => {
        if (i !== index) return e;
        const updated = { ...e, ...patch };
        if (patch.type && patch.type !== "enum") {
          delete updated.allowed;
        }
        return updated;
      }),
    );
  }

  return (
    <div className="ei-schema" data-testid="output-schema-editor">
      {entries.map((entry, i) => (
        <SchemaFieldRow
          key={i}
          entry={entry}
          onUpdate={(patch) => updateField(i, patch)}
          onRemove={() => removeField(i)}
          selectName={entry.name === justAdded}
          onNameSelected={() => setJustAdded(null)}
        />
      ))}
      <button type="button" onClick={addField} className="ei-addline" title="Add a frontmatter field" data-testid="add-schema-field">
        <Plus size={12} strokeWidth={1.5} aria-hidden />
        field
      </button>
    </div>
  );
}

function SchemaFieldRow({
  entry,
  onUpdate,
  onRemove,
  selectName,
  onNameSelected,
}: {
  entry: FieldEntry;
  onUpdate: (patch: Partial<FieldEntry>) => void;
  onRemove: () => void;
  /** Focus the name and select it, once. */
  selectName: boolean;
  onNameSelected: () => void;
}) {
  const nameRef = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (!selectName || !nameRef.current) return;
    nameRef.current.focus();
    nameRef.current.select();
    onNameSelected();
  }, [selectName, onNameSelected]);
  return (
    <div className="ei-field" data-testid="schema-field">
      <TextInput
        ref={nameRef}
        mono
        small
        value={entry.name}
        onChange={(e) => onUpdate({ name: e.target.value })}
        placeholder="field name"
        aria-label="Field name"
        title={entry.name}
        data-testid="schema-field-name"
      />
      <select
        value={entry.type}
        onChange={(e) => onUpdate({ type: e.target.value })}
        className="ei-select ei-select--sm ei-select--mono"
        aria-label="Field type"
        aria-invalid={known(entry.type) ? undefined : true}
        title={known(entry.type) ? undefined : `Unknown type: ${entry.type}`}
        data-testid="schema-field-type"
      >
        {!known(entry.type) && <option value={entry.type}>{entry.type} (unknown)</option>}
        {FIELD_TYPES.map((t) => (
          <option key={t} value={t}>
            {t}
          </option>
        ))}
      </select>
      <IconButton icon={X} size="sm" label="Delete field" onClick={onRemove} data-testid="schema-field-remove" />
      {entry.type === "enum" && (
        <AllowedChipList values={entry.allowed ?? []} onChange={(allowed) => onUpdate({ allowed })} />
      )}
    </div>
  );
}

function AllowedChipList({
  values,
  onChange,
}: {
  values: string[];
  onChange: (values: string[]) => void;
}) {
  const [draft, setDraft] = useState("");

  function addValue() {
    const trimmed = draft.trim();
    if (trimmed && !values.includes(trimmed)) {
      onChange([...values, trimmed]);
      setDraft("");
    }
  }

  function removeValue(index: number) {
    onChange(values.filter((_, i) => i !== index));
  }

  return (
    <div className="ei-field__enum" data-testid="allowed-values" title="Allowed values">
      {values.map((v, i) => (
        <span key={i} className="d-chip d-chip--mono" data-testid="allowed-chip">
          <span className="d-chip__t">{v}</span>
          <button
            type="button"
            onClick={() => removeValue(i)}
            className="d-chip__x"
            data-testid="remove-allowed-chip"
            aria-label={`Remove ${v}`}
            title={`Remove ${v}`}
          >
            <X size={10} strokeWidth={2} aria-hidden />
          </button>
        </span>
      ))}
      <TextInput
        mono
        small
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            addValue();
          }
        }}
        placeholder="value"
        aria-label="Allowed value"
        data-testid="allowed-input"
      />
      <IconButton icon={Plus} size="sm" label="Add value" onClick={addValue} data-testid="add-allowed-btn" />
    </div>
  );
}
