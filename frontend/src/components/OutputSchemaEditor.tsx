import { useState } from "react";
import { X } from "lucide-react";
import type { FrontmatterFieldDecl } from "../shared/generated/FrontmatterFieldDecl";

const FIELD_TYPES = ["enum", "int", "string", "bool", "list"] as const;
type FieldType = (typeof FIELD_TYPES)[number];

interface FieldEntry {
  name: string;
  type: FieldType;
  allowed?: string[];
}

function schemaToEntries(
  schema: Record<string, FrontmatterFieldDecl> | null | undefined,
): FieldEntry[] {
  if (!schema) return [];
  return Object.entries(schema).map(([name, decl]) => ({
    name,
    type: (FIELD_TYPES.includes(decl.type as FieldType) ? decl.type : "string") as FieldType,
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
      ...(e.type === "enum" && e.allowed ? { allowed: e.allowed } : {}),
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
    <div className="flex flex-col gap-2" data-testid="output-schema-editor">
      {entries.map((entry, i) => (
        <SchemaFieldRow
          key={i}
          entry={entry}
          onUpdate={(patch) => updateField(i, patch)}
          onRemove={() => removeField(i)}
        />
      ))}
      <button
        onClick={addField}
        className="fld-add-field"
        data-testid="add-schema-field"
      >
        + field
      </button>
    </div>
  );
}

function SchemaFieldRow({
  entry,
  onUpdate,
  onRemove,
}: {
  entry: FieldEntry;
  onUpdate: (patch: Partial<FieldEntry>) => void;
  onRemove: () => void;
}) {
  return (
    <div className="fld">
      <input
        value={entry.name}
        onChange={(e) => onUpdate({ name: e.target.value })}
        className="fld-name"
        placeholder="field name"
        data-testid="schema-field-name"
      />
      <select
        value={entry.type}
        onChange={(e) => onUpdate({ type: e.target.value as FieldType })}
        className="fld-type"
        data-testid="schema-field-type"
      >
        {FIELD_TYPES.map((t) => (
          <option key={t} value={t}>
            {t}
          </option>
        ))}
      </select>
      <button
        onClick={onRemove}
        className="fld-del"
        data-testid="schema-field-remove"
        aria-label="Delete field"
      >
        <X size={12} />
      </button>
      {entry.type === "enum" && (
        <AllowedChipList
          values={entry.allowed ?? []}
          onChange={(allowed) => onUpdate({ allowed })}
        />
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
    <div className="fld-enum" data-testid="allowed-values">
      <span className="fld-enum-h">allowed</span>
      {values.length > 0 && (
        <div className="fld-chips">
          {values.map((v, i) => (
            <span key={i} className="fld-chip" data-testid="allowed-chip">
              {v}
              <button
                onClick={() => removeValue(i)}
                className="fld-chip-del"
                data-testid="remove-allowed-chip"
                aria-label={`Remove ${v}`}
              >
                <X size={10} />
              </button>
            </span>
          ))}
        </div>
      )}
      <div className="fld-add-row">
        <input
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              addValue();
            }
          }}
          placeholder="add value…"
          data-testid="allowed-input"
        />
        <button
          className="fld-add-btn"
          onClick={addValue}
          data-testid="add-allowed-btn"
        >
          Add
        </button>
      </div>
    </div>
  );
}
