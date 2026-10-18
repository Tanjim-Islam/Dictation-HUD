// Fails when package.json, package-lock.json, src-tauri/Cargo.toml, src-tauri/Cargo.lock, and
// src-tauri/tauri.conf.json disagree on the app version, or (with --tag vX.Y.Z or on a tag build) when the tag does not match.
import { readFileSync } from 'node:fs';

const pkg = JSON.parse(readFileSync('package.json', 'utf8')).version;
const conf = JSON.parse(readFileSync('src-tauri/tauri.conf.json', 'utf8')).version;
const cargo = readFileSync('src-tauri/Cargo.toml', 'utf8').match(/^\[package\][\s\S]*?^version\s*=\s*"([^"]+)"/m)?.[1];
const lock = JSON.parse(readFileSync('package-lock.json', 'utf8'));
const lockRoot = lock.packages?.['']?.version ?? lock.version;
const cargoLock = readFileSync('src-tauri/Cargo.lock', 'utf8').match(/\[\[package\]\]\s*name = "dictation-hud"\s*version = "([^"]+)"/)?.[1];

const argTag = process.argv.includes('--tag') ? process.argv[process.argv.indexOf('--tag') + 1] : undefined;
const refTag = process.env.GITHUB_REF_TYPE === 'tag' ? process.env.GITHUB_REF_NAME : undefined;
const tag = argTag ?? refTag;

const versions = {
  'package.json': pkg,
  'package-lock.json': lockRoot,
  'src-tauri/Cargo.toml': cargo,
  'src-tauri/Cargo.lock': cargoLock,
  'src-tauri/tauri.conf.json': conf,
};
const unique = new Set(Object.values(versions));
let ok = unique.size === 1 && !unique.has(undefined);
for (const [file, v] of Object.entries(versions)) console.log(`${file.padEnd(28)} ${v ?? '(missing)'}`);
if (tag) {
  console.log(`${'tag'.padEnd(28)} ${tag}`);
  if (tag !== `v${pkg}`) ok = false;
}
if (!ok) {
  console.error('Version drift: all versions (and the release tag) must match.');
  process.exit(1);
}
console.log('Versions match.');
