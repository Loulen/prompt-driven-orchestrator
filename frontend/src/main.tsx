import { createRoot } from 'react-dom/client'
import './index.css'
import { bootstrap } from './app/bootstrap'
import { AppProviders } from './app/providers'
import { AppRoutes } from './app/routing'

bootstrap()

createRoot(document.getElementById('root')!).render(
  <AppProviders>
    <AppRoutes pathname={window.location.pathname} />
  </AppProviders>,
)
