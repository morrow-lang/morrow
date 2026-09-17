//! Identity independent of actor storage: a reusable slot plus a monotonic
//! generation. Validation never dereferences an actor, so a PID stays meaningful
//! after its actor is freed and after another actor occupies its slot.
// Wired into the PID path by the next step; until then the crate gate's
// `-D warnings` would reject the not-yet-called constructors.
#![allow(dead_code)]

use super::IDS;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Handle {
    pub slot: usize,
    pub generation: u64,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Entry {
    pub generation: u64,
    pub scheduler: usize,
}

pub(super) struct Registry {
    entries: Vec<Option<Entry>>,
    next_generation: u64,
}

impl Registry {
    pub(super) fn new() -> Self {
        Self {
            entries: Vec::new(),
            next_generation: 0,
        }
    }

    /// Reuse vacant storage without resurrecting the identity that held it.
    pub(super) fn issue(&mut self, scheduler: usize) -> Option<Handle> {
        let generation = self.next_generation.checked_add(1)?;
        let slot = match self.entries.iter().position(Option::is_none) {
            Some(slot) => slot,
            None if self.entries.len() < IDS => {
                self.entries.push(None);
                self.entries.len() - 1
            }
            None => return None,
        };
        self.next_generation = generation;
        self.entries[slot] = Some(Entry {
            generation,
            scheduler,
        });
        Some(Handle { slot, generation })
    }

    pub(super) fn resolve(&self, slot: usize, generation: u64) -> Option<&Entry> {
        let entry = self.entries.get(slot)?.as_ref()?;
        (entry.generation == generation && generation != 0).then_some(entry)
    }

    pub(super) fn retire(&mut self, slot: usize) {
        if let Some(entry) = self.entries.get_mut(slot) {
            *entry = None;
        }
    }

    pub(super) fn live(&self) -> usize {
        self.entries.iter().filter(|e| e.is_some()).count()
    }

    #[cfg(test)]
    pub(super) fn force_generation_for_test(&mut self, generation: u64) {
        self.next_generation = generation;
    }
}
