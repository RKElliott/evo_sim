// evo_sim - Copyright (c) 2026 Lens and Mix, LLC
// Licensed under the Apache License, Version 2.0. See LICENSE and NOTICE.
// More information: https://rkeithelliott.com
//! genome_def.rs — Data-driven genome definition system.
//!
//! Organisms are defined by a GenomeDef that describes their traits.
//! The actual genome is a flat Vec<f32> of trait values — no hardcoded
//! field names. New organism types can be added without modifying Rust code.
//!
//! Design:
//!   - GenomeDef describes what traits exist and their constraints
//!   - Genome holds the actual f32 values + a reference to its definition
//!   - Budget system: all trait values sum to GenomeDef::budget
//!   - Sexual reproduction: crossover + mutation between two parent genomes
//!   - Asexual fallback: self-pollination with mutation only

use crate::rng::Rng;

// ---------------------------------------------------------------------------
// Trait definition
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct TraitDef {
    /// Human-readable name (e.g. "drought_tolerance")
    pub name:    &'static str,
    /// Minimum value after rebalance (prevents zeroing out a trait)
    pub min:     f32,
}

// ---------------------------------------------------------------------------
// Genome definition — describes an organism type
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct GenomeDef {
    /// Human-readable organism name
    pub name:   &'static str,
    /// Total budget all trait values must sum to
    pub budget: f32,
    /// Ordered list of trait definitions
    pub traits: Vec<TraitDef>,
}

impl GenomeDef {
    pub fn n_traits(&self) -> usize { self.traits.len() }

    pub fn default_value(&self) -> f32 {
        self.budget / self.n_traits() as f32
    }

    pub fn trait_name(&self, idx: usize) -> &'static str {
        self.traits[idx].name
    }

    pub fn trait_min(&self, idx: usize) -> f32 {
        self.traits[idx].min
    }
}

// ---------------------------------------------------------------------------
// Genome — flat trait value array
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Genome {
    pub values: Vec<f32>,
}

impl Genome {
    /// Create a genome near the budget-equal default with Gaussian noise.
    pub fn random(def: &GenomeDef, rng: &mut Rng) -> Self {
        let v   = def.default_value();
        let mut g = Genome {
            values: def.traits.iter()
                .map(|t| (v + rng.gauss() * 0.06).max(t.min))
                .collect()
        };
        g.rebalance(def);
        g
    }

    /// Asexual reproduction — mutation only.
    pub fn mutate(&self, def: &GenomeDef, rate: f32, rng: &mut Rng) -> Self {
        let mut g = Genome {
            values: self.values.iter().zip(def.traits.iter())
                .map(|(v, t)| (v + rng.gauss() * rate).max(t.min))
                .collect()
        };
        g.rebalance(def);
        g
    }

    /// Sexual reproduction — uniform crossover of two parents + mutation.
    /// Each trait is independently inherited from one parent or the other,
    /// then mutation is applied to the child.
    pub fn crossover(
        parent_a: &Genome,
        parent_b: &Genome,
        def:      &GenomeDef,
        rate:     f32,
        rng:      &mut Rng,
    ) -> Self {
        let values: Vec<f32> = parent_a.values.iter()
            .zip(parent_b.values.iter())
            .zip(def.traits.iter())
            .map(|((a, b), t)| {
                // 50/50 inheritance from each parent
                let inherited = if rng.f32() < 0.5 { *a } else { *b };
                // Then mutate
                (inherited + rng.gauss() * rate).max(t.min)
            })
            .collect();
        let mut g = Genome { values };
        g.rebalance(def);
        g
    }

    /// Rebalance so trait values sum to budget.
    pub fn rebalance(&mut self, def: &GenomeDef) {
        let sum: f32 = self.values.iter().sum();
        if sum > 0.0 {
            let scale = def.budget / sum;
            for (v, t) in self.values.iter_mut().zip(def.traits.iter()) {
                *v = (*v * scale).max(t.min);
            }
            // Second pass to fix budget after min clamping
            let sum2: f32 = self.values.iter().sum();
            if (sum2 - def.budget).abs() > 0.001 {
                let scale2 = def.budget / sum2;
                for v in self.values.iter_mut() {
                    *v *= scale2;
                }
            }
        }
    }

    pub fn get(&self, idx: usize) -> f32 {
        self.values[idx]
    }
}

// ---------------------------------------------------------------------------
// Plant genome definition — the single species for Phase 5
// ---------------------------------------------------------------------------

/// Trait indices for the standard plant genome.
/// These are named constants so code stays readable even though
/// the underlying genome is a flat Vec<f32>.
pub const PT_DROUGHT_TOLERANCE:   usize = 0;
pub const PT_FLOOD_TOLERANCE:     usize = 1;
pub const PT_NUTRIENT_EFFICIENCY: usize = 2;
pub const PT_GROWTH_RATE:         usize = 3;
pub const PT_SEED_COUNT:          usize = 4;
pub const PT_SEED_SPREAD:         usize = 5;
pub const PT_SIZE:                usize = 6;
pub const PT_SHADE_TOLERANCE:     usize = 7;

/// The standard plant GenomeDef.
/// Budget = 4.0, 8 traits, default value 0.5 each.
pub fn plant_genome_def() -> GenomeDef {
    GenomeDef {
        name:   "Standard Plant",
        budget: 4.0,
        traits: vec![
            TraitDef { name: "drought_tolerance",   min: 0.02 },
            TraitDef { name: "flood_tolerance",     min: 0.02 },
            TraitDef { name: "nutrient_efficiency", min: 0.02 },
            TraitDef { name: "growth_rate",         min: 0.02 },
            TraitDef { name: "seed_count",          min: 0.02 },
            TraitDef { name: "seed_spread",         min: 0.02 },
            TraitDef { name: "size",                min: 0.02 },
            TraitDef { name: "shade_tolerance",     min: 0.02 },
        ],
    }
}
