// evo_sim - Copyright (c) 2026 Lens and Mix, LLC
// Licensed under the Apache License, Version 2.0. See LICENSE and NOTICE.
// More information: https://rkeithelliott.com
//! herbivore.rs — Herbivore agent: movement, boid steering, FOV, eating, reproduction.
//!
//! Key design principles:
//!   - Arc field of view — no animal has 360° vision
//!   - Boid steering — separation, cohesion, alignment + eating repulsion
//!   - Home base chosen during early life, not inherited
//!   - Soft personal space — repulsion force, not hard collision
//!   - Eating time proportional to plant visual_radius
//!   - Asexual reproduction with mutation (sex field reserved for Phase 4)

use crate::config::Config;
use crate::genome::HerbGenome;
use crate::plant::Plant;
use crate::rng::Rng;

#[cfg(feature = "trace")]
use utils::tracer::Subsystem;

// ---------------------------------------------------------------------------
// Herbivore state
// ---------------------------------------------------------------------------

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(u8)]
pub enum HerbState {
    Wandering = 0,
    Eating    = 1,
    Foraging  = 2,
    Sated     = 3,   // reserved
    Following = 4,
    Homing    = 5,
    Alarmed   = 6,   // heard alarm from nearby herb, fleeing without direct threat
    Resting   = 7,
    Fleeing   = 8,   // carnivore detected directly in FOV
}

// ---------------------------------------------------------------------------
// Sex — placeholder for Phase 4 sexual reproduction
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(u8)]
pub enum Sex { Male = 0, Female = 1 }

// ---------------------------------------------------------------------------
// Herbivore
// ---------------------------------------------------------------------------

static mut NEXT_HERB_ID: u64 = 0;
fn next_herb_id() -> u64 { unsafe { NEXT_HERB_ID += 1; NEXT_HERB_ID } }
/// Reset the herbivore id counter to 0. Called at world build / regenerate so
/// ids are stable and reproducible per world (same seed → same ids). Id is
/// excluded from state_signature, so this does not affect the harness.
pub(crate) fn reset_herb_ids() { unsafe { NEXT_HERB_ID = 0; } }

pub struct Herbivore {
    pub id:         u64,
    pub x:          f32,
    pub y:          f32,
    pub genome:     HerbGenome,
    pub energy:     f32,
    pub age:        u32,
    pub alive:      bool,
    pub state:      HerbState,
    pub sex:        Sex,          // reserved for Phase 4

    // Velocity (normalized direction × speed)
    pub vx:         f32,
    pub vy:         f32,

    // Home base
    home_x:         f32,
    home_y:         f32,
    home_established: bool,
    home_search_timer: u32,      // counts down from HOME_SEARCH_TICKS to 0

    // Eating
    pub eat_timer:      u32,
    pub eat_target_idx: Option<usize>,
    pub eat_energy:     f32,

    // Alarm system
    pub alarmed_cd:      u32,    // ticks remaining in Alarmed state
    pub flee_from_x:     f32,    // world position to flee away from
    pub flee_from_y:     f32,
    pub alarm_broadcast: bool,   // true this tick if we just detected threat

    // Reproduction
    pub repro_cd:   u32,

    // Parent tracking
    pub parent_id:  Option<u64>,
}

// Ticks before home base is established
const HOME_SEARCH_TICKS: u32 = 300;

impl Herbivore {
    pub fn new(x: f32, y: f32, genome: HerbGenome,
               parent_id: Option<u64>, rng: &mut Rng) -> Self {
        let energy   = genome.max_energy() * 0.5;  // start at 50% — must eat to reproduce
        let repro_cd = genome.repro_cooldown();
        let sex      = if rng.f32() < 0.5 { Sex::Male } else { Sex::Female };
        let angle    = rng.f32() * std::f32::consts::TAU;

        Self {
            id:               next_herb_id(),
            x, y,
            genome,
            energy,
            age:              0,
            alive:            true,
            state:            HerbState::Wandering,
            sex,
            vx:               angle.cos(),
            vy:               angle.sin(),
            home_x:           x,
            home_y:           y,
            home_established: false,
            home_search_timer:HOME_SEARCH_TICKS,
            eat_timer:        0,
            eat_target_idx:   None,
            eat_energy:       0.0,
            alarmed_cd:       0,
            flee_from_x:      0.0,
            flee_from_y:      0.0,
            alarm_broadcast:  false,
            repro_cd,
            parent_id,
        }
    }

