// evo_sim - Copyright (c) 2026 Lens and Mix, LLC
// Licensed under the Apache License, Version 2.0. See LICENSE and NOTICE.
// More information: https://rkeithelliott.com
//! carnivore.rs — Carnivore agent: stalking, chasing, latching, sustained drain.
//!
//! State machine:
//!   Resting → Wandering → Stalking → Chasing → Latched → (prey dies) → Resting
//!
//! Key design:
//!   - Stealth: prey only detects carnivore within stealth_factor * sight_radius
//!   - Sustained drain: carnivore latches and drains energy over multiple ticks
//!   - Shared kills: multiple carnivores can latch same prey simultaneously
//!   - Forward-biased FOV: narrow deep arc for accurate prey tracking
//!   - Alarm propagation: prey entering Fleeing state broadcasts alarm to herd

use crate::config::Config;
use crate::genome::CarnGenome;
use crate::herbivore::Herbivore;
use crate::rng::Rng;
use crate::spatial::SpatialGrid;

#[cfg(feature = "trace")]
use utils::tracer::Subsystem;

// ---------------------------------------------------------------------------
// Carnivore state
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(u8)]
pub enum CarnState {
    Wandering = 0,
    Stalking  = 1,   // prey in FOV, slow careful approach
    Chasing   = 2,   // prey detected/fleeing, full speed pursuit
    Latched   = 3,   // draining prey energy
    Resting   = 4,
    Homing    = 5,
}

// ---------------------------------------------------------------------------
// Carnivore
// ---------------------------------------------------------------------------

static mut NEXT_CARN_ID: u64 = 0;
fn next_carn_id() -> u64 { unsafe { NEXT_CARN_ID += 1; NEXT_CARN_ID } }
/// Reset the carnivore id counter to 0. Called at world build / regenerate so
/// ids are stable and reproducible per world (same seed → same ids), which the
/// overlay/trace/tag tooling relies on. Id is excluded from state_signature, so
/// this does not affect the harness.
pub(crate) fn reset_carn_ids() { unsafe { NEXT_CARN_ID = 0; } }

pub struct Carnivore {
    pub id:         u64,
    pub x:          f32,
    pub y:          f32,
    pub genome:     CarnGenome,
    pub energy:     f32,
    pub age:        u32,
    pub alive:      bool,
    pub state:      CarnState,

    pub vx:         f32,
    pub vy:         f32,

    // Home base
    home_x:         f32,
    home_y:         f32,
    home_established: bool,
    home_timer:     u32,

    // Hunt state
    pub target_herb_idx: Option<usize>,  // herb being stalked/chased/latched
    chase_ticks:    u32,                 // tracks stamina depletion

    // No-progress (water-trap) escape state
    trap_anchor_x:  f32,                 // position at start of current window
    trap_anchor_y:  f32,
    trap_timer:     u32,                 // ticks since anchor was set
    escape_cd:      u32,                 // >0 → in forced-escape mode
    escape_dx:      f32,                 // escape heading (unit), captured at detection
    escape_dy:      f32,

    // Reproduction
    pub repro_cd:   u32,
}

const HOME_SEARCH_TICKS: u32 = 600;  // longer than herbs — carnivores range further

// ── No-progress (water-trap) escape tuning ──────────────────────────────────
// A carnivore that is actively pursuing (Stalking/Chasing a target) but travels
// less than TRAP_MIN_PROGRESS world-units over TRAP_WINDOW_TICKS is judged
// trapped — e.g. oscillating at a concave shoreline where the pursuit pull and
// the avoid_water deflection cancel each tick (full energy burn, ~zero net
// displacement → starvation). It then drops the target and steers along a fixed
// "away from the unreachable prey" heading for TRAP_ESCAPE_TICKS, which exits
// the pocket because the prey sits across the water from the open land.
//
// These are first-pass GUESS values from a single observed trap (carn moved
// ~0.5u / 60 ticks; a genuinely slow-but-closing stalk covers far more than 5u,
// so the floor has wide margin against false positives). Grouped here so they
// are trivial to retune and to later promote to runtime/genome parameters.
const TRAP_WINDOW_TICKS: u32 = 60;   // measurement window
const TRAP_MIN_PROGRESS: f32 = 5.0;  // min net displacement (world units) per window
const TRAP_ESCAPE_TICKS: u32 = 30;   // forced-escape duration after a trap is detected

