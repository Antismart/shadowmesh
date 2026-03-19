import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { BrowserRouter } from 'react-router-dom';
import { AuthProvider } from './context/AuthContext';
import { ToastProvider } from './context/ToastContext';
import App from './App';
import './index.css';

// When the dashboard is served under a CID path (e.g. /QmXXX/), a <base> tag
// is injected by the gateway. Use its href as the router basename so React
// Router matches routes correctly regardless of the serving path.
const base = document.querySelector('base')?.getAttribute('href');
const basename = base ? base.replace(/\/+$/, '') : '';

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <BrowserRouter basename={basename}>
      <AuthProvider>
        <ToastProvider>
          <App />
        </ToastProvider>
      </AuthProvider>
    </BrowserRouter>
  </StrictMode>,
);