    // -----------------------------------------------------------------------
    // Per-tick update
    // Returns (optional child genome, optional consumed plant index)
    // -----------------------------------------------------------------------

    pub fn update(
        &mut self,
        plants:     &[Plant],
        herbs:      &[Herbivore],
        terrain:    &crate::terrain::Terrain,
        cfg:        &Config,
        rng:        &mut Rng,
        grid:       &crate::spatial::SpatialGrid,
        scratch:    &mut Vec<usize>,
    ) -> (Option<HerbGenome>, Option<usize>) {
        if !self.alive { return (None, None); }

        #[cfg(feature = "trace")]
        let prev_state = self.state;

        self.age += 1;
        if self.repro_cd > 0 { self.repro_cd -= 1; }
        self.alarm_broadcast = false;
        if self.alarmed_cd > 0 { self.alarmed_cd -= 1; }

        // Home base establishment
        if !self.home_established {
            if self.home_search_timer > 0 {
                self.home_search_timer -= 1;
            } else {
                self.home_x           = self.x;
                self.home_y           = self.y;
                self.home_established = true;
            }
        }

        // Energy drain — increased when fleeing (sprinting is expensive)
        let drain = match self.state {
            HerbState::Resting  => self.genome.rest_energy_drain(),
            HerbState::Fleeing  => self.genome.energy_drain() * cfg.herb_flee_drain_mult,
            HerbState::Alarmed  => self.genome.energy_drain() * cfg.herb_alarm_drain_mult,
            _                   => self.genome.energy_drain(),
        };
        self.energy -= drain;
        if self.energy <= 0.0 || self.age >= cfg.herb_max_age {
            self.alive = false;
            return (None, None);
        }

        // ── Priority stack ──────────────────────────────────────────────────

        // 0. Fleeing — carnivore in direct FOV, highest priority
        if self.state == HerbState::Fleeing {
            let dx = self.x - self.flee_from_x;
            let dy = self.y - self.flee_from_y;
            let d  = (dx*dx + dy*dy).sqrt();
            if d > 0.0 { self.vx = dx/d; self.vy = dy/d; }
            // Full sprint speed — this is a life-or-death situation
            let speed = self.genome.move_speed(cfg);
            self.x += self.vx * speed;
            self.y += self.vy * speed;
            utils::trace_agent!(Subsystem::Herbs, self.id, "herb#{} FLEEING d_threat={:.1} speed={:.2}", self.id, d, speed);
            self.avoid_water(terrain, cfg);
            self.clamp(cfg);
            return (None, None);
        }

        // 0b. Alarmed — heard alarm from herd member
        if self.state == HerbState::Alarmed && self.alarmed_cd > 0 {
            let dx = self.x - self.flee_from_x;
            let dy = self.y - self.flee_from_y;
            let d  = (dx*dx + dy*dy).sqrt();
            if d > 0.0 { self.vx = dx/d; self.vy = dy/d; }
            // Alarmed herbs move at 70% sprint — not quite full panic
            let speed = self.genome.move_speed(cfg) * 0.7;
            self.x += self.vx * speed;
            self.y += self.vy * speed;
            utils::trace_agent!(Subsystem::Herbs, self.id, "herb#{} ALARMED d_threat={:.1} speed={:.2} cd={}", self.id, d, speed, self.alarmed_cd);
            self.avoid_water(terrain, cfg);
            self.clamp(cfg);
            return (None, None);
        } else if self.state == HerbState::Alarmed {
            self.state = HerbState::Wandering;
        }

        // 1. Eating (in progress) — herbivore is STATIONARY while eating
        if self.eat_timer > 0 {
            self.state     = HerbState::Eating;
            self.eat_timer -= 1;
            self.energy    -= self.genome.eat_energy_cost(cfg);
            // No movement, no scanning, no boid forces while eating
            if self.eat_timer == 0 {
                self.energy = (self.energy + self.eat_energy)
                    .min(self.genome.max_energy());
                self.eat_target_idx  = None;
                self.eat_energy      = 0.0;
            }
            self.clamp(cfg);
            return (None, None);  // reproduction handled in lib.rs with density check
        }

        // 2. Restless burst removed — replaced by wake-from-rest random kick.
        // Restlessness was a legacy mechanism that caused more confusion than it solved.
        // The wake-from-rest transition now provides the same "unstick" behavior.

        let sated = self.energy >= self.genome.max_energy() * self.genome.satiation_threshold();
        let mut consumed_plant: Option<usize> = None;

        if !sated {
            // 3. Find plant in FOV
            if let Some(pi) = self.find_plant(plants, cfg) {
                let px = plants[pi].x;
                let py = plants[pi].y;
                let d  = ((self.x - px).powi(2) + (self.y - py).powi(2)).sqrt();
                let reach = 4.0_f32;

                if d < reach {
                    let eat_ticks = (cfg.herb_base_eat_ticks as f32
                        + plants[pi].visual_radius()
                        * cfg.herb_eat_ticks_per_radius) as u32;
                    let plant_energy = plants[pi].energy_value(cfg);
                    self.eat_timer      = eat_ticks;
                    self.eat_energy     = plant_energy * self.genome.extraction_rate();
                    self.eat_target_idx = Some(pi);
                    self.state          = HerbState::Eating;
                    consumed_plant      = Some(pi);
                } else {
                    self.state = HerbState::Foraging;
                    let speed  = self.genome.move_speed(cfg) * cfg.herb_forage_speed;
                    self.move_toward(px, py, speed);
                }
            } else {
                // 4. Follow kin
                if let Some(ki) = self.find_kin(herbs, cfg, grid, scratch) {
                    self.state = HerbState::Following;
                    let tx     = herbs[ki].x;
                    let ty     = herbs[ki].y;
                    let speed  = self.genome.move_speed(cfg) * cfg.herb_wander_speed;
                    self.move_toward(tx, ty, speed);
                } else {
                    let dh = ((self.x - self.home_x).powi(2)
                            + (self.y - self.home_y).powi(2)).sqrt();
                    if self.home_established && dh > cfg.herb_home_radius {
                        self.state = HerbState::Homing;
                        let speed  = self.genome.move_speed(cfg) * cfg.herb_wander_speed;
                        self.move_toward(self.home_x, self.home_y, speed);
                    } else {
                        self.state = HerbState::Wandering;
                        let speed  = self.genome.move_speed(cfg) * cfg.herb_wander_speed;
                        self.wander(speed, rng);
                    }
                }
            }
        } else {
            // Sated — animal is not hungry.
            // If energy above satiation threshold, enter/stay in Resting.
            // Wake to forage only when energy drops below hunger_threshold.
            if self.energy >= self.genome.max_energy() * self.genome.hunger_threshold() {
                // Zero velocity on first entry to Resting — don't carry over forage/eat speed
                if self.state != HerbState::Resting {
                    self.vx = 0.0;
                    self.vy = 0.0;
                }
                self.state = HerbState::Resting;
                // Velocity decays toward zero — gradual settling
                self.vx *= 0.85;
                self.vy *= 0.85;
                let mag = (self.vx*self.vx + self.vy*self.vy).sqrt();
                if mag < 0.01 { self.vx = 0.0; self.vy = 0.0; }

                // Weak herd cohesion — drift toward nearest herd member
                self.apply_herd_cohesion_resting(herbs, grid, scratch);
                self.x += self.vx;
                self.y += self.vy;
            } else {
                // Hunger returned — transition out of rest
                self.state = HerbState::Wandering;
                // Give a small random velocity to start moving again
                if self.vx.abs() < 0.01 && self.vy.abs() < 0.01 {
                    let angle = rng.f32() * std::f32::consts::TAU;
                    self.vx = angle.cos();
                    self.vy = angle.sin();
                }
                let speed = self.genome.move_speed(cfg) * cfg.herb_wander_speed;
                self.wander(speed, rng);
            }
        }

        // Apply boid forces — suppressed while resting (cohesion handled separately above)
        // and suppressed while foraging (clean straight-line approach to food)
        if self.state != HerbState::Foraging && self.state != HerbState::Resting {
            self.apply_boid_forces(herbs, grid, scratch);
        } else if self.state == HerbState::Foraging {
            self.apply_separation(herbs, grid, scratch);
        }
        self.avoid_water(terrain, cfg);
        self.clamp(cfg);

        #[cfg(feature = "trace")]
        {
            if self.state != prev_state {
                utils::trace_agent!(Subsystem::Herbs, self.id, "herb#{} {:?}->{:?}", self.id, prev_state, self.state);
            }
        }

        (None, consumed_plant)  // reproduction handled in lib.rs with density check
    }

