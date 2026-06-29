# Evolution Simulator — Project Skill File

This file provides critical context for any Claude session working on this
project. Read it before writing any code or making any suggestions.

---

## Project identity

- **Project:** Browser-based evolution simulator in Rust/WASM + TypeScript/Canvas
- **Owner:** Keith (RKeithElliott), independent musician and former Allstate Technology Portfolio Manager
- **Location:** `C:\Users\rkell\code\Claude\evoSim\evo_sim\`
- **Artist/label:** Lens and Mix, LLC
- **Contact:** contact@RKeithElliott.com

---

## Working relationship rules

These are non-negotiable. Violating them is the primary source of wasted time.

1. **Never generate code without Keith's explicit "yes" or "go ahead."** State
   your plan and reasoning first. Wait for approval.
2. **Never say "the problem is clear" or "now I understand" unless you can
   prove it from evidence.** False confidence is worse than admitted uncertainty.
3. **Diagnostic first.** Before changing anything, gather evidence. If you
   cannot predict exactly what a change will produce, say so.
4. **One problem at a time.** Fix one thing, confirm it works, then proceed.
5. **No magic numbers in code.** All tunable values belong in `config.rs`.
6. **State predictions before changes.** "I predict this will X because Y."
   If the prediction is wrong, stop and diagnose before trying again.
7. **Keith reads and reflects before the next step.** Do not chain multiple
   changes or suggestions without giving him space to respond.
8. **You maintain the canonical file tree.** At the start of a thread Keith
   uploads a full source snapshot; hold it and edit from it. Your latest
   output is the source of truth between you.
9. **Deliver changes as delta zips.** Return only the files you changed or
   added, in their correct `evo_sim/` relative paths (Keith copies the zip
   contents over his dev folder). Zip the full tree only when he asks. State
   whether each change is reload-only or needs a rebuild (see Build and deploy
   workflow below).
10. **Guard against drift.** If Keith says he edited a file locally, ask him to
   re-upload it before you modify that file, so your copy can't silently
   overwrite his change.

---

## Build and deploy workflow

```
# Build (from sim\ directory):
wasm-pack build --target web --release

# Build WITH the Tracer compiled in (diagnostic builds only):
wasm-pack build --target web --release -- --features trace

# Deploy (from evo_sim\ root):
xcopy sim\pkg web\pkg /E /I /Y

# Serve:
cd web && python server.py
```

- `server.py` must be running for SharedArrayBuffer support (COOP/COEP headers)
- After any HTML/JS change: hard reload (`Ctrl+Shift+R`) in browser
- After any Rust change: full rebuild required, then xcopy, then hard reload
- `config.rs` changes require a full rebuild
- `index.html` and `worker.js` changes do NOT require a rebuild

---

## Architecture overview

```
evo_sim/
├── DESIGN.md          — authoritative project backlog, phase status, housekeeping
├── SKILL.md           — this file
└── sim/src/
    ├── lib.rs         — SimWorld, update loop, all orchestration
    ├── genome_def.rs  — GenomeDef, Genome, crossover (data-driven, Phase 5)
    ├── genome.rs      — HerbGenome, CarnGenome
    ├── plant.rs       — Plant with terrain-aware survival, sexual reproduction
    ├── herbivore.rs   — Herbivore agent with FOV, eating, fleeing
    ├── carnivore.rs   — Carnivore agent with stalking, draining, home range
    ├── spatial.rs     — SpatialGrid O(n) neighbor lookup
    ├── config.rs      — ALL parameters — no magic numbers elsewhere
    ├── terrain.rs     — Heightmap, water, nutrients, hillshade
    ├── occupancy.rs   — Max plants per cell
    └── rng.rs         — Deterministic xorshift RNG
└── web/
    ├── index.html     — Renderer, UI, inspector panel, genome overlay
    ├── worker.js      — Web Worker: WASM sim, structured clone frame sends
    └── server.py      — COOP/COEP headers for SharedArrayBuffer
```

---

## Critical Rust/WASM gotchas

**wasm_bindgen field visibility**
- All `pub` fields on `#[wasm_bindgen]` structs must implement `IntoWasmAbi`,
  `FromWasmAbi`, and `Copy`
- `Vec<T>`, `String`, `&'static str` do NOT implement these traits
- Fix: make the field private (no `pub`). JavaScript never needs direct struct
  field access — expose data via methods instead
- Example: `plant_def: genome_def::GenomeDef` must be private

