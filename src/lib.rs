//! A memory-contiguous `Vec<Vec<T>>` with resizable inner vecs.
#![doc(html_root_url = "https://docs.rs/vv")]
#![crate_name = "vv"]
#![warn(
    missing_debug_implementations,
    trivial_casts,
    trivial_numeric_casts,
    unused_lifetimes,
    unused_import_braces,
    clippy::shadow_unrelated
)]
#![deny(missing_docs, unsafe_op_in_unsafe_fn)]

use std::{
    mem,
    ops::{Index, IndexMut, Range},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Run {
    len: u32,
    start: u32,
}

const TOMBSTONE_BIT: u32 = 1 << (u32::BITS - 1);

impl Run {
    #[inline(always)]
    pub fn is_dead(self) -> bool {
        self.start & TOMBSTONE_BIT != 0
    }
    #[inline(always)]
    pub fn start(self) -> usize {
        (self.start & !TOMBSTONE_BIT) as usize
    }
    #[inline(always)]
    pub fn end(self) -> usize {
        self.start() + self.len as usize
    }
    #[inline(always)]
    pub fn range(self) -> Range<usize> {
        self.start()..self.end()
    }
}

/// The star of the show, holds variably resizable vecs in contiguous memory.
#[derive(Debug)]
pub struct Vv<T> {
    count: usize,
    data: Vec<T>,
    runs: Vec<Run>,
}

impl<T> Vv<T> {
    /// Create a new `Vv`.
    #[inline(always)]
    pub fn new() -> Self {
        Self {
            count: 0,
            data: Vec::new(),
            runs: Vec::new(),
        }
    }

    /// Pushes a vec to the `Vv`.
    #[inline(always)]
    pub fn push(&mut self, iter: impl IntoIterator<Item = T>) -> usize {
        let iter = iter.into_iter();
        self.count = self.count.wrapping_add(1);
        let mut prev = self.runs.len().wrapping_sub(1);
        while self.runs.get(prev).is_some_and(|r| r.is_dead()) {
            prev = prev.wrapping_sub(1);
        }
        let start = self.runs.get(prev).map_or_else(|| 0, |r| r.end());
        let mut index = start;
        for t in iter {
            if index < self.data.len() {
                self.data[index] = t;
            } else {
                self.data.push(t);
            }
            index = index.wrapping_add(1);
        }
        let run = Run {
            len: index.wrapping_sub(start) as u32,
            start: start as u32,
        };
        let new = prev.wrapping_add(1);
        if new < self.runs.len() {
            self.runs[new] = run;
        } else {
            self.runs.push(run);
        }
        new
    }

    /// Pulls the last inner vec from the `Vv`. This does not drop contents immediately.
    #[inline(always)]
    pub fn pull(&mut self, index: usize) {
        if let Some(run) = self.runs.get_mut(index) {
            if run.is_dead() {
                return;
            }
            self.count = self.count.wrapping_sub(1);
            run.start |= TOMBSTONE_BIT;
        }
    }

    /// How many inner vecs are in this `Vv`.
    #[inline(always)]
    pub fn len(&self) -> usize {
        self.count
    }

    /// Is the `Vv` empty?
    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.count > 0
    }

    /// Immutably borrows the vec at `index` as a slice, if it exists.
    #[inline(always)]
    pub fn get(&self, index: usize) -> Option<&[T]> {
        let run = self.runs[index];
        if run.is_dead() {
            None
        } else {
            Some(&self.data[run.range()])
        }
    }

    /// Mutably borrows the vec at `index` as a slice, if it exists.
    #[inline(always)]
    pub fn get_mut(&mut self, index: usize) -> Option<&mut [T]> {
        let run = self.runs[index];
        if run.is_dead() {
            None
        } else {
            Some(&mut self.data[run.range()])
        }
    }

    #[inline(always)]
    fn try_insert_before(&mut self, index: usize, insert: usize, t: T) -> Option<T> {
        if let Some(run) = self.runs.get(index) {
            if run.start == 0 {
                return Some(t);
            }
            let mut prev = index.wrapping_sub(1);
            while self.runs.get(prev).is_some_and(|r| r.is_dead()) {
                prev = prev.wrapping_sub(1);
            }
            let new_start = run.start().wrapping_sub(1);
            if self.runs.get(prev).map_or_else(|| 0, |r| r.end()) <= new_start {
                let new_end = run.start().wrapping_add(insert);
                self.data[new_start..new_end].rotate_left(1);
                self.data[new_end.wrapping_sub(1)] = t;
                self.runs[index].len = self.runs[index].len.wrapping_add(1);
                self.runs[index].start = self.runs[index].start.wrapping_sub(1);
                return None;
            }
        }
        Some(t)
    }

    #[inline(always)]
    fn try_insert_after(&mut self, index: usize, insert: usize, t: T) -> Option<T> {
        if let Some(run) = self.runs.get(index) {
            let mut next = index.wrapping_add(1);
            while self.runs.get(next).is_some_and(|r| r.is_dead()) {
                next = next.wrapping_add(1);
            }
            let new_end = run.end().wrapping_add(1);
            if self.runs.get(next).is_none_or(|r| new_end < r.start()) {
                let new_start = run.start().wrapping_add(insert);
                if new_end > self.data.len() {
                    self.data.push(t);
                    self.data[new_start..new_end].rotate_right(1);
                    self.runs[index].len = self.runs[index].len.wrapping_add(1);
                    return None;
                }
                self.data[new_start..new_end].rotate_right(1);
                self.data[new_start] = t;
                self.runs[index].len = self.runs[index].len.wrapping_add(1);
                return None;
            }
        }
        Some(t)
    }

    /// Attempts to insert at `insert` the item `t` into the inner vec `index`.
    /// Returns `None` on success, and returns `Some(t)` if it failed to insert due to insufficient space.
    #[inline(always)]
    pub fn try_insert(&mut self, index: usize, insert: usize, mut t: T) -> Option<T> {
        if let Some(run) = self.runs.get(index) {
            debug_assert!(insert <= run.len as usize);
            debug_assert!(!run.is_dead());
            if insert < run.len as usize - insert {
                t = self.try_insert_before(index, insert, t)?;
            }
            t = self.try_insert_after(index, insert, t)?;
            t = self.try_insert_before(index, insert, t)?;
        } else if index == 0 {
            self.data.push(t);
            self.runs.push(Run { start: 0, len: 1 });
            self.count = self.count.wrapping_add(1);
            return None;
        }
        Some(t)
    }

    /// Insert at `insert` the item `t` into the inner vec `index`, reallocating if needed.
    /// Returns the index of the vec inserted to. This may be different
    /// than the `index` provided as input due to internal reallocation.
    #[inline(always)]
    pub fn insert(&mut self, index: usize, insert: usize, t: T) -> usize
    where
        T: Copy,
    {
        if let Some(t) = self.try_insert(index, insert, t) {
            let new = self.push([]);
            let mut inserted = false;
            for (i, e) in self.runs[index].range().enumerate() {
                if i == insert {
                    self.grow_last(t);
                    inserted = true;
                }
                self.grow_last(self.data[e]);
            }
            if !inserted {
                self.grow_last(t);
            }
            if index != new {
                self.pull(index);
            }
            return new;
        }
        index
    }

    /// Appends to the last-in-memory vec, guaranteeing that a inner vec reallocation is not needed.
    /// Note this may still reallocate the backing memory, but that has no user-facing effects.
    #[inline(always)]
    pub fn grow_last(&mut self, t: T) {
        let mut prev = self.runs.len().wrapping_sub(1);
        while self.runs.get(prev).is_some_and(|r| r.is_dead()) {
            prev = prev.wrapping_sub(1);
        }
        if let Some(run) = self.runs.get_mut(prev) {
            if self.data.len() <= run.end() {
                self.data.push(t);
            } else {
                self.data[run.end()] = t;
            }
            run.len += 1;
        } else {
            self.data.push(t);
            self.runs.push(Run { start: 0, len: 1 });
            self.count += 1;
        }
    }

    /// Remove a specific vec.
    #[inline(always)]
    pub fn remove(&mut self, index: usize, remove: usize) {
        if let Some(run) = self.runs.get_mut(index) {
            debug_assert!(remove < run.len as usize);
            if remove < run.len as usize - remove {
                let range = run.start()..run.start().wrapping_add(remove).wrapping_add(1);
                if range.len() > 1 {
                    self.data[range].rotate_right(1);
                }
                run.start += 1;
            } else {
                let range = run.start().wrapping_add(remove)..run.end();
                if range.len() > 1 {
                    self.data[range].rotate_left(1);
                }
            }
            run.len -= 1;
        }
    }

    /// Remove a range of vecs.
    #[inline(always)]
    pub fn remove_range(&mut self, index: usize, remove: Range<usize>) {
        if let Some(run) = self.runs.get_mut(index) {
            debug_assert!(remove.end <= run.len as usize);
            if remove.start < run.len as usize - remove.end {
                let range = run.start()..run.start().wrapping_add(remove.end);
                if range.len() > remove.len() {
                    self.data[range].rotate_right(remove.len());
                }
                run.start += remove.len() as u32;
            } else {
                let range = run.start().wrapping_add(remove.start)..run.end();
                if range.len() > remove.len() {
                    self.data[range].rotate_left(remove.len());
                }
            }
            run.len -= remove.len() as u32;
        }
    }

    /// Compacts the `Vv` to use as little space as possible for `T: Copy`.
    #[inline(always)]
    pub fn compact(&mut self)
    where
        T: Copy,
    {
        let mut rcursor = 0;
        let mut dcursor = 0;
        let mut skip = true;
        for r in 0..self.runs.len() {
            let run = self.runs[r];
            if run.is_dead() {
                skip = false;
                continue;
            }
            self.runs[rcursor] = self.runs[r];
            rcursor += 1;
            if skip {
                continue;
            }
            for d in run.range() {
                self.data[dcursor] = self.data[d];
                dcursor += 1;
            }
        }
        self.runs.truncate(rcursor);
        self.data.truncate(dcursor);
    }

    /// Compacts the `Vv` to use as little space as possible for `T: !Copy`.
    #[inline(always)]
    pub fn compact2(&mut self) {
        debug_assert!(
            mem::needs_drop::<T>(),
            "Use `compact` for better performance on `T: Copy` types"
        );
        let mut rcursor = 0;
        let mut dcursor = 0;
        let mut skip = true;
        for r in 0..self.runs.len() {
            let run = self.runs[r];
            if run.is_dead() {
                skip = false;
                continue;
            }
            self.runs[rcursor] = self.runs[r];
            rcursor += 1;
            if skip {
                continue;
            }
            for d in run.range() {
                self.data.swap(dcursor, d);
                dcursor += 1;
            }
        }
        self.runs.truncate(rcursor);
        self.data.truncate(dcursor);
    }

    /// Iterate over the inner vecs.
    #[inline]
    pub fn iter(&self) -> Iter<'_, T> {
        Iter {
            list: self,
            index: 0,
        }
    }
}

