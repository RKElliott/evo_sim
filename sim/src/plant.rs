//! plant.rs — Plant agent with data-driven genome and sexual reproduction.
//!
//! Lifecycle: Seed → Seedling → Mature → Dead
//!
//! Key behaviours:
//!   - Terrain-aware survival: flood/drought tolerance affect mortality
//!   - Canopy shadow: large plants reduce growth of nearby small plants
//!   - Sexual reproduction: pollen search within range, crossover + mutation
//!   - Self-pollination fallback when no donor found
//!   - Nutrient draw: plants consume terrain nutrients proportional to size

use crate::config::Config;
use crate::genome_def::{
    Genome, GenomeDef,
    PT_DROUGHT_TOLERANCE, PT_FLOOD_TOLERANCE, PT_NUTRIENT_EFFICIENCY,
    PT_GROWTH_RATE, PT_SEED_COUNT, PT_SEED_SPREAD, PT_SIZE, PT_SHADE_TOLERANCE,
};
use crate::rng::Rng;
use crate::terrain::Terrain;

#[cfg(feature = "trace")]
use utils::tracer::Subsystem;

// ---------------------------------------------------------------------------
// Plant lifecycle stage
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(u8)]
pub enum PlantStage {
    Seed     = 0,
    Seedling = 1,
    Mature   = 2,
    Dead     = 3,
}

// ---------------------------------------------------------------------------
// Plant
// ---------------------------------------------------------------------------

pub struct Plant {
    pub x:        f32,
    pub y:        f32,
    pub genome:   Genome,
    pub stage:    PlantStage,
    pub age:      u32,        // ticks since germination
    pub energy:   f32,        // internal energy reserve [0.0, 1.0]
    pub being_eaten: bool,    // locked by herbivore (kept for compatibility)
    pub consumed_fraction: f32, // 0.0 = full, 1.0 = fully eaten
}

impl Plant {
    pub fn new_seed(x: f32, y: f32, genome: Genome) -> Self {
        Plant {
            x, y, genome,
            stage:            PlantStage::Seed,
            age:              0,
            energy:           0.1,
            being_eaten:      false,
            consumed_fraction:0.0,
        }
    }

    pub fn is_mature(&self) -> bool  { self.stage == PlantStage::Mature }
    pub fn is_alive(&self)  -> bool  { self.stage != PlantStage::Dead  }

    /// Derived stats from genome
    pub fn maturation_ticks(&self, cfg: &Config) -> u32 {
        let gr = self.genome.get(PT_GROWTH_RATE);
        (cfg.plant_seedling_ticks as f32 / (cfg.plant_maturation_growth_base + gr * cfg.plant_maturation_growth_scale)) as u32
    }

    pub fn visual_radius(&self) -> f32 {
        let sz = self.genome.get(PT_SIZE);
        match self.stage {
            PlantStage::Seed     => 1.5,
            PlantStage::Seedling => 2.0 + sz * 3.0,
            PlantStage::Mature   => 4.0 + sz * 14.0,
            PlantStage::Dead     => 0.0,
        }
    }

    pub fn canopy_radius(&self) -> f32 {
        let sz = self.genome.get(PT_SIZE);
        4.0 + sz * 16.0  // shadow radius — larger plants shade more
    }

    pub fn pollen_range(&self, cfg: &Config) -> f32 {
        let ss = self.genome.get(PT_SEED_SPREAD);
        cfg.plant_pollen_range_base + ss * cfg.plant_pollen_range_scale  // local pollination — keeps gene flow regional
    }

    pub fn seed_spread(&self, cfg: &Config) -> f32 {
        let ss = self.genome.get(PT_SEED_SPREAD);
        cfg.plant_seed_dispersal_base + ss * cfg.plant_seed_dispersal_scale  // short dispersal promotes local adaptation
    }

    pub fn seeds_per_cycle(&self, cfg: &Config) -> u32 {
        let sc = self.genome.get(PT_SEED_COUNT);
        ((cfg.plant_base_seeds as f32) * (cfg.plant_seed_count_base + sc * cfg.plant_seed_count_scale)) as u32
    }

    pub fn nutrient_draw(&self) -> f32 {
        let sz  = self.genome.get(PT_SIZE);
        let gr  = self.genome.get(PT_GROWTH_RATE);
        0.0002 + sz * 0.0003 + gr * 0.0002
    }

