import { useCallback, useEffect, useRef, useState } from "react";
import { fetchClones } from "../api/repos";
import type { CloneJob } from "../../shared/generated/CloneJob";

/** How often the clones are read again while one runs. */
const POLL_MS = 500;

/**
 * The daemon's clones (CORE-97), read while `active` and read again while one
 * runs, so a page opened in the middle of a clone shows where it is. `track`
 * adds a clone just started; `onFinished` hears each clone that stops running
 * while it is watched (done, failed or cancelled).
 */
export function useClones(active: boolean, onFinished?: (job: CloneJob) => void) {
  const [jobs, setJobs] = useState<CloneJob[]>([]);
  const finished = useRef(onFinished);
  useEffect(() => {
    finished.current = onFinished;
  });
  const running = useRef(new Set<string>());

  const absorb = useCallback((next: CloneJob[]) => {
    for (const job of next) {
      if (job.state === "running") running.current.add(job.id);
      else if (running.current.delete(job.id)) finished.current?.(job);
    }
    setJobs(next);
  }, []);

  // A failed read keeps the last list; the next poll tries again.
  const read = useCallback(
    () =>
      fetchClones()
        .then(absorb)
        .catch(() => {}),
    [absorb],
  );

  useEffect(() => {
    if (!active) return;
    let current = true;
    fetchClones()
      .then((next) => current && absorb(next))
      .catch(() => {});
    return () => {
      current = false;
    };
  }, [active, absorb]);

  const anyRunning = jobs.some((job) => job.state === "running");
  useEffect(() => {
    if (!active || !anyRunning) return;
    const timer = setInterval(() => void read(), POLL_MS);
    return () => clearInterval(timer);
  }, [active, anyRunning, read]);

  const track = useCallback(
    (job: CloneJob) => {
      setJobs((prev) => {
        const next = [job, ...prev.filter((j) => j.id !== job.id)];
        if (job.state === "running") running.current.add(job.id);
        return next;
      });
    },
    [],
  );

  return { jobs, track, refresh: read };
}