impl<T> Default for Vv<T> {
    fn default() -> Self {
        Self {
            count: Default::default(),
            data: Default::default(),
            runs: Default::default(),
        }
    }
}

impl<T> Clone for Vv<T>
where
    T: Clone,
{
    fn clone(&self) -> Self {
        Self {
            count: self.count,
            data: self.data.clone(),
            runs: self.runs.clone(),
        }
    }
}

impl<T> IndexMut<usize> for Vv<T> {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        self.get_mut(index).unwrap()
    }
}

impl<T> Index<usize> for Vv<T> {
    type Output = [T];

    fn index(&self, index: usize) -> &Self::Output {
        self.get(index).unwrap()
    }
}

impl<'a, T> IntoIterator for &'a Vv<T> {
    type Item = usize;
    type IntoIter = Iter<'a, T>;

    #[inline]
    fn into_iter(self) -> Iter<'a, T> {
        self.iter()
    }
}

/// Iterator over a `Vv`.
#[derive(Debug)]
pub struct Iter<'a, T> {
    list: &'a Vv<T>,
    index: usize,
}

impl<'a, T> Iterator for Iter<'a, T> {
    type Item = usize;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.index < self.list.runs.len() {
            while self.list.runs[self.index].is_dead() {
                self.index += 1;
                if self.index == self.list.runs.len() {
                    return None;
                }
            }
            let index = self.index;
            self.index += 1;
            Some(index)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod test {
    use crate::{Run, Vv};

    #[test]
    fn push() {
        let mut vv = Vv::<i32>::new();
        vv.push([2, 3, 4]);
        assert_eq!(vv.data.len(), 3);
        assert_eq!(vv.data[0..3], [2, 3, 4]);
        assert_eq!(vv.runs[0], Run { len: 3, start: 0 });
        vv.push([6, 7]);
        assert_eq!(vv.data.len(), 5);
        assert_eq!(vv.data[0..5], [2, 3, 4, 6, 7]);
        assert_eq!(vv.runs[0], Run { len: 3, start: 0 });
        assert_eq!(vv.runs[1], Run { len: 2, start: 3 });
        vv.push([9]);
        assert_eq!(vv.data.len(), 6);
        assert_eq!(vv.data[0..6], [2, 3, 4, 6, 7, 9]);
        assert_eq!(vv.runs[0], Run { len: 3, start: 0 });
        assert_eq!(vv.runs[1], Run { len: 2, start: 3 });
        assert_eq!(vv.runs[2], Run { len: 1, start: 5 });
    }

    #[test]
    fn pull() {
        let mut vv = Vv::<i32>::new();
        vv.push([2, 3, 4]);
        vv.pull(0);
        assert_eq!(vv.len(), 0);
        vv.pull(0);
        assert_eq!(vv.len(), 0);
        vv.push([6, 7]);
        assert_eq!(vv.data.len(), 3);
        assert_eq!(vv.data[0..2], [6, 7]);
        assert_eq!(vv.runs[0], Run { len: 2, start: 0 });
        vv.push([9, 8]);
        assert_eq!(vv.data.len(), 4);
        assert_eq!(vv.data[0..4], [6, 7, 9, 8]);
        assert_eq!(vv.runs[0], Run { len: 2, start: 0 });
        assert_eq!(vv.runs[1], Run { len: 2, start: 2 });
        vv.pull(0);
        assert_eq!(vv.data.len(), 4);
        assert_eq!(vv.data[2..4], [9, 8]);
        assert!(vv.runs[0].is_dead());
        assert_eq!(vv.runs[1], Run { len: 2, start: 2 });
        vv.push([1, 2]);
        assert_eq!(vv.data.len(), 6);
        assert_eq!(vv.data[2..6], [9, 8, 1, 2]);
        assert!(vv.runs[0].is_dead());
        assert_eq!(vv.runs[1], Run { len: 2, start: 2 });
        assert_eq!(vv.runs[2], Run { len: 2, start: 4 });
    }

    #[test]
    fn len() {
        let mut vv = Vv::<i32>::new();
        assert_eq!(vv.len(), 0);
        vv.push([2]);
        assert_eq!(vv.len(), 1);
        vv.pull(0);
        assert_eq!(vv.len(), 0);
    }

    #[test]
    fn try_insert_before() {
        let mut vv = Vv::<i32>::new();
        assert_eq!(vv.try_insert_before(0, 0, 2), Some(2));
        assert_eq!(vv.count, 0);
        vv.push([2]);
        assert_eq!(vv.try_insert_before(0, 0, 2), Some(2));
        assert_eq!(vv.count, 1);
        vv.push([3]);
        assert_eq!(vv.runs[0], Run { len: 1, start: 0 });
        assert_eq!(vv.runs[1], Run { len: 1, start: 1 });
        assert_eq!(vv.try_insert_before(1, 0, 5), Some(5));
        assert_eq!(vv.data[0..2], [2, 3]);
        assert_eq!(vv.count, 2);
        vv.pull(0);
        assert_eq!(vv.count, 1);
        assert_eq!(vv.try_insert_before(1, 0, 4), None);
        assert_eq!(vv.runs[1], Run { len: 2, start: 0 });
        assert_eq!(vv.count, 1);
        assert_eq!(vv.data[0..2], [4, 3]);
        vv.push([5, 6]);
        vv.push([7]);
        vv.pull(2);
        assert_eq!(vv.try_insert_before(3, 0, 2), None);
        assert_eq!(vv.data[3..5], [2, 7]);
        assert_eq!(vv.runs[3], Run { len: 2, start: 3 });
        assert_eq!(vv.try_insert_before(3, 1, 6), None);
        assert_eq!(vv.runs[3], Run { len: 3, start: 2 });
        assert_eq!(vv.data[2..5], [2, 6, 7]);
        assert_eq!(vv.try_insert_before(3, 3, 9), Some(9));
        vv.pull(1);
        assert_eq!(vv.try_insert_before(3, 3, 9), None);
        assert_eq!(vv.runs[3], Run { len: 4, start: 1 });
        assert_eq!(vv.data[1..5], [2, 6, 7, 9]);
    }

    #[test]
    fn try_insert_after() {
        let mut vv = Vv::<i32>::new();
        assert_eq!(vv.try_insert_after(0, 0, 2), Some(2));
        assert_eq!(vv.count, 0);
        vv.push([2]);
        assert_eq!(vv.try_insert_after(0, 0, 3), None);
        assert_eq!(vv.data[0..2], [3, 2]);
        assert_eq!(vv.count, 1);
        vv.push([4]);
        assert_eq!(vv.runs[0], Run { len: 2, start: 0 });
        assert_eq!(vv.runs[1], Run { len: 1, start: 2 });
        assert_eq!(vv.try_insert_after(0, 1, 5), Some(5));
        assert_eq!(vv.try_insert_after(1, 1, 5), None);
        assert_eq!(vv.data[0..4], [3, 2, 4, 5]);
        vv.push([8, 7]);
        vv.pull(1);
        assert_eq!(vv.try_insert_after(0, 1, 6), None);
        assert_eq!(vv.data[0..3], [3, 6, 2]);
        assert_eq!(vv.runs[0], Run { len: 3, start: 0 });
        assert!(vv.runs[1].is_dead());
        assert_eq!(vv.runs[2], Run { len: 2, start: 4 });
    }

    #[test]
    fn try_insert() {
        let mut vv = Vv::<i32>::new();
        assert_eq!(vv.try_insert(0, 0, 5), None);
        assert_eq!(vv.count, 1);
        assert_eq!(vv.runs[0], Run { len: 1, start: 0 });
        assert_eq!(vv.data[0..1], [5]);
        assert_eq!(vv.try_insert(0, 0, 3), None);
        assert_eq!(vv.data[0..2], [3, 5]);
        assert_eq!(vv.count, 1);
        vv.push([4]);
        assert_eq!(vv.runs[0], Run { len: 2, start: 0 });
        assert_eq!(vv.runs[1], Run { len: 1, start: 2 });
        assert_eq!(vv.try_insert(0, 1, 2), Some(2));
        assert_eq!(vv.try_insert(1, 1, 2), None);
        assert_eq!(vv.data[0..4], [3, 5, 4, 2]);
        vv.push([8, 7]);
        vv.pull(1);
        assert_eq!(vv.try_insert(0, 1, 6), None);
        assert_eq!(vv.data[0..3], [3, 6, 5]);
        assert_eq!(vv.runs[0], Run { len: 3, start: 0 });
        assert!(vv.runs[1].is_dead());
        assert_eq!(vv.runs[2], Run { len: 2, start: 4 });
        assert_eq!(vv.try_insert(2, 0, 9), None);
        assert_eq!(vv.data[0..6], [3, 6, 5, 9, 8, 7]);
    }

    #[test]
    fn insert() {
        let mut vv = Vv::<i32>::new();
        assert_eq!(vv.insert(0, 0, 5), 0);
        assert_eq!(vv.count, 1);
        assert_eq!(vv.runs[0], Run { len: 1, start: 0 });
        assert_eq!(vv.data[0..1], [5]);
        assert_eq!(vv.insert(0, 0, 3), 0);
        assert_eq!(vv.runs[0], Run { len: 2, start: 0 });
        assert_eq!(vv.data[0..2], [3, 5]);
        assert_eq!(vv.count, 1);
        assert_eq!(vv.insert(1, 0, 3), 1);
        assert_eq!(vv.runs[0], Run { len: 2, start: 0 });
        assert_eq!(vv.runs[1], Run { len: 1, start: 2 });
        assert_eq!(vv.data[0..3], [3, 5, 3]);
        assert_eq!(vv.insert(0, 1, 2), 2);
        assert_eq!(vv.data[2..6], [3, 3, 2, 5]);
        assert_eq!(vv.insert(1, 1, 6), 1);
        assert_eq!(vv.data[1..6], [3, 6, 3, 2, 5]);
        assert_eq!(vv.runs[1], Run { len: 2, start: 1 });
        assert_eq!(vv.runs[2], Run { len: 3, start: 3 });
        assert_eq!(vv.insert(1, 2, 9), 1);
        assert_eq!(vv.data[0..6], [3, 6, 9, 3, 2, 5]);
        assert_eq!(vv.runs[1], Run { len: 3, start: 0 });
        assert_eq!(vv.insert(1, 3, 1), 3);
        assert_eq!(vv.runs[2], Run { len: 3, start: 3 });
        assert_eq!(vv.runs[3], Run { len: 4, start: 6 });
        assert_eq!(vv.data[3..10], [3, 2, 5, 3, 6, 9, 1]);
    }

    #[test]
    fn grow_last() {
        let mut vv = Vv::<i32>::new();
        vv.grow_last(2);
        assert_eq!(vv.count, 1);
        assert_eq!(vv.runs[0], Run { len: 1, start: 0 });
        assert_eq!(vv.data[0], 2);
        vv.grow_last(3);
        assert_eq!(vv.count, 1);
        assert_eq!(vv.runs[0], Run { len: 2, start: 0 });
        assert_eq!(vv.data[0..2], [2, 3]);
        vv.push([4]);
        assert_eq!(vv.count, 2);
        assert_eq!(vv.runs[0], Run { len: 2, start: 0 });
        assert_eq!(vv.runs[1], Run { len: 1, start: 2 });
        assert_eq!(vv.data[0..3], [2, 3, 4]);
        vv.grow_last(5);
        assert_eq!(vv.count, 2);
        assert_eq!(vv.runs[0], Run { len: 2, start: 0 });
        assert_eq!(vv.runs[1], Run { len: 2, start: 2 });
        assert_eq!(vv.data[0..4], [2, 3, 4, 5]);
    }

    #[test]
    fn remove() {
        let mut vv = Vv::<i32>::new();
        vv.push([5, 6]);
        vv.remove(0, 0);
        assert_eq!(vv.runs[0], Run { len: 1, start: 1 });
        assert_eq!(vv.data[1..2], [6]);
        vv.push([7, 8, 9]);
        vv.remove(1, 1);
        assert_eq!(vv.runs[1], Run { len: 2, start: 3 });
        assert_eq!(vv.data[3..5], [7, 9]);
        vv.remove(1, 1);
        assert_eq!(vv.runs[1], Run { len: 1, start: 3 });
        assert_eq!(vv.data[3..4], [7]);
    }

    #[test]
    fn remove_range() {
        let mut vv = Vv::<i32>::new();
        vv.push([5, 6]);
        vv.remove_range(0, 0..1);
        assert_eq!(vv.runs[0], Run { len: 1, start: 1 });
        assert_eq!(vv.data[1..2], [6]);
        vv.push([7, 8, 9]);
        vv.remove_range(1, 1..2);
        assert_eq!(vv.runs[1], Run { len: 2, start: 2 });
        assert_eq!(vv.data[2..4], [7, 9]);
        vv.remove_range(1, 1..2);
        assert_eq!(vv.runs[1], Run { len: 1, start: 2 });
        assert_eq!(vv.data[2..3], [7]);
        vv.push([5, 4, 3, 2, 1]);
        vv.remove_range(2, 1..4);
        assert_eq!(vv.runs[2], Run { len: 2, start: 3 });
        assert_eq!(vv.data[3..5], [5, 1]);
    }
}

//get
//get_mut
//compact
//compact2
//iter
//default
//clone
//into_iter
//next
