import { useState, useRef } from "react";
import { X } from "lucide-react";
import type { FrontmatterFieldDecl } from "../types";

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
    <div className="flex flex-col gap-1" data-testid="output-schema-editor">
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
        className="mt-0.5 cursor-pointer self-start rounded border border-line-strong bg-bg-3 px-2 py-0.5 text-fg-4 hover:text-fg-3"
        style={{ fontSize: "10px" }}
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
    <div className="flex flex-col gap-1 rounded border border-line-soft bg-bg-3 px-2 py-1">
      <div className="flex items-center gap-1.5">
        <input
          value={entry.name}
          onChange={(e) => onUpdate({ name: e.target.value })}
          className="min-w-0 flex-1 bg-transparent text-fg outline-none"
          style={{ fontSize: "11px" }}
          placeholder="field name"
          data-testid="schema-field-name"
        />
        <select
          value={entry.type}
          onChange={(e) => onUpdate({ type: e.target.value as FieldType })}
          className="cursor-pointer rounded border border-line-strong bg-bg-4 px-1 py-0.5 text-fg-3"
          style={{ fontSize: "10px" }}
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
          className="cursor-pointer text-fg-4 hover:text-st-failed"
          style={{ fontSize: "10px" }}
          data-testid="schema-field-remove"
        >
          ×
        </button>
      </div>
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
  const inputRef = useRef<HTMLInputElement>(null);

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
    <div className="flex flex-wrap items-center gap-1" data-testid="allowed-values">
      {values.map((v, i) => (
        <span
          key={i}
          className="inline-flex items-center gap-0.5 rounded bg-acc-bg px-1.5 py-0.5 text-acc"
          style={{ fontSize: "10px" }}
          data-testid="allowed-chip"
        >
          {v}
          <button
            onClick={() => removeValue(i)}
            className="cursor-pointer text-acc/60 hover:text-acc"
            data-testid="remove-allowed-chip"
          >
            <X size={10} />
          </button>
        </span>
      ))}
      <input
        ref={inputRef}
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            addValue();
          }
        }}
        onBlur={addValue}
        className="min-w-[60px] flex-1 bg-transparent text-fg outline-none"
        style={{ fontSize: "10px" }}
        placeholder="add value…"
        data-testid="allowed-input"
      />
    </div>
  );
}
