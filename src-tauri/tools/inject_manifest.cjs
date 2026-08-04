#!/usr/bin/env node
// Cargo `runner` for Windows (see src-tauri/.cargo/config.toml).
//
// Problem: `cargo test` builds a lib-unittest executable that is NOT a Tauri
// binary, so tauri_build does not attach a Common-Controls-v6 activation
// manifest to it. Without that manifest Windows loads the legacy comctl32 v5 at
// startup, and any transitive dependency (tauri / winit / windows-rs) that
// calls TaskDialogIndirect fails with STATUS_ENTRYPOINT_NOT_FOUND
// (0xc0000139) before main() even runs -> the test EXE crashes and Windows
// shows an "entry point not found" dialog.
//
// Fix: inject the comctl32 v6 manifest into the executable right before it is
// run. The injection is idempotent and runs on every `cargo test` / `cargo
// bench` invocation, so it needs no manual `mt.exe` step and survives
// `cargo clean`. The app binary already carries its own (richer) manifest from
// tauri_build, so re-injecting here is harmless for it.
//
// This file uses the `.cjs` extension on purpose: the workspace package.json
// sets "type": "module", which would otherwise make Node treat a `.js` runner
// as ESM and reject `require`.
//
// Usage (configured in .cargo/config.toml):
//   [target.'cfg(windows)']
//   runner = ["node", "<abs path>/inject_manifest.cjs"]

const { spawnSync, execSync } = require('child_process');
const fs = require('fs');
const path = require('path');

const argv = process.argv.slice(2);
if (argv.length === 0) {
  console.error('inject_manifest: no executable provided');
  process.exit(2);
}
const exe = argv[0];
const exeArgs = argv.slice(1);

const manifest = path.join(__dirname, '..', 'comctl32_v6.manifest');
if (!fs.existsSync(manifest)) {
  console.error('inject_manifest: manifest not found at ' + manifest);
  process.exit(2);
}

// Locate mt.exe (Windows SDK Manifest Tool). Try the known SDK paths first,
// then fall back to whatever is on PATH.
function findMt() {
  const candidates = [
    'C:/Program Files (x86)/Windows Kits/10/bin/10.0.26100.0/x64/mt.exe',
    'C:/Program Files (x86)/Windows Kits/10/bin/10.0.22621.0/x64/mt.exe',
  ];
  for (const c of candidates) {
    if (fs.existsSync(c)) return c;
  }
  try {
    const out = execSync('where mt.exe', { stdio: ['ignore', 'pipe', 'ignore'] })
      .toString()
      .trim()
      .split(/\r?\n/)[0];
    if (out) return out;
  } catch (_) {
    /* ignore */
  }
  return null;
}

const mt = findMt();
if (mt) {
  const res = spawnSync(
    mt,
    ['-manifest', manifest, '-outputresource:' + exe + ';#1'],
    { stdio: 'ignore' }
  );
  if (res.status !== 0) {
    console.warn(
      'inject_manifest: mt.exe returned ' +
        res.status +
        '; continuing without manifest injection'
    );
  }
} else {
  console.warn(
    'inject_manifest: mt.exe not found; the test EXE may crash on TaskDialogIndirect'
  );
}

// Run the real executable, forwarding stdio and its exit code.
const child = spawnSync(exe, exeArgs, { stdio: 'inherit' });
process.exit(child.status === null ? 1 : child.status);
