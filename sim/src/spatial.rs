// evo_sim - Copyright (c) 2026 Lens and Mix, LLC
// Licensed under the Apache License, Version 2.0. See LICENSE and NOTICE.
// More information: https://rkeithelliott.com
//! spatial.rs — Spatial hash grid for O(1) neighbor lookup.
//!
//! Divides the world into fixed-size cells. Each cell holds a list of
//! agent indices currently in that cell. Neighbor queries only check
//! cells within range rather than all agents.
//!
//! Reduces boid scanning from O(n²) to O(n) for large populations.

pub struct SpatialGrid {
    cell_size:  f32,
    cols:       usize,
    rows:       usize,
    cells:      Vec<Vec<usize>>,  // cell → list of agent indices
}

#[allow(dead_code)]
impl SpatialGrid {
    pub fn new(world_w: f32, world_h: f32, cell_size: f32) -> Self {
        let cols = ((world_w / cell_size).ceil() as usize).max(1);
        let rows = ((world_h / cell_size).ceil() as usize).max(1);
        Self {
            cell_size,
            cols,
            rows,
            cells: vec![Vec::new(); cols * rows],
        }
    }

    fn cell_idx(&self, col: usize, row: usize) -> usize {
        row * self.cols + col
    }

    fn world_to_cell(&self, x: f32, y: f32) -> (usize, usize) {
        let col = ((x / self.cell_size) as usize).min(self.cols - 1);
        let row = ((y / self.cell_size) as usize).min(self.rows - 1);
        (col, row)
    }

    /// Clear all cells — call at start of each tick before rebuilding.
    pub fn clear(&mut self) {
        for cell in self.cells.iter_mut() {
            cell.clear();
        }
    }

    /// Insert an agent at world position (x, y).
    pub fn insert(&mut self, idx: usize, x: f32, y: f32) {
        let (col, row) = self.world_to_cell(x, y);
        let cell_idx   = self.cell_idx(col, row);
        self.cells[cell_idx].push(idx);
    }

    /// Returns all agent indices within `radius` world units of (x, y).
    /// Checks only the cells that could contain agents within range.
    pub fn query_radius<'a>(&'a self, x: f32, y: f32, radius: f32,
                             out: &mut Vec<usize>) {
        out.clear();
        let cell_span = (radius / self.cell_size).ceil() as i32 + 1;
        let (cx, cy) = self.world_to_cell(x, y);
        let cx = cx as i32;
        let cy = cy as i32;

        for dy in -cell_span..=cell_span {
            for dx in -cell_span..=cell_span {
                let nx = cx + dx;
                let ny = cy + dy;
                if nx < 0 || ny < 0
                || nx >= self.cols as i32
                || ny >= self.rows as i32 { continue; }
                let idx = self.cell_idx(nx as usize, ny as usize);
                out.extend_from_slice(&self.cells[idx]);
            }
        }
    }

    /// Count agents within radius (cheaper than full query when count is all we need).
    pub fn count_radius(&self, x: f32, y: f32, radius: f32,
                        positions: &[(f32, f32)]) -> u32 {
        let mut scratch = Vec::new();
        self.query_radius(x, y, radius, &mut scratch);
        let r2 = radius * radius;
        scratch.iter().filter(|&&i| {
            let (px, py) = positions[i];
            (px-x)*(px-x) + (py-y)*(py-y) <= r2
        }).count() as u32
    }

    pub fn cols(&self) -> usize { self.cols }
    pub fn rows(&self) -> usize { self.rows }
    pub fn cell_size(&self) -> f32 { self.cell_size }
}
