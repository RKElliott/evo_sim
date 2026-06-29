//! lib.rs — WASM entry point.
//!
//! Exposes SimWorld to JavaScript.
//! Simulation state: terrain + plants (phase 2).

mod config;
mod rng;
mod terrain;
mod genome;
mod plant;
mod occupancy;
mod herbivore;
mod spatial;
mod carnivore;
mod genome_def;
mod signature;

use wasm_bindgen::prelude::*;
use crate::plant::{Plant, PlantStage};
use crate::genome::HerbGenome;
use crate::rng::Rng;
use crate::occupancy::OccupancyGrid;
use crate::herbivore::Herbivore;
use crate::carnivore::Carnivore;
use crate::genome_def::plant_genome_def;

// Tracer lives in the utils crate. The trace_scope!/trace_event! macros are
// invoked by full path (utils::...), so no macro import is needed. The value
// paths (tracer::*, Subsystem) are only referenced by feature-gated code, so
// the import is gated too — keeping the default (non-traced) build warning-clean
// and free of any tracer linkage.
#[cfg(feature = "trace")]
use utils::tracer::{self, Subsystem};

#[wasm_bindgen]
pub struct SimWorld {
    terrain:         terrain::Terrain,
    cfg:             config::Config,
    frame:           u64,
    generation:      u64,
    rng:             Rng,
    plants:          Vec<Plant>,
    herbs:           Vec<Herbivore>,
    occupancy:       OccupancyGrid,
    plant_buf:       Vec<f32>,
    herb_buf:        Vec<f32>,
    herb_id_buf:     Vec<f32>,   // parallel to herb_buf: agent id per slot (stride 1)
    n_plants_active: u32,
    n_herbs_active:  u32,
    herb_grid:       spatial::SpatialGrid,
    plant_def:       genome_def::GenomeDef,   // private — not wasm-exposed
    herbs_enabled:   bool,
    carns_enabled:   bool,
    carns:           Vec<Carnivore>,
    carn_buf:        Vec<f32>,       // [x, y, state_code, visual_radius, vx, vy] stride 6
    carn_id_buf:     Vec<f32>,       // parallel to carn_buf: agent id per slot (stride 1)
    // Per-agent time-series logger: when tag_id != 0, one CSV row is recorded
    // each tick for that agent. Read-only — never affects dynamics.
    tag_id:          u64,
    tag_kind:        u8,            // 0=Any (herb-first), 1=Carn, 2=Herb
    tag_log:         Vec<String>,
    n_carns_active:  u32,
    carn_grid:       spatial::SpatialGrid,
    // Population history — recorded every 30 frames, private to Rust
    // Accessed via pop_history_data() method which flattens to Vec<u32> with stride 4
    pop_history: Vec<[u32; 4]>,  // [frame, plants, herbs, carns]

    // -----------------------------------------------------------------------
    // Per-agent trace — rolling buffer of last 60 frames per herb
    // Only populated when trace_enabled = true
    pub trace_enabled: bool,
    trace_buf: Vec<String>,   // flat list of trace lines

    // Diagnostic counters — reset each generation
    // -----------------------------------------------------------------------
    pub diag_eats:           u32,   // successful eat events this gen
    pub diag_starvations:    u32,   // herbivores died of energy depletion
    pub diag_age_death_sum:  u32,   // sum of ages at starvation death
    pub diag_plants_locked:  u32,   // plants with being_eaten=true (snapshot)
    pub diag_energy_sum:     f32,   // sum of all herb energies (for avg)
}

#[wasm_bindgen]
impl SimWorld {

    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        carnivore::reset_carn_ids();
        herbivore::reset_herb_ids();
        let cfg      = config::Config::default();
        let terrain  = terrain::Terrain::new(&cfg);
        let mut rng  = Rng::new(cfg.terrain_seed);
        let plants   = seed_initial_plants(&cfg, &terrain, &mut rng, &plant_genome_def());
        let herbs    = seed_initial_herbs(&cfg, &terrain, &mut rng);
        let buf_cap  = cfg.initial_plants * 6;
        let hbuf_cap = cfg.herb_initial_count * 6;
        let mut occ  = OccupancyGrid::new(
            cfg.terrain_cols, cfg.terrain_rows, cfg.plant_max_per_cell);

        for p in &plants {
            if let Some((col, row)) = cfg.world_to_cell(p.x, p.y) {
                occ.occupy(col, row);
            }
        }

