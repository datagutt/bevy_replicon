use bevy::{
    ecs::{
        change_detection::{CheckChangeTicks, Tick},
        entity::EntityHashMap,
    },
    prelude::*,
};
use smallvec::SmallVec;

use super::{client_visibility::ClientVisibility, registry::FilterRegistry};
use crate::prelude::*;

/// Inverse of [`ClientVisibility`]: for each entity, the clients it's hidden from.
///
/// [`ClientVisibility`] stores what a client doesn't see, so finding the clients that see an entity
/// takes a lookup for every client. Collecting changes would then cost a lookup for every pair of
/// entity and client, even when each client sees only a few of the entities, as with clients
/// spread over rooms. This index keeps the same information per entity as a bitset over client
/// slots, so replication visits only the clients that can see an entity.
///
/// Only filters that hide the whole entity with [`ScopeLifetime::WhileVisible`] are indexed,
/// because only they make replication skip a client for an entity entirely.
///
/// Updated from [`ClientVisibility`] changes before collecting changes.
#[derive(Resource, Default)]
pub(crate) struct VisibilityIndex {
    /// Slots of clients that each entity is hidden from.
    ///
    /// Entities that are visible to all clients have no entry.
    hidden: EntityHashMap<SlotMask>,

    /// Last tick on which the visibility of each entity changed for any client.
    ///
    /// Lets replication skip clients that have the latest state of an entity without
    /// looking up its visibility for them.
    changed_at: EntityHashMap<Tick>,

    /// Slots of removed clients that can be given out again.
    free_slots: Vec<usize>,

    /// Slots of removed clients that are still set in [`Self::hidden`].
    released_slots: Vec<usize>,

    /// Number of slots given out so far.
    slots_len: usize,
}

impl VisibilityIndex {
    /// Returns the slots of clients that the entity is hidden from.
    pub(crate) fn hidden(&self, entity: Entity) -> Option<&SlotMask> {
        self.hidden.get(&entity)
    }

    /// Returns the last tick on which the visibility of the entity changed for any client.
    pub(crate) fn changed_at(&self, entity: Entity) -> Option<Tick> {
        self.changed_at.get(&entity).copied()
    }

    /// Forgets a despawned entity.
    pub(crate) fn remove(&mut self, entity: Entity) {
        self.hidden.remove(&entity);
        self.changed_at.remove(&entity);
    }

    pub(crate) fn check_change_ticks(&mut self, check: CheckChangeTicks) {
        for tick in self.changed_at.values_mut() {
            tick.check_tick(check);
        }
    }

    /// Marks the slot of a removed client for reuse.
    ///
    /// Its bits are cleared on the next [`Self::update`].
    pub(crate) fn release(&mut self, slot: usize) {
        self.released_slots.push(slot);
    }

    /// Applies visibility changes of a client since the last call, assigning it a slot if it has none.
    ///
    /// `tick` is the change tick of the replication that the changes will be sent in.
    pub(crate) fn update(
        &mut self,
        registry: &FilterRegistry,
        visibility: &mut ClientVisibility,
        tick: Tick,
    ) {
        let slot = match visibility.slot {
            Some(slot) => slot,
            None => {
                let slot = self.free_slots.pop().unwrap_or_else(|| {
                    self.slots_len += 1;
                    self.slots_len - 1
                });
                visibility.slot = Some(slot);
                slot
            }
        };

        // Taken out because `get` borrows the visibility too.
        let mut changed = core::mem::take(&mut visibility.changed_entities);
        for entity in changed.drain(..) {
            let hidden = visibility.get(entity).hidden_entity_lifetime(registry)
                == Some(ScopeLifetime::WhileVisible);
            self.set(entity, slot, hidden);
            self.changed_at.insert(entity, tick);
        }
        // Return the buffer to keep its allocation.
        visibility.changed_entities = changed;
    }

    /// Clears the bits of released slots and makes them available again.
    ///
    /// Should be called before [`Self::update`] to avoid giving out a slot that still has bits set.
    pub(crate) fn clear_released(&mut self) {
        if self.released_slots.is_empty() {
            return;
        }

        self.hidden.retain(|_, mask| {
            for &slot in &self.released_slots {
                mask.set(slot, false);
            }
            !mask.is_empty()
        });
        self.free_slots.append(&mut self.released_slots);
    }

    fn set(&mut self, entity: Entity, slot: usize, hidden: bool) {
        if hidden {
            self.hidden.entry(entity).or_default().set(slot, true);
        } else if let Some(mask) = self.hidden.get_mut(&entity) {
            mask.set(slot, false);
            if mask.is_empty() {
                self.hidden.remove(&entity);
            }
        }
    }
}

/// Bitset over client slots from [`VisibilityIndex`].
///
/// Stores up to 256 clients inline.
#[derive(Default, Clone, Debug)]
pub(crate) struct SlotMask(SmallVec<[u64; 4]>);

impl SlotMask {
    pub(crate) fn set(&mut self, slot: usize, value: bool) {
        let word = slot / u64::BITS as usize;
        let bit = 1 << (slot % u64::BITS as usize);
        if value {
            if word >= self.0.len() {
                self.0.resize(word + 1, 0);
            }
            self.0[word] |= bit;
        } else if let Some(bits) = self.0.get_mut(word) {
            *bits &= !bit;
        }
    }

    fn is_empty(&self) -> bool {
        self.0.iter().all(|&bits| bits == 0)
    }

    /// Iterates over slots that are set in `self`, but not in `excluded`.
    pub(crate) fn iter_except<'a>(
        &'a self,
        excluded: Option<&'a SlotMask>,
    ) -> impl Iterator<Item = usize> + 'a {
        self.0.iter().enumerate().flat_map(move |(index, &bits)| {
            let excluded_bits = excluded
                .and_then(|excluded| excluded.0.get(index))
                .copied()
                .unwrap_or_default();
            let mut bits = bits & !excluded_bits;
            core::iter::from_fn(move || {
                if bits == 0 {
                    return None;
                }
                let bit = bits.trailing_zeros() as usize;
                bits &= bits - 1; // Clear the lowest set bit.
                Some(index * u64::BITS as usize + bit)
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use alloc::vec::Vec;

    use super::*;

    #[test]
    fn slot_mask() {
        let mut active = SlotMask::default();
        for slot in [0, 3, 64, 200, 300] {
            active.set(slot, true);
        }

        let mut excluded = SlotMask::default();
        excluded.set(3, true);
        excluded.set(300, true);
        excluded.set(301, true);

        let slots: Vec<_> = active.iter_except(None).collect();
        assert_eq!(slots, [0, 3, 64, 200, 300]);

        let slots: Vec<_> = active.iter_except(Some(&excluded)).collect();
        assert_eq!(slots, [0, 64, 200]);

        excluded.set(3, false);
        excluded.set(300, false);
        assert!(!excluded.is_empty());
        excluded.set(301, false);
        assert!(excluded.is_empty());
    }
}