**herb array index invalidation**
- `self.herbs.retain(|h| h.alive)` reshuffles all indices
- Any carnivore storing `target_herb_idx = Some(23)` now points at a different
  herb after retain runs
- Fix: clear all carnivore target indices before every `herbs.retain()` call
- This was the primary cause of carnivores failing to make kills

**Rust borrow checker with simulation arrays**
- Cannot pass `&self.plants` to a method that also mutates `self`
- Pattern used: `unsafe { std::slice::from_raw_parts(self.plants.as_ptr(), n) }`
  for read-only pollen search inside plant update loop
- This is safe because we never mutate the slice we're reading from in the
  same iteration

**Budget-constrained genome system**
- `GenomeDef.budget = 4.0`, 8 traits, average value 0.5 per trait
- After crossover/mutation, `rebalance()` scales all values to sum to budget
- Trait values range roughly 0.02 (min) to ~1.5 (high specialization)
- Normalize for display: `(value - 0.02) / 1.48` → 0.0 to 1.0

**Constructor field ordering**
- All three SimWorld constructors (`new()`, `dev_world()`, `with_seed()`) must
  include every field
- The `with_seed()` constructor has slightly different structure from `new()`
  — Python string matching often misses it
- After adding any new SimWorld field, check all three constructors explicitly

**Struct fields and wasm_bindgen**
- `#[wasm_bindgen]` on `impl SimWorld` exposes methods, not fields
- Use getter methods to expose data to JavaScript
- Pattern: `pub fn n_carns_active(&self) -> u32 { self.n_carns_active }`

---

## Critical JavaScript gotchas

**Terrain cache**
- `buildTerrainCache()` renders terrain to an offscreen canvas once
- It only rebuilds when `terrainDirty = true`
- Setting `terrainDirty = true` in JS does NOT immediately rebuild —
  it rebuilds on the next frame received from the worker
- The cache is pixel data from `elevToRgb()` — changing that function
  changes the cache on next rebuild
- Controls that set terrainDirty (hillshade, water level, nutrient overlay)
  correctly trigger a rebuild; verify any new terrain-affecting code does too

**Canvas alpha compositing at high density**
- At 9,000+ plants with significant overlap, repeated `globalAlpha` draws
  accumulate toward white — this is a browser canvas artifact, not a bug
- Observed at gen 14,971 overnight stress test
- Mitigation: raise minimum plant lightness so they read correctly when composited

**Worker thread / main thread relationship**
- Rust/WASM runs in a Web Worker (separate JS thread)
- Communication is one-way: worker → main thread via `postMessage()`
- Structured clone objects accumulate over long sessions — GC pauses cause
  UI freezes after many hours at 32× speed
- The worker runs as fast as `setTimeout(loop, 0)` allows — it does NOT
  wait for the renderer

**Plant buffer stride = 13**
- `[x, y, stage, visual_radius, consumed_fraction, t0, t1, t2, t3, t4, t5, t6, t7]`
- Trait values at indices `base + 5` through `base + 12`
- Herb buffer stride = 7, Carn buffer stride = 6

**Organism enable/disable flags**
- `herbsEnabled` and `carnsEnabled` are JS variables controlling both
  rendering AND simulation update (via `set_herbs_enabled` / `set_carns_enabled`)
- Disabling freezes the population in place — does not clear it
- HUD shows `—` when disabled; chart continues showing last known values
- On a world (re)load these are re-pushed to the worker by
  `applyConfigSnapshot()` so user intent persists (a freshly built world has
  herbs/carns ON by default — see below)

**World-load UI sync (`config_snapshot` / `applyConfigSnapshot`)**
- On `ready` and `reset_done` the worker sends `config: sim.config_snapshot()`
  (JSON); the client calls `applyConfigSnapshot()`
- Environment settings flow world → UI (sliders reflect the loaded world); the
  herb/carn enable toggles flow UI → world (intent wins, see above)
- HUD seed + `currentSeed` (used by Restart) are read from this snapshot

**Worker WASM cache — silent black canvas (June 8 2026)**
- A module worker (`new Worker(..., {type:'module'})`) fetches its own
  `pkg/evo_sim.js` + `.wasm`. A page hard-reload (Ctrl+Shift+R) does NOT
  reliably bust those — you can run NEW `worker.js` against OLD cached WASM
- Symptom: after a rebuild, page chrome loads but the canvas stays black and
  unresponsive. (Worker `start()` errors used to be swallowed; now surfaced via
  the `case 'error'` handler → status bar + console)