        let world_w = cfg.world_w;
        let world_h = cfg.world_h;
        let carns    = seed_initial_carns(&cfg, &terrain, &mut rng);
        let cbuf_cap = cfg.carn_initial_count.max(10) * 8;
        let mut w = SimWorld {
            terrain, cfg, frame: 0, generation: 1,
            rng, plants, herbs, occupancy: occ,
            plant_buf:       vec![0.0_f32; buf_cap * 13],
            herb_buf:        vec![0.0_f32; hbuf_cap * 7],
            herb_id_buf:     Vec::new(),
            n_plants_active: 0,
            n_herbs_active:  0,
            pop_history:     Vec::with_capacity(2000),
            diag_eats:           0,
            diag_starvations:    0,
            diag_age_death_sum:  0,
            diag_plants_locked:  0,
            diag_energy_sum:     0.0,
            trace_enabled:       false,
            trace_buf:           Vec::with_capacity(600),
            herb_grid:       spatial::SpatialGrid::new(world_w, world_h, 80.0),
            plant_def:       plant_genome_def(),
            herbs_enabled:   true,
            carns_enabled:   true,
            carns,
            carn_buf:        vec![0.0_f32; cbuf_cap * 6],
            carn_id_buf:     Vec::new(),
            tag_id:          0,
            tag_kind:        1,
            tag_log:         Vec::new(),
            n_carns_active:  0,
            carn_grid:       spatial::SpatialGrid::new(world_w, world_h, 120.0),
        };
        w.write_plant_buffer();
        w.write_herb_buffer();
        w.write_carn_buffer();
        w
    }

    /// Dev world — small, flat, no water, guaranteed plants and herbivores.
    /// Used for behavioral testing with minimal environmental variables.
    pub fn dev_world() -> Self {
        carnivore::reset_carn_ids();
        herbivore::reset_herb_ids();
        let mut cfg             = config::Config::default();
        cfg.world_w             = 800.0;
        cfg.world_h             = 400.0;
        cfg.terrain_cols        = 64;
        cfg.terrain_rows        = 32;
        cfg.water_level         = 0.0;   // no water
        cfg.terrain_seed        = 42;
        cfg.initial_plants      = 50;
        cfg.herb_initial_count  = 20;
        cfg.nutrient_regen_rate = 0.002; // rich soil — plants always available
        cfg.nutrient_flow_rate  = 0.0;   // no flow — uniform nutrients
        cfg.plant_max_per_cell  = 3;

        let mut terrain  = terrain::Terrain::new(&cfg);
        // Override with perfectly uniform nutrients — no terrain-based variation
        for i in 0..terrain.nutrient.len() {
            if !terrain.water[i] {
                terrain.nutrient[i] = 0.8;
            }
        }
        // Also flatten elevation to remove island mask artifacts
        for i in 0..terrain.elevation.len() {
            terrain.elevation[i] = 0.5;
        }
        terrain.rebuild_render_buffer(cfg.hillshade_strength);
        let mut rng  = Rng::new(cfg.terrain_seed);
        let plants   = seed_initial_plants(&cfg, &terrain, &mut rng, &plant_genome_def());
        let herbs    = seed_initial_herbs(&cfg, &terrain, &mut rng);
        let buf_cap  = cfg.initial_plants * 8;
        let hbuf_cap = cfg.herb_initial_count * 6;
        let mut occ  = OccupancyGrid::new(
            cfg.terrain_cols, cfg.terrain_rows, cfg.plant_max_per_cell);
        for p in &plants {
            if let Some((col, row)) = cfg.world_to_cell(p.x, p.y) {
                occ.occupy(col, row);
            }
        }
        let mut w = SimWorld {
            terrain, cfg, frame: 0, generation: 1,
            rng, plants, herbs, occupancy: occ,
            plant_buf:       vec![0.0_f32; buf_cap * 13],
            herb_buf:        vec![0.0_f32; hbuf_cap * 7],
            herb_id_buf:     Vec::new(),
            n_plants_active: 0,
            n_herbs_active:  0,
            pop_history:     Vec::with_capacity(2000),
            diag_eats:           0,
            diag_starvations:    0,
            diag_age_death_sum:  0,
            diag_plants_locked:  0,
            diag_energy_sum:     0.0,
            trace_enabled:       false,
            trace_buf:           Vec::with_capacity(600),
            herb_grid:       spatial::SpatialGrid::new(800.0, 400.0, 40.0),
            plant_def:       plant_genome_def(),
            herbs_enabled:   true,
            carns_enabled:   true,
            carns:           Vec::new(),
            carn_buf:        vec![0.0_f32; 60],
            carn_id_buf:     Vec::new(),
            tag_id:          0,
            tag_kind:        1,
            tag_log:         Vec::new(),
            n_carns_active:  0,
            carn_grid:       spatial::SpatialGrid::new(800.0, 400.0, 80.0),
        };
        w.write_plant_buffer();
        w.write_herb_buffer();
        w.write_carn_buffer();
        w
    }

    pub fn with_seed(seed: u64) -> Self {
        carnivore::reset_carn_ids();
        herbivore::reset_herb_ids();
        let mut cfg      = config::Config::default();
        cfg.terrain_seed = seed;
        let terrain      = terrain::Terrain::new(&cfg);
        let mut rng      = Rng::new(seed);
        let plants       = seed_initial_plants(&cfg, &terrain, &mut rng, &plant_genome_def());
        let herbs        = seed_initial_herbs(&cfg, &terrain, &mut rng);
        let buf_cap      = cfg.initial_plants * 6;
        let hbuf_cap     = cfg.herb_initial_count * 6;  // stride 7, some headroom
        let mut occ      = OccupancyGrid::new(
            cfg.terrain_cols, cfg.terrain_rows, cfg.plant_max_per_cell);
        for p in &plants {
            if let Some((col, row)) = cfg.world_to_cell(p.x, p.y) {
                occ.occupy(col, row);
            }
        }
        let world_w    = cfg.world_w;
        let world_h    = cfg.world_h;
        let carns      = seed_initial_carns(&cfg, &terrain, &mut rng);
        let cbuf_cap   = cfg.carn_initial_count.max(10) * 8;
        let mut w = SimWorld {
            terrain, cfg, frame: 0, generation: 1,
            rng, plants, herbs, occupancy: occ,
            plant_buf:       vec![0.0_f32; buf_cap * 13],
            herb_buf:        vec![0.0_f32; hbuf_cap * 7],
            herb_id_buf:     Vec::new(),
            n_plants_active: 0,
            n_herbs_active:  0,
            pop_history:     Vec::with_capacity(2000),
            diag_eats:           0,
            diag_starvations:    0,
            diag_age_death_sum:  0,
            diag_plants_locked:  0,
            diag_energy_sum:     0.0,
            trace_enabled:       false,
            trace_buf:           Vec::with_capacity(600),
            herb_grid:       spatial::SpatialGrid::new(world_w, world_h, 80.0),
            plant_def:       plant_genome_def(),
            herbs_enabled:   true,
            carns_enabled:   true,
            carns,
            carn_buf:        vec![0.0_f32; cbuf_cap * 6],
            carn_id_buf:     Vec::new(),
            tag_id:          0,
            tag_kind:        1,
            tag_log:         Vec::new(),
            n_carns_active:  0,
            carn_grid:       spatial::SpatialGrid::new(world_w, world_h, 120.0),
        };
        w.write_plant_buffer();
        w.write_herb_buffer();
        w.write_carn_buffer();
        w
    }

    // -----------------------------------------------------------------------
    // Update
    // -----------------------------------------------------------------------

    pub fn update(&mut self) {
        utils::trace_scope!(Subsystem::World, "update_loop");
        self.frame += 1;
        self.terrain.update(&self.cfg);

        // Delayed carnivore spawn — wait until herbs have established and concentrated
        if self.carns.is_empty()
            && self.frame == self.cfg.carn_spawn_delay as u64
            && !self.herbs.is_empty() {
            let n = self.cfg.carn_initial_count;
            let r = self.cfg.carn_spawn_near_herb_r;
            let mut spawned = 0;
            let mut attempts = 0;
            while spawned < n && attempts < n * 30 {
                attempts += 1;
                // Pick a random living herb as anchor point
                let anchor_idx = (self.rng.f32() * self.herbs.len() as f32) as usize;
                let anchor_idx = anchor_idx.min(self.herbs.len() - 1);
                if !self.herbs[anchor_idx].alive { continue; }
                let ax = self.herbs[anchor_idx].x;
                let ay = self.herbs[anchor_idx].y;
                // Spawn carnivore within r world units of that herb
                let angle = self.rng.f32() * std::f32::consts::TAU;
                let dist  = self.rng.f32() * r;
                let x     = (ax + angle.cos() * dist).max(10.0).min(self.cfg.world_w - 10.0);
                let y     = (ay + angle.sin() * dist).max(10.0).min(self.cfg.world_h - 10.0);
                if !self.terrain.is_water_at_world(x, y, &self.cfg) {
                    let genome = genome::CarnGenome::random(&mut self.rng);
                    let mut c  = Carnivore::new(x, y, genome, &mut self.rng);
                c.repro_cd = (self.rng.f32() * 600.0) as u32;
                // Stagger ages so they don't all hit max_age simultaneously
                c.age      = (self.rng.f32() * (self.cfg.carn_max_age as f32 * 0.5)) as u32;
                    self.carns.push(c);
                    spawned += 1;
                }
            }
        }
        self.herb_grid.clear();
        for (i, h) in self.herbs.iter().enumerate() {
            if h.alive { self.herb_grid.insert(i, h.x, h.y); }
        }
        self.carn_grid.clear();
        for (i, c) in self.carns.iter().enumerate() {
            if c.alive { self.carn_grid.insert(i, c.x, c.y); }
        }

        // ── Carnivore detection by herbs ─────────────────────────────────────
        if self.carns_enabled {
        // Each herb checks nearby carnivores via carn_grid.
        // Detection range is reduced by carnivore stealth.
        // Detecting herbs enter Fleeing and broadcast alarm to herd.
        {
            let mut alarm_sources: Vec<(f32, f32, f32, f32)> = Vec::new();
            for h in self.herbs.iter_mut() {
                if !h.alive { continue; }
                let mut carn_scratch: Vec<usize> = Vec::new();
                self.carn_grid.query_radius(h.x, h.y, h.genome.sight_radius(), &mut carn_scratch);
                let mut detected = false;
                for &ci in &carn_scratch {
                    if ci >= self.carns.len() || !self.carns[ci].alive { continue; }
                    let c  = &self.carns[ci];
                    let dx = c.x - h.x;
                    let dy = c.y - h.y;
                    let d  = (dx*dx + dy*dy).sqrt();
                    let effective_range = h.genome.sight_radius() * c.genome.stealth_factor();
                    if d <= effective_range {
                        // Latched carnivore is occupied feeding — not an active chase threat
                        // Herbs within range of a latched carn should still flee briefly
                        // but don't maintain panic — allows non-targeted herbs to escape
                        let is_latched = c.state == carnivore::CarnState::Latched;
                        if !is_latched {
                            if h.state != crate::herbivore::HerbState::Fleeing {
                                h.flee_from_x = c.x;
                                h.flee_from_y = c.y;
                                utils::trace_agent!(Subsystem::Herbs, h.id, "herb#{} ->Fleeing carn#{} d={:.1} (broadcast alarm)", h.id, c.id, d);
                            }
                            h.state           = crate::herbivore::HerbState::Fleeing;
                            h.alarm_broadcast = true;
                            detected          = true;
                            alarm_sources.push((h.x, h.y, h.flee_from_x, h.flee_from_y));
                        } else if h.state == crate::herbivore::HerbState::Fleeing
                               || h.state == crate::herbivore::HerbState::Alarmed {
                            // Carnivore is latched — downgrade to brief Alarmed if currently Fleeing
                            #[cfg(feature = "trace")]
                            {
                                if h.state == crate::herbivore::HerbState::Fleeing {
                                    utils::trace_agent!(Subsystem::Herbs, h.id, "herb#{} Fleeing->Alarmed (carn#{} latched, downgrade)", h.id, c.id);
                                }
                            }
                            h.state      = crate::herbivore::HerbState::Alarmed;
                            h.alarmed_cd = h.alarmed_cd.min(120); // shorten remaining panic
                        }
                        break;
                    }
                }
                if !detected && h.state == crate::herbivore::HerbState::Fleeing {
                    if h.alarmed_cd == 0 {
                        h.alarmed_cd = 120;
                        h.state      = crate::herbivore::HerbState::Alarmed;
                        utils::trace_agent!(Subsystem::Herbs, h.id, "herb#{} Fleeing->Alarmed (threat left FOV, winddown)", h.id);
                    }
                }
            }
            // Propagate alarms to nearby herbs
            for (ax, ay, fx, fy) in alarm_sources {
                let mut alarm_scratch: Vec<usize> = Vec::new();
                self.herb_grid.query_radius(ax, ay, 150.0, &mut alarm_scratch);
                for &ni in &alarm_scratch {
                    if ni >= self.herbs.len() || !self.herbs[ni].alive { continue; }
                    let nh = &mut self.herbs[ni];
                    if nh.state != crate::herbivore::HerbState::Fleeing
                    && nh.state != crate::herbivore::HerbState::Alarmed {
                        let alarm_range = nh.genome.herd_cohesion_radius();
                        let dx = nh.x - ax; let dy = nh.y - ay;
                        let d  = (dx*dx + dy*dy).sqrt();
                        if d <= alarm_range {
                            nh.state       = crate::herbivore::HerbState::Alarmed;
                            nh.alarmed_cd  = (60.0 + nh.genome.herd_affinity * 180.0) as u32;
                            nh.flee_from_x = fx;
                            nh.flee_from_y = fy;
                            utils::trace_agent!(Subsystem::Herbs, nh.id, "herb#{} ->Alarmed (alarm propagation, d={:.1})", nh.id, d);
                        }
                    }
                }
            }
        }

        // ── Carnivore update loop ─────────────────────────────────────────────
        {
            utils::trace_scope!(Subsystem::Carns, "carn_update");
            let nc = self.carns.len();
            let mut new_carn_genomes: Vec<(f32, f32, genome::CarnGenome)> = Vec::new();
            let mut drain_targets: Vec<usize> = Vec::new();

            for i in 0..nc {
                if !self.carns[i].alive { continue; }
                let herbs_ref = unsafe {
                    std::slice::from_raw_parts(self.herbs.as_ptr(), self.herbs.len())
                };
                let (child_genome, drain_target) = self.carns[i].update(
                    herbs_ref,
                    &self.terrain,
                    &self.cfg,
                    &mut self.rng,
                    &self.herb_grid,
                    &mut Vec::new(),
                );
                if let Some(g) = child_genome {
                    let cx = (self.carns[i].x + self.rng.gauss() * 30.0)
                        .max(1.0).min(self.cfg.world_w - 1.0);
                    let cy = (self.carns[i].y + self.rng.gauss() * 30.0)
                        .max(1.0).min(self.cfg.world_h - 1.0);
                    new_carn_genomes.push((cx, cy, g));
                }
                if let Some(ti) = drain_target {
                    drain_targets.push(ti);
                }
            }

            // Apply sustained drain to latched herbs
            for ti in drain_targets {
                if ti >= self.herbs.len() || !self.herbs[ti].alive { continue; }
                let total_drain: f32 = self.carns.iter()
                    .filter(|c| c.alive
                        && c.state == carnivore::CarnState::Latched
                        && c.target_herb_idx == Some(ti))
                    .map(|c| c.genome.drain_per_tick())
                    .sum();
                if total_drain == 0.0 { continue; }
                self.herbs[ti].energy -= total_drain;
                for ci in 0..nc {
                    if self.carns[ci].alive
                    && self.carns[ci].state == carnivore::CarnState::Latched
                    && self.carns[ci].target_herb_idx == Some(ti) {
                        let gained = self.carns[ci].genome.drain_per_tick()
                            * self.carns[ci].genome.extraction_rate();
                        self.carns[ci].energy = (self.carns[ci].energy + gained)
                            .min(self.carns[ci].genome.max_energy());
                    }
                }
                if self.herbs[ti].energy <= 0.0 {
                    self.herbs[ti].alive = false;
                }
            }

            // Spawn new carnivores
            for (cx, cy, g) in new_carn_genomes {
                if !self.terrain.is_water_at_world(cx, cy, &self.cfg) {
                    self.carns.push(Carnivore::new(cx, cy, g, &mut self.rng));
                }
            }

            // Cull dead carnivores, grow buffer if needed
            self.carns.retain(|c| c.alive);
            let needed = self.carns.len() * 6;
            if needed > self.carn_buf.len() {
                self.carn_buf.resize(needed * 2, 0.0);
            }

            // Carnivore trace — all carns (usually small number)
            if self.trace_enabled && !self.carns.is_empty() {
                for (i, c) in self.carns.iter().enumerate() {
                    let nearest_herb = self.herbs.iter()
                        .filter(|h| h.alive)
                        .map(|h| ((h.x-c.x).powi(2) + (h.y-c.y).powi(2)).sqrt())
                        .fold(f32::INFINITY, f32::min);
                    // Also show distance to actual target if one exists
                    let target_dist = c.target_herb_idx
                        .and_then(|ti| self.herbs.get(ti))
                        .filter(|h| h.alive)
                        .map(|h| ((h.x-c.x).powi(2) + (h.y-c.y).powi(2)).sqrt());
                    let tgt_str = match target_dist {
                        Some(d) => format!("{:.1}wu", d),
                        None    => "none".to_string(),
                    };
                    self.trace_buf.push(format!(
                        "f:{} C:{} st:{:?} e:{:.1}/{:.0} pos:({:.0},{:.0}) nearest_h:{:.1}wu tgt:{:?} tgt_dist:{}",
                        self.frame, i, c.state,
                        c.energy, c.genome.max_energy(),
                        c.x, c.y,
                        nearest_herb,
                        c.target_herb_idx,
                        tgt_str
                    ));
                    if self.trace_buf.len() > 600 {
                        self.trace_buf.remove(0);
                    }
                }
            }
        }
        } // end carns_enabled

        // Canopy shade map — disabled pending plant spatial grid.
        // O(n²) at 10k+ plants is too expensive. Shade tolerance trait
        // is in the genome but all plants receive full sun for now.
        // TODO: add plant spatial grid and re-enable.
        {
        utils::trace_scope!(Subsystem::Plants, "plant_update");
        let shade_map = vec![0.0f32; self.plants.len()];

        // Update plants — collect new seeds
        let mut new_seeds: Vec<(f32, f32, genome_def::Genome)> = Vec::new();
        let n_plants = self.plants.len();
        for i in 0..n_plants {
            if !self.plants[i].is_alive() { continue; }
            let shade = shade_map[i];
            // Safety: we pass a read-only slice of all plants for pollen search
            let plants_ref = unsafe {
                std::slice::from_raw_parts(self.plants.as_ptr(), n_plants)
            };
            let seeds = self.plants[i].update(
                &self.terrain,
                &self.cfg,
                &mut self.rng,
                shade,
                plants_ref,
                &self.plant_def,
            );
            for genome in seeds {
                // Scatter seed position
                let angle = self.rng.f32() * std::f32::consts::TAU;
                let dist  = self.rng.f32() * self.plants[i].seed_spread(&self.cfg);
                let nx    = self.plants[i].x + angle.cos() * dist;
                let ny    = self.plants[i].y + angle.sin() * dist;
                // Discard out-of-bounds seeds
                if nx >= 1.0 && nx <= self.cfg.world_w - 1.0
                && ny >= 1.0 && ny <= self.cfg.world_h - 1.0 {
                    new_seeds.push((nx, ny, genome));
                }
            }
        }

        // Germinate new seeds
        for (x, y, genome) in new_seeds {
            if let Some((col, row)) = self.cfg.world_to_cell(x, y) {
                if !self.terrain.is_water(col, row)
                   && self.occupancy.can_occupy(col, row) {
                    self.occupancy.occupy(col, row);
                    self.plants.push(Plant::new_seed(x, y, genome));
                }
            }
        }

        // Release occupancy for dead plants, then cull
        for p in &self.plants {
            if !p.is_alive() {
                if let Some((col, row)) = self.cfg.world_to_cell(p.x, p.y) {
                    self.occupancy.release(col, row);
                }
            }
        }
        self.plants.retain(|p| p.is_alive());
        utils::trace_event!(Subsystem::Plants, "{} live plants after update", self.plants.len());
        }

        // Update herbivores — skipped if disabled
        if self.herbs_enabled {
        utils::trace_scope!(Subsystem::Herbs, "herb_update");
        // We need to separate the update loop from the herb slice due to Rust ownership.
        // Each herb gets a snapshot of positions from other herbs for boid forces.
        let n = self.herbs.len();
        let mut new_herb_genomes: Vec<(f32, f32, HerbGenome)> = Vec::new();
        let mut newly_eaten: Vec<usize>  = Vec::new();
        let mut fully_consumed: Vec<usize> = Vec::new();
        let mut scratch: Vec<usize> = Vec::with_capacity(64);  // reused each herb

        for i in 0..n {
            if !self.herbs[i].alive { continue; }

            let herbs_ref = unsafe {
                std::slice::from_raw_parts(
                    self.herbs.as_ptr(),
                    self.herbs.len(),
                )
            };

            let (child_genome, consumed) = self.herbs[i].update(
                &self.plants,
                herbs_ref,
                &self.terrain,
                &self.cfg,
                &mut self.rng,
                &self.herb_grid,
                &mut scratch,
            );

            if let Some(genome) = child_genome {
                // child_genome is now always None from update() —
                // reproduction handled below with density check
                let cx = (self.herbs[i].x + self.rng.gauss() * 20.0)
                    .max(1.0).min(self.cfg.world_w - 1.0);
                let cy = (self.herbs[i].y + self.rng.gauss() * 20.0)
                    .max(1.0).min(self.cfg.world_h - 1.0);
                new_herb_genomes.push((cx, cy, genome));
            }
            // Per-agent trace
            if self.trace_enabled && i < 5 {  // trace first 5 herbs only
                let h = &self.herbs[i];
                let nearest_plant = self.plants.iter()
                    .filter(|p| p.is_mature() && !p.being_eaten)
                    .map(|p| ((p.x - h.x).powi(2) + (p.y - h.y).powi(2)).sqrt())
                    .fold(f32::INFINITY, f32::min);
                self.trace_buf.push(format!(
                    "f:{} h:{} st:{:?} e:{:.1} pos:({:.0},{:.0}) v:({:.2},{:.2}) eat_t:{} nearest_p:{:.1}wu locked:{}",
                    self.frame, i, h.state, h.energy, h.x, h.y,
                    h.vx, h.vy, h.eat_timer,
                    nearest_plant,
                    self.plants.iter().filter(|p| p.being_eaten).count()
                ));
                if self.trace_buf.len() > 600 {
                    self.trace_buf.remove(0);
                }
            }

            if let Some(pi) = consumed {
                if pi < self.plants.len()
                   && self.plants[pi].is_alive()
                   && !self.plants[pi].being_eaten {
                    self.plants[pi].being_eaten = true;
                    newly_eaten.push(pi);
                    self.diag_eats += 1;
                }
            }

            // Tick consumed_fraction for plant this herb is currently eating
            if self.herbs[i].eat_timer > 0 {
                if let Some(pi) = self.herbs[i].eat_target_idx {
                    if pi < self.plants.len() && self.plants[pi].is_alive() {
                        let eat_total = (self.cfg.herb_base_eat_ticks as f32
                            + self.plants[pi].visual_radius()
                            * self.cfg.herb_eat_ticks_per_radius).max(1.0);
                        self.plants[pi].consumed_fraction =
                            1.0 - (self.herbs[i].eat_timer as f32 / eat_total);
                        if self.plants[pi].consumed_fraction >= 0.98 {
                            fully_consumed.push(pi);
                        }
                    }
                }
            }
        }

        // Finalize fully consumed plants
        fully_consumed.sort_unstable();
        fully_consumed.dedup();
        for pi in fully_consumed {
            if pi < self.plants.len() && self.plants[pi].is_alive() {
                self.plants[pi].stage = crate::plant::PlantStage::Dead;
                self.plants[pi].being_eaten = false;
                if let Some((col, row)) = self.cfg.world_to_cell(
                    self.plants[pi].x, self.plants[pi].y) {
                    // Return some nutrients to terrain when plant is consumed
                    let sz = self.plants[pi].genome.get(genome_def::PT_SIZE);
                    self.terrain.deposit_nutrient(col, row, sz * 0.02);
                }
            }
        }

        // Density-aware reproduction — separate pass after all updates
        // Uses the spatial grid rebuilt at start of this tick
        for i in 0..n {
            if !self.herbs[i].alive { continue; }
            let herbs_ref = unsafe {
                std::slice::from_raw_parts(self.herbs.as_ptr(), self.herbs.len())
            };
            if let Some(genome) = self.herbs[i].check_reproduce_density(
                &mut self.rng, &self.cfg, &self.herb_grid, herbs_ref, &mut scratch
            ) {
                let cx = (self.herbs[i].x + self.rng.gauss() * 20.0)
                    .max(1.0).min(self.cfg.world_w - 1.0);
                let cy = (self.herbs[i].y + self.rng.gauss() * 20.0)
                    .max(1.0).min(self.cfg.world_h - 1.0);
                new_herb_genomes.push((cx, cy, genome));
            }
        }

        // Spawn offspring
        for (cx, cy, genome) in new_herb_genomes {
            if !self.terrain.is_water_at_world(cx, cy, &self.cfg) {
                let child = Herbivore::new(cx, cy, genome, None, &mut self.rng);
                if self.trace_enabled {
                    self.trace_buf.push(format!(
                        "f:{} BIRTH h:{} pos:({:.0},{:.0}) total_herbs:{}",
                        self.frame, self.herbs.len(), cx, cy,
                        self.herbs.len() + 1
                    ));
                    if self.trace_buf.len() > 600 {
                        self.trace_buf.remove(0);
                    }
                }
                self.herbs.push(child);
            }
        }

        // -----------------------------------------------------------------------
        // Clear being_eaten flags for plants whose herbivore finished or died
        // This is the critical fix — without this plants stay locked permanently
        // -----------------------------------------------------------------------
        // Build set of plant indices still actively being eaten
        let mut active_eat_targets: std::collections::HashSet<usize> =
            std::collections::HashSet::new();
        for h in &self.herbs {
            if h.alive && h.eat_timer > 0 {
                if let Some(pi) = h.eat_target_idx {
                    active_eat_targets.insert(pi);
                }
            }
        }
        // Clear flag on any plant not in the active set
        for (i, p) in self.plants.iter_mut().enumerate() {
            if p.being_eaten && !active_eat_targets.contains(&i) {
                p.being_eaten = false;
            }
        }
        for h in &self.herbs {
            if !h.alive && h.energy <= 0.0 {
                self.diag_starvations   += 1;
                self.diag_age_death_sum += h.age;
            }
        }

        // Clear carnivore targets BEFORE culling herbs
        // herb.retain() reshuffles indices — any stored target_herb_idx becomes stale
        // Carnivores will re-acquire targets next tick via find_prey()
        // Log the tagged agent before target indices are invalidated / herbs culled.
        self.record_tagged();

        let any_dead_herbs = self.herbs.iter().any(|h| !h.alive);
        if any_dead_herbs {
            for c in self.carns.iter_mut() {
                c.target_herb_idx = None;
            }
        }

        // Remove dead herbs
        self.herbs.retain(|h| h.alive);
        } // end herbs_enabled

        // Generation counter
        if self.frame % 1200 == 0 {
            self.generation      += 1;
            // Reset per-generation counters
            self.diag_eats        = 0;
            self.diag_starvations = 0;
            self.diag_age_death_sum = 0;
        }

        // Grow buffers if needed
        let needed_p = self.plants.len() * 5;
        if needed_p > self.plant_buf.len() {
            self.plant_buf.resize(needed_p * 2, 0.0);
        }
        let needed_h = self.herbs.len() * 7;
        if needed_h > self.herb_buf.len() {
            self.herb_buf.resize(needed_h * 2, 0.0);
        }

        // Update snapshot diagnostics
        self.diag_plants_locked = self.plants.iter()
            .filter(|p| p.being_eaten).count() as u32;
        self.diag_energy_sum = self.herbs.iter()
            .map(|h| h.energy).sum();

        // Record population history every 30 frames
        const HIST_INTERVAL: u64 = 30;
        if self.frame % HIST_INTERVAL == 0 {
            let mature = self.plants.iter().filter(|p| p.is_mature()).count() as u32;
            self.pop_history.push([self.frame as u32, mature, self.herbs.len() as u32, self.carns.len() as u32]);
            // Keep last 4000 entries (~33 hours at 60fps with 30-frame interval)
            if self.pop_history.len() > 4000 {
                self.pop_history.remove(0);
            }
        }

        self.write_plant_buffer();
        self.write_herb_buffer();
        self.write_carn_buffer();
    }

    /// Set the agent whose per-tick metrics are logged (0 = off). Clears any
    /// existing log so each study starts fresh.
    pub fn set_tag(&mut self, id: u64, kind: u8) {
        self.tag_id   = id;
        self.tag_kind = kind;   // 0=Any (herb-first), 1=Carn, 2=Herb
        self.tag_log.clear();
    }

    /// Export the tagged agent's recorded time-series as CSV.
    pub fn export_agent_csv(&self) -> String {
        let mut s = String::from(
            "frame,generation,kind,id,x,y,state,energy,age,speed,alive,target_dist,alarmed_cd\n");
        for row in &self.tag_log {
            s.push_str(row);
            s.push('\n');
        }
        s
    }

    /// Inspector data for one organism, by kind + id (kind: 1=Carn, 2=Herb —
    /// matches the Tag kind convention). Returns a JSON object with the live
    /// snapshot (state/energy/age) plus the genome description (budget, raw
    /// traits, derived phenotypes). Read-only — never touches dynamics.
    pub fn inspect(&self, kind: u8, id: u64) -> String {
        match kind {
            1 => {
                if let Some(c) = self.carns.iter().find(|c| c.id == id) {
                    format!(
                        "{{\"kind\":\"carn\",\"id\":{},\"state\":\"{:?}\",\"energy\":{:.2},\"age\":{},{}}}",
                        c.id, c.state, c.energy, c.age, c.genome.describe(&self.cfg))
                } else {
                    String::from("{\"error\":\"carn not found\"}")
                }
            }
            2 => {
                if let Some(h) = self.herbs.iter().find(|h| h.id == id) {
                    format!(
                        "{{\"kind\":\"herb\",\"id\":{},\"state\":\"{:?}\",\"energy\":{:.2},\"age\":{},{}}}",
                        h.id, h.state, h.energy, h.age, h.genome.describe(&self.cfg))
                } else {
                    String::from("{\"error\":\"herb not found\"}")
                }
            }
            _ => String::from("{\"error\":\"unknown kind\"}"),
        }
    }

    /// Append one row for the tagged agent (if present). Read-only over sim
    /// state. MUST be called while carn target indices are still valid — i.e.
    /// before the per-tick target_herb_idx invalidation + herb cull.
    fn record_tagged(&mut self) {
        if self.tag_id == 0 { return; }
        let id = self.tag_id;
        let kind = self.tag_kind;   // 0=Any (herb-first), 1=Carn, 2=Herb
        let frame = self.frame;
        let gen = self.generation;
        // Herb match allowed unless Carn-only (kind 1). Produces an owned String,
        // so the borrow of self.herbs ends before the carn branch runs.
        let herb_row = if kind != 1 {
            self.herbs.iter().find(|h| h.id == id).map(|h| {
                let speed = (h.vx*h.vx + h.vy*h.vy).sqrt();
                format!("{},{},herb,{},{:.2},{:.2},{:?},{:.3},{},{:.4},{},,{}",
                    frame, gen, h.id, h.x, h.y, h.state, h.energy, h.age, speed,
                    h.alive as u8, h.alarmed_cd)
            })
        } else { None };
        let row = if herb_row.is_some() {
            herb_row
        } else if kind != 2 {
            // Carn match allowed unless Herb-only (kind 2).
            if let Some(idx) = self.carns.iter().position(|c| c.id == id) {
                let c = &self.carns[idx];
                let speed = (c.vx*c.vx + c.vy*c.vy).sqrt();
                let tdist = match c.target_herb_idx {
                    Some(ti) if ti < self.herbs.len() => {
                        let dx = c.x - self.herbs[ti].x;
                        let dy = c.y - self.herbs[ti].y;
                        format!("{:.2}", (dx*dx + dy*dy).sqrt())
                    }
                    _ => String::new(),
                };
                Some(format!("{},{},carn,{},{:.2},{:.2},{:?},{:.3},{},{:.4},{},{},",
                    frame, gen, c.id, c.x, c.y, c.state, c.energy, c.age, speed,
                    c.alive as u8, tdist))
            } else { None }
        } else {
            None
        };
        if let Some(row) = row {
            self.tag_log.push(row);
            if self.tag_log.len() >= 500_000 {
                self.tag_log.drain(0..250_000);
            }
        }
    }

    fn write_carn_buffer(&mut self) {
        let stride = 6usize;  // [x, y, state_code, visual_radius, vx, vy]
        let cap = self.carn_buf.len() / stride;
        if self.carn_id_buf.len() < cap { self.carn_id_buf.resize(cap, 0.0); }
        let n = self.carns.len().min(cap);
        for i in 0..n {
            let c = &self.carns[i];
            self.carn_buf[i*stride]     = c.x;
            self.carn_buf[i*stride + 1] = c.y;
            self.carn_buf[i*stride + 2] = c.state_code();
            self.carn_buf[i*stride + 3] = c.genome.visual_radius();
            self.carn_buf[i*stride + 4] = c.vx;
            self.carn_buf[i*stride + 5] = c.vy;
            self.carn_id_buf[i]         = c.id as f32;
        }
        for i in n..cap {
            for j in 0..stride { self.carn_buf[i*stride+j] = 0.0; }
            self.carn_id_buf[i] = 0.0;
        }
        self.n_carns_active = n as u32;
    }

    fn write_herb_buffer(&mut self) {
        // Layout: [x, y, state_code, sex_code, visual_radius, vx, vy] per herb
        let stride = 7usize;
        let cap = self.herb_buf.len() / stride;
        if self.herb_id_buf.len() < cap { self.herb_id_buf.resize(cap, 0.0); }
        let n = self.herbs.len().min(cap);
        for i in 0..n {
            let h = &self.herbs[i];
            self.herb_buf[i * stride]     = h.x;
            self.herb_buf[i * stride + 1] = h.y;
            self.herb_buf[i * stride + 2] = h.state_code();
            self.herb_buf[i * stride + 3] = h.sex_code();
            self.herb_buf[i * stride + 4] = h.genome.visual_radius();
            self.herb_buf[i * stride + 5] = h.vx;
            self.herb_buf[i * stride + 6] = h.vy;
            self.herb_id_buf[i]           = h.id as f32;
        }
        for i in n..cap {
            for j in 0..stride { self.herb_buf[i*stride+j] = 0.0; }
            self.herb_id_buf[i] = 0.0;
        }
        self.n_herbs_active = n as u32;
    }

    fn write_plant_buffer(&mut self) {
        // Layout: [x, y, stage, visual_radius, consumed_fraction, t0..t7] stride 13
        let stride = 13usize;
        let needed = self.plants.len() * stride;
        if needed > self.plant_buf.len() {
            self.plant_buf.resize(needed * 2, 0.0);
        }
        let n = self.plants.len().min(self.plant_buf.len() / stride);
        for i in 0..n {
            let p = &self.plants[i];
            self.plant_buf[i*stride]     = p.x;
            self.plant_buf[i*stride + 1] = p.y;
            self.plant_buf[i*stride + 2] = p.stage_code();
            self.plant_buf[i*stride + 3] = p.visual_radius();
            self.plant_buf[i*stride + 4] = p.consumed_fraction;
            // Trait values t0..t7
            for t in 0..8usize {
                self.plant_buf[i*stride + 5 + t] =
                    if t < p.genome.values.len() { p.genome.values[t] } else { 0.0 };
            }
        }
        for i in n..(self.plant_buf.len() / stride) {
            for j in 0..stride { self.plant_buf[i*stride+j] = 0.0; }
        }
        self.n_plants_active = n as u32;
    }

    // -----------------------------------------------------------------------
    // Shared buffer API — terrain
    // -----------------------------------------------------------------------

    pub fn terrain_buffer_ptr(&self) -> u32 {
        self.terrain.render_buffer_ptr() as u32
    }
    pub fn terrain_buffer_len(&self) -> u32 {
        self.terrain.render_buffer_len() as u32
    }

    // -----------------------------------------------------------------------
    // Shared buffer API — plants
    // Layout: [x, y, stage_code, visual_radius] per plant
    // stage_code: 0=seed, 1=seedling, 2=mature, 3=dead
    // -----------------------------------------------------------------------

    pub fn plant_buffer_ptr(&self) -> u32 {
        self.plant_buf.as_ptr() as u32
    }
    pub fn plant_buffer_len(&self) -> u32 {
        self.plant_buf.len() as u32
    }
    pub fn n_plants_active(&self) -> u32 {
        self.n_plants_active
    }

    // -----------------------------------------------------------------------
    // Shared buffer API — herbivores
    // Layout: [x, y, state_code, sex_code, visual_radius] per herb
    // -----------------------------------------------------------------------
    pub fn herb_buffer_ptr(&self) -> u32 { self.herb_buf.as_ptr() as u32 }
    pub fn herb_buffer_len(&self) -> u32 { self.herb_buf.len() as u32 }
    pub fn herb_id_buffer_ptr(&self) -> u32 { self.herb_id_buf.as_ptr() as u32 }
    pub fn herb_id_buffer_len(&self) -> u32 { self.herb_id_buf.len() as u32 }
    pub fn n_herbs_active(&self)  -> u32 { self.n_herbs_active }
    pub fn n_herbs_total(&self)   -> u32 { self.herbs.len() as u32 }

    // -----------------------------------------------------------------------
    // Stats
    // -----------------------------------------------------------------------

    pub fn terrain_cols(&self) -> u32  { self.cfg.terrain_cols as u32 }
    pub fn terrain_rows(&self) -> u32  { self.cfg.terrain_rows as u32 }
    pub fn world_w(&self)      -> f32  { self.cfg.world_w }
    pub fn world_h(&self)      -> f32  { self.cfg.world_h }
    pub fn frame(&self)        -> u64  { self.frame }
    pub fn generation(&self)   -> u64  { self.generation }

    pub fn n_plants_total(&self)    -> u32 { self.plants.len() as u32 }
    pub fn n_plants_seeds(&self)    -> u32 {
        self.plants.iter().filter(|p| p.stage == PlantStage::Seed).count() as u32
    }
    pub fn n_plants_seedlings(&self)-> u32 {
        self.plants.iter().filter(|p| p.stage == PlantStage::Seedling).count() as u32
    }
    pub fn n_plants_mature(&self)   -> u32 {
        self.plants.iter().filter(|p| p.stage == PlantStage::Mature).count() as u32
    }

    // -----------------------------------------------------------------------
    // Config setters
    // -----------------------------------------------------------------------

    pub fn set_water_level(&mut self, v: f32) {
        self.cfg.water_level = v.max(0.0).min(1.0);
        let water = self.terrain.elevation.iter()
            .map(|&e| e < self.cfg.water_level).collect();
        self.terrain.water = water;
        self.terrain.rebuild_render_buffer(self.cfg.hillshade_strength);

        // Drown any plants now sitting on flooded cells
        for p in self.plants.iter_mut() {
            if let Some((col, row)) = self.cfg.world_to_cell(p.x, p.y) {
                if self.terrain.is_water(col, row) && p.is_alive() {
                    p.stage = crate::plant::PlantStage::Dead;
                    self.occupancy.release(col, row);
                }
            }
        }
        self.plants.retain(|p| p.is_alive());
        self.write_plant_buffer();
    }
    pub fn set_nutrient_flow_rate(&mut self, v: f32) {
        self.cfg.nutrient_flow_rate = v.max(0.0).min(0.05);
    }
    pub fn set_hillshade_strength(&mut self, v: f32) {
        self.cfg.hillshade_strength = v.max(0.0).min(1.0);
        self.terrain.rebuild_render_buffer(self.cfg.hillshade_strength);
    }
    pub fn set_plants_evolve(&mut self, v: bool) {
        self.cfg.plants_evolve = v;
    }
    pub fn set_plant_mutation_rate(&mut self, v: f32) {
        self.cfg.plant_mutation_rate = v.max(0.0).min(0.30);
    }
    pub fn set_plant_seed_chance(&mut self, v: f32) {
        self.cfg.plant_seed_chance = v.max(0.0).min(1.0);
    }
    /// Population history as flat array [frame, plants, herbs, frame, plants, herbs, ...]
    pub fn pop_history_data(&self) -> Vec<u32> {
        self.pop_history.iter().flat_map(|e| e.iter().copied()).collect()
    }

    pub fn pop_history_len(&self) -> u32 {
        self.pop_history.len() as u32
    }

    // -----------------------------------------------------------------------
    // Diagnostic accessors
    // -----------------------------------------------------------------------
    pub fn diag_eats(&self)        -> u32 { self.diag_eats }
    pub fn diag_starvations(&self) -> u32 { self.diag_starvations }
    pub fn diag_avg_age_at_death(&self) -> u32 {
        if self.diag_starvations == 0 { 0 }
        else { self.diag_age_death_sum / self.diag_starvations }
    }
    pub fn diag_plants_locked(&self) -> u32 { self.diag_plants_locked }

    /// Snapshot of the current seed + all UI-tunable settings, as JSON.
    /// Sent on every world (re)load so the client can sync its controls to
    /// the world's actual state. Environment values flow world -> UI here;
    /// the herb/carn enable toggles are intentionally re-pushed UI -> world
    /// by the client (user intent persists across world loads).
    pub fn config_snapshot(&self) -> String {
        format!(
            "{{\"seed\":{},\"water_level\":{},\"hillshade_strength\":{},\"nutrient_flow_rate\":{},\"plant_seed_chance\":{},\"plant_mutation_rate\":{},\"plants_evolve\":{},\"herb_mutation_rate\":{},\"herbs_enabled\":{},\"carns_enabled\":{},\"trace_enabled\":{}}}",
            self.cfg.terrain_seed,
            self.cfg.water_level,
            self.cfg.hillshade_strength,
            self.cfg.nutrient_flow_rate,
            self.cfg.plant_seed_chance,
            self.cfg.plant_mutation_rate,
            self.cfg.plants_evolve,
            self.cfg.herb_mutation_rate,
            self.herbs_enabled,
            self.carns_enabled,
            self.trace_enabled,
        )
    }

    /// Export diagnostic log as CSV string.
    /// Columns: frame, generation, mature_plants, seedlings, seeds,
    ///          herbivores, eats_gen, starvations_gen, avg_age_death,
    ///          plants_locked, avg_herb_energy, nutrient_min, nutrient_max
    pub fn export_diag_csv(&self) -> String {
        let mut rows: Vec<String> = Vec::new();
        rows.push("frame,generation,mature_plants,seedlings,seeds,herbivores,carnivores,\
                   eats_gen,starvations_gen,avg_age_death,plants_locked,\
                   avg_herb_energy,nutrient_min,nutrient_max".to_string());

        let mature    = self.plants.iter().filter(|p| p.is_mature()).count();
        let seedlings = self.plants.iter()
            .filter(|p| p.stage == crate::plant::PlantStage::Seedling).count();
        let seeds     = self.plants.iter()
            .filter(|p| p.stage == crate::plant::PlantStage::Seed).count();

        let mut nmin = 1.0_f32;
        let mut nmax = 0.0_f32;
        for i in 0..self.terrain.cols * self.terrain.rows {
            if !self.terrain.water[i] {
                let n = self.terrain.nutrient[i];
                if n < nmin { nmin = n; }
                if n > nmax { nmax = n; }
            }
        }

        let avg_energy = if self.herbs.is_empty() { 0.0 }
            else { self.diag_energy_sum / self.herbs.len() as f32 };
        let avg_age = if self.diag_starvations == 0 { 0 }
            else { self.diag_age_death_sum / self.diag_starvations };

        rows.push(format!("{},{},{},{},{},{},{},{},{},{},{},{:.1},{:.3},{:.3}",
            self.frame, self.generation,
            mature, seedlings, seeds,
            self.herbs.len(),
            self.carns.len(),
            self.diag_eats, self.diag_starvations, avg_age,
            self.diag_plants_locked,
            avg_energy, nmin, nmax
        ));

        // Historical rows from pop_history
        for entry in &self.pop_history {
            rows.push(format!("{},{},{},,,{},{},,,,,,,",
                entry[0],
                entry[0] / 1200 + 1,
                entry[1],
                entry[2],
                entry[3],  // carnivores
            ));
        }

        rows.join("\n")
    }

    pub fn set_trace_enabled(&mut self, v: bool) {
        self.trace_enabled = v;
        if !v { self.trace_buf.clear(); }
        #[cfg(feature = "trace")]
        {
            tracer::set_enabled(v);
            tracer::set_all_subsystems(v);
            if !v { tracer::clear(); }
        }
    }

    /// Set the per-agent trace focus id (0 = no focus, all agents emit).
    /// Only meaningful in a traced build; a no-op otherwise. Note that herb and
    /// carn id spaces overlap, so a focus value can match both a herb and a carn
    /// bearing that number.
    pub fn set_trace_focus(&self, _id: u64) {
        #[cfg(feature = "trace")]
        tracer::set_focus(_id);
    }

    /// Export trace as newline-separated string.
    /// Traced build: drains the new tracer. Default build: the legacy trace_buf.
    #[cfg(feature = "trace")]
    pub fn export_trace(&self) -> String {
        tracer::export()
    }

    #[cfg(not(feature = "trace"))]
    pub fn export_trace(&self) -> String {
        self.trace_buf.join("\n")
    }

    #[cfg(feature = "trace")]
    pub fn trace_len(&self) -> u32 {
        tracer::len() as u32
    }

    #[cfg(not(feature = "trace"))]
    pub fn trace_len(&self) -> u32 {
        self.trace_buf.len() as u32
    }

    pub fn diag_avg_energy(&self) -> f32 {
        if self.herbs.is_empty() { 0.0 }
        else { self.diag_energy_sum / self.herbs.len() as f32 }
    }

    // Carnivore buffer API
    pub fn carn_buffer_ptr(&self) -> u32 { self.carn_buf.as_ptr() as u32 }
    pub fn carn_buffer_len(&self) -> u32 { self.carn_buf.len() as u32 }
    pub fn carn_id_buffer_ptr(&self) -> u32 { self.carn_id_buf.as_ptr() as u32 }
    pub fn carn_id_buffer_len(&self) -> u32 { self.carn_id_buf.len() as u32 }
    pub fn n_carns_active(&self)  -> u32 { self.n_carns_active }
    pub fn n_carns_total(&self)   -> u32 { self.carns.len() as u32 }

    pub fn set_herbs_enabled(&mut self, v: bool) { self.herbs_enabled = v; }
    pub fn set_carns_enabled(&mut self, v: bool) { self.carns_enabled = v; }

    pub fn set_herb_mutation_rate(&mut self, v: f32) {
        self.cfg.herb_mutation_rate = v.max(0.0).min(0.30);
    }

    /// Deterministic 64-bit fingerprint of the full persistent sim state, as
    /// a 16-char hex string. Read-only. Take it at a tick boundary (after
    /// `update()`), where transient per-tick flags are cleared. Used by the
    /// headless `tools/verify.cjs` regression / determinism harness.
    pub fn state_signature(&self) -> String {
        let mut h = signature::Fnv::new();
        h.u64(self.frame);
        h.u64(self.generation);
        h.u64(self.rng.state());
        h.u32(self.plants.len() as u32);
        h.u32(self.herbs.len()  as u32);
        h.u32(self.carns.len()  as u32);
        h.bool(self.herbs_enabled);
        h.bool(self.carns_enabled);
        for p in &self.plants { p.hash_into(&mut h); }
        for hb in &self.herbs { hb.hash_into(&mut h); }
        for c in &self.carns  { c.hash_into(&mut h); }
        h.hex()
    }

    pub fn regenerate(&mut self, seed: u64) {
        carnivore::reset_carn_ids();
        herbivore::reset_herb_ids();
        self.cfg.terrain_seed = seed;
        self.terrain    = terrain::Terrain::new(&self.cfg);
        let mut rng     = Rng::new(seed);
        self.plants     = seed_initial_plants(&self.cfg, &self.terrain, &mut rng, &self.plant_def);
        self.herbs      = seed_initial_herbs(&self.cfg, &self.terrain, &mut rng);
        self.rng        = rng;
        self.frame      = 0;
        self.generation = 1;
        self.tag_log.clear();
        self.occupancy.clear();
        for p in &self.plants {
            if let Some((col, row)) = self.cfg.world_to_cell(p.x, p.y) {
                self.occupancy.occupy(col, row);
            }
        }
        self.pop_history.clear();
        self.trace_buf.clear();
        #[cfg(feature = "trace")]
        tracer::clear();
        self.carns = Vec::new();  // will respawn after carn_spawn_delay ticks
        self.carn_grid.clear();
        self.diag_eats        = 0;
        self.diag_starvations = 0;
        self.diag_age_death_sum = 0;
        self.diag_plants_locked = 0;
        self.diag_energy_sum  = 0.0;
        self.write_plant_buffer();
        self.write_herb_buffer();
        self.write_carn_buffer();
    }
}