    // -----------------------------------------------------------------------
    // FOV scan — arc-based, not circular
    // -----------------------------------------------------------------------

    /// Find nearest mature plant within eating reach (no FOV restriction).
    /// Kept for potential future use (e.g. opportunistic eating triggers).
    #[allow(dead_code)]
    fn find_adjacent_plant(&self, plants: &[Plant]) -> Option<usize> {
        let reach = 4.0_f32;
        let mut best_idx = None;
        let mut best_d   = f32::INFINITY;
        for (i, p) in plants.iter().enumerate() {
            if !p.is_mature() || p.being_eaten { continue; }
            let d = ((self.x - p.x).powi(2) + (self.y - p.y).powi(2)).sqrt();
            if d < reach && d < best_d {
                best_idx = Some(i);
                best_d   = d;
            }
        }
        best_idx
    }

    /// Returns index of nearest mature plant within FOV arc.
    /// When velocity is near zero (stationary), scans omnidirectionally
    /// to prevent the undefined-facing-direction starvation bug.
    fn find_plant(&self, plants: &[Plant], cfg: &Config) -> Option<usize> {
        let _ = cfg;
        let sr    = self.genome.sight_radius();
        let fov   = self.genome.fov_half_angle();
        let speed = (self.vx*self.vx + self.vy*self.vy).sqrt();
        let omnidirectional = speed < 0.05;  // no facing direction when stationary

        let mut best_idx = None;
        let mut best_d   = f32::INFINITY;

        for (i, p) in plants.iter().enumerate() {
            if !p.is_mature() { continue; }
            if p.being_eaten  { continue; }
            let dx = p.x - self.x;
            let dy = p.y - self.y;
            let d  = (dx*dx + dy*dy).sqrt();
            if d > sr || d >= best_d { continue; }

            if omnidirectional {
                // Stationary — accept any plant within range regardless of direction
                best_idx = Some(i);
                best_d   = d;
            } else {
                // Moving — apply FOV arc check
                let angle_to = dy.atan2(dx);
                let facing   = self.vy.atan2(self.vx);
                let mut diff = (angle_to - facing).abs();
                if diff > std::f32::consts::PI { diff = std::f32::consts::TAU - diff; }
                if diff <= fov {
                    best_idx = Some(i);
                    best_d   = d;
                }
            }
        }
        best_idx
    }

