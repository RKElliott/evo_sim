# Evolution Simulator — Design Document

Maintained jointly by Keith Elliott and Claude.
Last substantive update: June 8 2026 — Phase 5h: step execution, world controls (Dev/Main presets, seed input, Restart, config-snapshot UI sync).

---

## Table of contents

1. Vision and philosophy
2. Architecture
3. Phase status
4. Validated results
5. Immediate backlog
6. Architectural decisions
7. Future features
8. User documentation notes

---

## Vision and philosophy

A browser-based scientific simulation tool for studying evolutionary dynamics,
ecological relationships, and emergent behavior. Designed to be usable as an
educational and research tool — experimenters can define organisms, sculpt
terrain, run controlled experiments, and observe results visually.

**Primary goals:**
- Testable hypotheses with real-world analogs
- Visually compelling — evolutionary change visible over accelerated timelines
- Scientifically rigorous — no magic numbers, clean separation of concerns
- Shareable — scenario files, reproducible seeds, data export

**Simulation timescale:**
The simulation operates at evolutionary timescales — hundreds to thousands of
generations. Day/night cycles, seasons, and weather are abstracted into terrain
and config parameters rather than simulated in real time.

**Individual behavior is symbolic:**
Individual organism behaviors (a carnivore killing a specific herb) are
statistical tokens representing population-level dynamics. They produce correct
selection pressures but are not accurate representations of real predation.
This is a known and intentional property of agent-based modeling.

**No magic numbers anywhere in the codebase.**
All constants live in Config (config.rs), passed at construction.
The Rust compiler enforces this — if it's not in Config it can't be tuned.

---

## Architecture

### Three layers, strictly separated

**Simulation core — Rust/WASM (Web Worker thread)**
- All logic, zero rendering code
- Shared buffer output — zero-copy reads from JavaScript
- Deterministic given the same seed
- Files: lib.rs, config.rs, terrain.rs, genome_def.rs, genome.rs,
  plant.rs, herbivore.rs, carnivore.rs, spatial.rs, occupancy.rs, rng.rs

**Coordinator/Renderer — JavaScript (Main thread)**
- Receives frames from worker via postMessage (structured clone)
- Manages rendering, UI, camera, controls
- Files: web/worker.js, web/index.html

### File responsibilities

| File | Responsibility |
|------|---------------|
| lib.rs | SimWorld struct, main update loop, all orchestration |
| config.rs | All tunable parameters — single source of truth |
| terrain.rs | Heightmap, water, nutrients, hillshading |
| genome_def.rs | Data-driven GenomeDef/Genome system, crossover, mutation |
| genome.rs | HerbGenome, CarnGenome (pre-data-driven, Phase 3/4) |
| plant.rs | Plant lifecycle, terrain-aware survival, sexual reproduction |
| herbivore.rs | Herbivore agent: FOV, eating, fleeing, reproduction |
| carnivore.rs | Carnivore agent: stalking, draining, home range |
| spatial.rs | O(n) spatial grid for neighbor lookup |
| occupancy.rs | Max plants per terrain cell |
| rng.rs | Deterministic xorshift64 RNG |

### World geometry
- Fixed aspect ratio: **2:1 width:height** — permanent rule, not configurable
- Current world: 4000 x 2000 world units, 256 x 256 terrain cells
- Target cell size ~15wu (cells are currently non-square: 15.6wu x 7.8wu)
- Light source: fixed northwest — standard cartographic convention

---

## Phase status

| Phase | Description | Status |
|-------|-------------|--------|
| 1 | Terrain (heightmap, water, nutrients, hillshading) | Complete |
| 2 | Plants (lifecycle, genome, occupancy, nutrient draw) | Complete |
| 3 | Herbivores (FOV, eating, reproduction, resting, boid steering) | Complete |
| 4a | Performance (spatial grid, Web Worker threading) | Complete |
| 4b | Carnivores (genome, state machine, sustained drain, stealth) | Functional |
| 4c | Alarm system (Fleeing/Alarmed states, propagation, visual ripples) | Functional |
| 5a | Plant sexual reproduction (crossover, pollen search, GenomeDef) | Complete |
| 5b | Geographic trait clustering confirmed | Validated gen 130 |
| 5h | Housekeeping: code review, SKILL.md, DESIGN.md, cleanup | In progress |
| 5c | Herb/carn sexual reproduction | Planned |
| 6 | Experimenter tools (terrain editor, organism editor, scenario system) | Planned |

Current focus: Phase 5h — code review and documentation before resuming active development.

---

## Validated results

### Seed 137 world characteristics
- World: 4000 x 2000 wu, 256 x 256 terrain cells
- Plant carrying capacity: ~9,063-9,265 (stable oscillation)
- Herb equilibrium (no carns): ~103-125, avg 119
- Overnight stress test: stable at gen 14,971 (32x speed, ~9,150 plants)

### Phase 2 — Plant population dynamics
- Pre-occupancy baseline: bloom to 35,000 then extinction by gen 43
- Post-occupancy + regen fix: stable equilibrium ~10,500-10,600
- Sea level drop (0.35 to 0.30): rapid coastal colonization, equilibrium ~23,000
- Carrying capacity scales with fertile land area (confirmed)

### Phase 3 — Herb equilibrium
- Gen 413: plants 10,718 stable, herbs 103-125, avg 119
- Confirmed at 8x speed with spatial grid

### Phase 4 — Carnivore dynamics
- Gen 22: herb population suppressed to 40-66 under carnivore predation
- Carns reproduce and make kills confirmed
- Long-term (gen 14,971): carns extinct from demographic fragility
- Root cause fixed: herb array index invalidation after retain() reshuffling

### Phase 5 — Plant evolution
- Reduced seed dispersal (32.5wu avg) prevents panmixia
- Gen 130: flood tolerance clusters near coast, drought tolerance on highlands
- Coastal plants evolve LOW seed spread (emergent, not programmed)
- Sexual reproduction stable at gen 480 (9,063-9,265 plants)

---

## Validation checklist

Ordered by dependency — each item requires the ones above it to be in place.
Do not mark an item complete without a documented test result.

### Prerequisites (build these first)