// ---------------------------------------------------------------------------
// Initial plant seeding — scatter generalist plants on fertile land
// ---------------------------------------------------------------------------

fn seed_initial_herbs(
    cfg:     &config::Config,
    terrain: &terrain::Terrain,
    rng:     &mut Rng,
) -> Vec<Herbivore> {
    let mut herbs = Vec::with_capacity(cfg.herb_initial_count);
    let mut attempts = 0;

    while herbs.len() < cfg.herb_initial_count && attempts < cfg.herb_initial_count * 20 {
        attempts += 1;
        let x = rng.range(10.0, cfg.world_w - 10.0);
        let y = rng.range(10.0, cfg.world_h - 10.0);
        if let Some((col, row)) = cfg.world_to_cell(x, y) {
            if !terrain.is_water(col, row) {
                let genome = genome::HerbGenome::random(rng);
                let mut h  = Herbivore::new(x, y, genome, None, rng);
                // Stagger initial ages so they don't all die simultaneously
                // Random age between 0 and half max_age
                h.age = (rng.f32() * (cfg.herb_max_age as f32 * 0.5)) as u32;
                // Also stagger repro cooldown so not all reproduce at once
                h.repro_cd = (rng.f32() * 1800.0) as u32;
                herbs.push(h);
            }
        }
    }
    herbs
}

