// evo_sim - Copyright (c) 2026 Lens and Mix, LLC
// Licensed under the Apache License, Version 2.0. See LICENSE and NOTICE.
// More information: https://rkeithelliott.com
//! genome.rs — Heritable traits for all organisms.
//!
//! Design principles:
//!   - Each trait is an f32 floored at 0.01; no per-trait upper cap
//!   - Traits draw from a shared budget — improving one costs another
//!   - Derived stats (actual game values) computed from trait values
//!   - No magic numbers — all scaling constants come from Config
//!   - Mutation adds Gaussian noise and re-floors at 0.01; rebalance() then
//!     normalizes the trait sum to the budget (avg ≈ budget/8 ≈ 0.375)

use crate::rng::Rng;

// ---------------------------------------------------------------------------
// Inspector serialization helper
// ---------------------------------------------------------------------------

/// Build the genome-portion JSON for the inspector: budget, raw trait values,
/// and derived phenotypes (each with a display range for the bar). Emitted
/// WITHOUT outer braces so lib.rs::inspect can wrap it with kind/id/state.
/// Keeping this here (beside the trait structs and their derived-stat methods)
/// means the inspector can never silently disagree with the sim, and the
/// eventual struct→GenomeDef refactor only has to touch this module.
fn describe_json(
    budget: f32,
    traits: &[(&str, f32)],
    phen:   &[(&str, f32, f32, f32, &str)],
) -> String {
    let mut s = String::new();
    s.push_str(&format!("\"budget\":{:.3},\"traits\":[", budget));
    for (i, (n, v)) in traits.iter().enumerate() {
        if i > 0 { s.push(','); }
        s.push_str(&format!("{{\"name\":\"{}\",\"value\":{:.4}}}", n, v));
    }
    s.push_str("],\"phenotypes\":[");
    for (i, (n, v, lo, hi, u)) in phen.iter().enumerate() {
        if i > 0 { s.push(','); }
        s.push_str(&format!(
            "{{\"name\":\"{}\",\"value\":{:.3},\"lo\":{:.3},\"hi\":{:.3},\"unit\":\"{}\"}}",
            n, v, lo, hi, u));
    }
    s.push(']');
    s
}

// ---------------------------------------------------------------------------
// Herbivore genome
// ---------------------------------------------------------------------------

/// Total trait budget for herbivores.
/// 8 traits at budget 3.0 — average value 0.375 each.
pub const HERB_TRAIT_BUDGET: f32 = 3.0;

#[derive(Clone, Debug)]
pub struct HerbGenome {
    pub speed:            f32,
    pub size:             f32,
    pub sight_range:      f32,
    pub eye_placement:    f32,
    pub efficiency:       f32,
    pub reproduction_rate:f32,
    /// Vigilance — time spent scanning for threats vs foraging.
    /// High = detects predators earlier, slight energy cost.
    /// Actively used: feeds the rest-energy cost and the carnivore stalk-vs-detect arms race.
    pub vigilance:        f32,
    /// Herd affinity — cohesion strength toward conspecifics.
    /// High = tight herds, better alarm propagation.
    pub herd_affinity:    f32,
}

impl HerbGenome {
    pub fn random(rng: &mut Rng) -> Self {
        let v = HERB_TRAIT_BUDGET / 8.0;
        let mut g = Self {
            speed:             (v + rng.gauss() * 0.06).max(0.01),
            size:              (v + rng.gauss() * 0.06).max(0.01),
            sight_range:       (v + rng.gauss() * 0.06).max(0.01),
            eye_placement:     (v + rng.gauss() * 0.06).max(0.01),
            efficiency:        (v + rng.gauss() * 0.06).max(0.01),
            reproduction_rate: (v + rng.gauss() * 0.06).max(0.01),
            vigilance:         (v + rng.gauss() * 0.06).max(0.01),
            herd_affinity:     (v + rng.gauss() * 0.06).max(0.01),
        };
        g.rebalance();
        g
    }

