// evo_sim - Copyright (c) 2026 Lens and Mix, LLC
// Licensed under the Apache License, Version 2.0. See LICENSE and NOTICE.
// More information: https://rkeithelliott.com
//! terrain.rs — Heightmap, water, nutrients, hillshading.
//!
//! The terrain is the physical foundation everything else builds on.
//! It knows nothing about agents or plants — pure environment.
//!
//! Data layout: flat Vec<f32> arrays indexed by (row * cols + col).
//! All arrays are the same length: cfg.terrain_cols * cfg.terrain_rows.
//!
//! Shared buffer layout for JS renderer (written by write_render_buffer):
//!   Per cell, 4 f32 values packed contiguously:
//!   [elevation, nutrient, water(0/1), hillshade]
//!   Total floats: cols * rows * 4

use crate::config::Config;
use crate::rng::Rng;

pub struct Terrain {
    pub cols:      usize,
    pub rows:      usize,
    /// Elevation values 0.0–1.0
    pub elevation: Vec<f32>,
    /// Nutrient values 0.0–1.0
    pub nutrient:  Vec<f32>,
    /// True if cell is water
    pub water:     Vec<bool>,
    /// Hillshade values 0.0–1.0 (pre-computed, static)
    pub hillshade: Vec<f32>,
    /// Pre-allocated render buffer [elev, nutrient, water, hillshade] per cell
    render_buf:    Vec<f32>,
}

#[allow(dead_code)]
impl Terrain {
    pub fn new(cfg: &Config) -> Self {
        let cols  = cfg.terrain_cols;
        let rows  = cfg.terrain_rows;
        let n     = cols * rows;

        let elevation = generate_elevation(cols, rows, cfg);
        let water     = generate_water(&elevation, cfg.water_level);
        let nutrient  = generate_nutrients(&elevation, &water, cfg);
        let hillshade = generate_hillshade(&elevation, cols, rows);

        let render_buf = vec![0.0_f32; n * 4];

        let mut t = Self { cols, rows, elevation, nutrient, water, hillshade, render_buf };
        t.rebuild_render_buffer(cfg.hillshade_strength);
        t
    }

    // -----------------------------------------------------------------------
    // Per-tick update — nutrient flow
    // -----------------------------------------------------------------------

    pub fn update(&mut self, cfg: &Config) {
        // Background nutrient regeneration — every land cell gets a tiny
        // baseline each tick regardless of flow. Models decomposition,
        // rainfall, and weathering that happens independent of surface flow.
        // Prevents complete depletion in areas between flow events.
        for i in 0..self.cols * self.rows {
            if !self.water[i] {
                self.nutrient[i] = (self.nutrient[i] + cfg.nutrient_regen_rate)
                    .min(1.0);
            }
        }

        flow_nutrients(
            &self.elevation,
            &mut self.nutrient,
            &self.water,
            self.cols,
            self.rows,
            cfg,
        );
        // Update nutrient channel in render buffer
        for i in 0..self.cols * self.rows {
            self.render_buf[i * 4 + 1] = self.nutrient[i];
        }
    }

    // -----------------------------------------------------------------------
    // Render buffer
    // -----------------------------------------------------------------------

    /// Rebuild the full render buffer from current terrain state.
    /// hillshade_strength is applied here so the slider works in real time
    /// without recomputing the raw hillshade values.
    pub fn rebuild_render_buffer(&mut self, hillshade_strength: f32) {
        for i in 0..self.cols * self.rows {
            // Apply strength: blend raw hillshade toward 0.5 (neutral) based on strength
            let raw_hs = self.hillshade[i];
            let hs     = 0.5 + (raw_hs - 0.5) * hillshade_strength;
            self.render_buf[i * 4]     = self.elevation[i];
            self.render_buf[i * 4 + 1] = self.nutrient[i];
            self.render_buf[i * 4 + 2] = if self.water[i] { 1.0 } else { 0.0 };
            self.render_buf[i * 4 + 3] = hs;
        }
    }

    /// Pointer into WASM linear memory for the render buffer.
    /// JavaScript creates a Float32Array view at this offset — zero copy.
    pub fn render_buffer_ptr(&self) -> *const f32 {
        self.render_buf.as_ptr()
    }

    pub fn render_buffer_len(&self) -> usize {
        self.render_buf.len()
    }