impl Carnivore {
    pub fn new(x: f32, y: f32, genome: CarnGenome, rng: &mut Rng) -> Self {
        let energy   = genome.max_energy() * 0.8;  // start well-fed — time to find first prey
        let repro_cd = genome.repro_cooldown();
        let angle    = rng.f32() * std::f32::consts::TAU;
        Self {
            id:               next_carn_id(),
            x, y, genome, energy,
            age:              0,
            alive:            true,
            state:            CarnState::Wandering,
            vx:               angle.cos(),
            vy:               angle.sin(),
            home_x:           x,
            home_y:           y,
            home_established: false,
            home_timer:       HOME_SEARCH_TICKS,
            target_herb_idx:  None,
            chase_ticks:      0,
            trap_anchor_x:    x,
            trap_anchor_y:    y,
            trap_timer:       0,
            escape_cd:        0,
            escape_dx:        0.0,
            escape_dy:        0.0,
            repro_cd,
        }
    }

    // -----------------------------------------------------------------------
    // Per-tick update
    // Returns: (Option<child genome>, Option<herb index being drained>)
    // -----------------------------------------------------------------------

    pub fn update(
        &mut self,
        herbs:   &[Herbivore],
        terrain: &crate::terrain::Terrain,
        cfg:     &Config,
        rng:     &mut Rng,
        grid:    &SpatialGrid,
        scratch: &mut Vec<usize>,
    ) -> (Option<CarnGenome>, Option<usize>) {
        if !self.alive { return (None, None); }

        #[cfg(feature = "trace")]
        let prev_state = self.state;

        self.age += 1;
        if self.repro_cd > 0 { self.repro_cd -= 1; }

        // Home base establishment
        if !self.home_established {
            if self.home_timer > 0 { self.home_timer -= 1; }
            else {
                self.home_x = self.x;
                self.home_y = self.y;
                self.home_established = true;
            }
        }

        // Energy drain — varies by activity level
        let drain = match self.state {
            CarnState::Resting   => self.genome.rest_energy_drain(),
            CarnState::Wandering => self.genome.energy_drain() * cfg.carn_wander_drain_mult,
            CarnState::Homing    => self.genome.energy_drain() * cfg.carn_wander_drain_mult,
            _                    => self.genome.energy_drain(),  // stalking/chasing/latched
        };
        self.energy -= drain;

        if self.energy <= 0.0 || self.age >= cfg.carn_max_age {
            self.alive = false;
            return (None, None);
        }

        // ── Priority stack ──────────────────────────────────────────────────

        // 1. Latched — draining prey
        if self.state == CarnState::Latched {
            if let Some(ti) = self.target_herb_idx {
                if ti < herbs.len() && herbs[ti].alive {
                    // Stay on target — no movement
                    utils::trace_agent!(Subsystem::Carns, self.id, "carn#{} LATCHED herb#{} drain (e={:.2})", self.id, ti, self.energy);
                    self.clamp(cfg);
                    return (self.try_reproduce(rng, cfg), Some(ti));
                }
            }
            // Prey died or disappeared
            self.state           = CarnState::Wandering;
            self.target_herb_idx = None;
            self.chase_ticks     = 0;
        }

        let sated = self.energy >= self.genome.max_energy()
                   * self.genome.satiation_threshold();

        if sated {
            // Resting — velocity decays to zero
            if self.state != CarnState::Resting {
                self.vx = 0.0; self.vy = 0.0;
                utils::trace_agent!(Subsystem::Carns, self.id, "carn#{} →Resting (sated, e={:.2})", self.id, self.energy);
            }
            self.state = CarnState::Resting;
            self.vx   *= 0.85; self.vy *= 0.85;
            let mag    = (self.vx*self.vx + self.vy*self.vy).sqrt();
            if mag < 0.01 { self.vx = 0.0; self.vy = 0.0; }
            self.x    += self.vx; self.y += self.vy;
            self.clamp(cfg);
            return (self.try_reproduce(rng, cfg), None);
        }

        // Hungry — wake from rest
        if self.state == CarnState::Resting {
            if self.energy < self.genome.max_energy() * self.genome.hunger_threshold() {
                self.state = CarnState::Wandering;
                let angle  = rng.f32() * std::f32::consts::TAU;
                self.vx    = angle.cos(); self.vy = angle.sin();
            } else {
                // Still resting — not hungry enough yet
                self.clamp(cfg);
                return (self.try_reproduce(rng, cfg), None);
            }
        }

        // ── No-progress (water-trap) detection + forced escape ──────────────
        // Detection: once per window, judge whether a pursuing carn made real
        // progress. If not, it's trapped — capture an away-from-prey heading and
        // enter escape mode. trap_timer only advances on hunting ticks (Latched/
        // Resting carns return earlier), so intentional stillness never trips it.
        self.trap_timer += 1;
        if self.trap_timer >= TRAP_WINDOW_TICKS {
            let moved = ((self.x - self.trap_anchor_x).powi(2)
                       + (self.y - self.trap_anchor_y).powi(2)).sqrt();
            if matches!(self.state, CarnState::Stalking | CarnState::Chasing)
                && moved < TRAP_MIN_PROGRESS {
                if let Some(ti) = self.target_herb_idx {
                    if ti < herbs.len() {
                        // Heading: from the (unreachable, across-water) prey
                        // toward us — i.e. back out of the pocket toward land.
                        let mut hx = self.x - herbs[ti].x;
                        let mut hy = self.y - herbs[ti].y;
                        let m = (hx*hx + hy*hy).sqrt();
                        if m > 1e-4 { hx /= m; hy /= m; } else { hx = 1.0; hy = 0.0; }
                        self.escape_dx       = hx;
                        self.escape_dy       = hy;
                        self.escape_cd       = TRAP_ESCAPE_TICKS;
                        self.target_herb_idx = None;
                        self.chase_ticks     = 0;
                        utils::trace_agent!(Subsystem::Carns, self.id, "carn#{} TRAP detected (moved={:.1}/{} ticks) → escape", self.id, moved, TRAP_WINDOW_TICKS);
                    }
                }
            }
            self.trap_anchor_x = self.x;
            self.trap_anchor_y = self.y;
            self.trap_timer    = 0;
        }

        // Escape: steer along the captured heading until the countdown expires.
        // avoid_water still runs below, so we won't be driven into the water.
        if self.escape_cd > 0 {
            self.escape_cd -= 1;
            self.state = CarnState::Homing;   // out-and-away; re-scans when it ends
            let speed  = self.genome.move_speed(cfg) * cfg.carn_wander_speed;
            self.vx = self.escape_dx;
            self.vy = self.escape_dy;
            self.x += self.vx * speed;
            self.y += self.vy * speed;
            utils::trace_agent!(Subsystem::Carns, self.id, "carn#{} TRAP-escape ({} left)", self.id, self.escape_cd);
            self.avoid_water(terrain, cfg);
            self.clamp(cfg);
            return (self.try_reproduce(rng, cfg), None);
        }

        // 2. Find prey in FOV — but if already chasing, stick with current target
        let prey_idx = if (self.state == CarnState::Chasing || self.state == CarnState::Stalking)
                && self.target_herb_idx.map_or(false, |ti| ti < herbs.len() && herbs[ti].alive) {
            // Already pursuing — keep same target unless it dies
            self.target_herb_idx
        } else {
            // Fresh scan
            self.find_prey(herbs, cfg, grid, scratch)
        };

        if let Some(ti) = prey_idx {
            let px = herbs[ti].x;
            let py = herbs[ti].y;
            let d  = ((self.x-px).powi(2) + (self.y-py).powi(2)).sqrt();

            if d <= self.genome.strike_reach() {
                // Latch on
                utils::trace_agent!(Subsystem::Carns, self.id, "carn#{} {:?}->Latched herb#{} d={:.1} reach={:.1}", self.id, self.state, ti, d, self.genome.strike_reach());
                self.state           = CarnState::Latched;
                self.target_herb_idx = Some(ti);
                self.chase_ticks     = 0;
                self.clamp(cfg);
                return (self.try_reproduce(rng, cfg), Some(ti));
            }

            // Is prey fleeing? Switch to Chase
            if herbs[ti].state == crate::herbivore::HerbState::Fleeing {
                self.state = CarnState::Chasing;
            } else if d > self.genome.strike_reach() * 3.0 {
                self.state = CarnState::Stalking;
            } else {
                self.state = CarnState::Chasing;
            }

            self.target_herb_idx = Some(ti);

            let speed = if self.state == CarnState::Stalking {
                self.genome.stalk_speed(cfg) * cfg.carn_stalk_speed_mult
            } else {
                // Chase speed — fatigue only reduces speed after stamina_ticks exceeded
                self.chase_ticks += 1;
                let stamina_ticks = self.genome.chase_stamina_ticks();
                let speed_mult = if self.chase_ticks <= stamina_ticks {
                    cfg.carn_chase_speed_mult  // full speed within stamina
                } else {
                    let over = (self.chase_ticks - stamina_ticks) as f32;
                    cfg.carn_chase_speed_mult * (1.0 - (over / stamina_ticks as f32).min(0.5))
                };
                self.genome.move_speed(cfg) * speed_mult
            };

            utils::trace_agent!(Subsystem::Carns, self.id, "carn#{} {:?} herb#{} d={:.1} reach={:.1} speed={:.2} chase={}", self.id, self.state, ti, d, self.genome.strike_reach(), speed, self.chase_ticks);
            self.move_toward(px, py, speed);
        } else {
            // No prey — wander or home
            self.target_herb_idx = None;
            self.chase_ticks     = 0;
            let dh = ((self.x-self.home_x).powi(2)
                    + (self.y-self.home_y).powi(2)).sqrt();
            if self.home_established && dh > cfg.carn_home_radius {
                self.state = CarnState::Homing;
                self.move_toward(self.home_x, self.home_y,
                    self.genome.move_speed(cfg) * cfg.carn_wander_speed);
            } else {
                self.state = CarnState::Wandering;
                self.wander_toward_prey(
                    self.genome.move_speed(cfg) * cfg.carn_wander_speed,
                    rng, grid, herbs);
            }
        }

        #[cfg(feature = "trace")]
        {
            if self.state != prev_state {
                utils::trace_agent!(Subsystem::Carns, self.id, "carn#{} {:?}->{:?}", self.id, prev_state, self.state);
            }
        }

        self.avoid_water(terrain, cfg);
        self.clamp(cfg);
        (self.try_reproduce(rng, cfg), None)
    }

