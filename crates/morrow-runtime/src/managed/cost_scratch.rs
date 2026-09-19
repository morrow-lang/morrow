//! Small validation graphs use stack storage; larger graphs retain Vec semantics.
use std::ops::{Deref, DerefMut};

const INLINE: usize = 16;

pub(super) struct Scratch<T: Copy + Default> {
    inline: [T; INLINE],
    len: usize,
    overflow: Vec<T>,
}

impl<T: Copy + Default> Default for Scratch<T> {
    fn default() -> Self {
        Self {
            inline: [T::default(); INLINE],
            len: 0,
            overflow: Vec::new(),
        }
    }
}

impl<T: Copy + Default> Scratch<T> {
    pub(super) fn push(&mut self, value: T) {
        if self.overflow.capacity() == 0 && self.len < INLINE {
            self.inline[self.len] = value;
        } else {
            if self.overflow.capacity() == 0 {
                self.overflow.reserve(INLINE * 2);
                self.overflow.extend_from_slice(&self.inline);
            }
            self.overflow.push(value);
        }
        self.len += 1;
    }

    pub(super) fn pop(&mut self) -> Option<T> {
        self.len = self.len.checked_sub(1)?;
        if self.overflow.capacity() == 0 {
            Some(self.inline[self.len])
        } else {
            self.overflow.pop()
        }
    }
}

impl<T: Copy + Default> Deref for Scratch<T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        if self.overflow.capacity() == 0 {
            &self.inline[..self.len]
        } else {
            &self.overflow
        }
    }
}

impl<T: Copy + Default> DerefMut for Scratch<T> {
    fn deref_mut(&mut self) -> &mut [T] {
        if self.overflow.capacity() == 0 {
            &mut self.inline[..self.len]
        } else {
            &mut self.overflow
        }
    }
}
