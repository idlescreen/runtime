//! Cell-grid bounds for a single monitor in multi-monitor layouts.

/// Inclusive-start / exclusive-end cell rectangle for one monitor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonitorCellBounds {
    pub start_col: usize,
    pub end_col: usize,
    pub start_row: usize,
    pub end_row: usize,
    pub is_primary: bool,
}

impl MonitorCellBounds {
    pub fn width(&self) -> usize {
        self.end_col.saturating_sub(self.start_col)
    }

    pub fn height(&self) -> usize {
        self.end_row.saturating_sub(self.start_row)
    }

    pub fn center_col(&self) -> usize {
        self.start_col + self.width() / 2
    }

    pub fn center_row(&self) -> usize {
        self.start_row + self.height() / 2
    }

    /// Returns true if `(col, row)` is inside this bounds (half-open: `end_col`/`end_row` excluded).
    ///
    /// # Example
    ///
    /// ```
    /// use idle_api::MonitorCellBounds;
    /// let b = MonitorCellBounds {
    ///     start_col: 0,
    ///     end_col: 10,
    ///     start_row: 0,
    ///     end_row: 5,
    ///     is_primary: true,
    /// };
    /// assert!(b.contains(5, 3));
    /// assert!(!b.contains(11, 3));
    /// assert!(!b.contains(10, 0)); // end_col is exclusive
    /// ```
    pub fn contains(&self, col: usize, row: usize) -> bool {
        col >= self.start_col && col < self.end_col && row >= self.start_row && row < self.end_row
    }
}
