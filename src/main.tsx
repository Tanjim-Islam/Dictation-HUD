import React from 'react';
import ReactDOM from 'react-dom/client';
import './styles/globals.css';
import { App } from './app/App';
import { Hud } from './windows/Hud';

// Legacy mirror of audio flags; settings are owned by Rust now.
try {
  window.localStorage.removeItem('dictation-audio-flags');
} catch {
  // storage unavailable
}

const isHud = window.location.hash.includes('hud');
document.body.setAttribute('data-window', isHud ? 'hud' : 'main');

async function start() {
  // Browser preview (no Tauri): mock the backend so the UI can be inspected in isolation.
  if (import.meta.env.DEV && !('__TAURI_INTERNALS__' in window)) {
    const { installMockBackend } = await import('./dev/mockBackend');
    installMockBackend();
  }
  ReactDOM.createRoot(document.getElementById('root')!).render(<React.StrictMode>{isHud ? <Hud /> : <App />}</React.StrictMode>);
}

start();
