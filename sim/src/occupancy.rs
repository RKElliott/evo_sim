//! occupancy.rs — Spatial occupancy grid for plants.
//!
//! Tracks which terrain cells are occupied by mature plants.
//! Prevents overcrowding — seeds landing in occupied cells are rejected.
//! This spreads plant draw across more terrain cells, preventing
//! local nutrient depletion that causes population crashes.
//!
//! Resolution matches terrain grid (terrain_cols × terrain_rows).
//! O(1) lookup and update per plant.

#[allow(dead_code)]
pub struct OccupancyGrid {
    cols:  usize,
    rows:  usize,
    /// Count of mature plants per terrain cell
    grid:  Vec<u8>,
    /// Maximum mature plants allowed per cell
    pub max_per_cell: u8,
}

#[allow(dead_code)]
impl OccupancyGrid {
    pub fn new(cols: usize, rows: usize, max_per_cell: u8) -> Self {
        Self {
            cols, rows,
            grid: vec![0u8; cols * rows],
            max_per_cell,
        }
    }

    fn idx(&self, col: usize, row: usize) -> usize {
        row * self.cols + col
    }

    /// Returns true if a new plant can occupy this cell.
    pub fn can_occupy(&self, col: usize, row: usize) -> bool {
        let i = self.idx(col, row);
        self.grid[i] < self.max_per_cell
    }

    /// Register a plant occupying this cell.
    pub fn occupy(&mut self, col: usize, row: usize) {
        let i = self.idx(col, row);
        if self.grid[i] < 255 {
            self.grid[i] += 1;
        }
    }

    /// Release a plant from this cell (called when plant dies).
    pub fn release(&mut self, col: usize, row: usize) {
        let i = self.idx(col, row);
        if self.grid[i] > 0 {
            self.grid[i] -= 1;
        }
    }

    /// Count of plants in this cell.
    pub fn count(&self, col: usize, row: usize) -> u8 {
        self.grid[self.idx(col, row)]
    }

    pub fn clear(&mut self) {
        for v in self.grid.iter_mut() { *v = 0; }
    }
}
