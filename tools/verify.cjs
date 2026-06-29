#!/usr/bin/env node
// evo_sim - Copyright (c) 2026 Lens and Mix, LLC
// Licensed under the Apache License, Version 2.0. See LICENSE and NOTICE.
// More information: https://rkeithelliott.com
/*
 * verify.cjs — headless determinism / regression harness for evo_sim.
 *
 * Build the Node package first (from the sim/ directory):
 *     wasm-pack build --target nodejs --release --out-dir ../tools/pkg-node
 *
 * Then, from the tools/ directory:
 *     node verify.cjs selftest <seed|dev> <ticks> [--every N]
 *     node verify.cjs capture  <seed|dev> <ticks> [--every N]  > golden/seed137_5000.txt
 *     node verify.cjs check    <seed|dev> <ticks> <goldenfile> [--every N]
 *
 * - selftest: runs the same seed twice and asserts identical signatures.
 *             This is STEP ZERO — it proves the run is deterministic before
 *             you trust any golden master.
 * - capture:  prints "tick T: <sig>" at every checkpoint; redirect to a file.
 * - check:    recomputes and compares to a golden file; exits 0 on a full
 *             match, or 1 and prints the first diverging tick.
 *
 * "seed" is a u64 (any integer string). Use the literal word "dev" to run the
 * deterministic dev_world() preset instead of new()+regenerate(seed).
 *
 * Exit codes: 0 = OK/match, 1 = mismatch/divergence, 2 = usage/load error.
 */
'use strict';

const fs = require('fs');
const path = require('path');

let wasm;
try {
  wasm = require(path.join(__dirname, 'pkg-node', 'evo_sim.js'));
} catch (e) {
  console.error('Could not load ./pkg-node/evo_sim.js — build it first:');
  console.error('  (from sim/)  wasm-pack build --target nodejs --release --out-dir ../tools/pkg-node');
  console.error('Underlying error: ' + e.message);
  process.exit(2);
}
const { SimWorld } = wasm;

function makeWorld(seed) {
  if (String(seed) === 'dev') return SimWorld.dev_world();
  const w = new SimWorld();     // #[wasm_bindgen(constructor)] → JS `new`, not a static .new()
  w.regenerate(BigInt(seed));   // deterministic plants+herbs from seed; carns respawn after the delay
  return w;
}

// Run `ticks` updates, returning ["tick T: sig", ...] at tick 0, each checkpoint, and the final tick.
function run(seed, ticks, every) {
  const w = makeWorld(seed);
  const lines = [`tick 0: ${w.state_signature()}`];
  for (let t = 1; t <= ticks; t++) {
    w.update();
    if (t % every === 0 || t === ticks) lines.push(`tick ${t}: ${w.state_signature()}`);
  }
  return lines;
}

function getEvery(argv) {
  const i = argv.indexOf('--every');
  return i >= 0 ? parseInt(argv[i + 1], 10) : 500;
}

function main() {
  const argv  = process.argv.slice(2);
  const cmd   = argv[0];
  const seed  = argv[1];
  const ticks = parseInt(argv[2], 10);
  const every = getEvery(argv);

  if (!cmd || seed === undefined || !Number.isFinite(ticks)) {
    console.error('usage: node verify.cjs <selftest|capture|check> <seed|dev> <ticks> [goldenfile] [--every N]');
    process.exit(2);
  }

  if (cmd === 'capture') {
    process.stdout.write(run(seed, ticks, every).join('\n') + '\n');
    return;
  }

  if (cmd === 'selftest') {
    const a = run(seed, ticks, every);
    const b = run(seed, ticks, every);
    for (let i = 0; i < a.length; i++) {
      if (a[i] !== b[i]) {
        console.error(`NON-DETERMINISTIC at ${a[i].split(':')[0]}`);
        console.error(`  run A: ${a[i]}`);
        console.error(`  run B: ${b[i]}`);
        process.exit(1);
      }
    }
    console.log(`determinism OK — ${a.length} checkpoints identical across two runs (seed ${seed}, ${ticks} ticks, every ${every})`);
    return;
  }

  if (cmd === 'check') {
    const goldenFile = argv[3];
    if (!goldenFile || goldenFile.startsWith('--')) {
      console.error('check needs a golden file: node verify.cjs check <seed|dev> <ticks> <goldenfile> [--every N]');
      process.exit(2);
    }
    // Normalize CRLF/CR -> LF so a golden written via PowerShell '>' (Windows
    // line endings) compares cleanly against the in-memory LF lines.
    const golden = fs.readFileSync(goldenFile, 'utf8').replace(/\r\n?/g, '\n').trimEnd().split('\n');
    const now = run(seed, ticks, every);
    if (now.length !== golden.length) {
      console.error(`length mismatch: golden has ${golden.length} checkpoints, this run ${now.length} — was it captured with the same ticks/--every?`);
      process.exit(1);
    }
    for (let i = 0; i < now.length; i++) {
      if (now[i] !== golden[i]) {
        console.error(`DIVERGENCE at ${now[i].split(':')[0]}`);
        console.error(`  golden: ${golden[i]}`);
        console.error(`  now:    ${now[i]}`);
        process.exit(1);
      }
    }
    console.log(`MATCH — ${now.length} checkpoints identical to golden (seed ${seed}, ${ticks} ticks, every ${every})`);
    return;
  }

  console.error(`unknown command: ${cmd}`);
  process.exit(2);
}

main();
