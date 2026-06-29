// evo_sim - Copyright (c) 2026 Lens and Mix, LLC
// Licensed under the Apache License, Version 2.0. See LICENSE and NOTICE.
// More information: https://rkeithelliott.com
//! rng.rs — Deterministic pseudo-random number generator.
//!
//! Uses a simple xorshift64 LCG.
//! Deterministic: same seed always produces the same terrain.
//! No external dependencies.

#[allow(dead_code)]
pub struct Rng {
    state: u64,
}

#[allow(dead_code)]
impl Rng {
    pub fn new(seed: u64) -> Self {
        // Mix the seed so that seed=0 doesn't produce all zeros
        Self { state: seed ^ 0x9E3779B97F4A7C15 }
    }

    /// Current internal state — read-only, for deterministic fingerprinting
    /// (see signature.rs / verify.cjs). Does not advance the stream.
    pub fn state(&self) -> u64 { self.state }

    /// Advance state and return next u64
    pub fn next_u64(&mut self) -> u64 {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 7;
        self.state ^= self.state << 17;
        self.state
    }

    /// Uniform float in [0.0, 1.0)
    pub fn f32(&mut self) -> f32 {
        (self.next_u64() >> 11) as f32 / (1u64 << 53) as f32
    }

    /// Uniform float in [lo, hi)
    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + self.f32() * (hi - lo)
    }

    /// Approximate Gaussian via Box-Muller
    pub fn gauss(&mut self) -> f32 {
        let u = (self.f32() + 1e-10_f32).ln() * -2.0;
        let v = self.f32() * std::f32::consts::TAU;
        u.sqrt() * v.cos()
    }

    /// Deterministic 2D value noise in [-1.0, 1.0].
    /// Used as the primitive for terrain generation.
    /// ix, iy are integer grid coordinates.
    pub fn value_noise_2d(ix: i32, iy: i32, seed: u64) -> f32 {
        // Hash the grid coordinates into a deterministic float
        let mut h = seed;
        h ^= (ix as u64).wrapping_mul(0x9E3779B97F4A7C15);
        h ^= (iy as u64).wrapping_mul(0x6C62272E07BB0142);
        h ^= h >> 30;
        h = h.wrapping_mul(0xBF58476D1CE4E5B9);
        h ^= h >> 27;
        h = h.wrapping_mul(0x94D049BB133111EB);
        h ^= h >> 31;
        // Map to [-1.0, 1.0]
        (h >> 11) as f32 / (1u64 << 52) as f32 - 1.0
    }
}