| Item | Status | Notes |
|------|--------|-------|
| Step execution (single-tick advance) | Done | Implemented June 8 2026 — worker.js `step` case + Step button / `.` key in index.html. No rebuild. One tick per step regardless of sim speed |
| Test world UI access (dev_world selectable from UI) | Done | June 8 2026 — Dev World + new Main World presets. dev_world now 800×400 (2:1; sole sanctioned exception is gone). Seed input + Restart (current seed) replace fixed Seed 42/137. New `config_snapshot()` syncs UI on every load: environment world→UI, herb/carn toggles intent-wins (UI→world). Needed a rebuild |
| Tracer module (cfg-based, per-subsystem) | Done | June 9 2026 — `utils` crate + `trace_scope!`/`trace_event!` macros gated by the `trace` cargo feature (zero cost without it). Master compile-time gate; runtime master + per-subsystem (AtomicU32 bitmask) toggles. Consumer-owned buffer bridged to `export_trace` + live in-browser trace panel. World + Plants instrumented so far; herbs/carns + `trace_agent!` focus filter added June 10 2026 (per-agent `set_trace_focus`; instrumentation verified behavior-neutral — seed-137 harness still MATCHes). `trace_buf` retirement remains |

### Rendering validation

| Item | Status | Notes |
|------|--------|-------|
| Hillshade slope-direction | Pending | NW-facing slope should appear brighter than SE-facing. Test: find a steep ridge on Seed 137, compare north vs south faces |
| Plant display contrast vs terrain | Pending | Plants too dark at low growth_rate — unresolved from earlier session. Needs side-by-side comparison at different lightness values |
| Plant color encoding (nutrient_efficiency + growth_rate) | Pending | Do vivid deep-green plants actually appear in nutrient-rich valleys? Do muted yellowish plants appear on highlands? |

### Infrastructure validation

| Item | Status | Notes |
|------|--------|-------|
| Occupancy symmetry | Pending | Every seed death must trigger occupancy.release(). Test: run dev_world, observe plant count vs occupancy grid count over 1000 ticks — they must match |
| being_eaten HashSet correctness | Pending | Kill an herb mid-meal (via step execution), verify plant being_eaten flag clears correctly next tick |
| Herb array index invalidation fix | Confirmed | Fixed — carnivores failed to make kills before this; kills confirmed after fix |

### Plant evolution validation

| Item | Status | Notes |
|------|--------|-------|
| Flood tolerance geographic clustering | Confirmed gen 130 | Coastal plants show high flood tolerance |
| Drought tolerance geographic clustering | Confirmed gen 130 | Highland plants show high drought tolerance |
| Coastal low seed spread (emergent) | Confirmed | Coastal plants evolve low seed spread without explicit pressure |
| Seed spread geographic clustering | Pending | Not yet formally validated — genome view should show low seed spread near coast |
| Nutrient efficiency clustering | Pending | Should cluster in fertile valleys — not yet validated |
| Shade tolerance clustering | Pending | Blocked on canopy shading being re-enabled |
| Pollen donor inverse-distance weighting | First result (gen 2, 512 events) | NOT random — weighting works. Mean/median donor distance ~15.5 wu ≈ half the ~33 wu max reach, vs ~22 (two-thirds) expected under random-by-area; short matings over-represented, long ones suppressed. Strength still to quantify — needs candidate-count + mean-candidate-distance enrichment for a chosen-vs-available comparison |
| Multi-seed validation | Pending | Geographic clustering confirmed on Seed 137 only — should validate on at least one other seed |

### Canopy shading validation (blocked on plant spatial grid)

| Item | Status | Notes |
|------|--------|-------|
| Plant spatial grid implementation | Pending | Required before canopy shading can be re-enabled |
| Canopy shade map re-enabled | Pending | Blocked on plant spatial grid |
| Shade effect on seedling growth | Pending | Test world: place 3 large plants, scatter seeds beneath them. Shade-tolerant seedlings should survive, intolerant should die |
| Shade tolerance geographic clustering | Pending | After shading enabled — does shade tolerance cluster under dense canopy areas? |

### Herbivore behavioral validation

| Item | Status | Notes |
|------|--------|-------|
| Flee speed correctness | Pending | Speed was reduced but never formally validated. Test: step-trace a fleeing herb, measure wu/tick against expected move_speed() value |
| Alarm propagation radius (150wu) | Pending | Does 150wu produce visually meaningful herd alarm responses? Test: place single carnivore near herd in test world, count alarmed herbs vs distance |
| Boid separation behavior | Pending | Do herbs maintain appropriate spacing? Test: dev_world, observe minimum inter-herb distances |
| Homing behavior | Pending | Do herbs return to home base when far away? Test: step-trace herb position vs home_x/home_y |
| Full movement speed audit | Pending | Multiple inline speed fractions identified — need systematic test of each movement state speed |

### Carnivore behavioral validation

| Item | Status | Notes |
|------|--------|-------|
| Full hunt sequence | Pending | Wander → detect → stalk → chase → latch → drain → kill. Step-trace single carnivore through complete hunt |
| Target locking fix | Confirmed | Kills confirmed after fix — but full sequence not step-traced |
| Carnivore flee response (herbs fleeing) | Pending | Do fleeing herbs actually outrun carnivores or not? Measure relative speeds |
| Density-directed wandering effectiveness | Pending | Does wander_toward_prey actually reduce time-to-first-kill vs pure random walk? Compare in test world |
| Long-term population stability | Pending | Carns extinct by gen 14,971 — demographic fragility unresolved. Sexual reproduction (Phase 5c) may fix this |
| Latched carnivore herb calming | Confirmed | Herbs near latched carnivore correctly downgrade from Fleeing to Alarmed |

### Performance validation

| Item | Status | Notes |
|------|--------|-------|
| Smooth 32x at 10,000+ plants | Confirmed gen 14,971 | ~9,150 plants stable |
| UI responsiveness after long runs | Pending | Known GC issue after overnight 32x — document threshold before freeze occurs |
| find_plant O(n) impact | Pending | After plant spatial grid: measure frame time before and after to quantify improvement |
| Scratch buffer refactor impact | Pending | After lib.rs refactor: measure allocation reduction |

---

## Development sequence (do in this order)

Before any fixes, build diagnostic infrastructure:

```
1. Step execution (worker.js + index.html only — no rebuild)   ✓ DONE June 8 2026
2. Test/Main world UI access + world controls (rebuild)        ✓ DONE June 8 2026
3. Tracer module (utils crate foundation)                      ✓ DONE June 9 2026
4. Run validation checklist items that are now unblocked       ◐ IN PROGRESS (pollen-donor done; herb/carn tracer + focus filter + real-id overlay + scale readout + per-agent CSV time-series logger [now with Carn/Herb/Any kind selector] done; FIRST behavior fix — avoid_water no-progress escape — landed June 26, golden re-baselined)
5. Plant spatial grid (unblocks 3 items: canopy shade, pollen O(n), find_plant O(n))
6. Canopy shade map re-enabled + validated
7. Reusable scratch buffers in lib.rs (performance)
8. Shared constructor builder in lib.rs (correctness)
9. HerbGenome/CarnGenome migration to GenomeDef (architecture)
10. Organism trait definition (architecture)
11. Runtime parameter panel (experimenter tools)
12. Terrain editor, organism editor, scenario system (Phase 6)
```