    fn find_kin(&self, herbs: &[Herbivore], cfg: &Config,
                 grid: &crate::spatial::SpatialGrid,
                 scratch: &mut Vec<usize>) -> Option<usize> {
        let _ = cfg;
        let sr  = self.genome.sight_radius() * 0.6;
        let fov = self.genome.fov_half_angle();
        let speed = (self.vx*self.vx + self.vy*self.vy).sqrt();
        let omnidirectional = speed < 0.05;
        let mut best_idx = None;
        let mut best_d   = f32::INFINITY;

        grid.query_radius(self.x, self.y, sr, scratch);
        for &i in scratch.iter() {
            let h = &herbs[i];
            if std::ptr::eq(h, self) || !h.alive { continue; }
            if h.state == HerbState::Following { continue; }
            let dx = h.x - self.x;
            let dy = h.y - self.y;
            let d  = (dx*dx + dy*dy).sqrt();
            if d > sr || d >= best_d { continue; }

            let in_fov = if omnidirectional { true } else {
                let angle_to = dy.atan2(dx);
                let facing   = self.vy.atan2(self.vx);
                let mut diff = (angle_to - facing).abs();
                if diff > std::f32::consts::PI { diff = std::f32::consts::TAU - diff; }
                diff <= fov
            };

            if in_fov {
                let ed = if Some(h.id) == self.parent_id { d * 0.1 } else { d };
                if ed < best_d { best_idx = Some(i); best_d = ed; }
            }
        }
        best_idx
    }

