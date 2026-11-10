import { useMemo, useRef, useState } from "react";
import { exportSkills, uploadSkillsZip } from "../api/bundle";
import { saveBlob } from "../../shared/lib/download";
import type { SkillBank } from "../model/skill";
import { clickCheck } from "../model/skillExport";
import { buildRows, type TreeNodeRef } from "../model/skillTree";
import type { Pending, Toast } from "./useSkillBankPanel";

interface Args {
  bank: SkillBank;
  expanded: ReadonlySet<string>;
  filter: string;
  onChanged: () => Promise<void>;
  setMenuFor: (ref: TreeNodeRef | null) => void;
  setAddMenuOpen: (open: boolean) => void;
  setError: (error: string | null) => void;
  failWith: (cause: unknown, fallback: string) => void;
  showToast: (toast: Toast) => void;
  expandTo: (folderId: string | null) => void;
  setSelected: (ref: TreeNodeRef | null) => void;
  setPending: (pending: Pending | null) => void;
}

/**
 * The bank's skills as a zip: the tree's multiple selection (skill ids; a
 * folder checks its skills, ⇧ a range of rows), the export of a selection, a
 * skill or a folder, and the upload of such a zip.
 */
export function useSkillBundle({
  bank, expanded, filter, onChanged, setMenuFor, setAddMenuOpen, setError, failWith, showToast,
  expandTo, setSelected, setPending,
}: Args) {
  const { skills, folders } = bank;
  /** The multiple selection of the tree (skill ids), for an export. */
  const [checked, setChecked] = useState<Set<string>>(() => new Set());
  /** The last box clicked: ⇧ checks the rows between it and the next one. */
  const checkAnchor = useRef<TreeNodeRef | null>(null);

  const visibleRows = useMemo(() => buildRows(folders, skills, expanded, filter), [folders, skills, expanded, filter]);
  const checkedCount = useMemo(() => skills.filter((skill) => checked.has(skill.id)).length, [skills, checked]);

  const toggleCheck = (ref: TreeNodeRef, extend: boolean) => {
    const anchor = checkAnchor.current;
    setChecked((prev) => clickCheck(prev, ref, bank, extend ? { anchor, rows: visibleRows } : undefined));
    checkAnchor.current = ref;
  };

  const clearChecked = () => {
    setChecked(new Set());
    checkAnchor.current = null;
  };

  /** Download the zip of `body`'s skills and folders. */
  const exportBundle = async (body: { skill_ids: string[]; folder_ids: string[] }) => {
    setMenuFor(null);
    setError(null);
    try {
      const { blob, fileName, count } = await exportSkills(body);
      saveBlob(blob, fileName);
      showToast({ message: `Exported ${count} skill${count === 1 ? "" : "s"} to ${fileName}` });
    } catch (cause) {
      failWith(cause, "Export failed");
    }
  };

  /** Upload a zip of skills: the new ones land in a folder named after it. */
  const uploadZip = async (file: File) => {
    setAddMenuOpen(false);
    setError(null);
    try {
      const report = await uploadSkillsZip(file);
      await onChanged();
      if (report.folder) {
        expandTo(report.folder.id);
        setSelected({ kind: "folder", id: report.folder.id });
        setPending(null);
      }
      const created = report.created.length;
      const parts = [
        `Uploaded ${created} skill${created === 1 ? "" : "s"}${report.folder ? ` into ${report.folder.name}` : ""}`,
      ];
      if (report.kept.length > 0) parts.push(`${report.kept.length} already in the bank`);
      if (report.missing.length > 0) parts.push(`${report.missing.length} invalid`);
      showToast({ message: parts.join(" · ") });
    } catch (cause) {
      failWith(cause, "Upload failed");
    }
  };

  return { checked, checkedCount, toggleCheck, clearChecked, exportBundle, uploadZip };
}
