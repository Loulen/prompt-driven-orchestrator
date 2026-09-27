import { Plus } from "lucide-react";
import type { RunsPaneState } from "../../store/runsList/useRunsPane";

interface Props {
  pane: RunsPaneState;
  onNewRun: () => void;
}

/** The Runs tab's title bar: the « Project » configurator and « New Run ». */
export default function RunsPaneHeader({ pane, onNewRun }: Props) {
  const { availableRepos, setProjectEditor, projectOf, openProjectEditor } = pane;
  return (
    <div
      className="flex h-[32px] shrink-0 items-center border-b border-line px-3 font-medium text-fg-2"
      style={{ fontSize: "11.5px" }}
    >
      Runs
      {availableRepos.length > 0 && (
        <button
          type="button"
          aria-label="Configure project"
          onClick={() => {
            if (availableRepos.length > 1) {
              setProjectEditor({
                project: null,
                name: "Project",
                memberPaths: [],
              });
              return;
            }
            const path = availableRepos[0];
            const project = projectOf(path);
            openProjectEditor(
              project
                ? {
                    kind: "project",
                    key: `project:${project.id}`,
                    repoPath: "",
                    label: project.name,
                  }
                : {
                    kind: "path",
                    key: path,
                    repoPath: path,
                    label: path.split("/").filter(Boolean).pop() ?? path,
                  },
            );
          }}
          className="ml-auto cursor-pointer rounded border border-line-strong bg-bg-3 px-1.5 py-0.5 text-fg-3 hover:border-acc hover:text-fg"
          style={{ fontSize: "10.5px" }}
        >
          Project
        </button>
      )}
      <button
        onClick={onNewRun}
        data-testid="new-run-button"
        className={`${availableRepos.length > 0 ? "ml-1.5" : "ml-auto"} flex cursor-pointer items-center gap-1 rounded bg-acc px-1.5 py-0.5 font-medium text-on-acc transition-colors hover:bg-acc-dim`}
        style={{ fontSize: "10.5px" }}
      >
        <Plus size={10} />
        New Run
      </button>
    </div>
  );
}