    // -----------------------------------------------------------------------
    // Boid steering forces
    // -----------------------------------------------------------------------

    fn apply_herd_cohesion_resting(&mut self, herbs: &[Herbivore],
                                    grid: &crate::spatial::SpatialGrid,
                                    scratch: &mut Vec<usize>) {
        let speed = (self.vx*self.vx + self.vy*self.vy).sqrt();
        if speed > 0.05 { return; }

        let radius   = self.genome.herd_cohesion_radius();
        let strength = self.genome.herd_cohesion_strength();
        let mut nearest_d = f32::INFINITY;
        let mut nearest_x = self.x;
        let mut nearest_y = self.y;

        grid.query_radius(self.x, self.y, radius, scratch);
        for &i in scratch.iter() {
            let h = &herbs[i];
            if std::ptr::eq(h, self) || !h.alive { continue; }
            let dx = h.x - self.x;
            let dy = h.y - self.y;
            let d  = (dx*dx + dy*dy).sqrt();
            if d < radius && d < nearest_d {
                nearest_d = d;
                nearest_x = h.x;
                nearest_y = h.y;
            }
        }

        if nearest_d < f32::INFINITY {
            self.vx += (nearest_x - self.x) * strength;
            self.vy += (nearest_y - self.y) * strength;
            let new_speed = (self.vx*self.vx + self.vy*self.vy).sqrt();
            let max_rest  = 0.08;
            if new_speed > max_rest {
                self.vx = self.vx / new_speed * max_rest;
                self.vy = self.vy / new_speed * max_rest;
            }
        }
    }

    fn apply_boid_forces(&mut self, herbs: &[Herbivore],
                          grid: &crate::spatial::SpatialGrid,
                          scratch: &mut Vec<usize>) {
        self.apply_separation(herbs, grid, scratch);
        if matches!(self.state, HerbState::Wandering | HerbState::Following) {
            self.apply_cohesion_alignment(herbs, grid, scratch);
        }
    }

    fn apply_separation(&mut self, herbs: &[Herbivore],
                         grid: &crate::spatial::SpatialGrid,
                         scratch: &mut Vec<usize>) {
        let ps = self.genome.personal_space();
        let mut fx = 0.0_f32;
        let mut fy = 0.0_f32;
        let mut count = 0u32;

        grid.query_radius(self.x, self.y, ps, scratch);
        for &i in scratch.iter() {
            let h = &herbs[i];
            if std::ptr::eq(h, self) || !h.alive { continue; }
            let dx = self.x - h.x;
            let dy = self.y - h.y;
            let d  = (dx*dx + dy*dy).sqrt();
            if d < ps && d > 0.0 {
                let strength = if h.state == HerbState::Eating { 4.5 } else { 1.0 };
                fx += (dx / d) * (1.0 - d / ps) * strength;
                fy += (dy / d) * (1.0 - d / ps) * strength;
                count += 1;
            }
        }

        if count > 0 {
            self.vx += fx * 0.15;
            self.vy += fy * 0.15;
            self.norm();
        }
    }

