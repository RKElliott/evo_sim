//! tracer — CTracex-style RAII call tracing for the evolution simulator.
//!
//! Ported in spirit from Keith's CTracex C++ library (Windows Developer
//! Journal, ~1995): a guard object logs an "enter" line on construction and the
//! matching "exit" line when it drops, with call depth driving indentation, so
//! the output reads as an indented call tree.
//!
//! Design (the decisions locked in the 3b plan):
//!   - The mechanism in this module is ALWAYS compiled. It is reached only
//!     through the `trace_scope!` / `trace_event!` macros, which are gated
//!     behind the `trace` cargo feature. With the feature off there are no call
//!     sites, so LTO strips this code from the release `.wasm` — that is the
//!     "zero cost in release" guarantee, and why 3a kept `lto = true`.
//!   - A single COMPILE-TIME master switch (the `trace` feature) turns the
//!     machinery on. WITHIN a traced build, the master on/off and the
//!     per-subsystem switches are RUNTIME — toggle them from the UI without
//!     rebuilding (set_enabled / set_subsystem).
//!   - Output accumulates in a module-global buffer; `sim` owns the policy
//!     (enable, export, clear) and bridges `export()` to its existing
//!     `export_trace` path. The buffer is consumer-drained, not pushed.
//!
//! Thread-safety is real, not ceremonial: `cargo test` runs tests in parallel
//! threads that share these globals, so the atomics + mutex prevent races.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;

/// Spaces of indentation per call-stack level (matches the DESIGN.md sample).
const INDENT_WIDTH: usize = 2;

/// Hard cap on retained trace lines. Oldest lines drop first so a long run
/// cannot grow the buffer without bound.
const MAX_LINES: usize = 4000;

// ---------------------------------------------------------------------------
// Global state — const-initialized statics, so no lazy_static / once_cell and
// thus no dependencies (keeps utils buildable under native `cargo test`).
// ---------------------------------------------------------------------------

/// Master runtime switch. Driven by the UI trace checkbox via `set_enabled`.
static ENABLED: AtomicBool = AtomicBool::new(false);

/// Per-subsystem runtime switches, packed as bit flags in one word
/// (the bit-mask-in-a-register pattern from 360 assembler / C).
static SUBSYS_MASK: AtomicU32 = AtomicU32::new(0);

/// Per-agent focus filter. 0 = no focus (all agents emit). Nonzero = only
/// per-agent events whose id matches are recorded. NOTE: herb and carn id
/// spaces overlap (separate counters), so a focus value can match both a
/// herb and a carn bearing that number.
static FOCUS_ID: AtomicU64 = AtomicU64::new(0);

/// Current call-stack depth, drives indentation.
static DEPTH: AtomicUsize = AtomicUsize::new(0);

/// The trace sink. `sim` drains this via `export()`. VecDeque so trimming the
/// oldest line is pop_front (O(1)), not Vec::remove(0) (O(n)).
static BUFFER: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());

// ---------------------------------------------------------------------------
// Subsystems
// ---------------------------------------------------------------------------

/// Which part of the simulation a trace line belongs to. The discriminant is
/// the bit position used in SUBSYS_MASK.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Subsystem {
    World  = 0,
    Plants = 1,
    Herbs  = 2,
    Carns  = 3,
}

impl Subsystem {
    #[inline]
    fn bit(self) -> u32 {
        1u32 << (self as u32)
    }
}

// ---------------------------------------------------------------------------
// Control API — called by `sim` (in 3c) to drive the tracer.
// ---------------------------------------------------------------------------

/// Master on/off. When false, all guards and events are inert.
pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Turn one subsystem's tracing on or off at runtime.
pub fn set_subsystem(sys: Subsystem, on: bool) {
    let bit = sys.bit();
    if on {
        SUBSYS_MASK.fetch_or(bit, Ordering::Relaxed);
    } else {
        SUBSYS_MASK.fetch_and(!bit, Ordering::Relaxed);
    }
}

/// Turn every subsystem on or off at once.
pub fn set_all_subsystems(on: bool) {
    SUBSYS_MASK.store(if on { u32::MAX } else { 0 }, Ordering::Relaxed);
}

pub fn subsystem_enabled(sys: Subsystem) -> bool {
    SUBSYS_MASK.load(Ordering::Relaxed) & sys.bit() != 0
}

/// Set the per-agent focus id (0 = no focus, all agents emit).
pub fn set_focus(id: u64) {
    FOCUS_ID.store(id, Ordering::Relaxed);
}

pub fn focus_id() -> u64 {
    FOCUS_ID.load(Ordering::Relaxed)
}

/// True if this agent id passes the focus filter: no focus set, or it matches.
#[inline]
pub fn focus_matches(id: u64) -> bool {
    let f = FOCUS_ID.load(Ordering::Relaxed);
    f == 0 || f == id
}

