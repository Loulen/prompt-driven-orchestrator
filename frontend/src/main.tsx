import { createRoot } from 'react-dom/client'
import './index.css'
import { bootstrap } from './app/bootstrap'
import { AppProviders } from './app/providers'
import { AppRoutes } from './app/routing'
import SignInGate from './app/SignInGate'

bootstrap()

createRoot(document.getElementById('root')!).render(
  <AppProviders>
    <SignInGate>
      <AppRoutes pathname={window.location.pathname} />
    </SignInGate>
  </AppProviders>,
)