    fn apply_cohesion_alignment(&mut self, herbs: &[Herbivore],
                                  grid: &crate::spatial::SpatialGrid,
                                  scratch: &mut Vec<usize>) {
        let sr = self.genome.sight_radius() * 0.4;
        let mut cx = 0.0_f32; let mut cy = 0.0_f32;
        let mut ax = 0.0_f32; let mut ay = 0.0_f32;
        let mut count = 0u32;

        grid.query_radius(self.x, self.y, sr, scratch);
        for &i in scratch.iter() {
            let h = &herbs[i];
            if std::ptr::eq(h, self) || !h.alive { continue; }
            let dx = h.x - self.x;
            let dy = h.y - self.y;
            let d  = (dx*dx + dy*dy).sqrt();
            if d < sr {
                cx += h.x; cy += h.y;
                ax += h.vx; ay += h.vy;
                count += 1;
            }
        }

        if count > 0 {
            let n = count as f32;
            self.vx += (cx / n - self.x) * 0.0005;
            self.vy += (cy / n - self.y) * 0.0005;
            self.vx += ax / n * 0.02;
            self.vy += ay / n * 0.02;
            self.norm();
        }
    }

    // -----------------------------------------------------------------------
    // Movement helpers
    // -----------------------------------------------------------------------

    fn avoid_water(&mut self, terrain: &crate::terrain::Terrain, cfg: &Config) {
        // If currently on water, push back toward land immediately
        if let Some((col, row)) = cfg.world_to_cell(self.x, self.y) {
            if terrain.is_water(col, row) {
                // Reverse velocity and move back
                self.vx = -self.vx;
                self.vy = -self.vy;
                self.x += self.vx * self.genome.move_speed(cfg) * 2.0;
                self.y += self.vy * self.genome.move_speed(cfg) * 2.0;
                return;
            }
        }

        // Lookahead — if next step would land in water, steer away
        let next_x = self.x + self.vx * self.genome.move_speed(cfg) * 3.0;
        let next_y = self.y + self.vy * self.genome.move_speed(cfg) * 3.0;
        if let Some((col, row)) = cfg.world_to_cell(next_x, next_y) {
            if terrain.is_water(col, row) {
                // Deflect perpendicular to current direction
                let perp_x =  self.vy;
                let perp_y = -self.vx;
                self.vx = perp_x;
                self.vy = perp_y;
                self.norm();
            }
        }
    }

    fn move_toward(&mut self, tx: f32, ty: f32, speed: f32) {
        let dx = tx - self.x;
        let dy = ty - self.y;
        let d  = (dx*dx + dy*dy).sqrt();
        if d > 0.0 { self.vx = dx/d; self.vy = dy/d; }
        self.x += self.vx * speed;
        self.y += self.vy * speed;
    }

    fn wander(&mut self, speed: f32, rng: &mut Rng) {
        self.vx += rng.gauss() * 0.12;
        self.vy += rng.gauss() * 0.12;
        self.norm();
        self.x += self.vx * speed;
        self.y += self.vy * speed;
    }

    fn norm(&mut self) {
        let m = (self.vx*self.vx + self.vy*self.vy).sqrt();
        if m > 1e-6 { self.vx /= m; self.vy /= m; }
        else        { self.vx = 1.0; self.vy = 0.0; }
    }

    fn clamp(&mut self, cfg: &Config) {
        let resting = self.state == HerbState::Resting;
        if self.x < 0.0         { self.x = 0.0;         if resting { self.vx = 0.0; } else { self.vx =  self.vx.abs(); } }
        if self.x > cfg.world_w { self.x = cfg.world_w; if resting { self.vx = 0.0; } else { self.vx = -self.vx.abs(); } }
        if self.y < 0.0         { self.y = 0.0;         if resting { self.vy = 0.0; } else { self.vy =  self.vy.abs(); } }
        if self.y > cfg.world_h { self.y = cfg.world_h; if resting { self.vy = 0.0; } else { self.vy = -self.vy.abs(); } }
    }

