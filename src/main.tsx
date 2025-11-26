import React from 'react';
import ReactDOM from 'react-dom/client';
import './styles/globals.css';
import { Settings } from './windows/Settings';
import { Hud } from './windows/Hud';
import { invoke } from '@tauri-apps/api/core';

// Patch getUserMedia so audio echo/noise flags follow Settings behavior toggles.
// We read flags from localStorage key "dictation-audio-flags".
if (typeof navigator !== 'undefined' && navigator.mediaDevices?.getUserMedia) {
  const originalGetUserMedia = navigator.mediaDevices.getUserMedia.bind(navigator.mediaDevices);
  navigator.mediaDevices.getUserMedia = (constraints: MediaStreamConstraints) => {
    try {
      if (constraints && typeof constraints === 'object' && 'audio' in constraints) {
        const raw = window.localStorage.getItem('dictation-audio-flags');
        let echoCancellation = true;
        let noiseSuppression = true;
        if (raw) {
          try {
            const parsed = JSON.parse(raw);
            if (typeof parsed.echoCancellation === 'boolean') {
              echoCancellation = parsed.echoCancellation;
            }
            if (typeof parsed.noiseSuppression === 'boolean') {
              noiseSuppression = parsed.noiseSuppression;
            }
          } catch {
            // ignore parse errors, fall back to defaults
          }
        }

        const audio =
          typeof (constraints as any).audio === 'object'
            ? { ...(constraints as any).audio, echoCancellation, noiseSuppression }
            : { echoCancellation, noiseSuppression };

        constraints = { ...constraints, audio };
      }
    } catch {
      // if anything goes wrong, fall back to original constraints
    }
    return originalGetUserMedia(constraints);
  };
}

// Log startup IMMEDIATELY
console.log('🚀🚀🚀 MAIN.TSX LOADING 🚀🚀🚀');
invoke('log_to_terminal', { message: '🚀🚀🚀 MAIN.TSX LOADING 🚀🚀🚀' }).catch(() => {});

function Root() {
  const hash = typeof window !== 'undefined' ? window.location.hash : '';
  const isHud = hash.includes('hud');
  console.log('Root component rendering, hash:', hash, 'isHud:', isHud);
  invoke('log_to_terminal', { message: `Root rendering - hash: ${hash}, isHud: ${isHud}` }).catch(() => {});

  // Set data attribute on body to differentiate HUD from Settings
  React.useEffect(() => {
    document.body.setAttribute('data-window', isHud ? 'hud' : 'settings');
  }, [isHud]);

  return isHud ? <Hud /> : <Settings />;
}

console.log('Creating React root...');
invoke('log_to_terminal', { message: 'Creating React root...' }).catch(() => {});

ReactDOM.createRoot(document.getElementById('root')!).render(
  <Root />
);

console.log('✅ React root rendered');
invoke('log_to_terminal', { message: '✅ React root rendered' }).catch(() => {});
