//! Archetype: a group of entities sharing the exact same component type set.

use std::any::TypeId;

use super::registry::ComponentDescriptor;
use super::storage::Column;

/// One archetype: same component type set, SoA columns, row-major entity
/// indices. TypeIds are kept in registry registration order so the drop
/// order is deterministic.
pub struct Archetype {
    id: usize,
    types: Vec<TypeId>,
    pub(crate) columns: Vec<Column>,
    /// Entity slot indices, one per row.
    pub(crate) entities: Vec<u32>,
}

impl Archetype {
    /// Creates an empty archetype. `descriptors` aligns with `types`.
    pub fn new(id: usize, types: Vec<TypeId>, descriptors: &[ComponentDescriptor]) -> Self {
        debug_assert_eq!(types.len(), descriptors.len());
        let columns = descriptors.iter().copied().map(Column::new).collect();
        Self {
            id,
            types,
            columns,
            entities: Vec::new(),
        }
    }

    pub fn id(&self) -> usize {
        self.id
    }

    pub fn types(&self) -> &[TypeId] {
        &self.types
    }

    pub fn len(&self) -> usize {
        self.entities.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }

    pub fn entity_at(&self, row: usize) -> u32 {
        self.entities[row]
    }

    /// Does this archetype hold the given component type?
    pub fn has(&self, t: &TypeId) -> bool {
        self.types.contains(t)
    }

    /// Column index for a component type.
    pub fn column_index(&self, t: &TypeId) -> Option<usize> {
        self.types.iter().position(|x| x == t)
    }

    /// Appends a row. `sources` must contain one live pointer per column,
    /// in column order, each matching the column descriptor.
    ///
    /// # Safety
    /// Each source pointer must be valid for `desc.size` bytes.
    pub unsafe fn push_row(&mut self, entity_index: u32, sources: &[*const u8]) {
        debug_assert_eq!(sources.len(), self.columns.len());
        for (col, src) in self.columns.iter_mut().zip(sources) {
            // Safety: delegated to the caller's contract.
            unsafe { col.push_copy(*src) };
        }
        self.entities.push(entity_index);
    }

    /// Removes row `row` completely: drops every column's value at the row,
    /// then swap-moves the last row in. Returns the entity index that was
    /// **moved into** `row` (i.e. the last row's entity, or `None` when `row`
    /// was already the last row and no entity moved).
    pub fn remove_row(&mut self, row: usize) -> Option<u32> {
        debug_assert!(row < self.len());
        for col in &mut self.columns {
            col.remove_swap(row);
        }
        let last = self.len() - 1;
        if row != last {
            let moved_in = self.entities[last];
            self.entities.swap_remove(row);
            Some(moved_in)
        } else {
            self.entities.pop();
            None
        }
    }

    /// Drops every element in every column (registration order) and clears
    /// the entity list. Caller must also invalidate the matching slots.
    pub fn drop_all(&mut self) {
        for col in &mut self.columns {
            col.clear();
        }
        self.entities.clear();
    }

    /// Removes row `row` from an archetype (dropping the value at a specific
    /// column `drop_col` while bitwise-moving every other column out to the
    /// target archetype first). Used for migration. Returns the entity index
    /// that was **moved into** `row` (the last row's entity), or `None` when
    /// `row` was already the last row.
    ///
    /// # Safety
    /// `drop_col` must be a valid column index whose value at `row` is still
    /// owned by this archetype.
    pub unsafe fn remove_row_migrate(
        &mut self,
        row: usize,
        drop_col: Option<usize>,
    ) -> Option<u32> {
        debug_assert!(row < self.len());
        for (ci, col) in self.columns.iter_mut().enumerate() {
            if Some(ci) == drop_col {
                // Safety: value at row is still owned; drop it in place.
                col.remove_swap(row);
            } else {
                col.move_swap(row);
            }
        }
        let last = self.len() - 1;
        if row != last {
            let moved_in = self.entities[last];
            self.entities.swap_remove(row);
            Some(moved_in)
        } else {
            self.entities.pop();
            None
        }
    }
}
