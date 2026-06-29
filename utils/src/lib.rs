// evo_sim - Copyright (c) 2026 Lens and Mix, LLC
// Licensed under the Apache License, Version 2.0. See LICENSE and NOTICE.
// More information: https://rkeithelliott.com
//! utils — shared library crate for the evolution simulator.
//!
//! Home for reusable, simulation-agnostic building blocks. This crate has NO
//! WASM dependencies on purpose: it links into the `sim` cdylib for the WASM
//! build and also compiles under native `cargo test`.
//!
//! Current contents:
//!   - tracer — CTracex-style RAII call tracing. The `trace_scope!` and
//!     `trace_event!` macros are exported at this crate root (via
//!     #[macro_export]); the guard, control API, and Subsystem live in
//!     `utils::tracer`.
//!
//! Planned future tenants (separate dev-sequence steps, NOT part of this chunk):
//!   - rng     (deterministic xorshift64, moved from sim)
//!   - spatial (spatial hash grid, moved from sim)

pub mod tracer;
