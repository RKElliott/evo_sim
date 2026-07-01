// evo_sim - Copyright (c) 2026 Lens and Mix, LLC
// Licensed under the Apache License, Version 2.0. See LICENSE and NOTICE.
// More information: https://rkeithelliott.com
//! config.rs — All simulation constants in one place.
//!
//! No magic numbers anywhere else in the codebase.
//! Pass a Config instance into every subsystem at construction.
//! Change a value here and it propagates everywhere.

/// Master configuration struct.
/// All fields are public so the TypeScript coordinator can read them
/// after construction for display in the UI.
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct Config {

    // -----------------------------------------------------------------------
    // World dimensions (world-space units)
    // -----------------------------------------------------------------------
    pub world_w: f32,
    pub world_h: f32,

    // -----------------------------------------------------------------------
    // Terrain grid
    // Fixed at 256×256 for initial testing.
    // terrain_cols and terrain_rows are configurable so scaling up later
    // is a one-line change.
    // Target cell size ~15 world units — adjust cols/rows proportionally
    // when changing world_w / world_h.
    // -----------------------------------------------------------------------
    pub terrain_cols: usize,
    pub terrain_rows: usize,

    // -----------------------------------------------------------------------
    // Terrain generation
    // -----------------------------------------------------------------------
    /// Seed for procedural noise. Different seeds = different worlds.
    pub terrain_seed: u64,

    /// Number of noise octaves. More octaves = more fine detail.
    /// 6 gives a good balance of large hills and local variation.
    pub noise_octaves: u32,

    /// Amplitude scaling between octaves (persistence).
    /// 0.5 = each octave contributes half as much as the previous.
    pub noise_persistence: f32,

    /// Frequency scaling between octaves (lacunarity).
    /// 2.0 = each octave has twice the frequency of the previous.
    pub noise_lacunarity: f32,

    // -----------------------------------------------------------------------
    // Water
    // -----------------------------------------------------------------------
    /// Elevation threshold below which a cell is water (0.0–1.0).
    /// 0.35 gives roughly 30% water coverage on typical noise terrain.
    pub water_level: f32,

    /// When true, terrain is a smooth wet→dry linear ramp (plant lab) instead
    /// of Perlin noise — low elevation on the left, high on the right, so the
    /// shoreline moves smoothly as water_level changes (seasons).
    pub terrain_flat_ramp: bool,

    // -----------------------------------------------------------------------
    // Plant settings
    // -----------------------------------------------------------------------

    /// Whether plants evolve (mutate on reproduction) or clone exactly.
    /// Toggle for controlled experiments.
    pub plants_evolve:        bool,

    /// Mutation rate for plant genomes (std dev of Gaussian noise).
    pub plant_mutation_rate:  f32,

    /// Probability per propagation attempt that a seed is cast.
    pub plant_seed_chance:    f32,

    /// Ticks a seed can wait for nutrients before dying ungerminated.
    pub seed_survival_ticks:  u32,

    /// Initial number of plants seeded at world start.
    pub initial_plants:       usize,

    /// Maximum mature plants allowed per terrain cell (occupancy limit).
    pub plant_max_per_cell:   u8,

    // ── Plant evolution (Phase 5) ─────────────────────────────────────────
    pub plant_flood_death_rate:         f32,
    pub plant_drought_threshold:        f32,
    pub plant_drought_death_rate:       f32,
    pub plant_highland_threshold:       f32,
    pub plant_germination_ticks:        u32,
    pub plant_germination_nutrient_min: f32,
    pub plant_germination_chance:       f32,
    pub plant_seed_interval:            u32,
    pub plant_base_seeds:               u32,
    pub plant_max_age:                  u32,
    pub plant_seedling_ticks:           u32,

    // Derived-stat scaling constants (lifted from plant.rs — Batch 3 cleanup):
    pub plant_pollen_range_base:        f32,
    pub plant_pollen_range_scale:       f32,
    pub plant_seed_dispersal_base:      f32,
    pub plant_seed_dispersal_scale:     f32,
    pub plant_maturation_growth_base:   f32,
    pub plant_maturation_growth_scale:  f32,
    pub plant_seed_count_base:          f32,
    pub plant_seed_count_scale:         f32,
    pub plant_energy_value_base:        f32,
    pub plant_energy_value_scale:       f32,
    pub plant_energy_gain_rate:         f32,
    pub plant_highland_penalty_slope:   f32,
    pub plant_highland_penalty_rate:    f32,
    pub plant_repro_energy_min:         f32,
    pub plant_seed_energy_cost:         f32,

    // ── Energy economy (Stage 4a) ─────────────────────────────────────────
    /// Energy cap for a plant = size_at_maturity * this (biomass ceiling).
    pub plant_energy_cap_scale:  f32,
    /// Baseline intake per tick (lets small plants get started).
    pub plant_intake_base:       f32,
    /// Intake per unit current biomass (bigger plants pull more).
    pub plant_intake_scale:      f32,
    /// Upkeep per unit current biomass — the metabolic headwind (key dial).
    pub plant_upkeep_rate:       f32,
    /// Energy at/below which a plant starves and dies.
    pub plant_starve_floor:      f32,
    /// Master switch for seed production (off in the plant lab so the energy
    /// loop can be tested closed; on in the main world).
    pub plant_reproduction_enabled: bool,

    // -----------------------------------------------------------------------
    // Herbivore settings
    // -----------------------------------------------------------------------
    pub herb_initial_count:   usize,
    pub herb_mutation_rate:   f32,
    pub herb_max_age:         u32,
    pub herb_min_repro_age:   u32,
    pub herb_satiation:       f32,
    pub herb_wander_speed:    f32,
    pub herb_forage_speed:    f32,
    pub herb_home_radius:     f32,
    pub herb_base_eat_ticks:  u32,
    pub herb_eat_ticks_per_radius: f32,
    /// Energy spent per tick while actively eating.
    pub herb_eat_energy_cost: f32,
    pub herb_restless_ticks:  u32,
    /// Max nearby herbs before reproduction is fully suppressed.
    /// Density-dependent reproduction — prevents unlimited population growth.
    pub herb_max_local_density: u32,
    /// Base movement speed added to speed trait contribution.
    pub herb_speed_base:        f32,
    /// Speed trait multiplier for herbivore movement.
    pub herb_speed_scale:       f32,
    /// Energy drain multiplier while actively fleeing a predator.
    pub herb_flee_drain_mult:   f32,
    /// Energy drain multiplier while alarmed (secondary flee response).
    pub herb_alarm_drain_mult:  f32,

    // -----------------------------------------------------------------------
    // Carnivore settings
    // -----------------------------------------------------------------------
    pub carn_initial_count:   usize,
    pub carn_spawn_delay:     u32,
    pub carn_spawn_near_herb_r: f32,
    /// Base movement speed for carnivores.
    pub carn_speed_base:        f32,
    /// Speed trait multiplier for carnivore chase movement.
    pub carn_speed_scale:       f32,
    /// Base stalk speed for carnivores.
    pub carn_stalk_base:        f32,
    /// Speed trait multiplier for carnivore stalk movement.
    pub carn_stalk_scale:       f32,
    pub carn_mutation_rate:   f32,
    pub carn_max_age:         u32,
    pub carn_min_repro_age:   u32,
    pub carn_wander_speed:    f32,
    pub carn_wander_drain_mult: f32,
    pub carn_stalk_speed_mult:f32,
    pub carn_chase_speed_mult:f32,
    pub carn_home_radius:     f32,
    pub carn_max_local_density: u32,
    /// Fraction of nutrients that flow to lower neighbors each tick.
    pub nutrient_flow_rate: f32,

    /// Fraction of nutrients that evaporate each tick.
    pub nutrient_decay_rate: f32,

    /// Background nutrient regeneration rate per land cell per tick.
    pub nutrient_regen_rate:  f32,

    /// Base nutrient value for a newly generated cell (0.0–1.0).
    /// Higher on low-elevation land, lower on highlands.
    pub nutrient_base_high_elev: f32,
    pub nutrient_base_low_elev:  f32,

    // -----------------------------------------------------------------------
    // Hillshading
    // -----------------------------------------------------------------------
    /// Strength of the slope-based edge brightening effect (0.0–1.0).
    /// 0.0 = flat colour, 1.0 = strong hillshade.
    pub hillshade_strength: f32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            world_w:               4000.0,
            world_h:               2000.0,

            terrain_cols:          256,
            terrain_rows:          256,

            terrain_seed:          12345,
            noise_octaves:         6,
            noise_persistence:     0.5,
            noise_lacunarity:      2.0,

            water_level:           0.35,
            terrain_flat_ramp:     false,

            nutrient_flow_rate:    0.002,
            nutrient_decay_rate:   0.0001,
            nutrient_base_high_elev: 0.2,
            nutrient_base_low_elev:  0.8,
            nutrient_regen_rate:   0.00020,  // bumped from 0.00015 — prevents slow long-term decline

            plants_evolve:        true,
            plant_mutation_rate:  0.06,
            plant_seed_chance:    0.4,
            seed_survival_ticks:  1200,
            initial_plants:       400,
            plant_max_per_cell:   2,
            plant_flood_death_rate:         0.004,
            plant_drought_threshold:        0.15,
            plant_drought_death_rate:       0.003,
            plant_highland_threshold:       0.72,
            plant_germination_ticks:        30,
            plant_germination_nutrient_min: 0.05,
            plant_germination_chance:       0.85,
            plant_seed_interval:            600,
            plant_base_seeds:               3,
            plant_max_age:                  18000,
            plant_seedling_ticks:           480,
            plant_pollen_range_base:        8.0,
            plant_pollen_range_scale:       40.0,
            plant_seed_dispersal_base:      5.0,
            plant_seed_dispersal_scale:     55.0,
            plant_maturation_growth_base:   0.3,
            plant_maturation_growth_scale:  1.4,
            plant_seed_count_base:          0.2,
            plant_seed_count_scale:         1.8,
            plant_energy_value_base:        25.0,
            plant_energy_value_scale:       75.0,
            plant_energy_gain_rate:         0.1,
            plant_highland_penalty_slope:   0.3,
            plant_highland_penalty_rate:    0.001,
            plant_repro_energy_min:         0.4,
            plant_seed_energy_cost:         0.01,
            plant_energy_cap_scale:         1.0,
            plant_intake_base:              0.010,
            plant_intake_scale:             0.040,
            plant_upkeep_rate:              0.010,
            plant_starve_floor:             0.001,
            plant_reproduction_enabled:     true,

            herb_initial_count:       30,
            herb_mutation_rate:       0.06,
            herb_max_age:             7200,
            herb_min_repro_age:       600,
            herb_satiation:           0.85,
            herb_wander_speed:        0.018,
            herb_forage_speed:        0.07,
            herb_home_radius:         150.0,
            herb_base_eat_ticks:      20,
            herb_eat_ticks_per_radius:1.5,
            herb_eat_energy_cost:     0.005,
            herb_restless_ticks:      240,
            herb_max_local_density:   12,
            herb_speed_base:          0.1,
            herb_speed_scale:         0.75,
            herb_flee_drain_mult:     2.5,
            herb_alarm_drain_mult:    1.4,

            carn_initial_count:       8,
            carn_spawn_delay:         3000,  // ticks before carnivores appear (herbs establish first)
            carn_spawn_near_herb_r:   200.0, // spawn within this radius of a random herb
            carn_speed_base:          0.2,
            carn_speed_scale:         1.2,
            carn_stalk_base:          0.05,
            carn_stalk_scale:         0.25,
            carn_mutation_rate:       0.06,
            carn_max_age:             28800,  // 8 minutes — enough to complete multiple hunts
            carn_min_repro_age:       3600,  // must survive 1 full generation before reproducing
            carn_wander_speed:        0.35,  // was 0.03 — carnivores must cover ground while searching
            carn_wander_drain_mult:   0.3,   // energy costs less while trotting than sprinting
            carn_stalk_speed_mult:    0.8,
            carn_chase_speed_mult:    1.4,  // chase speed — kept below 1.0 so chases are observable
            carn_home_radius:         300.0,
            carn_max_local_density:   4,

            hillshade_strength:    0.4,
        }
    }
}

#[allow(dead_code)]
impl Config {
    /// Cell width in world units.
    pub fn cell_w(&self) -> f32 {
        self.world_w / self.terrain_cols as f32
    }

    /// Cell height in world units.
    pub fn cell_h(&self) -> f32 {
        self.world_h / self.terrain_rows as f32
    }

    /// Total number of terrain cells.
    pub fn cell_count(&self) -> usize {
        self.terrain_cols * self.terrain_rows
    }

    /// Convert world coordinates to grid cell index (col, row).
    /// Returns None if the position is outside the world.
    pub fn world_to_cell(&self, x: f32, y: f32) -> Option<(usize, usize)> {
        if x < 0.0 || x >= self.world_w || y < 0.0 || y >= self.world_h {
            return None;
        }
        let col = (x / self.cell_w()) as usize;
        let row = (y / self.cell_h()) as usize;
        Some((
            col.min(self.terrain_cols - 1),
            row.min(self.terrain_rows - 1),
        ))
    }

    /// Linear index into flat terrain arrays from (col, row).
    pub fn idx(&self, col: usize, row: usize) -> usize {
        row * self.terrain_cols + col
    }
}
