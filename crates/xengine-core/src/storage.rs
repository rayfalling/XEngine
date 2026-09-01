//! Type-erased SoA column storage with correct alignment and drop semantics.
//!
//! A [`Column`] owns a contiguous buffer of one component type's data.
//! Internally this is raw memory that respects the component type's
//! alignment and size, so hot-path iteration stays cache friendly.

use std::alloc::{Layout, alloc, dealloc, realloc};
use std::ptr::NonNull;

use super::registry::ComponentDescriptor;

/// SoA column of one component type. Elements are tightly packed.
pub struct Column {
    ptr: NonNull<u8>,
    cap: usize,
    len: usize,
    desc: ComponentDescriptor,
}

// Safety: the column data is only ever read/written through World APIs that
// preserve exclusivity; the descriptor's size/align/drop come from the
// registry, the single source of truth.
unsafe impl Send for Column {}
unsafe impl Sync for Column {}

impl Column {
    /// Creates an empty column for a descriptor.
    pub fn new(desc: ComponentDescriptor) -> Self {
        Self {
            ptr: NonNull::dangling(),
            cap: 0,
            len: 0,
            desc,
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn elem_size(&self) -> usize {
        self.desc.size.max(1)
    }

    fn layout(&self, cap: usize) -> Layout {
        Layout::from_size_align(self.elem_size().saturating_mul(cap), self.desc.align.max(1))
            .expect("column layout")
    }

    fn grow(&mut self) {
        let new_cap = if self.cap == 0 { 8 } else { self.cap * 2 };
        let new_layout = self.layout(new_cap);
        if self.cap == 0 {
            // Safety: layout has non-zero size and valid alignment.
            let ptr = unsafe { alloc(new_layout) };
            self.ptr = match NonNull::new(ptr) {
                Some(p) => p,
                // OOM: abort like the standard allocator instead of UB.
                None => std::alloc::handle_alloc_error(new_layout),
            };
        } else {
            // Safety: the old pointer came from alloc with old_layout.
            let old_layout = self.layout(self.cap);
            // Safety: realloc keeps the old data and the new layout's size
            // is computed with the same element size.
            let ptr = unsafe { realloc(self.ptr.as_ptr(), old_layout, new_layout.size()) };
            self.ptr = match NonNull::new(ptr) {
                Some(p) => p,
                None => std::alloc::handle_alloc_error(new_layout),
            };
        }
        self.cap = new_cap;
    }

    /// Copies a component value into the column (the caller owns the source
    /// bytes' validity for the descriptor's type).
    ///
    /// # Safety
    /// `src` must point to a valid instance matching `desc.size`/`desc.align`
    /// that stays alive for the duration of the copy.
    pub unsafe fn push_copy(&mut self, src: *const u8) {
        if self.len == self.cap {
            self.grow();
        }
        // Safety: layout contract ensures capacity and destination validity.
        unsafe {
            std::ptr::copy_nonoverlapping(
                src,
                self.ptr.as_ptr().add(self.len * self.elem_size()),
                self.desc.size,
            );
        }
        self.len += 1;
    }

    /// Raw pointer to element `i` (no bounds check).
    ///
    /// # Safety
    /// `i` must be `< len`.
    pub fn get_ptr(&self, i: usize) -> *const u8 {
        unsafe { self.ptr.as_ptr().add(i * self.elem_size()) }
    }

    /// Mutable raw pointer to element `i` (no bounds check).
    ///
    /// # Safety
    /// `i` must be `< len`.
    pub fn get_mut_ptr(&mut self, i: usize) -> *mut u8 {
        unsafe { self.ptr.as_ptr().add(i * self.elem_size()) }
    }

    /// Swaps-removes element at `i`: drops it and moves the last element in.
    pub fn remove_swap(&mut self, i: usize) {
        debug_assert!(i < self.len);
        let last = self.len - 1;
        // Safety: element i is live per the invariant above.
        unsafe {
            if let Some(drop_fn) = self.desc.drop_fn {
                drop_fn(self.ptr.as_ptr().add(i * self.elem_size()));
            }
            if i != last {
                std::ptr::copy_nonoverlapping(
                    self.ptr.as_ptr().add(last * self.elem_size()),
                    self.ptr.as_ptr().add(i * self.elem_size()),
                    self.desc.size,
                );
            }
        }
        self.len -= 1;
    }

    /// Moves the last element into slot `i` and shrinks, WITHOUT dropping
    /// anything. Used for archetype migrations where the value at `i` was
    /// already moved out (bitwise) by [`Column::take_bytes`].
    pub fn move_swap(&mut self, i: usize) {
        debug_assert!(i < self.len);
        let last = self.len - 1;
        // Safety: i is a live (logically moved-out) slot; the last element
        // is bitwise moved into it, and len shrinks so it is not re-dropped.
        unsafe {
            if i != last {
                std::ptr::copy_nonoverlapping(
                    self.ptr.as_ptr().add(last * self.elem_size()),
                    self.ptr.as_ptr().add(i * self.elem_size()),
                    self.desc.size,
                );
            }
        }
        self.len -= 1;
    }

    /// Bitwise-copies the value at `i` into a fresh buffer WITHOUT dropping
    /// it. The caller becomes responsible for the value (e.g. it will be
    /// moved into another column); the source slot must be swap-removed
    /// without dropping afterwards.
    ///
    /// # Safety
    /// `i` must be `< len`, and the caller must treat the value as moved.
    pub unsafe fn take_bytes(&mut self, i: usize) -> Vec<u8> {
        debug_assert!(i < self.len);
        let mut buf = vec![0u8; self.desc.size];
        // Safety: both pointers are valid for desc.size bytes.
        unsafe {
            std::ptr::copy_nonoverlapping(
                self.ptr.as_ptr().add(i * self.elem_size()),
                buf.as_mut_ptr(),
                self.desc.size,
            );
        }
        buf
    }

    /// Drops the element at `i` without moving anything.
    pub fn drop_at(&mut self, i: usize) {
        debug_assert!(i < self.len);
        // Safety: element i is live per the caller's invariant.
        unsafe {
            if let Some(drop_fn) = self.desc.drop_fn {
                drop_fn(self.ptr.as_ptr().add(i * self.elem_size()));
            }
        }
    }

    /// Drops every element and resets the length (buffer is retained).
    pub fn clear(&mut self) {
        for i in 0..self.len {
            self.drop_at(i);
        }
        self.len = 0;
    }
}

impl Drop for Column {
    fn drop(&mut self) {
        self.clear();
        if self.cap > 0 {
            // Safety: ptr came from alloc/realloc with the same layout.
            unsafe {
                dealloc(self.ptr.as_ptr(), self.layout(self.cap));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq)]
    struct Marker(i32);

    #[test]
    fn push_and_read_back() {
        let desc = ComponentDescriptor::of::<Marker>(false);
        let mut col = Column::new(desc);
        let v = Marker(42);
        // Safety: &v is a valid live instance and stays alive during copy.
        unsafe { col.push_copy(&v as *const Marker as *const u8) };
        assert_eq!(col.len(), 1);
        // Safety: the column element holds a Marker here.
        let got = unsafe { &*(col.get_ptr(0) as *const Marker) };
        assert_eq!(got, &Marker(42));
    }

    #[test]
    fn remove_swap_drops_and_moves_last() {
        let mut col = Column::new(ComponentDescriptor::of::<Marker>(false));
        let a = Marker(1);
        let b = Marker(2);
        unsafe {
            col.push_copy(&a as *const Marker as *const u8);
            col.push_copy(&b as *const Marker as *const u8);
        }
        col.remove_swap(0);
        assert_eq!(col.len(), 1);
        // Safety: element 0 now holds the moved last element.
        assert_eq!(unsafe { &*(col.get_ptr(0) as *const Marker) }, &Marker(2));
    }

    #[test]
    fn drop_function_is_called_once_per_element() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static COUNT: AtomicUsize = AtomicUsize::new(0);
        #[derive(Debug)]
        #[allow(dead_code)]
        struct DropCounter(usize);
        let drop_fn: fn(*mut u8) = |ptr| {
            // Safety: ptr is a live DropCounter (drops happen on live data).
            let _ = unsafe { &*(ptr as *const DropCounter) };
            COUNT.fetch_add(1, Ordering::SeqCst);
        };
        let desc = ComponentDescriptor {
            size: std::mem::size_of::<DropCounter>(),
            align: std::mem::align_of::<DropCounter>(),
            scriptable: false,
            drop_fn: Some(drop_fn),
        };
        let vals = [DropCounter(1), DropCounter(2)];
        let mut col = Column::new(desc);
        for v in &vals {
            // Safety: v is a live instance for the whole copy.
            unsafe { col.push_copy(v as *const DropCounter as *const u8) };
        }
        col.clear();
        assert_eq!(
            COUNT.load(Ordering::SeqCst),
            2,
            "drop must run once per element"
        );
        drop(col);
        assert_eq!(
            COUNT.load(Ordering::SeqCst),
            2,
            "clear empties; Drop must not re-drop"
        );
    }
}