    // -----------------------------------------------------------------------
    // Queries used by biology subsystems
    // -----------------------------------------------------------------------

    pub fn elevation_at(&self, col: usize, row: usize) -> f32 {
        self.elevation[row * self.cols + col]
    }

    pub fn nutrient_at(&self, col: usize, row: usize) -> f32 {
        self.nutrient[row * self.cols + col]
    }

    pub fn is_water(&self, col: usize, row: usize) -> bool {
        self.water[row * self.cols + col]
    }

    /// Check water at world coordinates directly.
    pub fn is_water_at_world(&self, x: f32, y: f32, cfg: &crate::config::Config) -> bool {
        match cfg.world_to_cell(x, y) {
            Some((col, row)) => self.is_water(col, row),
            None => true,  // out of bounds treated as water
        }
    }

    /// Add fertility at a world-space location (from herbivore movement / carcasses).
    pub fn deposit_nutrient(&mut self, col: usize, row: usize, amount: f32) {
        let i = row * self.cols + col;
        self.nutrient[i] = (self.nutrient[i] + amount).min(1.0);
        self.render_buf[i * 4 + 1] = self.nutrient[i];
    }
}

// ---------------------------------------------------------------------------
// Elevation generation — layered value noise
// ---------------------------------------------------------------------------

fn generate_elevation(cols: usize, rows: usize, cfg: &Config) -> Vec<f32> {
    let n    = cols * rows;
    let seed = cfg.terrain_seed;
    let mut elev = vec![0.0_f32; n];

    // Plant-lab mode: a smooth wet→dry ramp by column (x). Left edge low (wet,
    // nutrient-rich), right edge high (dry). water_level sets the shoreline.
    if cfg.terrain_flat_ramp {
        let denom = (cols.max(2) - 1) as f32;
        for row in 0..rows {
            for col in 0..cols {
                elev[row * cols + col] = 0.2 + 0.6 * (col as f32 / denom);
            }
        }
        return elev;
    }

    for row in 0..rows {
        for col in 0..cols {
            // Normalized coordinates [0.0, 1.0]
            let nx = col as f32 / cols as f32;
            let ny = row as f32 / rows as f32;

            let mut value     = 0.0_f32;
            let mut amplitude = 1.0_f32;
            let mut frequency = 1.0_f32;
            let mut max_val   = 0.0_f32;

            for octave in 0..cfg.noise_octaves {
                // Scale coordinates by frequency
                let fx = nx * frequency * 4.0;
                let fy = ny * frequency * 4.0;

                // Integer cell coordinates for this octave
                let ix = fx.floor() as i32;
                let iy = fy.floor() as i32;

                // Fractional part for smooth interpolation
                let tx = fx - ix as f32;
                let ty = fy - iy as f32;

                // Smoothstep interpolation weights
                let ux = tx * tx * (3.0 - 2.0 * tx);
                let uy = ty * ty * (3.0 - 2.0 * ty);

                // Sample the four corners of the cell with octave-offset seed
                let oct_seed = seed.wrapping_add(octave as u64 * 0x9E3779B97F4A7C15);
                let v00 = Rng::value_noise_2d(ix,     iy,     oct_seed);
                let v10 = Rng::value_noise_2d(ix + 1, iy,     oct_seed);
                let v01 = Rng::value_noise_2d(ix,     iy + 1, oct_seed);
                let v11 = Rng::value_noise_2d(ix + 1, iy + 1, oct_seed);

                // Bilinear interpolation
                let sample = lerp(lerp(v00, v10, ux), lerp(v01, v11, ux), uy);

                value    += sample * amplitude;
                max_val  += amplitude;
                amplitude *= cfg.noise_persistence;
                frequency *= cfg.noise_lacunarity;
            }

            // Normalize to [0.0, 1.0]
            elev[row * cols + col] = (value / max_val + 1.0) * 0.5;
        }
    }

    // Apply a subtle radial falloff to encourage land in the centre
    // and water/coast around the edges — more interesting starting terrain
    apply_island_mask(&mut elev, cols, rows, 0.6);

    elev
}

