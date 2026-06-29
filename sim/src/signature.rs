// evo_sim - Copyright (c) 2026 Lens and Mix, LLC
// Licensed under the Apache License, Version 2.0. See LICENSE and NOTICE.
// More information: https://rkeithelliott.com
//! Deterministic state fingerprinting — FNV-1a 64-bit hasher.
//!
//! Used by `SimWorld::state_signature()` and the headless `tools/verify.cjs`
//! regression harness. Read-only, no dependencies. Floats are hashed via
//! `to_bits()` so the fingerprint is exact and bit-stable; the same seed run
//! for the same number of ticks always produces the same signature, and any
//! behavioral divergence shows up as a different signature within a few ticks
//! (a divergence propagates into the hashed positions/energies quickly).
//!
//! This captures CHANGE from a baseline (regression), not correctness.

const FNV_OFFSET: u64 = 0xcbf29ce484222325;
const FNV_PRIME:  u64 = 0x00000100000001B3;

pub struct Fnv(u64);

impl Fnv {
    #[inline]
    pub fn new() -> Self { Fnv(FNV_OFFSET) }

    /// Fold 8 bytes (little-endian) of a u64 into the hash, FNV-1a style.
    #[inline]
    pub fn u64(&mut self, v: u64) {
        let mut x = v;
        for _ in 0..8 {
            self.0 ^= x & 0xff;
            self.0 = self.0.wrapping_mul(FNV_PRIME);
            x >>= 8;
        }
    }

    #[inline]
    pub fn u32(&mut self, v: u32) { self.u64(v as u64); }

    /// Hash a float by its exact bit pattern.
    #[inline]
    pub fn f32(&mut self, v: f32) { self.u64(v.to_bits() as u64); }

    #[inline]
    pub fn bool(&mut self, v: bool) { self.u64(v as u64); }

    /// Hash an Option<usize> as (present-flag, value) so None and Some(0) differ.
    #[inline]
    pub fn opt_usize(&mut self, v: Option<usize>) {
        match v {
            Some(i) => { self.u64(1); self.u64(i as u64); }
            None    => { self.u64(0); self.u64(0); }
        }
    }

    #[inline]
    pub fn finish(&self) -> u64 { self.0 }

    /// 16-char zero-padded hex — the form the harness prints and diffs.
    pub fn hex(&self) -> String { format!("{:016x}", self.finish()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_and_sensitive() {
        // Same inputs → same hash.
        let mut a = Fnv::new(); a.u64(42); a.f32(1.5); a.bool(true);
        let mut b = Fnv::new(); b.u64(42); b.f32(1.5); b.bool(true);
        assert_eq!(a.finish(), b.finish());

        // A single differing bit in one float → different hash.
        let mut c = Fnv::new(); c.u64(42); c.f32(1.5000001); c.bool(true);
        assert_ne!(a.finish(), c.finish());

        // Order matters (catches reordered / dropped fields).
        let mut d = Fnv::new(); d.f32(1.5); d.u64(42); d.bool(true);
        assert_ne!(a.finish(), d.finish());

        // None vs Some(0) distinguished.
        let mut e = Fnv::new(); e.opt_usize(None);
        let mut f = Fnv::new(); f.opt_usize(Some(0));
        assert_ne!(e.finish(), f.finish());
    }
}