    pub fn energy_value(&self, cfg: &Config) -> f32 {
        // Nutritional energy a herbivore gains from eating this plant.
        let sz = self.genome.get(PT_SIZE);
        cfg.plant_energy_value_base + sz * cfg.plant_energy_value_scale
    }

    /// Stage code for renderer buffer
    pub fn stage_code(&self) -> f32 { self.stage as u8 as f32 }

    // -----------------------------------------------------------------------
    // Per-tick update
    // Returns list of new seed genomes (position handled by caller)
    // -----------------------------------------------------------------------

    pub fn update(
        &mut self,
        terrain:    &Terrain,
        cfg:        &Config,
        rng:        &mut Rng,
        shade:      f32,        // 0.0 = full sun, 1.0 = full shade
        all_plants: &[Plant],   // for pollen search
        def:        &GenomeDef,
    ) -> Vec<Genome> {
        if self.stage == PlantStage::Dead { return Vec::new(); }

        self.age += 1;

        // Get terrain values at plant position
        let (nutrient, water_adj, elevation) = self.terrain_values(terrain, cfg);

        // ── Survival checks ──────────────────────────────────────────────

        // Flood check — water-adjacent cells kill flood-intolerant plants
        if water_adj {
            let ft   = self.genome.get(PT_FLOOD_TOLERANCE);
            let risk = (1.0 - ft) * cfg.plant_flood_death_rate;
            if rng.f32() < risk {
                self.stage = PlantStage::Dead;
                return Vec::new();
            }
        }

        // Drought check — low nutrient cells kill drought-intolerant plants
        if nutrient < cfg.plant_drought_threshold {
            let dt   = self.genome.get(PT_DROUGHT_TOLERANCE);
            let risk = (1.0 - dt) * cfg.plant_drought_death_rate
                     * (1.0 - nutrient / cfg.plant_drought_threshold);
            if rng.f32() < risk {
                self.stage = PlantStage::Dead;
                return Vec::new();
            }
        }

        // Shade check — low shade tolerance in full shade stunts growth
        let shade_tolerance = self.genome.get(PT_SHADE_TOLERANCE);
        let effective_shade = (shade - shade_tolerance).max(0.0);

        // ── Nutrient draw ────────────────────────────────────────────────
        // Plants consume terrain nutrients; this is handled in lib.rs
        // but we track internal energy here
        let ne         = self.genome.get(PT_NUTRIENT_EFFICIENCY);
        let energy_gain = nutrient * ne * (0.5 + 0.5 * (1.0 - effective_shade));
        self.energy    = (self.energy + energy_gain * cfg.plant_energy_gain_rate).min(1.0);

        // Elevation penalty — high elevation means harsh conditions
        if elevation > cfg.plant_highland_threshold {
            let penalty = (elevation - cfg.plant_highland_threshold) * cfg.plant_highland_penalty_slope;
            self.energy -= penalty * cfg.plant_highland_penalty_rate;
        }

        // Die if energy depleted
        if self.energy <= 0.0 {
            self.stage = PlantStage::Dead;
            return Vec::new();
        }

        // ── Age and stage progression ────────────────────────────────────
        let mut new_seeds: Vec<Genome> = Vec::new();

        match self.stage {
            PlantStage::Seed => {
                // Seeds germinate after a short delay
                if self.age > cfg.plant_germination_ticks {
                    // Germination probability based on nutrient and flood/drought
                    let germ_ok = nutrient > cfg.plant_germination_nutrient_min
                        && rng.f32() < cfg.plant_germination_chance;
                    if germ_ok {
                        self.stage = PlantStage::Seedling;
                        self.age   = 0;
                    } else if self.age > cfg.plant_germination_ticks * 3 {
                        self.stage = PlantStage::Dead; // failed to germinate
                    }
                }
            }

            PlantStage::Seedling => {
                let mat_ticks = self.maturation_ticks(cfg);
                let shade_slowdown = 1.0 + effective_shade * 2.0;
                if self.age as f32 > mat_ticks as f32 * shade_slowdown {
                    self.stage = PlantStage::Mature;
                    self.age   = 0;
                }
            }

            PlantStage::Mature => {
                // Age-based mortality
                let max_age  = cfg.plant_max_age as f32;
                let age_risk = (self.age as f32 / max_age).powi(2) * 0.001;
                if rng.f32() < age_risk {
                    self.stage = PlantStage::Dead;
                    return Vec::new();
                }

                // Seed production
                if self.age % cfg.plant_seed_interval == 0 && self.energy > cfg.plant_repro_energy_min {
                    let n_seeds = self.seeds_per_cycle(cfg);
                    for _ in 0..n_seeds {
                        if rng.f32() < cfg.plant_seed_chance {
                            // Find pollen donor for sexual reproduction
                            let donor = self.find_pollen_donor(all_plants, cfg, rng);
                            let child_genome = match donor {
                                Some(d) => {
                                    utils::trace_event!(Subsystem::Plants, "pollen donor d={:.1}",
                                        { let dx = d.x - self.x; let dy = d.y - self.y;
                                          (dx * dx + dy * dy).sqrt() });
                                    Genome::crossover(
                                        &self.genome, &d.genome, def,
                                        cfg.plant_mutation_rate, rng,
                                    )
                                },
                                None => self.genome.mutate(def, cfg.plant_mutation_rate, rng),
                            };
                            new_seeds.push(child_genome);
                        }
                    }
                    // Seed production costs energy
                    self.energy -= cfg.plant_seed_energy_cost * new_seeds.len() as f32;
                }
            }

            PlantStage::Dead => {}
        }

        new_seeds
    }

