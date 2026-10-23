// Builds dictation-asr (local speech helper) and places it where Tauri expects it:
// src-tauri/binaries/dictation-asr-<target-triple>.exe (`externalBin`) plus the runtime DLLs
// listed under `bundle.resources`. Usage: node scripts/build-asr-helper.mjs [debug|release]
import { execFileSync } from 'node:child_process';
import { copyFileSync, mkdirSync } from 'node:fs';

const profile = process.argv[2] === 'release' ? 'release' : 'debug';
const triple = execFileSync('rustc', ['-vV'], { encoding: 'utf8' }).match(/^host: (\S+)$/m)[1];
const args = ['build', '-p', 'asr-helper', '-j', '4', '--manifest-path', 'src-tauri/Cargo.toml'];
if (profile === 'release') args.push('--release');
execFileSync('cargo', args, { stdio: 'inherit' });

const out = `src-tauri/target/${profile}`;
mkdirSync('src-tauri/binaries', { recursive: true });
const ext = process.platform === 'win32' ? '.exe' : '';
copyFileSync(`${out}/dictation-asr${ext}`, `src-tauri/binaries/dictation-asr-${triple}${ext}`);
if (process.platform === 'win32') {
  for (const dll of ['onnxruntime.dll', 'onnxruntime_providers_shared.dll', 'sherpa-onnx-c-api.dll', 'sherpa-onnx-cxx-api.dll']) {
    copyFileSync(`${out}/${dll}`, `src-tauri/binaries/${dll}`);
  }
}
console.log(`dictation-asr (${profile}) ready for ${triple}`);
