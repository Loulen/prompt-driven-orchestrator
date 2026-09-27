import App from "./App";
import ReviewPage from "../pages/ReviewPage";
import { reviewRunIdFromPath } from "../run/model/runRefs";

/**
 * The app has no router: navigation is state (selected run, pane owner). The
 * Review page (#749) is its first real URL — `/runs/<id>/review` — rendered
 * full-window here; everything else renders `App` untouched. A router can
 * replace this switch later without moving the URL.
 */
export function AppRoutes({ pathname }: { pathname: string }) {
  const reviewRunId = reviewRunIdFromPath(pathname);
  return reviewRunId ? <ReviewPage runId={reviewRunId} /> : <App />;
}