    // -----------------------------------------------------------------------
    // Reproduction
    // -----------------------------------------------------------------------

    #[allow(dead_code)]
    fn try_reproduce(&mut self, rng: &mut Rng, cfg: &Config) -> Option<HerbGenome> {
        if self.repro_cd > 0 { return None; }
        if self.age < cfg.herb_min_repro_age { return None; }
        if self.energy < self.genome.max_energy() * self.genome.repro_threshold() {
            return None;
        }
        self.energy  *= 0.5;   // reproduction costs half current energy
        self.repro_cd = self.genome.repro_cooldown();
        Some(self.genome.mutate(cfg.herb_mutation_rate, rng))
    }

    /// Density-aware reproduction check — called from lib.rs where grid is available.
    /// Returns true if reproduction should proceed, consuming energy and resetting cooldown.
    pub fn check_reproduce_density(
        &mut self,
        rng:     &mut Rng,
        cfg:     &Config,
        grid:    &crate::spatial::SpatialGrid,
        herbs:   &[Herbivore],
        scratch: &mut Vec<usize>,
    ) -> Option<HerbGenome> {
        if self.repro_cd > 0 { return None; }
        if self.age < cfg.herb_min_repro_age { return None; }
        if self.energy < self.genome.max_energy() * self.genome.repro_threshold() {
            return None;
        }

        // Density check — count nearby living herbs
        let radius = self.genome.personal_space() * 4.0;
        grid.query_radius(self.x, self.y, radius, scratch);
        let nearby = scratch.iter()
            .filter(|&&i| !std::ptr::eq(&herbs[i], self) && herbs[i].alive)
            .count();

        // Probability of reproducing falls linearly to near-zero at max density
        let max_d = cfg.herb_max_local_density as usize;
        if nearby >= max_d { return None; }
        let prob = 1.0 - (nearby as f32 / max_d as f32) * 0.95;
        if rng.f32() > prob { return None; }

        self.energy  *= 0.5;
        self.repro_cd = self.genome.repro_cooldown();
        Some(self.genome.mutate(cfg.herb_mutation_rate, rng))
    }

    // -----------------------------------------------------------------------
    // Accessors
    // -----------------------------------------------------------------------

    pub fn state_code(&self) -> f32  { self.state as u8 as f32 }
    pub fn sex_code(&self) -> f32    { self.sex   as u8 as f32 }
}

impl Herbivore {
    /// Fold this herbivore's persistent state into the signature hasher.
    pub fn hash_into(&self, h: &mut crate::signature::Fnv) {
        // id / parent_id are deliberately EXCLUDED from the fingerprint: they
        // are identity tags, not dynamics. (id resets per world via
        // reset_herb_ids() as of the id-stability fix, so it's reproducible now,
        // but stays excluded so the signature captures dynamics only.)
        h.f32(self.x);
        h.f32(self.y);
        h.f32(self.energy);
        h.u32(self.age);
        h.bool(self.alive);
        h.u32(self.state as u32);
        h.u32(self.sex as u32);
        h.f32(self.vx);
        h.f32(self.vy);
        h.f32(self.home_x);
        h.f32(self.home_y);
        h.bool(self.home_established);
        h.u32(self.home_search_timer);
        h.u32(self.eat_timer);
        h.opt_usize(self.eat_target_idx);
        h.f32(self.eat_energy);
        h.u32(self.alarmed_cd);
        h.f32(self.flee_from_x);
        h.f32(self.flee_from_y);
        h.bool(self.alarm_broadcast);
        h.u32(self.repro_cd);
        let g = &self.genome;
        h.f32(g.speed);
        h.f32(g.size);
        h.f32(g.sight_range);
        h.f32(g.eye_placement);
        h.f32(g.efficiency);
        h.f32(g.reproduction_rate);
        h.f32(g.vigilance);
        h.f32(g.herd_affinity);
    }
}
