//! `GET /runs/{run_id}/diff`: the Run's raw patch, fork point → Run tip.

use pdo_run::domain::run_refs;

use super::failure::{Failure, Outcome};
use super::review::Review;

impl Review {
    /// The raw `git diff` of the Run: its errors are plain text, as they always
    /// were.
    pub async fn run_diff(&self, run_id: &str) -> Outcome<String> {
        let run = match self.runs.load(run_id).await {
            Ok(Some(run)) => run,
            Ok(None) => return Err(Failure::Text(404, "run not found".into())),
            Err(e) => return Err(Failure::Text(500, format!("error: {e}"))),
        };
        // Base on the Run's frozen fork point, NOT the shared checkout's
        // wandering HEAD: a checkout parked on a divergent branch would sweep
        // in phantom files. Three-dot (merge-base = fork point) so main's
        // advance after the fork does not surface as phantom deletions;
        // `:(exclude).pdo/` drops the blackboard artefacts. Mirrors
        // `compute_run_loc`.
        let range = format!(
            "{}...{}",
            run_refs::fork_base(&run.state),
            run_refs::tip_branch(run_id)
        );
        // #748: the Run's branch lives in its EFFECTIVE repository (ADR-0033),
        // never in the daemon's cwd.
        let output = self
            .git
            .raw_diff(&run.repo, &range)
            .map_err(|e| Failure::Text(500, format!("git diff failed: {e}")))?;
        if !output.success {
            if output.stderr.contains("unknown revision")
                || output.stderr.contains("not a git repository")
            {
                return Err(Failure::Text(404, "run branch not found".into()));
            }
            return Err(Failure::Text(
                500,
                format!("git diff failed: {}", output.stderr),
            ));
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}