    /// Find a mature pollen donor within pollen_range.
    /// Prefers closer donors (more realistic — pollen concentration drops with distance).
    fn find_pollen_donor<'a>(&self, plants: &'a [Plant], cfg: &Config, rng: &mut Rng) -> Option<&'a Plant> {
        let range = self.pollen_range(cfg);
        let r2    = range * range;

        // Collect candidates
        let candidates: Vec<&Plant> = plants.iter()
            .filter(|p| {
                if !p.is_mature() || std::ptr::eq(*p, self) { return false; }
                let dx = p.x - self.x;
                let dy = p.y - self.y;
                dx*dx + dy*dy <= r2
            })
            .collect();

        if candidates.is_empty() { return None; }

        // Weight by inverse distance — closer donors more likely
        let weights: Vec<f32> = candidates.iter().map(|p| {
            let dx = p.x - self.x;
            let dy = p.y - self.y;
            let d  = (dx*dx + dy*dy).sqrt().max(1.0);
            1.0 / d
        }).collect();

        let total: f32 = weights.iter().sum();
        let mut pick  = rng.f32() * total;
        for (c, w) in candidates.iter().zip(weights.iter()) {
            pick -= w;
            if pick <= 0.0 { return Some(c); }
        }
        candidates.last().copied()
    }

    /// Get terrain values relevant to this plant's position.
    fn terrain_values(&self, terrain: &Terrain, cfg: &Config) -> (f32, bool, f32) {
        if let Some((col, row)) = cfg.world_to_cell(self.x, self.y) {
            let idx      = row * terrain.cols + col;
            let nutrient  = terrain.nutrient[idx];
            let water_adj = self.is_water_adjacent(terrain, col, row);
            let elevation = terrain.elevation[idx];
            (nutrient, water_adj, elevation)
        } else {
            (0.5, false, 0.5)
        }
    }

    /// Check if cell is adjacent to water (not necessarily underwater).
    fn is_water_adjacent(&self, terrain: &Terrain, col: usize, row: usize) -> bool {
        for dy in -1i32..=1 {
            for dx in -1i32..=1 {
                let nc = col as i32 + dx;
                let nr = row as i32 + dy;
                if nc < 0 || nr < 0
                || nc >= terrain.cols as i32
                || nr >= terrain.rows as i32 { continue; }
                if terrain.water[nr as usize * terrain.cols + nc as usize] {
                    return true;
                }
            }
        }
        false
    }
}

impl Plant {
    /// Fold this plant's persistent state into the signature hasher.
    /// (Used by SimWorld::state_signature for the verify harness.)
    pub fn hash_into(&self, h: &mut crate::signature::Fnv) {
        h.f32(self.x);
        h.f32(self.y);
        h.u32(self.stage as u32);
        h.u32(self.age);
        h.f32(self.energy);
        h.bool(self.being_eaten);
        h.f32(self.consumed_fraction);
        for v in &self.genome.values { h.f32(*v); }
    }
}