- Mitigation in place: `server.py` sends `Cache-Control: no-store`. If it
  recurs anyway: DevTools → Network → "Disable cache" → reload, and confirm
  `evo_sim_bg.wasm` loads 200 with a real transfer size (not "memory cache")

---

## Critical ReportLab gotchas (for PDF generation)

- **Y=0 is at the BOTTOM of the page**, not the top
- Letter page = 612 × 792 points
- Safe margins: 36pt each side. Usable area: 540 × 720pt
- All element positions must be calculated bottom-up
- Calculate the full layout on paper BEFORE writing any drawing code:
  - Find the bottom of the lowest element first
  - Work upward to verify nothing overlaps and nothing goes off-page
- `roundRect(x, y, width, height, radius)` — x,y is BOTTOM-LEFT corner
- Text baseline is at the y coordinate — glyphs extend UPWARD from y
- Never use Unicode subscript/superscript characters — use `<sub>` tags
- Draw background fills BEFORE drawing boxes; draw boxes BEFORE drawing text
- Structural containers must be drawn before the elements inside them

---

## Simulation science notes

**Why geographic trait clustering requires short seed dispersal**
- At seed_spread = 105wu avg, genes cross the 4000wu island in ~38 generations
- After 480 generations with long dispersal the population was genetically uniform
- Current setting: `5 + seed_spread_trait * 55` → avg 32.5wu
- At 32.5wu, genes cross the island in ~123 generations — local adaptation visible

**Selection pressure values (config.rs)**
- Flood-intolerant plant (ft=0.02) on coast: ~4.3s expected lifespan
- Drought-intolerant plant (dt=0.02) on dry cell: ~8.5s expected lifespan
- These are strong enough to produce geographic clustering within 100-130 generations
- Confirmed: flood tolerance and drought tolerance show geographic distribution by gen 130

**Carnivore population dynamics**
- Key bug fixed: `herbs.retain()` reshuffled indices, carns chased wrong herbs
- Carnivores reproduce only after a successful kill brings energy above repro threshold
- Repro threshold (0.745) must be BELOW satiation threshold (0.794) or
  carnivores never have energy for reproduction
- Demographic fragility: populations of 3-5 cannot buffer synchronized deaths
- Initial spawn delayed to frame 3000 so herb population establishes first
- Spawn location: within 200wu of existing herbs (not random world position)

**Current phase status**
- Phase 4b/4c: Carnivores functional but population collapses after ~15,000 gens
- Phase 5: Plant sexual reproduction working, geographic trait clustering confirmed
- Herbs and carnivores disabled by default for plant evolution study
- See DESIGN.md for full phase status and feature backlog

---

## Known open issues (do not re-investigate without reading this first)

| Issue | Status | Notes |
|-------|--------|-------|
| Plant contrast vs terrain | Open | Plants too dark at low growth_rate; terrain is dark olive not green |
| Canopy shade map | Disabled | O(n²) at 10k plants; needs plant spatial grid first |
| Herb/carn genome inspector bars | Placeholder | Plant inspector working; animals show state label only |
| Carnivore pop collapse long-term | Known | Demographic fragility; sexual reproduction (Phase 5) may help |
| UI freeze after overnight 32× run | Known | GC pressure from structured clone accumulation |
| White plant wash at high density | Known | Canvas alpha compositing artifact; not a sim bug |

---

## Critical fix — do not remove the being_eaten HashSet (lib.rs lines 668-682)

Each tick, lib.rs builds a HashSet of plant indices currently being eaten
by a living herb, then clears being_eaten on any plant not in that set.
Without this, plants stay locked permanently after an herb dies mid-meal —
they become invisible to other herbs forever.
This was a hard-won bug fix. The comment in lib.rs explains it clearly.
Do not remove or simplify this block without fully understanding the consequence.

## Rust type notes

**usize** is a primitive type built into Rust — not defined anywhere in our
code. It is the pointer-sized unsigned integer, used for all array indices
and collection sizes. The compiler requires usize for indexing — using u32
or i32 requires an explicit cast.

Important: WASM runs in a 32-bit address space in most browsers, so usize
is 32 bits in our WASM build even on a 64-bit machine. Arrays are limited
to 2^32 elements — far more than we need, but worth knowing.

## Simulation philosophy (read before suggesting features)

- **Timescale:** Evolutionary, not real-time. Day/night, seasons, weather are
  abstracted into terrain and config — not simulated explicitly.
- **Individual behavior is symbolic:** Carnivore hunts and herb deaths are
  statistical tokens, not accurate representations of real predation. The
  visual engagement is intentional but should not be mistaken for biological
  accuracy at the individual level.