---

## Immediate backlog

Small concrete tasks for the next active development session.

### Code cleanup (no behavior change)

| Item | File | Notes |
|------|------|-------|
| Fix misleading comment on carn_chase_speed_mult (says "below 1.0", value is 1.4) | config.rs | Simple comment fix |
| Add inline comments to Phase 5 plant fields | config.rs | e.g. what does flood_death_rate: 0.004 mean in practice? |
| Add comment at elevation normalization ([-1,1] to [0,1] mapping) | terrain.rs | Line ~196 |
| Add comment clarifying nutrient formula direction (low elev = high nutrients) | terrain.rs | Lines ~240-243 |
| Remove dead field herb_restless_ticks (Restless state was removed) | config.rs | Dead code |
| Move plant_genome_def() and PT_* constants to organisms.rs | genome_def.rs | Separation of concerns |
| Fix misleading module comment in genome.rs (traits not bounded to [0,1]) | genome.rs | Budget system means values average 0.375, not 0.5 |
| Update stale vigilance comment (says "Phase 4 placeholder" — Phase 4 complete) | genome.rs | vigilance is actively used in alarm system |
| Move eat_energy_cost (0.005) to config.rs | genome.rs | Magic number |
| Migrate HerbGenome and CarnGenome to GenomeDef system | genome.rs | Prerequisite for organism editor — see architectural decisions |
| Audit plant.rs for magic numbers — move all scaling constants to config.rs | plant.rs | visual_radius, canopy_radius, maturation_ticks, nutrient_draw, energy_value, highland penalty, seed energy cost |
| Remove cfg parameter from is_water_adjacent or use it | plant.rs | let _ = cfg is a code smell |
| Change update() to take mutable output buffer instead of returning Vec<Genome> | plant.rs | Avoids 540,000 Vec allocations/sec at 9,000 plants x 60fps |
| Remove or clarify being_eaten field | plant.rs | Remnant from pre-Phase-5; verify nothing still reads it |
| Replace terrain_values return tuple with named struct | plant.rs | (f32, bool, f32) is fragile — names would clarify intent |
| Verify seed death always triggers occupancy.release() in lib.rs | occupancy.rs | Symmetry between occupy() and release() must be maintained or grid drifts |
| Remove unnecessary 'a lifetime from query_radius signature | spatial.rs | Compiler doesn't require it — minor cleanup |
| Change count_radius to accept scratch buffer rather than allocating internally | spatial.rs | Avoids per-call Vec allocation |
| find_plant() must use plant spatial grid — O(n_plants) per herb per tick currently | herbivore.rs | 1.575M checks/tick at 9k plants x 175 herbs — high priority |
| Move eating reach (4.0wu hardcoded twice) to config | herbivore.rs | cfg.herb_eat_reach |
| Move alarmed speed fraction (0.7) to config | herbivore.rs | cfg.herb_alarm_speed_mult |
| Move wander noise (0.12) to config | herbivore.rs | cfg.herb_wander_noise |
| Move all boid force constants to config | herbivore.rs | separation 0.15, max_rest 0.08, cohesion 0.0005, alignment 0.02, eat_separation 4.5 |
| Replace static mut NEXT_HERB_ID with AtomicU64 | herbivore.rs | Unsafe pattern — same fix needed in carnivore.rs |
| Remove or document try_reproduce (dead method — check_reproduce_density is active) | herbivore.rs | Dead code |
| Full movement speed audit — identify all inline speed fractions | herbivore.rs | Several multipliers not yet in config; validate against observed behavior |
| Move HOME_SEARCH_TICKS (600) to config | carnivore.rs | cfg.carn_home_search_ticks |
| Move resting velocity decay (0.85) to config | carnivore.rs | cfg.carn_rest_decay (separate from herb_rest_decay — tunable independently) |
| Move wander_toward_prey constants to config | carnivore.rs | search_r multiplier 1.5, n_samples 8, blend 0.60, noise 0.10 — all affect hunting effectiveness |
| Fix wander_toward_prey to reuse scratch buffer instead of allocating Vec | carnivore.rs | Fresh Vec allocation every wandering tick — same issue as count_radius |
| Move stalk-to-chase distance ratio (3.0x strike_reach) to config | carnivore.rs | cfg.carn_stalk_to_chase_ratio — affects when carnivore breaks into run |
| Move water avoidance lookahead constants (2.0x, 3.0x) to config | carnivore.rs | Affects coastline navigation behavior |
| Update stale module comment in lib.rs (still says "Phase 2") | lib.rs | Now Phase 5h |
| Refactor three constructors (new, dev_world, with_seed) into shared private builder | lib.rs | Eliminates per-field duplication — adding a field currently requires updating three places |
| Fix indentation inconsistency in delayed carnivore spawn | lib.rs | Lines 274-276 misaligned from surrounding code |
| Move repeated Vec allocations in update loop to reusable SimWorld scratch buffers | lib.rs | ~10 fresh Vec/HashSet allocations per tick — allocate once, clear each tick |
| Replace pop_history Vec with VecDeque | lib.rs | Vec::remove(0) is O(n) — VecDeque::pop_front is O(1) |
| Fix stale plant buffer layout comment (says stride 4, now stride 13) | lib.rs | Line 823 |
| Gate diag_energy_sum calculation on herbs_enabled | lib.rs | Wasted iteration when herbs disabled |
| Remove or simplify seed_initial_carns (no-op placeholder) | lib.rs | Returns Vec::new() — three unused parameters |

### Hardcoded values to move to config.rs

| Value | Location | Proposed name |
|-------|----------|--------------|
| Alarm propagation radius (150wu) | lib.rs | cfg.herb_alarm_radius |
| Post-flee alarmed_cd duration (120 ticks) | lib.rs | cfg.herb_flee_linger_ticks |
| Herb birth scatter radius (20wu) | lib.rs | cfg.herb_birth_scatter |
| Carnivore birth scatter radius (30wu) | lib.rs | cfg.carn_birth_scatter |
| Resting velocity decay (0.85) | herbivore.rs | cfg.herb_rest_decay |

### Placeholder fields (wire up when needed)

| Item | File | Notes |
|------|------|-------|
| carnivore::id field never read | carnivore.rs | For lineage tracking |
| CarnGenome::personal_space() never called | genome.rs | For density reproduction |

### Open behavior question — nutrient_draw never called (found June 9 2026)

`Plant::nutrient_draw()` (plant.rs ~103) computes a per-plant nutrient consumption
(`0.0002 + size*0.0003 + growth_rate*0.0002`) but is never called — the compiler flags
it dead, and the plant update loop reads terrain nutrient directly without ever
subtracting a draw. Decide which it is:
  - **Vestigial** → remove the method (pure cleanup), or
  - **Unhooked feedback** → it was meant to deplete the local terrain nutrient pool
    and never got wired in.