    pub fn mutate(&self, rate: f32, rng: &mut Rng) -> Self {
        let mut g = Self {
            speed:             (self.speed             + rng.gauss() * rate).max(0.01),
            size:              (self.size              + rng.gauss() * rate).max(0.01),
            sight_range:       (self.sight_range       + rng.gauss() * rate).max(0.01),
            eye_placement:     (self.eye_placement     + rng.gauss() * rate).max(0.01),
            efficiency:        (self.efficiency        + rng.gauss() * rate).max(0.01),
            reproduction_rate: (self.reproduction_rate + rng.gauss() * rate).max(0.01),
            vigilance:         (self.vigilance         + rng.gauss() * rate).max(0.01),
            herd_affinity:     (self.herd_affinity     + rng.gauss() * rate).max(0.01),
        };
        g.rebalance();
        g
    }

    fn rebalance(&mut self) {
        let sum = self.speed + self.size + self.sight_range
                + self.eye_placement + self.efficiency + self.reproduction_rate
                + self.vigilance + self.herd_affinity;
        if sum > 0.0 {
            let scale = HERB_TRAIT_BUDGET / sum;
            self.speed             *= scale;
            self.size              *= scale;
            self.sight_range       *= scale;
            self.eye_placement     *= scale;
            self.efficiency        *= scale;
            self.reproduction_rate *= scale;
            self.vigilance         *= scale;
            self.herd_affinity     *= scale;
        }
    }

    // -----------------------------------------------------------------------
    // Derived stats
    // -----------------------------------------------------------------------

    /// World-units per tick. Scaled by cfg.herb_speed_base and herb_speed_scale.
    pub fn move_speed(&self, cfg: &crate::config::Config) -> f32 {
        cfg.herb_speed_base + self.speed * cfg.herb_speed_scale
    }

    /// Max energy storage. Range: 80 – 240
    pub fn max_energy(&self) -> f32 { 80.0 + self.size * 240.0 }

    /// Personal space radius in world units. Range: 8 – 32
    pub fn personal_space(&self) -> f32 { 8.0 + self.size * 36.0 }

    /// Sight radius in world units. Range: 30 – 150
    /// Kept ecologically realistic so selection pressure on sight_range is meaningful.
    /// Herbivores evolve moderate range + wide arc; carnivores evolve long range + narrow arc.
    pub fn sight_radius(&self) -> f32 { 30.0 + self.sight_range * 120.0 }

    /// Field of view half-angle in radians.
    pub fn fov_half_angle(&self) -> f32 {
        let degrees = 150.0 - self.eye_placement * 100.0;
        degrees.to_radians()
    }

    /// Energy drain per tick when active.
    pub fn energy_drain(&self) -> f32 {
        0.03 + self.speed * 0.04 + self.sight_range * 0.01
    }

    /// Energy drain per tick when resting — reduced but meaningful.
    /// Vigilant animals burn a little more even at rest.
    /// Must be high enough that herbs need multiple meals per lifetime.
    pub fn rest_energy_drain(&self) -> f32 {
        (self.energy_drain() * 0.6 + self.vigilance * 0.008).max(0.025)
    }

    /// Energy fraction at which herbivore enters Rest. Range: 0.78 – 0.90
    pub fn satiation_threshold(&self) -> f32 {
        0.85 - self.reproduction_rate * 0.07
    }

    /// Energy fraction at which resting herbivore wakes to forage. Range: 0.55 – 0.70
    /// Raised so herbs wake up sooner and need to eat more frequently.
    pub fn hunger_threshold(&self) -> f32 {
        0.65 - self.efficiency * 0.10
    }

    /// Herd cohesion radius when resting. Range: 20 – 120 world units
    pub fn herd_cohesion_radius(&self) -> f32 {
        20.0 + self.herd_affinity * 100.0
    }