    // -----------------------------------------------------------------------
    // FOV prey scan — uses spatial grid
    // Stealth reduces the range at which prey is visible
    // -----------------------------------------------------------------------

    fn find_prey(&self, herbs: &[Herbivore], _cfg: &Config,
                  grid: &SpatialGrid, scratch: &mut Vec<usize>) -> Option<usize> {
        let sr    = self.genome.sight_radius();
        let fov   = self.genome.fov_half_angle();
        let speed = (self.vx*self.vx + self.vy*self.vy).sqrt();
        let omni  = speed < 0.05;
        let mut best_idx = None;
        let mut best_d   = f32::INFINITY;

        grid.query_radius(self.x, self.y, sr, scratch);
        for &i in scratch.iter() {
            let h = &herbs[i];
            if !h.alive { continue; }

            // Check stealth — carnivore's stealth_factor reduces effective range
            // (this is used when herbs scan for carnivores; here we scan for herbs)
            let dx = h.x - self.x;
            let dy = h.y - self.y;
            let d  = (dx*dx + dy*dy).sqrt();
            if d > sr || d >= best_d { continue; }

            // FOV check
            let in_fov = if omni { true } else {
                let angle_to = dy.atan2(dx);
                let facing   = self.vy.atan2(self.vx);
                let mut diff = (angle_to - facing).abs();
                if diff > std::f32::consts::PI { diff = std::f32::consts::TAU - diff; }
                diff <= fov
            };

            if in_fov {
                best_idx = Some(i);
                best_d   = d;
            }
        }
        best_idx
    }