- **World aspect ratio is fixed at 2:1.** Do not suggest or implement other
  aspect ratios. `apply_island_mask` and spatial calculations depend on this.
- **Light source is fixed northwest** — standard cartographic convention.
  No day/night cycle planned.
- **Hillshade slope-direction validation pending** — know this before
  diagnosing any hillshade visual issues.

## Debug builds and conditional compilation

Two build modes:
- `wasm-pack build --target web --release` — what we always use (optimized, no debug symbols)
- `cargo build` (no --release) — debug build with integer overflow checking and debug_assertions enabled

Rust's `#[cfg(debug_assertions)]` attribute marks code that compiles only in
debug builds — zero cost in release. Prefer this over runtime flags for
diagnostics.

The Tracer (June 9 2026, in the `utils` crate) is the realized version of this.
The `trace` cargo feature is a master COMPILE-TIME gate — build with
`wasm-pack build --target web --release -- --features trace`. The `trace_scope!` /
`trace_event!` macros expand to nothing without the feature, so a normal release
build carries zero tracer code (LTO strips it). Within a traced build, the master
on/off and per-subsystem switches (World/Plants/Herbs/Carns, an AtomicU32 bitmask)
are RUNTIME — toggle from the UI (Trace checkbox + live panel) without rebuilding.
Output bridges to `export_trace`. World + Plants are instrumented; herbs/carns
still to come.

The legacy `trace_enabled` / `trace_buf` system still coexists in the default
(non-feature) build and is slated for retirement once the new tracer covers the
subsystems it does.

Periodically running a debug build (not for WASM deployment, just `cargo build`)
is useful for catching integer overflow bugs that release builds silently ignore.

## Product vision

The intended end product is a browser-hosted scientific tool with three
integrated components:

1. **Terrain editor** — visual world sculpting tool
2. **Organism editor** — form-based UI to define new organism types using the
   GenomeDef system, without modifying Rust code
3. **Scenario system** — deposit organisms into worlds, save/share as JSON

The GenomeDef data-driven genome system (genome_def.rs) is the foundation for
the organism editor. Config parameters governing organism behavior are not yet
experimenter-configurable — this is a known architectural gap.

The hardest open design problem: connecting experimenter-defined genome traits
to emergent behavior without requiring Rust code changes. See DESIGN.md for
full discussion.

## Future architecture: private Cargo workspace

The Cargo workspace + `utils` crate now exist (June 9 2026, dev-seq #3); the
Tracer lives there. Keith prefers proactive generalization over waiting for a
second use case. Remaining candidates to extract into `utils`: `rng.rs` and
`spatial.rs`. See DESIGN.md for structure. Do not undertake the rng/spatial
migration during active feature work.

---

## Production hosting notes

The Python server is for local development only. In production:

- Host all files in `web/` and `web/pkg/` as static files on any web server
- The COOP/COEP headers that `server.py` provides must be set at the web
  server level instead. For Apache/.htaccess (e.g. RKeithElliott.com):

```
Header set Cross-Origin-Opener-Policy "same-origin"
Header set Cross-Origin-Embedder-Policy "require-corp"
```

- No server-side processing, no database, no installation required for visitors
- Everything runs client-side in the browser

**WASM binary size**
- The `.wasm` file is typically 1-3MB uncompressed after wasm-opt optimization
- wasm-pack generates compressed versions (`.wasm.gz` and `.wasm.br`) alongside
  the main binary — ensure the web server serves these with correct
  `Content-Encoding` headers
- Brotli (`.br`) typically compresses to 300-500KB — acceptable for a modern
  web application
- Verify the server has `mod_brotli` or `mod_deflate` enabled

---

## Seed 137 world characteristics

- World: 4000 × 2000 world units, 256 × 256 terrain cells
- Main island with distinct coast, valleys, and highlands
- Plant carrying capacity: ~9,063–9,265 (stable oscillation)
- Herb equilibrium (no carns): ~103–125, avg 119
- Confirmed stable at gen 14,971 in overnight stress test

---

## File last updated

Session: June 9, 2026 — Phase 5h; Tracer module landed (dev-seq #3: workspace + `utils` crate, `trace` feature, runtime per-subsystem toggles, live panel, World/Plants instrumented); pollen-donor distance validated (first result); docs closed out. Next: safe-cleanup pass on plant.rs/genome.rs.
Session: June 8, 2026 — Phase 5h; step execution + world controls (Dev/Main presets, seed input, Restart, config_snapshot UI sync)
