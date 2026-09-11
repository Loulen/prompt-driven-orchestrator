import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import './index.css'
import App from './App.tsx'
import ReviewPage from './pages/ReviewPage.tsx'
import { reviewRunIdFromPath } from './lib/runRefs'
import { initTheme } from './hooks/useTheme'

// #759: adopt the stored theme and start following the OS. The inline guard in
// `index.html` already stamped the same value before first paint; this call
// takes ownership of it, so a later `prefers-color-scheme` flip is honoured
// while the preference is `system`.
initTheme()

// The app has no router: navigation is state (selected run, pane owner). The
// Review page (#749) is its first real URL — `/runs/<id>/review` — rendered
// full-window here; everything else renders `App` untouched. A router can
// replace this switch later without moving the URL.
const reviewRunId = reviewRunIdFromPath(window.location.pathname)

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    {reviewRunId ? <ReviewPage runId={reviewRunId} /> : <App />}
  </StrictMode>,
)