    // -----------------------------------------------------------------------
    // Movement
    // -----------------------------------------------------------------------

    fn move_toward(&mut self, tx: f32, ty: f32, speed: f32) {
        let dx = tx - self.x; let dy = ty - self.y;
        let d  = (dx*dx + dy*dy).sqrt();
        if d > 0.0 { self.vx = dx/d; self.vy = dy/d; }
        self.x += self.vx * speed;
        self.y += self.vy * speed;
    }

    /// Density-directed wander — blends random walk with drift toward
    /// highest nearby herb concentration. Queries herb grid at 2× sight
    /// radius, finds the densest direction, and steers toward it.
    /// The blend strength means carnivores still cover new ground but
    /// persistently drift toward where prey is most likely to be found.
    fn wander_toward_prey(&mut self, speed: f32, rng: &mut Rng,
                           grid: &SpatialGrid, herbs: &[Herbivore]) {
        // Sample candidate directions in a ring at 1.5× sight radius
        let search_r  = self.genome.sight_radius() * 1.5;
        let n_samples  = 8usize;
        let mut best_count = 0usize;
        let mut best_dx    = 0.0_f32;
        let mut best_dy    = 0.0_f32;

        // Sample herb density at positions around the carnivore
        let mut sample_scratch: Vec<usize> = Vec::with_capacity(32);
        for k in 0..n_samples {
            let angle = (k as f32 / n_samples as f32) * std::f32::consts::TAU;
            let sx    = self.x + angle.cos() * search_r * 0.5;
            let sy    = self.y + angle.sin() * search_r * 0.5;
            grid.query_radius(sx, sy, search_r * 0.4, &mut sample_scratch);
            let count = sample_scratch.iter()
                .filter(|&&i| i < herbs.len() && herbs[i].alive)
                .count();
            if count > best_count {
                best_count = count;
                best_dx    = angle.cos();
                best_dy    = angle.sin();
            }
        }

        // Blend: if prey found in a direction, drift 60% toward it,
        // 40% random walk. If nothing found, pure random walk.
        if best_count > 0 {
            let blend  = 0.60;
            let rdx    = rng.gauss() * 0.10;
            let rdy    = rng.gauss() * 0.10;
            self.vx    = self.vx * (1.0 - blend) + best_dx * blend + rdx;
            self.vy    = self.vy * (1.0 - blend) + best_dy * blend + rdy;
        } else {
            // Pure random walk — no prey sensed anywhere nearby
            self.vx   += rng.gauss() * 0.10;
            self.vy   += rng.gauss() * 0.10;
        }
        self.norm();
        self.x += self.vx * speed;
        self.y += self.vy * speed;
    }