    /// Cohesion drift strength toward herd. Range: 0.0002 – 0.0012
    pub fn herd_cohesion_strength(&self) -> f32 {
        0.0002 + self.herd_affinity * 0.001
    }

    /// Energy gained per unit of plant energy_value consumed.
    pub fn extraction_rate(&self) -> f32 { 0.6 + self.efficiency * 1.30 }

    /// Energy fraction needed to reproduce.
    pub fn repro_threshold(&self) -> f32 { 0.90 - self.reproduction_rate * 0.15 }

    /// Reproduction cooldown in ticks. Range: 1800 – 3600
    /// At 60fps: 30s – 60s between offspring
    pub fn repro_cooldown(&self) -> u32 {
        (3600.0 - self.reproduction_rate * 1800.0).max(1800.0) as u32
    }

    /// Visual display radius for renderer. Range: 6 – 18
    pub fn visual_radius(&self) -> f32 { 6.0 + self.size * 18.0 }

    /// Energy cost of eating a plant.
    pub fn eat_energy_cost(&self, cfg: &crate::config::Config) -> f32 { cfg.herb_eat_energy_cost }

    /// Inspector description — raw traits + a curated set of derived phenotypes.
    pub fn describe(&self, cfg: &crate::config::Config) -> String {
        let traits: [(&str, f32); 8] = [
            ("speed",             self.speed),
            ("size",              self.size),
            ("sight_range",       self.sight_range),
            ("eye_placement",     self.eye_placement),
            ("efficiency",        self.efficiency),
            ("reproduction_rate", self.reproduction_rate),
            ("vigilance",         self.vigilance),
            ("herd_affinity",     self.herd_affinity),
        ];
        let move_lo = cfg.herb_speed_base;
        let move_hi = cfg.herb_speed_base + 0.8 * cfg.herb_speed_scale;
        let phen: [(&str, f32, f32, f32, &str); 8] = [
            ("Move speed",     self.move_speed(cfg),                move_lo, move_hi, "u/t"),
            ("Sight radius",   self.sight_radius(),                 30.0,    150.0,   "u"),
            ("FOV half-angle", self.fov_half_angle().to_degrees(),  70.0,    150.0,   "deg"),
            ("Max energy",     self.max_energy(),                   80.0,    240.0,   ""),
            ("Extraction",     self.extraction_rate(),              0.6,     1.6,     "x"),
            ("Rest drain",     self.rest_energy_drain(),            0.025,   0.10,    "/t"),
            ("Herd radius",    self.herd_cohesion_radius(),         20.0,    120.0,   "u"),
            ("Repro cooldown", self.repro_cooldown() as f32,        1800.0,  3600.0,  "t"),
        ];
        describe_json(HERB_TRAIT_BUDGET, &traits, &phen)
    }
}


// ---------------------------------------------------------------------------
// Carnivore genome
// ---------------------------------------------------------------------------

pub const CARN_TRAIT_BUDGET: f32 = 3.0;

#[derive(Clone, Debug)]
pub struct CarnGenome {
    /// Movement speed during chase. Higher = faster pursuit.
    pub speed:            f32,
    /// Body size. Larger = more energy capacity, bigger strike reach.
    pub size:             f32,
    /// Sensory range. Higher = detects prey from further away.
    pub sight_range:      f32,
    /// Eye placement. Forward (1.0) = narrow deep FOV for accurate striking.
    pub eye_placement:    f32,
    /// Stealth. Higher = prey detects carnivore at shorter range.
    /// Creates stalk-vs-detect arms race with herbivore vigilance.
    pub stealth:          f32,
    /// Strike power. Higher = more energy drained from prey per tick.
    pub strike_power:     f32,
    /// Stamina. Higher = longer sustained chase before tiring.
    pub stamina:          f32,
    /// Reproduction rate.
    pub reproduction_rate:f32,
}

