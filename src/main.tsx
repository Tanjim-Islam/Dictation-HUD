import React from 'react';
import ReactDOM from 'react-dom/client';
import './styles/globals.css';
import { Settings } from './windows/Settings';
import { Hud } from './windows/Hud';

// Legacy mirror of audio flags; settings are owned by Rust now.
try {
  window.localStorage.removeItem('dictation-audio-flags');
} catch {
  // storage unavailable
}

function Root() {
  const isHud = window.location.hash.includes('hud');

  React.useEffect(() => {
    document.body.setAttribute('data-window', isHud ? 'hud' : 'settings');
  }, [isHud]);

  return isHud ? <Hud /> : <Settings />;
}

ReactDOM.createRoot(document.getElementById('root')!).render(<Root />);