This is a *behavior* question, NOT part of the no-behavior-change cleanup pass — wiring
it in would change dynamics (plants would begin depleting local nutrients). Consequence
for the seasonality note (Future features): the "local nutrient depletion/recovery"
route to emergent cycles is currently INACTIVE for this reason. Investigate before the
next plant-dynamics work.

### Pending validation

| Item | Notes |
|------|-------|
| Hillshade slope-direction validation | Does NW-facing slope appear brighter than SE-facing? |
| Plant display contrast vs terrain | Plants too dark at low growth_rate — needs investigation |
| Carnivore population long-term stability | Extinct by gen 14,971 — demographic fragility |

### Observed issues + ideas (June 9 2026 verification session)

Noticed while watching runs during cleanup-pass verification — none previously recorded.

**[BUG] HUD not refreshed on Reload/Restart.** After a page reload or a Restart, the
HUD briefly shows stale/incorrect values until the first frame update corrects it.
Likely cause: the HUD is not re-initialized from the new world's state at reset — it
waits for the next frame message. Fix direction: push an immediate HUD update right
after Restart/reload (drive it from the config_snapshot sync, or emit one frame
immediately on reset). Likely web-only (index.html), no rebuild.

**[BUG] Carnivore debug visuals leak outside Debug view.** Once carns spawn, their
circle + directional arrow render even when Debug view mode is OFF. Other debug
overlays respect the flag; the carn circle/arrow apparently don't. Fix direction:
gate the carn circle/arrow draw behind the same debug-view flag as the rest. Likely
web-only (index.html draw code), no rebuild.

**[FEATURE] Build versioning + on-screen version stamp.** Develop a version/build
numbering scheme and show it on screen. Directly mitigates the stale-build confusion
hit earlier (no way to tell which worker.js / wasm was actually loaded): a visible
stamp confirms the running build at a glance. Suggested design: a version constant
compiled into the wasm (exposed via a small `version()` method) shown next to a
JS/HTML build marker — if the two disagree, you've caught a wasm/JS mismatch.
Optionally add a build timestamp or short hash.