impl CarnGenome {
    pub fn random(rng: &mut Rng) -> Self {
        let v = CARN_TRAIT_BUDGET / 8.0;
        let mut g = Self {
            speed:             (v + rng.gauss() * 0.06).max(0.01),
            size:              (v + rng.gauss() * 0.06).max(0.01),
            sight_range:       (v + rng.gauss() * 0.06).max(0.01),
            eye_placement:     (v + rng.gauss() * 0.06).max(0.01),
            stealth:           (v + rng.gauss() * 0.06).max(0.01),
            strike_power:      (v + rng.gauss() * 0.06).max(0.01),
            stamina:           (v + rng.gauss() * 0.06).max(0.01),
            reproduction_rate: (v + rng.gauss() * 0.06).max(0.01),
        };
        g.rebalance();
        g
    }

    pub fn mutate(&self, rate: f32, rng: &mut Rng) -> Self {
        let mut g = Self {
            speed:             (self.speed             + rng.gauss() * rate).max(0.01),
            size:              (self.size              + rng.gauss() * rate).max(0.01),
            sight_range:       (self.sight_range       + rng.gauss() * rate).max(0.01),
            eye_placement:     (self.eye_placement     + rng.gauss() * rate).max(0.01),
            stealth:           (self.stealth           + rng.gauss() * rate).max(0.01),
            strike_power:      (self.strike_power      + rng.gauss() * rate).max(0.01),
            stamina:           (self.stamina           + rng.gauss() * rate).max(0.01),
            reproduction_rate: (self.reproduction_rate + rng.gauss() * rate).max(0.01),
        };
        g.rebalance();
        g
    }

    fn rebalance(&mut self) {
        let sum = self.speed + self.size + self.sight_range
                + self.eye_placement + self.stealth + self.strike_power
                + self.stamina + self.reproduction_rate;
        if sum > 0.0 {
            let scale = CARN_TRAIT_BUDGET / sum;
            self.speed             *= scale;
            self.size              *= scale;
            self.sight_range       *= scale;
            self.eye_placement     *= scale;
            self.stealth           *= scale;
            self.strike_power      *= scale;
            self.stamina           *= scale;
            self.reproduction_rate *= scale;
        }
    }

    // -----------------------------------------------------------------------
    // Derived stats
    // -----------------------------------------------------------------------

    /// Chase speed. Scaled by cfg.carn_speed_base and carn_speed_scale.
    pub fn move_speed(&self, cfg: &crate::config::Config) -> f32 {
        cfg.carn_speed_base + self.speed * cfg.carn_speed_scale
    }

    /// Stalk speed. Scaled by cfg.carn_stalk_base and carn_stalk_scale.
    pub fn stalk_speed(&self, cfg: &crate::config::Config) -> f32 {
        cfg.carn_stalk_base + self.speed * cfg.carn_stalk_scale
    }

    /// Max energy. Range: 150 – 430 (higher — carnivores need reserve to search)
    pub fn max_energy(&self) -> f32 { 150.0 + self.size * 370.0 }

    /// Energy drain per tick when active.
    pub fn energy_drain(&self) -> f32 {
        0.12 + self.speed * 0.10 + self.sight_range * 0.03
    }

    /// Energy drain when resting — reduced but still significant.
    /// Large predators have high basal metabolic rates even at rest.
    /// Target: hunger returns after ~600-900 ticks (10-15s) of resting.
    pub fn rest_energy_drain(&self) -> f32 {
        self.energy_drain() * 0.55
    }

    /// Sight radius. Range: 200 – 600 — carnivores need long range on sparse world
    pub fn sight_radius(&self) -> f32 { 200.0 + self.sight_range * 550.0 }

    /// FOV half-angle. Forward-biased (predator). Range: 40° – 120°
    pub fn fov_half_angle(&self) -> f32 {
        let degrees = 120.0 - self.eye_placement * 80.0;
        degrees.to_radians()
    }