    /// Pure random walk — kept as fallback for comparison testing.
    #[allow(dead_code)]
    fn wander(&mut self, speed: f32, rng: &mut Rng) {
        self.vx += rng.gauss() * 0.10;
        self.vy += rng.gauss() * 0.10;
        self.norm();
        self.x += self.vx * speed;
        self.y += self.vy * speed;
    }

    fn norm(&mut self) {
        let m = (self.vx*self.vx + self.vy*self.vy).sqrt();
        if m > 1e-6 { self.vx /= m; self.vy /= m; }
        else         { self.vx = 1.0; self.vy = 0.0; }
    }

    fn clamp(&mut self, cfg: &Config) {
        let resting = self.state == CarnState::Resting;
        if self.x < 0.0         { self.x = 0.0;         if resting { self.vx = 0.0; } else { self.vx =  self.vx.abs(); } }
        if self.x > cfg.world_w { self.x = cfg.world_w; if resting { self.vx = 0.0; } else { self.vx = -self.vx.abs(); } }
        if self.y < 0.0         { self.y = 0.0;         if resting { self.vy = 0.0; } else { self.vy =  self.vy.abs(); } }
        if self.y > cfg.world_h { self.y = cfg.world_h; if resting { self.vy = 0.0; } else { self.vy = -self.vy.abs(); } }
    }

