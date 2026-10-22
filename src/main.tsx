import React from 'react';
import ReactDOM from 'react-dom/client';
import './styles/globals.css';
import { App } from './app/App';
import { FlowBar } from './windows/flowbar/FlowBar';
import { applyCachedTheme } from './lib/theme';

// Legacy mirror of audio flags; settings are owned by Rust now.
try {
  window.localStorage.removeItem('dictation-audio-flags');
} catch {
  // storage unavailable
}

const isFlowBar = window.location.hash.includes('flowbar');
document.body.setAttribute('data-window', isFlowBar ? 'flowbar' : 'main');
// The Flow Bar is always dark; the main window follows the Theme setting.
if (!isFlowBar) applyCachedTheme();

async function start() {
  // Browser preview (no Tauri): mock the backend so the UI can be inspected in isolation.
  if (import.meta.env.DEV && !('__TAURI_INTERNALS__' in window)) {
    const { installMockBackend } = await import('./dev/mockBackend');
    installMockBackend();
  }
  ReactDOM.createRoot(document.getElementById('root')!).render(<React.StrictMode>{isFlowBar ? <FlowBar /> : <App />}</React.StrictMode>);
}

start();