    /// Strike reach in world units. Range: 8 – 33
    pub fn strike_reach(&self) -> f32 { 8.0 + self.size * 25.0 }

    /// Energy drained from prey per tick while latched.
    pub fn drain_per_tick(&self) -> f32 { 0.8 + self.strike_power * 3.0 }

    /// Energy carnivore gains per unit drained from prey.
    pub fn extraction_rate(&self) -> f32 { 0.5 + self.stamina * 0.6 }

    /// How far stealth reduces detection — prey can only see carnivore
    /// within (1.0 - stealth * 0.7) * prey_sight_radius.
    pub fn stealth_factor(&self) -> f32 { 1.0 - self.stealth * 0.7 }

    /// Stamina ticks before chase speed degrades. Range: 120 – 600
    pub fn chase_stamina_ticks(&self) -> u32 {
        (120.0 + self.stamina * 480.0) as u32
    }

    /// Satiation threshold — enters rest above this. Range: 0.75 – 0.88
    pub fn satiation_threshold(&self) -> f32 {
        0.82 - self.reproduction_rate * 0.07
    }

    /// Hunger threshold — wakes from rest below this. Range: 0.40 – 0.55
    pub fn hunger_threshold(&self) -> f32 {
        0.48 - self.stamina * 0.08
    }

    /// Repro threshold — just below satiation so a fresh kill triggers reproduction.
    /// Range: 0.72 – 0.78  (satiation is 0.75-0.82)
    pub fn repro_threshold(&self) -> f32 { 0.76 - self.reproduction_rate * 0.04 }

    /// Repro cooldown. Range: 7200 – 14400 — slow enough to prevent prey overshoot
    pub fn repro_cooldown(&self) -> u32 {
        (14400.0 - self.reproduction_rate * 7200.0).max(7200.0) as u32
    }

    /// Visual radius for renderer. Range: 5 – 16
    pub fn visual_radius(&self) -> f32 { 5.0 + self.size * 16.0 }

    /// Personal space radius.
    pub fn personal_space(&self) -> f32 { 10.0 + self.size * 30.0 }

    /// Inspector description — raw traits + a curated set of derived phenotypes.
    pub fn describe(&self, cfg: &crate::config::Config) -> String {
        let traits: [(&str, f32); 8] = [
            ("speed",             self.speed),
            ("size",              self.size),
            ("sight_range",       self.sight_range),
            ("eye_placement",     self.eye_placement),
            ("stealth",           self.stealth),
            ("strike_power",      self.strike_power),
            ("stamina",           self.stamina),
            ("reproduction_rate", self.reproduction_rate),
        ];
        let move_lo  = cfg.carn_speed_base;
        let move_hi  = cfg.carn_speed_base + 0.8 * cfg.carn_speed_scale;
        let stalk_lo = cfg.carn_stalk_base;
        let stalk_hi = cfg.carn_stalk_base + 0.8 * cfg.carn_stalk_scale;
        let phen: [(&str, f32, f32, f32, &str); 8] = [
            ("Chase speed",        self.move_speed(cfg),               move_lo,  move_hi,  "u/t"),
            ("Stalk speed",        self.stalk_speed(cfg),              stalk_lo, stalk_hi, "u/t"),
            ("Sight radius",       self.sight_radius(),                200.0,    600.0,    "u"),
            ("Strike reach",       self.strike_reach(),                8.0,      33.0,     "u"),
            ("Chase stamina",      self.chase_stamina_ticks() as f32,  120.0,    600.0,    "t"),
            ("Max energy",         self.max_energy(),                  150.0,    430.0,    ""),
            ("Stealth (prey see)", self.stealth_factor(),              0.44,     1.0,      "x"),
            ("Energy drain",       self.energy_drain(),                0.12,     0.25,     "/t"),
        ];
        describe_json(CARN_TRAIT_BUDGET, &traits, &phen)
    }
}