    fn avoid_water(&mut self, terrain: &crate::terrain::Terrain, cfg: &Config) {
        if let Some((col, row)) = cfg.world_to_cell(self.x, self.y) {
            if terrain.is_water(col, row) {
                self.vx = -self.vx; self.vy = -self.vy;
                self.x += self.vx * self.genome.move_speed(cfg) * 2.0;
                self.y += self.vy * self.genome.move_speed(cfg) * 2.0;
                return;
            }
        }
        let nx = self.x + self.vx * self.genome.move_speed(cfg) * 3.0;
        let ny = self.y + self.vy * self.genome.move_speed(cfg) * 3.0;
        if let Some((col, row)) = cfg.world_to_cell(nx, ny) {
            if terrain.is_water(col, row) {
                let px = self.vy; let py = -self.vx;
                self.vx = px; self.vy = py; self.norm();
            }
        }
    }

    // -----------------------------------------------------------------------
    // Reproduction
    // -----------------------------------------------------------------------

    fn try_reproduce(&mut self, rng: &mut Rng, cfg: &Config) -> Option<CarnGenome> {
        if self.repro_cd > 0 { return None; }
        if self.age < cfg.carn_min_repro_age { return None; }
        if self.energy < self.genome.max_energy() * self.genome.repro_threshold() {
            return None;
        }
        self.energy  *= 0.5;
        self.repro_cd = self.genome.repro_cooldown();
        Some(self.genome.mutate(cfg.carn_mutation_rate, rng))
    }

    // -----------------------------------------------------------------------
    // Accessors
    // -----------------------------------------------------------------------

    pub fn state_code(&self) -> f32 { self.state as u8 as f32 }
}

impl Carnivore {
    /// Fold this carnivore's persistent state into the signature hasher.
    pub fn hash_into(&self, h: &mut crate::signature::Fnv) {
        // id EXCLUDED from the fingerprint: it's an identity tag, not a
        // dynamics value. (As of the id-stability fix it resets per world via
        // reset_carn_ids(), so it's now reproducible — but still excluded so the
        // signature stays focused on dynamics and the golden master is stable.)
        h.f32(self.x);
        h.f32(self.y);
        h.f32(self.energy);
        h.u32(self.age);
        h.bool(self.alive);
        h.u32(self.state as u32);
        h.f32(self.vx);
        h.f32(self.vy);
        h.f32(self.home_x);
        h.f32(self.home_y);
        h.bool(self.home_established);
        h.u32(self.home_timer);
        h.opt_usize(self.target_herb_idx);
        h.u32(self.chase_ticks);
        h.u32(self.repro_cd);
        let g = &self.genome;
        h.f32(g.speed);
        h.f32(g.size);
        h.f32(g.sight_range);
        h.f32(g.eye_placement);
        h.f32(g.stealth);
        h.f32(g.strike_power);
        h.f32(g.stamina);
        h.f32(g.reproduction_rate);
    }
}
