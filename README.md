# Dictation HUD

Cross-platform dictation utility built with Tauri 2.0.0-rc.3, React + Vite + TypeScript, Tailwind, and Framer Motion. Single Settings window plus a slim HUD overlay while dictating.

## Install

Prereqs: Node 18+, npm (or pnpm), Rust stable, system build tools, microphone access.

```bash
# install deps
npm i

# dev (starts Vite and Tauri dev)
npm run dev

# build desktop bundles
npm run build
```

If you prefer pnpm, swap commands accordingly.

## Environment

Create a `.env` at repo root (see `.env.example`). Keys are stored at runtime in Stronghold; `.env` is only for local smoke tests.

```env
DEEPGRAM_API_KEY=...
OPENROUTER_API_KEY=...
OPENROUTER_MODEL=openai/gpt-oss-20b:free
```

## Scripts

- `npm run dev` — Tauri dev with Vite dev server
- `npm run build` — production build/bundles
- `npm run test:providers` — OpenRouter request + Deepgram WS handshake
- `npm run lint` / `npm run format`

## Capabilities

See `src-tauri/capabilities/default.json`: tray, global shortcuts, clipboard, store, stronghold, window.

## Windows

- `settings` — normal window, hidden by default (shown from tray)
- `hud` — 600x84, transparent, frameless, always-on-top, hidden by default

## Notes

- Auto-paste may require Accessibility on macOS and is limited on some Wayland setups; the app falls back to manual paste.
- Audio is never persisted. Only text goes to OpenRouter for refinement.

## Troubleshooting

- If the global hotkey fails to register, pick a different combo in Settings.
- On first microphone use, accept the OS prompt. If denied, HUD shows a small badge.