fn apply_island_mask(elev: &mut Vec<f32>, cols: usize, rows: usize, strength: f32) {
    for row in 0..rows {
        for col in 0..cols {
            let nx = col as f32 / (cols - 1) as f32 * 2.0 - 1.0;
            let ny = row as f32 / (rows - 1) as f32 * 2.0 - 1.0;
            // Elliptical mask — wider than tall to match typical landscape aspect
            let dist = (nx * nx + ny * ny * 1.4).sqrt().min(1.0);
            let mask = 1.0 - dist * strength;
            let i    = row * cols + col;
            elev[i]  = (elev[i] * mask).max(0.0).min(1.0);
        }
    }
}

// ---------------------------------------------------------------------------
// Water generation — simple threshold
// ---------------------------------------------------------------------------

fn generate_water(elevation: &[f32], water_level: f32) -> Vec<bool> {
    elevation.iter().map(|&e| e < water_level).collect()
}

// ---------------------------------------------------------------------------
// Nutrient generation — high in valleys, low on ridges
// ---------------------------------------------------------------------------

fn generate_nutrients(elevation: &[f32], water: &[bool], cfg: &Config) -> Vec<f32> {
    elevation.iter().zip(water.iter()).map(|(&e, &w)| {
        if w {
            // Water cells have no plant-accessible nutrients
            0.0
        } else {
            // Invert elevation: low land = high nutrients
            let base = cfg.nutrient_base_low_elev
                     + (cfg.nutrient_base_high_elev - cfg.nutrient_base_low_elev)
                       * e;
            base.max(0.0).min(1.0)
        }
    }).collect()
}

// ---------------------------------------------------------------------------
// Hillshading — slope-based edge brightening
// ---------------------------------------------------------------------------

fn generate_hillshade(elevation: &[f32], cols: usize, rows: usize) -> Vec<f32> {
    let mut hs = vec![0.5_f32; cols * rows];
    // Simulated light direction: upper-left (northwest)
    let light_x = -1.0_f32;
    let light_y = -1.0_f32;

    for row in 1..(rows - 1) {
        for col in 1..(cols - 1) {
            let e_right = elevation[row * cols + col + 1];
            let e_left  = elevation[row * cols + col - 1];
            let e_down  = elevation[(row + 1) * cols + col];
            let e_up    = elevation[(row - 1) * cols + col];

            // Amplify differences so slopes are more visible
            let dx = (e_right - e_left) * 4.0;
            let dy = (e_down  - e_up)   * 4.0;

            let nx = -dx;
            let ny = -dy;
            let nz = 1.0_f32;
            let len = (nx*nx + ny*ny + nz*nz).sqrt();

            let dot = (nx * light_x + ny * light_y + nz) / len;
            // Store raw [-1,1] mapped to [0,1] — strength applied at render time
            hs[row * cols + col] = (dot + 1.0) * 0.5;
        }
    }
    hs
}

// ---------------------------------------------------------------------------
// Nutrient flow — bleeds downhill each tick
// ---------------------------------------------------------------------------

fn flow_nutrients(
    elevation: &[f32],
    nutrient:  &mut Vec<f32>,
    water:     &[bool],
    cols:      usize,
    rows:      usize,
    cfg:       &Config,
) {
    // We need a snapshot to avoid order-dependent updates
    let snapshot = nutrient.clone();

    for row in 0..rows {
        for col in 0..cols {
            let i = row * cols + col;
            if water[i] { continue; }

            let e     = elevation[i];
            let n     = snapshot[i];
            let rate  = cfg.nutrient_flow_rate;
            let decay = cfg.nutrient_decay_rate;

            // Find lowest accessible neighbor
            let neighbors: &[(i32, i32)] = &[(-1,0),(1,0),(0,-1),(0,1)];
            let mut lowest_elev = e;
            let mut lowest_idx  = None;

            for &(dc, dr) in neighbors {
                let nc = col as i32 + dc;
                let nr = row as i32 + dr;
                if nc < 0 || nc >= cols as i32 || nr < 0 || nr >= rows as i32 { continue; }
                let ni = nr as usize * cols + nc as usize;
                if water[ni] { continue; }
                if elevation[ni] < lowest_elev {
                    lowest_elev = elevation[ni];
                    lowest_idx  = Some(ni);
                }
            }

            // Flow to lowest neighbor
            if let Some(li) = lowest_idx {
                let flow = n * rate;
                nutrient[i]  = (n  - flow - n * decay).max(0.0);
                nutrient[li] = (snapshot[li] + flow).min(1.0);
            } else {
                // Local minimum — just decay
                nutrient[i] = (n - n * decay).max(0.0);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}