/// True when a line for this subsystem should actually be recorded.
#[inline]
fn should_record(sys: Subsystem) -> bool {
    is_enabled() && subsystem_enabled(sys)
}

/// Number of buffered trace lines.
pub fn len() -> usize {
    BUFFER.lock().map(|b| b.len()).unwrap_or(0)
}

/// Join the buffered lines into one newline-separated string (for export_trace).
pub fn export() -> String {
    match BUFFER.lock() {
        Ok(b) => b.iter().cloned().collect::<Vec<_>>().join("\n"),
        Err(_) => String::new(),
    }
}

/// Full reset of trace state: empties the buffer and zeroes the depth counter.
/// Called by `sim` on world regenerate/reset. Does not touch the enable flags.
pub fn clear() {
    if let Ok(mut b) = BUFFER.lock() {
        b.clear();
    }
    DEPTH.store(0, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Internal line writer
// ---------------------------------------------------------------------------

fn push_line(depth: usize, text: &str) {
    let line = format!("{}{}", " ".repeat(depth * INDENT_WIDTH), text);
    if let Ok(mut b) = BUFFER.lock() {
        if b.len() >= MAX_LINES {
            b.pop_front();
        }
        b.push_back(line);
    }
}

// ---------------------------------------------------------------------------
// RAII guard
// ---------------------------------------------------------------------------

/// Scope guard. Construct with `Tracer::enter`; the matching exit line is
/// written automatically when it drops (Rust's `Drop` is the C++ destructor).
/// Prefer the `trace_scope!` macro, which also compiles out entirely when the
/// `trace` feature is off.
pub struct Tracer {
    label:  &'static str,
    active: bool,
}

impl Tracer {
    /// Log entry to a scope and return a guard that logs exit on drop.
    /// Captures `active` at entry so enter/exit stay balanced even if the
    /// runtime switches are toggled mid-scope.
    pub fn enter(sys: Subsystem, label: &'static str) -> Tracer {
        let active = should_record(sys);
        if active {
            // fetch_add returns the prior depth: enter prints at that depth,
            // then depth is incremented for anything nested below.
            let depth = DEPTH.fetch_add(1, Ordering::Relaxed);
            push_line(depth, label);
        }
        Tracer { label, active }
    }
}

impl Drop for Tracer {
    fn drop(&mut self) {
        if self.active {
            // fetch_sub returns the prior depth; exit prints at depth-1 so it
            // lines up with the matching enter line.
            let prev = DEPTH.fetch_sub(1, Ordering::Relaxed);
            let depth = prev.saturating_sub(1);
            push_line(depth, &format!("{} exit", self.label));
        }
    }
}

/// Record a point-in-time event at the current call depth.
/// Prefer the `trace_event!` macro, which performs the runtime check BEFORE
/// `format!` so a disabled subsystem pays nothing to build the string.
pub fn event(sys: Subsystem, message: String) {
    if should_record(sys) {
        let depth = DEPTH.load(Ordering::Relaxed);
        push_line(depth, &message);
    }
}

// ---------------------------------------------------------------------------
// Macros — the public entry points. Gated on the `trace` feature: with it off
// they expand to nothing (no call site -> mechanism stripped by LTO). This is
// the exact role of a C `#ifdef TRACE` wrapper around a trace macro.
// ---------------------------------------------------------------------------

/// Open a traced scope. Use one per function, at the top:
/// `trace_scope!(utils::tracer::Subsystem::Plants, "update_plants");`
/// Expands to a guard bound to a hidden local that drops at end of scope.
#[cfg(feature = "trace")]
#[macro_export]
macro_rules! trace_scope {
    ($sys:expr, $label:expr) => {
        let _trace_guard = $crate::tracer::Tracer::enter($sys, $label);
    };
}

#[cfg(not(feature = "trace"))]
#[macro_export]
macro_rules! trace_scope {
    ($($t:tt)*) => {};
}

/// Record an event:
/// `trace_event!(Subsystem::Plants, "plant[{}] flood risk={:.4}", i, risk);`
/// The runtime check runs BEFORE `format!`, mirroring how the `log` crate's
/// macros avoid formatting when the level is disabled.
#[cfg(feature = "trace")]
#[macro_export]
macro_rules! trace_event {
    ($sys:expr, $($fmt:tt)*) => {
        if $crate::tracer::is_enabled() && $crate::tracer::subsystem_enabled($sys) {
            $crate::tracer::event($sys, format!($($fmt)*));
        }
    };
}

#[cfg(not(feature = "trace"))]
#[macro_export]
macro_rules! trace_event {
    ($($t:tt)*) => {};
}

/// Record a per-agent event, subject to the focus filter:
/// `trace_agent!(Subsystem::Carns, self.id, "carn#{} d={:.1}", self.id, d);`
/// The second argument is the agent id used for focus matching; the rest is a
/// normal `format!` payload. Records only when enabled, the subsystem is on,
/// AND the focus filter passes (no focus, or this id is the focused one). The
/// id is evaluated only after the cheaper enable/subsystem checks pass.
#[cfg(feature = "trace")]
#[macro_export]
macro_rules! trace_agent {
    ($sys:expr, $id:expr, $($fmt:tt)*) => {
        if $crate::tracer::is_enabled()
            && $crate::tracer::subsystem_enabled($sys)
            && $crate::tracer::focus_matches($id) {
            $crate::tracer::event($sys, format!($($fmt)*));
        }
    };
}

#[cfg(not(feature = "trace"))]
#[macro_export]
macro_rules! trace_agent {
    ($($t:tt)*) => {};
}

// ---------------------------------------------------------------------------
// Tests
//   API-level tests run with:   cargo test -p utils
//   Macro tests additionally need the feature: cargo test -p utils --features trace
// The tests share global tracer state and cargo runs them in parallel threads,
// so they serialize on TEST_LOCK and reset state at the top of each.
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn reset() {
        set_enabled(false);
        set_all_subsystems(false);
        set_focus(0);
        clear();
    }

    #[test]
    fn nested_scopes_indent_correctly() {
        let _g = TEST_LOCK.lock().unwrap();
        reset();
        set_enabled(true);
        set_subsystem(Subsystem::World, true);
        set_subsystem(Subsystem::Plants, true);

        {
            let _outer = Tracer::enter(Subsystem::World, "update_loop");
            {
                let _inner = Tracer::enter(Subsystem::Plants, "plant_update");
                event(Subsystem::Plants, "plant[847] flood check".to_string());
            } // _inner drops -> "  plant_update exit"
        } // _outer drops -> "update_loop exit"

        let expected = "update_loop\n  plant_update\n    plant[847] flood check\n  plant_update exit\nupdate_loop exit";
        assert_eq!(export(), expected);
    }

    #[test]
    fn disabled_records_nothing() {
        let _g = TEST_LOCK.lock().unwrap();
        reset(); // master stays off
        {
            let _t = Tracer::enter(Subsystem::World, "x");
            event(Subsystem::World, "y".to_string());
        }
        assert_eq!(len(), 0);
        assert_eq!(export(), "");
    }

    #[test]
    fn subsystem_gating_records_only_enabled() {
        let _g = TEST_LOCK.lock().unwrap();
        reset();
        set_enabled(true);
        set_subsystem(Subsystem::World, true); // Plants left OFF
        {
            let _w = Tracer::enter(Subsystem::World, "world_scope");
            let _p = Tracer::enter(Subsystem::Plants, "plant_scope"); // inert
            event(Subsystem::Plants, "ignored".to_string()); // inert
        }
        assert_eq!(export(), "world_scope\nworld_scope exit");
    }

    #[test]
    fn clear_resets_buffer_and_depth() {
        let _g = TEST_LOCK.lock().unwrap();
        reset();
        set_enabled(true);
        set_all_subsystems(true);
        {
            let _t = Tracer::enter(Subsystem::World, "scope");
            event(Subsystem::World, "evt".to_string());
        }
        assert!(len() > 0);
        clear();
        assert_eq!(len(), 0);
        // Depth back to 0: a fresh scope must start at column 0.
        {
            let _t = Tracer::enter(Subsystem::World, "again");
        }
        assert_eq!(export(), "again\nagain exit");
    }

    #[cfg(feature = "trace")]
    #[test]
    fn macros_expand_and_record() {
        let _g = TEST_LOCK.lock().unwrap();
        reset();
        set_enabled(true);
        set_all_subsystems(true);
        {
            crate::trace_scope!(Subsystem::World, "macro_scope");
            crate::trace_event!(Subsystem::Plants, "value={}", 42);
        }
        assert_eq!(export(), "macro_scope\n  value=42\nmacro_scope exit");
    }

    #[test]
    fn focus_matches_logic() {
        let _g = TEST_LOCK.lock().unwrap();
        reset();
        // No focus -> everything passes.
        assert!(focus_matches(5));
        assert!(focus_matches(999));
        // Focus set -> only the matching id passes.
        set_focus(7);
        assert!(focus_matches(7));
        assert!(!focus_matches(8));
        assert!(!focus_matches(0));
        set_focus(0); // clear so other tests are unaffected
    }

    #[cfg(feature = "trace")]
    #[test]
    fn trace_agent_respects_focus() {
        let _g = TEST_LOCK.lock().unwrap();
        reset();
        set_enabled(true);
        set_all_subsystems(true);
        set_focus(6);
        crate::trace_agent!(Subsystem::Carns, 6u64, "carn#{} hit", 6);
        crate::trace_agent!(Subsystem::Carns, 9u64, "carn#{} hit", 9); // filtered out
        set_focus(0);
        assert_eq!(export(), "carn#6 hit");
    }
}
