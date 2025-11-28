# Dictation HUD

Cross-platform AI-powered dictation utility built with Tauri 2.0.0-rc.3, React + Vite + TypeScript, Tailwind, and Framer Motion. Features real-time speech-to-text with AI refinement and automatic paste functionality.

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

Create a `.env` at repo root for local development. API keys can also be configured via the Settings UI and are securely stored using Tauri's store plugin.

```env
# Speech-to-Text providers (choose one or both)
DEEPGRAM_API_KEY=
ELEVENLABS_API_KEY=

# AI refinement providers (choose one or both)
OPENROUTER_API_KEY=
OPENROUTER_MODEL=
MEGALLM_API_KEY=

# Optional behavior
HUD_SILENCE_SECONDS=2
AUTO_PASTE=true
LOG_TO_FILE=false
```

**Provider notes:**
- **Deepgram**: Real-time WebSocket streaming with nova-2 model for accurate transcription
- **ElevenLabs**: Alternative STT provider with voice recognition capabilities
- **OpenRouter**: Access to multiple LLM providers for text refinement
- **MegaLLM**: Alternative AI provider for text post-processing

## How AI Works

The app uses a two-stage AI pipeline for high-quality dictation:

### 1. Speech-to-Text (STT)
Choose between two providers in Settings:
- **Deepgram** (default): Real-time WebSocket streaming using Web Audio API to send raw PCM audio (16kHz mono, linear16). Uses the Nova-2 model with smart formatting, punctuation, and interim results.
- **ElevenLabs**: Alternative STT provider with voice recognition.

### 2. AI Text Refinement
After transcription, the raw text is optionally refined by an LLM to:
- Fix punctuation and capitalization
- Correct speech-to-text mishearings based on context
- Remove filler words (um, uh, like) and stammering
- Preserve meaning, intent, and special characters
- **Never** respond conversationally or refuse input

The refinement uses a carefully crafted system prompt that treats the AI as a "text processing machine" rather than an assistant. This prevents the AI from:
- Treating dictated text as instructions ("tell me a joke" → "Tell me a joke.")
- Adding explanations or refusing requests
- Engaging in conversation

**Anti-refusal system**: If the AI response matches refusal patterns (e.g., "I'm sorry," "I can't," "As an AI"), the app automatically falls back to the raw transcription. This ensures your dictated text is never lost due to AI safety filters.

Choose between:
- **OpenRouter**: Access to various LLM providers (configurable model)
- **MegaLLM**: Alternative AI provider for text post-processing

### 3. Auto-Paste
When enabled, refined text is automatically pasted into the focused application using platform-native clipboard and keyboard simulation.

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

## Features

- **Multiple AI Providers**: Switch between OpenRouter and MegaLLM for text refinement
- **Multiple STT Providers**: Choose between Deepgram and ElevenLabs for speech recognition
- **Configurable Silence Detection**: Set custom silence duration (default: 2 seconds) to determine when dictation ends
- **Auto-Paste**: Automatically paste refined text into focused applications (requires Accessibility permissions on macOS)
- **Echo Cancellation & Noise Suppression**: Enhanced audio processing for clearer transcriptions
- **Stream Insert Mode**: Real-time text insertion as you speak (experimental)
- **Autostart**: Launch on system startup
- **Global Hotkey**: Customizable keyboard shortcut to start/stop dictation
- **System Tray**: Quick access to settings and controls

## Notes

- **Privacy**: Audio is never persisted or stored. Only text is sent to AI providers for refinement.
- **Auto-paste** may require Accessibility permissions on macOS and is limited on some Wayland setups; the app falls back to manual clipboard copy.
- **Platform-specific**: The HUD attempts to appear on the currently focused monitor (Windows) or primary monitor (other platforms).
- **Audio Quality**: Uses 16kHz mono linear16 PCM for optimal Deepgram compatibility and efficient bandwidth usage.

## Troubleshooting

- If the global hotkey fails to register, pick a different combo in Settings.
- On first microphone use, accept the OS prompt. If denied, HUD shows a small badge.

