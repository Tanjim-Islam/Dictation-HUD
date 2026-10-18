# Wispr Flow clone - Dictation HUD

This is an open source Wispr Flow clone. Press a shortcut in any app, speak, press it again, and your words are typed where you were working, cleaned up and formatted.

Dictation HUD is a Windows desktop app built with Tauri 2 (Rust) and React.

## Features

- **Dictate anywhere.** One global shortcut (default `Ctrl+Shift+Alt+H`) starts and finishes dictation in any app. A small Flow Bar shows a live waveform and lets you cancel or finish with a click. It never takes focus away from the app you are typing in.
- **Safe delivery.** Text is pasted only into the window you started in. If you switched apps, switched tabs, or the window closed, the text is copied to the clipboard instead and the app tells you why.
- **Speech to text.** Deepgram (Nova-3, 70+ languages including Bengali) or ElevenLabs (Scribe). ElevenLabs supports a pool of API keys: credits add up and the app switches keys when one runs low. Paste a whole list of keys at once.
- **AI refinement (optional).** Fixes punctuation, grammar, and filler words with an OpenRouter or MegaLLM model. If the model is slow or fails, the formatted transcript is used instead, so nothing is lost.
- **History.** Every dictation is saved locally with its recording. Play it back, copy it, retry the transcript, retry refinement, recover interrupted dictations, export WAV, or delete. Recordings are kept 14 days by default; transcripts are kept.
- **Dictionary and snippets.** Teach it names and terms, fix words it keeps mishearing, and say a short trigger to insert saved text exactly as written. It can suggest dictionary entries when you correct a word right after dictating.
- **Coding awareness.** In code editors and terminals, say "camel case get user name" for `getUserName`, plus snake, pascal, kebab, and constant case. Terminal commands keep a lowercase first word and no trailing period.
- **Context awareness (optional).** Gives AI refinement the text just before your cursor so names and tone match. Never saved, and never read from password or PIN fields.
- **Insights.** Words per minute, total words, time saved, apps you dictate in, and your streak, all calculated on your computer.
- **Native audio.** Microphone capture, echo cancellation, and noise suppression run in Rust, so dictation starts instantly.
- **Tray and updates.** Tray menu for the microphone, language, and pasting your last transcript. Signed automatic updates from GitHub releases.

## Privacy

- API keys are stored in Windows Credential Manager. They never reach the app's web views and are never written to logs.
- Transcripts, recordings, and your dictionary stay on your computer (SQLite and Opus files in your app data folder).
- Audio goes only to the speech provider you choose. Text goes to an AI provider only when AI refinement is on.

## Install

Download the latest installer from [Releases](https://github.com/Tanjim-Islam/Dictation-HUD/releases). Then open Settings, add a Deepgram or ElevenLabs API key, and press the shortcut in any app.

## Development

Prerequisites: Windows 10 or 11, Node 20+, Rust stable, CMake and Ninja (for the bundled Opus encoder), and the WebView2 runtime.

```powershell
npm ci
npm run dev              # app with hot reload
npm run dev:frontend     # UI only, in a browser, with a mocked backend
```

Checks (the same ones CI runs):

```powershell
npm run check:versions
npm run typecheck
npm run lint
npm test
npm run build:frontend
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
npx tauri build --no-bundle
```

Development builds can read API keys from a `.env` file at the repo root (`DEEPGRAM_API_KEY`, `ELEVENLABS_API_KEY`, `OPENROUTER_API_KEY`, `MEGALLM_API_KEY`). Release builds never read `.env`; add keys in Settings.

Signed installers are built by the release workflow when a `v<version>` tag is pushed. The version in `package.json`, `package-lock.json`, `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`, and `src-tauri/tauri.conf.json` must match the tag.

## Project layout

```text
src/            React UI: History, Insights, Dictionary, Snippets, Settings, Flow Bar
src-tauri/src/  Rust: session state machine, audio engine, providers, delivery,
                text pipeline, history database, tray, updates
```
