# tools/ — headless verification harness

A deterministic regression/determinism check for the sim core, run in Node with
no browser. Backed by `SimWorld::state_signature()` (a 64-bit fingerprint of the
full persistent state) and `tools/verify.cjs`.

It catches **change** from a known-good baseline (regression), not correctness.
Ideal for confirming that a *no-behavior-change* refactor (or a perf refactor like
the plant spatial grid) didn't alter a single result.

## One-time / after any sim (Rust) change: build the Node package

From the `sim/` directory:

```
wasm-pack build --target nodejs --release --out-dir ../tools/pkg-node
```

This produces `tools/pkg-node/` (CommonJS + wasm), separate from `web/pkg`, so
the live app is untouched. `pkg-node/` is regenerated and can be git-ignored.

## Workflow

From the `tools/` directory:

```
# STEP ZERO — prove the run is deterministic before trusting any golden:
node verify.cjs selftest 137 5000

# Capture a golden master from a known-good build:
node verify.cjs capture 137 5000 --every 500 > golden/seed137_5000.txt

# After a no-behavior-change refactor (rebuild pkg-node first), verify nothing moved:
node verify.cjs check 137 5000 golden/seed137_5000.txt
```

`check` exits 0 on a full match, or 1 and prints the first diverging tick with
both signatures — so you know exactly where behavior changed.

- `seed` is any integer (a u64). Use the word `dev` to run the `dev_world()`
  preset instead (small, flat, seed 42).
- `--every N` controls checkpoint spacing (default 500). Capture and check must
  use the same `--every` and tick count, or the lengths won't line up.

## Notes / gotchas

- **Behavior-changing work** (new features, intentional dynamics changes)
  regenerates the golden deliberately — `capture` over the old file.
- A `selftest` failure means real non-determinism (e.g. a `HashMap`/`HashSet`
  iteration order leaking into results). That's a genuine bug worth fixing
  before relying on the golden master.
- The signature is taken at tick boundaries (after `update()`), where transient
  per-tick flags are cleared, so they don't perturb it.
- Rebuild `pkg-node` after *every* Rust change — otherwise you're checking a
  stale build (same trap as the web `pkg`).
