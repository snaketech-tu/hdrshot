//! The rows picked in the file list: by clicking one, Ctrl-clicking more
//! (Cmd on macOS), Shift-clicking a range or pressing Ctrl+A.

/// Picked rows, by index.
#[derive(Default)]
pub(crate) struct Selection {
    /// Sorted, without duplicates.
    rows: Vec<usize>,
    /// The row clicked last: Shift-click extends from it, the arrow keys
    /// move it, and Space and Ctrl+C act on it.
    anchor: Option<usize>,
}

impl Selection {
    pub(crate) fn rows(&self) -> &[usize] {
        &self.rows
    }

    pub(crate) fn contains(&self, row: usize) -> bool {
        self.rows.binary_search(&row).is_ok()
    }

    pub(crate) fn len(&self) -> usize {
        self.rows.len()
    }

    pub(crate) fn anchor(&self) -> Option<usize> {
        self.anchor
    }

    pub(crate) fn clear(&mut self) {
        self.rows.clear();
        self.anchor = None;
    }

    /// Picks `row` alone.
    pub(crate) fn set(&mut self, row: usize) {
        self.rows = vec![row];
        self.anchor = Some(row);
    }

    /// Picks `rows`, the last of them as the anchor.
    pub(crate) fn set_all(&mut self, mut rows: Vec<usize>) {
        self.anchor = rows.last().copied();
        rows.sort_unstable();
        rows.dedup();
        self.rows = rows;
    }

    /// Adds `row`, or takes it out if picked (Ctrl-click).
    pub(crate) fn toggle(&mut self, row: usize) {
        match self.rows.binary_search(&row) {
            Ok(at) => {
                self.rows.remove(at);
            }
            Err(at) => self.rows.insert(at, row),
        }
        self.anchor = Some(row);
    }

    /// Picks the rows from the anchor to `row` (Shift-click), keeping the
    /// anchor.
    pub(crate) fn extend_to(&mut self, row: usize) {
        let Some(anchor) = self.anchor else {
            self.set(row);
            return;
        };
        self.rows = (anchor.min(row)..=anchor.max(row)).collect();
    }

    /// Picks the rows from 0 to `count`.
    pub(crate) fn all(&mut self, count: usize) {
        self.rows = (0..count).collect();
        if self.anchor.is_none_or(|a| a >= count) {
            self.anchor = count.checked_sub(1);
        }
    }

    /// Moves the selection to the next row (`down`) or the previous one,
    /// in a list of `count` rows.
    pub(crate) fn step(&mut self, down: bool, count: usize) {
        let Some(last) = count.checked_sub(1) else {
            return;
        };
        let row = match (self.anchor, down) {
            (Some(row), true) => (row + 1).min(last),
            (Some(row), false) => row.saturating_sub(1),
            (None, true) => 0,
            (None, false) => last,
        };
        self.set(row);
    }

    /// Follows the removal of `removed` rows, sorted: their rows leave the
    /// selection and later ones move up.
    pub(crate) fn after_removal(&mut self, removed: &[usize]) {
        let shift = |row: usize| removed.binary_search(&row).err().map(|before| row - before);
        self.rows = self.rows.iter().filter_map(|&r| shift(r)).collect();
        self.anchor = self.anchor.and_then(shift);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clicks_pick_rows() {
        let mut s = Selection::default();
        s.set(2);
        s.toggle(5);
        s.toggle(0);
        assert_eq!(s.rows(), [0, 2, 5]);
        s.toggle(2);
        assert_eq!(s.rows(), [0, 5]);
        assert_eq!(s.anchor(), Some(2));
        s.set(3);
        s.extend_to(6);
        assert_eq!(s.rows(), [3, 4, 5, 6]);
        s.extend_to(1);
        assert_eq!(s.rows(), [1, 2, 3]);
        assert_eq!(s.anchor(), Some(3));
        s.all(4);
        assert_eq!(s.rows(), [0, 1, 2, 3]);
        s.step(true, 4);
        assert_eq!((s.rows(), s.anchor()), (&[3][..], Some(3)));
    }

    #[test]
    fn selection_follows_removal() {
        let mut s = Selection::default();
        s.set_all(vec![1, 3, 6]);
        s.after_removal(&[0, 3]);
        assert_eq!(s.rows(), [0, 4]);
        assert_eq!(s.anchor(), Some(4));
        s.after_removal(&[4]);
        assert_eq!(s.rows(), [0]);
        assert_eq!(s.anchor(), None);
    }
}