fn seed_initial_carns(
    _cfg:     &config::Config,
    _terrain: &terrain::Terrain,
    _rng:     &mut Rng,
) -> Vec<Carnivore> {
    // Carnivores spawn later via delayed_spawn_carns() once herb population
    // is established — prevents immediate starvation in the early empty world
    Vec::new()
}

fn seed_initial_plants(
    cfg:     &config::Config,
    terrain: &terrain::Terrain,
    rng:     &mut Rng,
    def:     &genome_def::GenomeDef,
) -> Vec<Plant> {
    let mut plants = Vec::with_capacity(cfg.initial_plants);
    let mut attempts = 0;

    while plants.len() < cfg.initial_plants && attempts < cfg.initial_plants * 10 {
        attempts += 1;
        let x = rng.range(10.0, cfg.world_w - 10.0);
        let y = rng.range(10.0, cfg.world_h - 10.0);

        if let Some((col, row)) = cfg.world_to_cell(x, y) {
            if terrain.is_water(col, row) { continue; }
            let nutrient = terrain.nutrient_at(col, row);
            if nutrient > 0.15 {
                let genome = genome_def::Genome::random(def, rng);
                let mut p  = Plant::new_seed(x, y, genome);
                p.stage    = PlantStage::Mature;
                p.energy   = 0.8;
                plants.push(p);
            }
        }
    }
    plants
}