**[NOTE / CLEANUP] Entity ids come from process-global `static mut` counters.**
`NEXT_HERB_ID` (herbivore.rs) and `NEXT_CARN_ID` (carnivore.rs) are process-global
monotonic counters. Originally they kept climbing for the life of the process, so ids
were NOT reproducible across runs/restarts. **Fixed June 11 2026:** `reset_herb_ids()` /
`reset_carn_ids()` now zero the counters at the start of every world build (`new`,
`dev_world`, `with_seed`) and `regenerate`, BEFORE any agent is seeded — so ids are now
stable and reproducible per world (same seed → same ids, and a Restart re-issues the same
ids rather than fresh higher ones). This is what makes the overlay/trace/Tag tooling
reliable: the id you read off the overlay is the id you can Tag/Focus. Id is still
excluded from the regression fingerprint (it's an identity tag, not dynamics), so the
golden master is unaffected. The `static mut` itself remains a smell worth retiring
(Rust 2024 discourages it) — a per-world counter field on SimWorld is the proper future
refactor; the reset is the minimal fix. Original issue found June 9 2026 while building
the verify harness.

**[NOTE] `parent_id` is never populated → kin recognition is inert.** Reproduction
passes `None` for the parent (herbivore.rs:662), so every herb has `parent_id = None`,
which makes the kin check at herbivore.rs:415 (`Some(h.id) == self.parent_id`) always
false. The lineage/kin-bias feature is wired but unhooked — same flavor as
`nutrient_draw`. Decide later: populate it (pass the parent's id at birth) or remove
the dormant check. Found June 9 2026.

### June 10 2026 — herb/carn trace session findings + new observations

First real read of the herb/carn instrumentation (traced build). Reconstructing
one carnivore's full hunt (carn#6) from the interleaved buffer is what motivated
the per-agent focus filter, now built (`trace_agent!` + `set_trace_focus` + a
"Focus id" UI input; 0 = all agents). Empirical findings, all from trace:

- **Chase outruns flee ~2.4×, tirelessly.** Chase ≈ 0.95 wu/tick vs herb full-sprint
  flee ≈ 0.39 (alarmed ≈ 0.27, forage ≈ 0.1). Observed a 62-tick chase holding a
  flat 0.95 — the stamina/fatigue term never bit. Once a chase starts, escape is
  essentially impossible. Needs tuning + a real stamina cap.
- **Prey can be latched without ever reacting.** herb#23 was latched while grazing
  with zero flee/alarm lines — most likely the carn closed from outside the herb's
  FOV arc (or a late target-switch). The "interactions look wrong" case.
- **Panic triggers the kill.** The chase burst didn't start from direct sight — a
  *neighbour's* alarm broadcast made the target flee, and a fleeing target is what
  flips the carn from stalk (0.12) to chase (0.95). The herd's own alarm commits the
  predator.
- **Stalk→chase is an ~8× speed discontinuity** (0.12 → 0.95). Against a world that
  otherwise moves 0.1–0.39, that snap is the "moves too fast" / "everything zips
  away" the eye catches — amplified when an alarm cascade makes a cluster jump
  speed-tier at once.

**[FIXED June 26 2026] Carnivore `avoid_water` trap.** A stalking carn that runs into
a concave water boundary oscillated in place (avoidance vector flips back and forth,
net progress ≈ 0) and starved there. Confirmed quantitatively by a per-agent CSV on the
lake carn: energy bled linearly 56→0 over ~331 ticks, position frozen in a ~0.1×3.3u
box, target_dist pinned at ~77 (never closing), speed pegged at 1.0 the whole time —
i.e. full energy burn, zero net displacement.
Fix implemented (no-progress escape, carnivore.rs): a pursuing carn that travels
< TRAP_MIN_PROGRESS (5u) over TRAP_WINDOW_TICKS (60) is judged trapped; it drops the
target and steers along a captured "away from the unreachable prey" heading for
TRAP_ESCAPE_TICKS (30), then re-scans. The heading points from the across-water prey
back toward open land, so it exits the pocket; `avoid_water` still runs during escape
so it can't be driven into the water. Constants grouped at the top of carnivore.rs for
easy retuning / future promotion to runtime/genome params. The original "hysteresis on
avoid_water" idea was set aside after reading the code: `avoid_water` only rotates the
velocity (position advance is in `move_toward`), and `wander_toward_prey` biases 60%
toward the densest prey — which at the lake is *across* the water — so leaning on either
would re-approach the trap. A deliberate escape heading was needed instead.
KNOWN follow-on to watch: if the only reachable prey is across the bay, the carn may
re-stalk and re-trap; the detector simply fires again (no longer pinned-to-death). A
repeating escape/re-trap signature would point at across-water targeting — the separate
later fix being line-of-sight target rejection, NOT folded in here.

**[CANDIDATE FIX] Movement-based predator visibility.** A carn's detectability is
currently `sight_radius × stealth_factor` (a static gene), indifferent to its speed
— so a creeping stalker and a sprinting chaser are equally (in)visible. Making
detectability scale with the carn's current speed would (a) keep stalking stealthy
as intended and (b) make the chase burst *pop* into prey visibility right as it
commits — likely fixing the "latched without flinching" case and restoring a fair
chase. Precedent already in code: the carn's *own* vision keys off speed (slow carn
→ omnidirectional sight); this mirrors it on the prey side.

**[RENDERING] Terrain looks blotchy/pixelated.** The terrain grid is drawn as hard
discrete cells. Smoothing is a pure rendering nicety (bilinear-interpolate the
terrain field on the cache canvas, a light blur pass, or higher-resolution noise) —
no behavior impact. Low priority; "ok for now" per observation.

### Testing infrastructure (backlog, added June 8 2026)

Goal: a standard automated smoke test so a regression is caught without a manual
click-through. Two natural layers:

1. **Rust `cargo test` smoke test** — cheap, high value, can land anytime. Build
   `dev_world` (deterministic: seed 42, no water, known initial counts), step it
   N ticks, assert invariants: no NaN positions/energy, population stays in sane
   bounds, occupancy grid stays consistent, `being_eaten` clears, and same seed →
   same result. Does NOT need the Tracer — asserts on diag counters + direct
   internal state. Conveniently converts two Pending validation items (occupancy
   symmetry, being_eaten clearing) into automated asserts, and its determinism
   assert feeds the save/load decision (see Scenario system).
2. **Tracer-based behavioral smoke** — assert specific event *sequences* (a hunt
   completing, a flee, a pollination) over a few ticks. Needs the Tracer to be
   tick-legible (dev-sequence item #3, the Tracer module — now done, so this is
   unblocked).
3. **Golden-master comparison (regression diff)** — the automated form of the manual
   before/after done June 9, aimed squarely at verifying *no-behavior-change* refactors.
   Capture a fingerprint of a known-good run, store it in the repo, and diff future
   builds against it: zero diff = behavior unchanged; any diff is flagged with the tick
   it first appeared. Behavior-changing work regenerates the golden deliberately.

**Shared primitive — `state_signature()`.** All three layers get easy if the core
exposes a deterministic full-state fingerprint: hash (over `to_bits()` of floats) every
plant/herb/carn's position + energy + stage/state + genome, plus the RNG internal state
and the tick. One u64 per checkpoint replaces eyeballing counts, makes the determinism
assert trivial (run twice → same signature), and is the unit the golden-master diffs on.
Read-only over existing state, so low-risk to add.

**Runner options:**
- *Native `cargo test`* (layers 1 + 3) — fastest, CI-able. Prerequisite: the pure-Rust
  core must be reachable without the `#[wasm_bindgen]` wrapper, which likely needs a
  core/wrapper split (its own small refactor). The clean long-term home.
- *Headless Node harness* — build with `wasm-pack --target nodejs` and drive it from a
  `verify.js seed ticks` script that prints signatures. No browser, no DOM, no
  hard-reload trap, no human error. Sidesteps the wrapper-split prerequisite, so it's
  the pragmatic near-term path; the sim core is pure computation, so the nodejs target
  works.
- *In-browser fallback* (zero core changes) — a "run to tick N then auto-pause" control
  plus an on-screen signature readout removes the two biggest manual-error sources
  (imprecise stepping, eyeballed counts) without leaving the browser. Weaker but cheap.

Recommended first build: `state_signature()` + the Node harness — small, additive, and
it retires most of the manual ritual immediately.

Cluster: smoke test → determinism check → golden-master diff → save/load run-sharing.

### Architecture explainer — accessible writeup for a non-specialist audience (backlog)

Requested June 9 2026 (Keith). A standalone document — NOT internal reference — that
explains what we've built to technically-capable friends/peers who don't live in this
stack, so the work can be understood and appreciated (and so Keith can walk someone
through it). A pride-of-work piece.

Tone / accessibility (firm requirements):
- Avoid jargon; do not assume the reader knows the terms.
- When a new idea or term first appears, define it in a sidebar callout / glossary box /
  analogy — explained on first use, never assumed.
- Lead with the "why" and the big picture before any mechanics.

Scope to cover (conceptually, not exhaustively):
1. What the simulation IS and what it shows — evolution emerging from simple rules.
2. How it works conceptually — the tick loop, the agents (plants / herbivores /
   carnivores), genomes + the shared trait budget, and determinism (same seed → same
   world).
3. The technology stack and how the pieces relate, each explained from scratch:
   Rust (the language the core is written in) → compiled to WebAssembly / WASM (fast
   code that runs in a browser) → run inside a Web Worker (a background thread, so the
   page stays responsive) → JavaScript + Canvas drawing the picture → the worker /
   main-thread split. A simple data-flow diagram would carry a lot of weight here.
4. How the regression testing works and WHY it matters: determinism + the state
   "fingerprint" (one number summarizing the whole world) + the golden master + the
   headless check — framed as the "one-command diff" that makes refactoring safe.

Source material exists in DESIGN.md and SKILL.md, but must be RE-FRAMED for a curious
non-expert (those docs assume the stack). Not urgent — write it when the simulation work
hits a natural pause; the core (sim + stack + harness) is already stable enough to
describe.

---

## Architectural decisions

Permanent record of significant design choices and reasoning.

### Spatial hash grid (spatial.rs)
Standard data structure used throughout game development and real-time
simulation since the 1990s. Divides the world into fixed-size cells; each
cell stores a list of agent indices. Neighbor queries check only nearby
cells rather than all agents — reduces O(n²) boid scanning to O(n).

Implementation choices:
- Agent indices (usize) rather than references — required by Rust's ownership
  model; storing references would create borrowing conflicts during updates
- clear() retains Vec capacity — avoids per-tick reallocation of cell buffers
- cell_span + 1 conservative buffer — never misses agents near cell boundaries;
  callers do their own distance check on returned candidates

Second strongest candidate for the utils crate after rng.rs. Has no
dependencies on simulation-specific types — only f32 world coordinates
and usize agent indices. Immediately useful in any spatial simulation.

### Organism trait — path toward the organism editor

Current code has no shared abstraction between Plant, Herbivore, and Carnivore.
Each is an independent struct with its own update loop in lib.rs. This works
for three known types but cannot support runtime-defined organism types from
the organism editor.

The target architecture is a shared Organism trait:

```rust
trait Organism {
    fn genome(&self) -> &Genome;
    fn update(&mut self, world: &WorldContext, rng: &mut Rng);
    fn render_data(&self) -> RenderRecord;
}
```

This is the Rust equivalent of a C++ pure virtual base class. Rust has no
class inheritance — shared behavior is expressed through traits, shared data
through composition (fields). The GenomeDef system is already the data layer
of this solution. The Organism trait is the behavioral layer still to be built.

**Performance of dynamic dispatch (dyn Organism):**
The organism editor requires types defined at runtime, which means dynamic
dispatch (dyn Organism) rather than static dispatch. Cost analysis:
- Each virtual method call: ~1-3 nanoseconds (one vtable pointer lookup)
- At 10,000 organisms x 60fps: ~30 microseconds per frame
- Frame budget at 60fps: 16,000 microseconds
- Overhead: well under 1% — genuinely negligible

Memory cost: 8 bytes per organism for vtable pointer = 80KB at 10,000 organisms.

Object instantiation: zero additional cost vs current approach.

The current separate update loops in lib.rs are equivalent in performance to
static dispatch. Moving to dyn Organism costs almost nothing at runtime while
enabling the full organism editor vision.

**Migration path:**
1. Define Organism trait (behavioral contract)
2. Implement for Plant, Herbivore, Carnivore
3. Migrate HerbGenome/CarnGenome to GenomeDef (prerequisite)
4. lib.rs update loop becomes a single pass over Vec<Box<dyn Organism>>
5. Organism editor creates new types implementing Organism at runtime

### Data-driven genome system (genome_def.rs)
Organisms defined by GenomeDef struct rather than hardcoded fields.
Genome is a flat Vec<f32> indexed by named constants.
Reason: Enables organism editor — new organisms without Rust code changes.
Note: &'static str in TraitDef must change to String when runtime-defined
trait names are needed (organism editor phase).

### Fixed 2:1 world aspect ratio
All worlds are 4000x2000 (or proportional scaling). Not configurable.
Reason: apply_island_mask ellipse factor (1.4) calibrated for 2:1.
Non-square terrain cells at this ratio are a known accepted imprecision.
Different world shapes achieved via terrain editor, not ratio changes.

### Deterministic RNG (rng.rs)
Custom xorshift64 rather than the rand crate.
Reason: Absolute guarantee that Seed 137 produces identical worlds across
all machines, browsers, and future builds. No external dependency risk.
First candidate for extraction into the utils crate.

### Web Worker threading
Rust/WASM simulation in dedicated Web Worker; rendering on main thread.
Communication via structured clone (postMessage).
Reason: Simulation never blocks rendering; rendering never blocks simulation.
Known issue: GC pressure from structured clone accumulation after long 32x runs.
Mitigation: restart browser tab after overnight runs.

### Budget-constrained genome (RPG cost system)
All trait values sum to a fixed budget (4.0 for plants).
Improving one trait costs another.
Reason: Prevents runaway optimization; creates genuine evolutionary tradeoffs.

### Uniform crossover for sexual reproduction
Each trait independently inherited 50/50 from either parent, then mutated.
Reason: Maximum recombination — any trait combination can emerge.
Two-pass rebalance is an approximation adequate for current use.

### Seed dispersal range (current: 5 + trait*55 wu)
Original range (~105wu avg) caused panmixia by gen 38.
Reduced to ~32.5wu avg so gene flow crosses island in ~123 generations.
Reason: Geographic trait clustering requires local mating.

### Northwest fixed light source
Hillshade computed once at generation; strength applied at render time.
Light direction hardcoded as northwest.
Reason: Standard cartographic convention. Day/night cycle not planned.

### Canopy shade map disabled
O(n^2) shade map disabled at ~10,000 plants — too expensive.
Fix required: add plant spatial grid, then re-enable shade calculation O(n).

---

## Future features

Agreed as worth building, not yet scheduled.

### Runtime-adjustable parameters and dynamic parameter panel

Many config values can be meaningfully changed mid-simulation to run
experiments — establish a baseline population, change conditions, observe
the evolutionary response.

**Currently runtime-adjustable (WASM setters already exist):**
water_level, nutrient_flow_rate, hillshade_strength, plants_evolve,
plant_mutation_rate, plant_seed_chance, herbs_enabled, carns_enabled

**Should be added as runtime-adjustable:**
- plant_flood_death_rate — increase coastal pressure, watch flood tolerance evolve
- plant_drought_death_rate — increase highland pressure
- herb_mutation_rate, carn_mutation_rate — already done for plants
- carn_wander_drain_mult — adjust predator hunting pressure in real time
- plant_seed_interval — speed up/slow down plant reproduction
- Boid force constants — adjust herd behavior live
- Pollen-donor distance-weighting exponent — the gene-flow kernel shape (currently
  fixed linear 1/d in plant.rs find_pollen_donor). Exposing it lets an experimenter
  contrast evolutionary scenarios by pollination distance: tight (1/d²/exponential) →
  strong local genetic structure + sharper trait clustering; loose (→ random) → more
  mixing/diversity, clustering washes out. NOTE this is a WORLD RULE, not a creature
  trait (no budget cost), so it lives on the slider panel, not in the genome. When
  multiple plant species arrive, promote it to a per-species constant on the GenomeDef
  (still a species-defining field, not a budget trait). Discussion June 9 2026.

**Must NOT be runtime-adjustable (would break simulation state):**
- world_w, world_h — terrain and spatial grids sized to these
- terrain_cols, terrain_rows — same
- plant_max_per_cell — occupancy grid built with this value
- terrain_seed — only meaningful at initialization

**Dynamic parameter panel (medium term):**
Rather than hardcoding sliders in index.html, expose config field metadata
from the simulation itself — field name, current value, min, max, and
whether it's runtime-adjustable. The UI generates sliders automatically.

Benefits:
- Adding a runtime-adjustable parameter in config.rs automatically
  appears in the UI without touching index.html
- Connects to the experimenter tools vision — parameter panels become
  part of the scenario/experiment definition
- Enables saving parameter sets alongside scenario files

Implementation: add a `runtime_params()` WASM method that returns a JSON
string listing all adjustable parameters and their ranges. JavaScript
parses it and builds the panel dynamically.

### Environmental rhythms / seasonality (exogenous forcing + emergent-cycle experiments)

Recorded June 9 2026 (Keith). Two linked threads — a feature and a research question.

**Feature — configurable, runtime-adjustable seasonality.**
The environment is currently static (fixed terrain, constant NW light, steady
nutrient flow; water level only changes on demand) — there is no environmental
clock. Add optional *periodic forcing* the experimenter can switch on and tune
mid-run: a cyclic modulation (sine to start) on one or more environmental
parameters — nutrient_flow_rate, water_level, and/or a new global light /
"temperature" factor — with period, amplitude, and phase as live knobs. Slots
directly into the runtime-parameter-panel work above. Lets an experimenter
contrast "seasonal" vs "aseasonal" worlds and watch the evolutionary response
(e.g. do plants evolve toward synchronized reproduction under forcing?).

**Research question — does seasonality emerge endogenously without a clock?**
Even with NO exogenous forcing, does something *akin* to seasonality arise from
the organisms' own interactions? The sim is a clean testbed precisely because it
has no external clock, so any periodicity is necessarily endogenous. Two
candidate sources already in the model: (1) trophic population oscillation
(plant↔herb↔carn — the "stable oscillation" in carrying capacity is its
fingerprint); (2) density-dependent pollination — find_pollen_donor needs a
donor in range, so mating success rises in dense (boom) phases and falls in
sparse (bust) phases, plausibly producing a reproductive rhythm coupled to the
population cycle, with mean donor distance widening in the busts. Measure with
the Tracer: bin reproduction events into tick windows vs population count, look
for co-oscillation + donor-distance shifts (same enrichment flagged on the
pollen-donor validation row). A third potential pathway — local nutrient
depletion/recovery — is NOT currently live: Plant::nutrient_draw() is defined but
never called (see "Open behavior question — nutrient_draw never called" in the
backlog), so plants do not yet deplete local nutrients.

**This is established territory — don't reinvent the framing.** Endogenous vs
exogenous drivers of population cycles is a century-old ecology question
(Elton onward). Relevant prior work: Lotka–Volterra self-organized predator–prey
oscillation; emergent long-range synchronization of oscillating populations
*without* external forcing (Noble, Machta & Hastings, Nat. Commun. 2015);
self-organized criticality / Holling's adaptive cycle / panarchy; intrinsic
ecological dynamics driving community turnover without external change
(O'Sullivan et al., Nat. Commun. 2021). Key distinction to keep straight:
emergent cycles are quasi-periodic (period set by internal dynamics), NOT
"seasons" in the strict sense (a fixed externally-imposed period) — true
seasonality needs the forcing feature above.

**Intellectual lineage (motivation).** Earth carries overlapping geophysical
rhythms (day/night, tides, lunar, seasonal, solar) that act as pervasive
selection pressures; organisms evolved endogenous clocks (circadian ~24h,
circatidal ~12.4h, circalunar ~29.5d, circannual ~1yr) that anticipate rather
than merely react to them. (Keith, via E.O. Wilson, Lyall Watson, et al.)
Caveat for accuracy: the clock systems are the genuine internal "mirror" of
external cycles — fast physiological oscillators like heartbeat/breathing are
endogenous rhythms but are not entrained reflections of geophysical cycles.

### Test worlds (small scenario worlds for behavior validation)
Small purpose-built worlds for debugging specific behaviors without
running the full Seed 137 simulation. Constructed by modifying config
and rebuilding — no editor needed yet.

Proposed test scenarios:
- **Movement testbed** — flat 400x200 world, no water, uniform nutrients.
  Drop 2-3 herbs and observe wandering, homing, and boid separation.
  Validate that all speed multipliers produce visually correct behavior.
- **Predation testbed** — small world, 10 herbs, 1 carnivore.
  Observe full hunt sequence: detect → stalk → chase → latch → drain.
  Validate alarm propagation radius and flee behavior.
- **Reproduction testbed** — flat world, single herb pair.
  Observe density-gated reproduction and offspring dispersal.
- **Selection pressure testbed** — small island with steep terrain gradient.
  Verify flood/drought tolerance clustering appears within 20-30 generations.

Implementation: add a `dev_scenario()` constructor to SimWorld alongside
the existing `dev_world()`. Scenario is selected via a URL parameter or
UI dropdown. No rebuild required to switch scenarios once implemented.

### Step execution (single-tick advance) — IMPLEMENTED June 8 2026
A pause/step control allowing the simulation to advance exactly one tick
at a time. Essential for debugging movement behavior and state transitions.
The Python version had this; the current Wasm version does not.

Implementation: when paused, a Step button sends a single
`{type: 'step'}` message to the worker which runs one tick and posts
the frame back. The existing pause infrastructure already handles
the paused state — step is a small addition to worker.js.

As built (matches the sketch above):
- worker.js: a `step` case calls `sim.update()` once then `sendFrame()`,
  guarded by `paused` so a stray step can't inject a tick during playback.
  sendFrame() bypasses the loop's throttle so the stepped frame shows immediately.
- index.html: Step button (disabled unless paused), `stepOnce()` handler,
  enable/disable wiring in `togglePause()`, and the `.` (period) key.
- One tick per step regardless of sim-speed slider (intentional, for debugging).
- The main render loop is already decoupled (rAF), so the stepped frame is
  drawn on the next animation frame with no extra trigger needed.

This combined with the Tracer module would give tick-level visibility
into exactly what each agent is doing at each step.
Required for:
1. Re-enabling canopy shade map (currently O(n²), disabled)
2. find_pollen_donor performance (currently O(n) per plant per seed event —
   225x reduction with spatial grid at 9,000 plants)
Use same SpatialGrid pattern as herb/carn grids.
Move plant_genome_def() and PT_* constants out of genome_def.rs.
New file becomes registry for all built-in organism types.

**Plant spatial grid**
Required before re-enabling canopy shade map.
Use same SpatialGrid pattern as herb/carn grids.

**Herb/carn genome inspector bars**
Currently show state label only. Plant inspector fully working.
Add trait bars using same pattern.

**Herb/carn sexual reproduction**
Two parents; offspring genome is crossover + mutation.
Use existing spatial grid for mate detection.
Expected: natural selection tunes parameters currently set manually.

### Medium term

**Tracer module (utils crate — first candidate)** — IMPLEMENTED June 9 2026 (dev-seq #3).
Rust equivalent of Keith's CTracex C++ library (Windows Developer Journal ~1995).
Uses Drop trait (RAII) for automatic entry/exit logging with call-stack indentation.
Thread-safe via atomics. Zero cost in release builds via the `trace` cargo feature.
As built (a refinement of the original plan): a single master COMPILE-TIME gate (the
`trace` feature) compiles the machinery in; within a traced build the master on/off AND
per-subsystem switches (World/Plants/Herbs/Carns, packed in an AtomicU32 bitmask) are
RUNTIME — toggle from the UI without rebuilding. Entry points are the `trace_scope!` /
`trace_event!` macros (expand to nothing without the feature, so LTO strips it all).
Output accumulates in a consumer-owned buffer, bridged to the existing `export_trace`
path plus a live in-browser trace panel.
Remaining follow-ups: instrument herbs/carns; retire the legacy `trace_buf`.

Usage pattern:
  fn update_plants(&mut self) {
      let _t = Tracer::new("update_plants");  // entry logged
      // ... function body ...
  }  // exit logged automatically when _t drops

Output (same indented call-tree style as CTracex):
  update_loop
    plant_update
      plant[847] flood check: tol=0.12 risk=0.0039
      plant[847] died (flood)
    plant_update exit
  update_loop exit

**Private Cargo workspace** — workspace + `utils` crate stood up June 9 2026 (dev-seq #3, step 3a).
Extract reusable modules into a utils crate.
Keith's position: proactive generalization benefits outweigh costs for private projects.
Candidates: rng.rs (deterministic xorshift64), spatial.rs (spatial hash grid),
terrain noise functions. The Tracer already lives in `utils`; rng/spatial migration still pending.
Workspace structure:
  evo_sim/
  +-- Cargo.toml    (workspace root)
  +-- sim/          (simulation crate, depends on utils)
  +-- utils/        (shared library crate)
  +-- web/
Do at a natural pause — requires updating all import paths in sim/src/.

**Replace runtime trace system with cfg-based diagnostics**
Current trace_buf / trace_enabled system runs in release builds.
Replace with #[cfg(debug_assertions)] and feature-flag logging.
Zero cost in release; full detail in debug/feature builds.

**Scent / prey density trails**
Herbs deposit diffusing scent onto terrain. Carnivores follow gradient.
Current density-directed wandering is instantaneous approximation.
True scent creates evolutionary arms race.
~1 day implementation. Terrain already has multi-layer buffer infrastructure.

### Longer term — experimenter tools

**Terrain editor**
Visual tool for painting elevation, nutrients, and water barriers.
Enables hypothesis-driven world design.
Terrain JSON format compatible with scenario system.

**Organism editor**
Form-based UI to define new organism types using GenomeDef system.
No Rust changes required for new organisms.
Hard open problem: connecting genome traits to emergent behavior requires
either a scripting layer or predefined behavioral primitives.
Design carefully before building.

**Scenario system / save-load (experimenter tool, longer-term)**
Use case: let experimenters share simulation runs with each other.
Save/load complete simulation states as JSON.
Terrain + organism definitions + config + optional initial genome pool.
Deposit organism UI: click map location, choose type, place.
Shareable files for reproducible experiments.

Design note (June 8 2026): how big this needs to be depends on determinism.
If the RNG is seeded and nothing pulls from wall-clock or unordered iteration,
then seed + config + tick-count already reproduces a run exactly — "sharing a
run" is sharing those values, not serializing thousands of organisms. The
seed input + config_snapshot added in Phase 5h are the first usable slice of
this (a world is already shareable by its seed). Full-state serialization then
earns its keep only for (a) sharing a *mid-run* snapshot without replaying it,
or (b) papering over any non-determinism. The smoke test's "same seed → same
result" assertion (below) is exactly the check that tells us whether the cheap
seed-sharing path is viable. Cluster: smoke test → determinism → save/load.

### Deferred / low priority
- Day/night cycle (drives diurnal/nocturnal speciation)
- Weather system (periodic nutrient redistribution)
- Wound system for herbs (escaped predation leaves weakness)
- Zoom overlay panel (picture-in-picture organism inspection)
- Tabbed UI panel (Stats | Inspector | Parameters | Debug | Log)
- Touch controls for iPad
- Architecture diagram light theme
- Cargo.toml repository and license fields

---

## User documentation notes

Drafts for eventual user-facing documentation.

**On timescale:**
"The simulation models evolutionary processes across many generations.
Environmental variables like light direction, temperature, and seasonal change
are represented implicitly through terrain gradients and selection pressures
rather than simulated explicitly."

**On individual behavior:**
"Individual organism behaviors are abstractions that produce correct
population-level statistics. They are not accurate representations of how
real predators hunt or real plants compete. Think of each agent as a token
in a statistical model that happens to be visually engaging."

**On world shape:**
"All simulation worlds are 2:1 width to height. This matches standard
widescreen displays and ensures consistent behavior across all terrain tools."

**On light source:**
"Terrain shading uses a fixed northwest light source, following standard
cartographic convention. This helps read terrain elevation visually but
does not represent a real sun position."

**On simulation speed:**
"All tick-based time values (organism lifespans, seed intervals, etc.) are
expressed in simulation ticks. At 1x speed, 60 ticks = 1 real second.
At 32x speed, the same number of ticks passes 32 times faster."

---

## Performance notes

- Plant update: O(n) with spatial grid (shade map disabled — O(n^2) pending fix)
- Herb/carn update: O(n) via spatial grid (80wu herb cells, 120wu carn cells)
- Web Worker: simulation on dedicated thread, rendering on main thread
- Target: stable 60fps at 10,000+ agents on Seed 137 (4000x2000)
- Confirmed: smooth 32x at gen 14,971 with ~9,150 plants, ~175 herbs
- Known issue: GC pressure from structured clone after many hours at 32x

### Declared scale (L, T) + scale readout (added June 10 2026)

**The reframe:** the sim has no *wrong* scale — it has *no declared* scale. A world
in arbitrary units/ticks is scale-free; its character is fixed by dimensionless
ratios (chase÷flee ≈ 2.4, strike-reach÷detection ≈ 0.3, detection÷world-width,
agents-per-world, lifespan-in-ticks). Choosing two conversion factors — L (metres
per world-unit) and T (seconds per tick) — declares it England or a petri dish
without changing a single pixel; only the labels move.

**Done (display-only):** a front-end scale readout (index.html only, zero sim/Rust
change, harness-irrelevant): L/T inputs ("m/unit", "s/tick"), a fixed facts line
(world ≈ W×H km + the L/T legend), and a **map-style scale bar** that resizes with
the scroll wheel (metres-per-pixel = L ÷ camZ). The bar is the zoom-tracking part;
the facts are zoom-invariant sim properties.

**Pending (heavy):** making it *plausibly* a chosen regime (e.g. island-scale
mammals) is NOT a relabel — the current ratios read as "a few large agents in a
small bounded arena." It needs (a) far more, far smaller-relative agents → a much
bigger world + the perf work (plant spatial grid, dev-seq #5), and (b) speed/stamina
ratios re-derived from real animal data (ties into the chase-fatigue / speed-scaling
tuning in the June 10 trace findings). Two components of "too fast" stay separable:
the in-sim *ratio* (chase vs flee — tuning) and the *playback rate* (ticks per
wall-second — a fast-forward knob; declaring T + a playback slider exposes it).
