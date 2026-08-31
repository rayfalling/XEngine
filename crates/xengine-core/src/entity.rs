//! Generational entity handles.

/// A generational entity handle: `(index, generation)`.
///
/// The index addresses a slot; the generation detects reuse after destroy.
/// A handle is only valid while its generation matches the slot's current
/// generation. All operations are O(1).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Entity {
    index: u32,
    generation: u32,
}

impl Entity {
    /// Creates a handle from raw parts. The index is the slot address, the
    /// generation the version of that slot.
    pub fn from_parts(index: u32, generation: u32) -> Self {
        Self { index, generation }
    }

    /// The slot index (address) of this handle.
    pub fn index(&self) -> u32 {
        self.index
    }

    /// The generation of this handle.
    pub fn generation(&self) -> u32 {
        self.generation
    }
}

/// Allocates entity indices with generations and a reuse recycle list.
///
/// A destroyed entity's index returns to the free list; on reuse the
/// generation is incremented so old handles become stale. When the
/// generation would wrap to zero the slot is retired permanently.
#[derive(Default)]
pub struct EntityAllocator {
    /// Next generation to hand out per index (bumped after each allocation).
    generations: Vec<u32>,
    /// `true` once an index is retired after generation overflow.
    retired: Vec<bool>,
    /// Indices of destroyed entities available for reuse.
    free: Vec<u32>,
    /// First never-allocated index.
    next: u32,
}

impl EntityAllocator {
    /// Creates an empty allocator.
    pub fn new() -> Self {
        Self::default()
    }

    /// Allocates a fresh entity handle. O(1) amortized.
    pub fn allocate(&mut self) -> Entity {
        let index = if let Some(reused) = self.free.pop() {
            reused
        } else {
            let idx = self.next;
            self.next += 1;
            self.generations.push(0);
            self.retired.push(false);
            idx
        };
        debug_assert!(!self.retired[index as usize]);
        let generation = self.generations[index as usize];
        let next_gen = generation.wrapping_add(1);
        self.generations[index as usize] = next_gen;
        if next_gen == 0 {
            // Wrap: retire the slot, it is never handed out again.
            self.retired[index as usize] = true;
            self.free.retain(|&i| i != index);
        }
        Entity::from_parts(index, generation)
    }

    /// Returns the index to the free list for reuse. If the slot is retired
    /// it is not reused.
    pub fn release(&mut self, index: u32) {
        if index >= self.retired.len() as u32 {
            return;
        }
        if self.retired[index as usize] {
            return;
        }
        // The generation was already bumped by allocate(); reuse is safe.
        if !self.free.contains(&index) {
            self.free.push(index);
        }
    }

    /// Current generation counter for an index (used for slot validation).
    pub fn generation_of(&self, index: u32) -> Option<u32> {
        if index >= self.generations.len() as u32 || self.retired[index as usize] {
            None
        } else {
            Some(self.generations[index as usize])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocation_and_reuse_bumps_generation() {
        let mut alloc = EntityAllocator::new();
        let a = alloc.allocate();
        let b = alloc.allocate();
        assert_eq!(a.index(), 0);
        assert_eq!(a.generation(), 0);
        assert_eq!(b.index(), 1);
        assert_eq!(b.generation(), 0);

        let index = a.index();
        alloc.release(a.index());
        let c = alloc.allocate();
        assert_eq!(c.index(), index);
        assert_eq!(c.generation(), 1, "reuse must bump the generation");
    }

    #[test]
    fn slot_is_retired_after_generation_overflow() {
        let mut alloc = EntityAllocator::new();
        let first = alloc.allocate();
        // Force the generation to the maximum so the next allocation wraps.
        alloc.generations[first.index() as usize] = u32::MAX;
        alloc.release(first.index());
        let wrapped = alloc.allocate();
        assert_eq!(wrapped.generation(), u32::MAX);
        // The next release-and-allocate must NOT reuse the retired slot.
        alloc.release(first.index());
        assert!(alloc.generation_of(first.index()).is_none());
        let after = alloc.allocate();
        assert_ne!(after.index(), first.index());
    }

    #[test]
    fn stale_detection_via_generation() {
        let mut alloc = EntityAllocator::new();
        let a = alloc.allocate();
        alloc.release(a.index());
        let b = alloc.allocate();
        assert_ne!(a.generation(), b.generation(), "old handle must be stale");
    }
}
