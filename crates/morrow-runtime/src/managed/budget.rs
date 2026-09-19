//! Shared admission and retained-byte accounting for one managed invocation.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

pub(super) const MAX_LIVE_ACTORS: usize = 1024;
pub(super) const MAX_MAILBOX_MESSAGES: usize = 4096;
pub(super) const MAX_QUEUED_MESSAGES: usize = 65_536;
pub(super) const MAX_RETAINED_BYTES: usize = 64 * 1024 * 1024;

/// One invocation-wide set of admission counters shared by every scheduler.
pub(super) struct Budget {
    initial_bytes: usize,
    retained: AtomicUsize,
    live: AtomicUsize,
    messages: AtomicUsize,
    next_generation: AtomicU64,
    _accounting: crate::memory::ControlAllocation,
}

impl Budget {
    /// Construct one shared budget with the invocation's historical base charge.
    pub(super) fn new(initial_bytes: usize) -> Option<Self> {
        (initial_bytes <= MAX_RETAINED_BYTES).then(|| Self {
            initial_bytes,
            retained: AtomicUsize::new(initial_bytes),
            live: AtomicUsize::new(0),
            messages: AtomicUsize::new(0),
            next_generation: AtomicU64::new(0),
            _accounting: crate::memory::account_control(std::mem::size_of::<Self>() + 16, 1),
        })
    }

    #[cfg(any(test, feature = "simulation"))]
    pub(super) fn retained(&self) -> usize {
        self.retained.load(Ordering::Acquire)
    }

    pub(super) fn live(&self) -> usize {
        self.live.load(Ordering::Acquire)
    }

    #[cfg(any(test, feature = "simulation"))]
    pub(super) fn messages(&self) -> usize {
        self.messages.load(Ordering::Acquire)
    }

    #[cfg(any(test, feature = "simulation"))]
    pub(super) fn generation(&self) -> u64 {
        self.next_generation.load(Ordering::Acquire)
    }

    /// Reserve logical bytes, rolling the charge back unless it is committed.
    pub(super) fn try_charge(&self, bytes: usize) -> Option<ByteReservation<'_>> {
        try_add(&self.retained, bytes, MAX_RETAINED_BYTES).then(|| ByteReservation {
            budget: self,
            bytes,
            active: true,
        })
    }

    /// Release a previously committed standalone byte reservation.
    pub(super) fn release_bytes(&self, bytes: usize) {
        self.release_retained(bytes);
    }

    /// Reserve one live actor, its bytes and a unique nonwrapping generation.
    pub(super) fn try_reserve_actor(&self, bytes: usize) -> Option<ActorReservation<'_>> {
        if !try_increment(&self.live, MAX_LIVE_ACTORS) {
            return None;
        }
        if !try_add(&self.retained, bytes, MAX_RETAINED_BYTES) {
            release_count(&self.live, "live actor");
            return None;
        }
        let generation = match self.next_generation.try_update(
            Ordering::AcqRel,
            Ordering::Acquire,
            |generation| generation.checked_add(1),
        ) {
            Ok(generation) => generation + 1,
            Err(_) => {
                self.release_retained(bytes);
                release_count(&self.live, "live actor");
                return None;
            }
        };
        Some(ActorReservation {
            budget: self,
            bytes,
            generation,
            active: true,
        })
    }

    /// Release one actor reservation after that actor has retired.
    pub(super) fn release_actor(&self, bytes: usize) {
        release_count(&self.live, "live actor");
        self.release_retained(bytes);
    }

    /// Reserve one mailbox entry, one global message entry and its logical bytes.
    pub(super) fn try_reserve_message<'a>(
        &'a self,
        mailbox: &'a AtomicUsize,
        bytes: usize,
    ) -> Option<MessageReservation<'a>> {
        if !try_increment(mailbox, MAX_MAILBOX_MESSAGES) {
            return None;
        }
        if !try_increment(&self.messages, MAX_QUEUED_MESSAGES) {
            release_count(mailbox, "mailbox message");
            return None;
        }
        if !try_add(&self.retained, bytes, MAX_RETAINED_BYTES) {
            release_count(&self.messages, "queued message");
            release_count(mailbox, "mailbox message");
            return None;
        }
        Some(MessageReservation {
            budget: self,
            mailbox,
            bytes,
            active: true,
        })
    }

    /// Release one message reservation after dequeue or actor retirement.
    pub(super) fn release_message(&self, mailbox: &AtomicUsize, bytes: usize) {
        release_count(mailbox, "mailbox message");
        release_count(&self.messages, "queued message");
        self.release_retained(bytes);
    }

    fn release_retained(&self, bytes: usize) {
        let released = self
            .retained
            .try_update(Ordering::AcqRel, Ordering::Acquire, |retained| {
                retained
                    .checked_sub(bytes)
                    .filter(|remaining| *remaining >= self.initial_bytes)
            });
        assert!(released.is_ok(), "retained-byte reservation released twice");
    }
}

#[must_use = "dropping the reservation rolls its byte charge back"]
pub(super) struct ByteReservation<'a> {
    budget: &'a Budget,
    bytes: usize,
    active: bool,
}

impl ByteReservation<'_> {
    pub(super) fn commit(mut self) {
        self.active = false;
    }
}

impl Drop for ByteReservation<'_> {
    fn drop(&mut self) {
        if self.active {
            self.budget.release_retained(self.bytes);
        }
    }
}

#[must_use = "dropping the reservation rolls its live actor and byte charges back"]
pub(super) struct ActorReservation<'a> {
    budget: &'a Budget,
    bytes: usize,
    generation: u64,
    active: bool,
}

impl ActorReservation<'_> {
    /// Keep the actor and byte charges and return its immutable generation.
    pub(super) fn commit(mut self) -> u64 {
        self.active = false;
        self.generation
    }
}

impl Drop for ActorReservation<'_> {
    fn drop(&mut self) {
        if self.active {
            self.budget.release_retained(self.bytes);
            release_count(&self.budget.live, "live actor");
        }
    }
}

#[must_use = "dropping the reservation rolls its message and byte charges back"]
pub(super) struct MessageReservation<'a> {
    budget: &'a Budget,
    mailbox: &'a AtomicUsize,
    bytes: usize,
    active: bool,
}

impl MessageReservation<'_> {
    pub(super) fn commit(mut self) {
        self.active = false;
    }
}

impl Drop for MessageReservation<'_> {
    fn drop(&mut self) {
        if self.active {
            self.budget.release_retained(self.bytes);
            release_count(&self.budget.messages, "queued message");
            release_count(self.mailbox, "mailbox message");
        }
    }
}

fn try_add(counter: &AtomicUsize, amount: usize, limit: usize) -> bool {
    counter
        .try_update(Ordering::AcqRel, Ordering::Acquire, |current| {
            current.checked_add(amount).filter(|next| *next <= limit)
        })
        .is_ok()
}

fn try_increment(counter: &AtomicUsize, limit: usize) -> bool {
    try_add(counter, 1, limit)
}

fn release_count(counter: &AtomicUsize, name: &str) {
    let released = counter.try_update(Ordering::AcqRel, Ordering::Acquire, |count| {
        count.checked_sub(1)
    });
    assert!(released.is_ok(), "{name} reservation released twice");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier};

    #[test]
    fn initial_bytes_preserve_the_original_absolute_charge() {
        let identity_bytes = 65_536 * 8;
        let normal = Budget::new(120 + identity_bytes).unwrap();
        let simulation = Budget::new(144 + identity_bytes).unwrap();
        assert_eq!(normal.retained(), 120 + identity_bytes);
        assert_eq!(simulation.retained(), 144 + identity_bytes);
        assert!(Budget::new(MAX_RETAINED_BYTES + 1).is_none());
    }

    #[test]
    fn byte_reservations_are_exact_and_roll_back_once() {
        let budget = Budget::new(MAX_RETAINED_BYTES - 1).unwrap();
        {
            let reservation = budget.try_charge(1).unwrap();
            assert_eq!(budget.retained(), MAX_RETAINED_BYTES);
            assert!(budget.try_charge(1).is_none());
            assert!(budget.try_charge(usize::MAX).is_none());
            drop(reservation);
        }
        assert_eq!(budget.retained(), MAX_RETAINED_BYTES - 1);

        budget.try_charge(1).unwrap().commit();
        budget.release_bytes(1);
        assert_eq!(budget.retained(), MAX_RETAINED_BYTES - 1);
    }

    #[test]
    fn only_one_concurrent_byte_reservation_reaches_the_last_byte() {
        let budget = Budget::new(MAX_RETAINED_BYTES - 1).unwrap();
        let start = Arc::new(Barrier::new(9));
        let reserved = Arc::new(Barrier::new(9));
        let successes = std::thread::scope(|scope| {
            let mut threads = Vec::new();
            for _ in 0..8 {
                let start = Arc::clone(&start);
                let reserved = Arc::clone(&reserved);
                let budget = &budget;
                threads.push(scope.spawn(move || {
                    start.wait();
                    let reservation = budget.try_charge(1);
                    reserved.wait();
                    reservation.is_some()
                }));
            }
            start.wait();
            reserved.wait();
            threads
                .into_iter()
                .map(|thread| usize::from(thread.join().unwrap()))
                .sum::<usize>()
        });
        assert_eq!(successes, 1);
        assert_eq!(budget.retained(), MAX_RETAINED_BYTES - 1);
    }

    #[test]
    fn actor_reservations_share_one_exact_limit() {
        let budget = Budget::new(0).unwrap();
        for expected in 1..MAX_LIVE_ACTORS {
            assert_eq!(
                budget.try_reserve_actor(0).unwrap().commit(),
                expected as u64
            );
        }

        let start = Arc::new(Barrier::new(9));
        let reserved = Arc::new(Barrier::new(9));
        let successes = std::thread::scope(|scope| {
            let mut threads = Vec::new();
            for _ in 0..8 {
                let start = Arc::clone(&start);
                let reserved = Arc::clone(&reserved);
                let budget = &budget;
                threads.push(scope.spawn(move || {
                    start.wait();
                    let reservation = budget.try_reserve_actor(0);
                    reserved.wait();
                    reservation.is_some()
                }));
            }
            start.wait();
            reserved.wait();
            threads
                .into_iter()
                .map(|thread| usize::from(thread.join().unwrap()))
                .sum::<usize>()
        });
        assert_eq!(successes, 1);
        assert_eq!(budget.live(), MAX_LIVE_ACTORS - 1);

        for _ in 1..MAX_LIVE_ACTORS {
            budget.release_actor(0);
        }
        assert_eq!(budget.live(), 0);
        assert_eq!(budget.retained(), 0);
    }

    #[test]
    fn actor_failure_rolls_back_every_reserved_counter() {
        let budget = Budget::new(MAX_RETAINED_BYTES - 7).unwrap();
        assert!(budget.try_reserve_actor(8).is_none());
        assert_eq!(budget.live(), 0);
        assert_eq!(budget.retained(), MAX_RETAINED_BYTES - 7);
        assert_eq!(budget.generation(), 0);

        budget.next_generation.store(u64::MAX, Ordering::Release);
        assert!(budget.try_reserve_actor(0).is_none());
        assert_eq!(budget.live(), 0);
        assert_eq!(budget.retained(), MAX_RETAINED_BYTES - 7);
        assert_eq!(budget.generation(), u64::MAX);
    }

    #[test]
    fn generations_reach_max_without_wrapping() {
        let budget = Budget::new(0).unwrap();
        budget
            .next_generation
            .store(u64::MAX - 1, Ordering::Release);
        assert_eq!(budget.try_reserve_actor(1).unwrap().commit(), u64::MAX);
        budget.release_actor(1);
        assert!(budget.try_reserve_actor(1).is_none());
        assert_eq!(budget.generation(), u64::MAX);
        assert_eq!(budget.live(), 0);
        assert_eq!(budget.retained(), 0);
    }

    #[test]
    fn mailbox_reservations_share_one_exact_limit() {
        let budget = Budget::new(0).unwrap();
        let mailbox = AtomicUsize::new(0);
        for _ in 0..MAX_MAILBOX_MESSAGES - 1 {
            budget.try_reserve_message(&mailbox, 0).unwrap().commit();
        }

        let start = Arc::new(Barrier::new(9));
        let reserved = Arc::new(Barrier::new(9));
        let successes = std::thread::scope(|scope| {
            let mut threads = Vec::new();
            for _ in 0..8 {
                let start = Arc::clone(&start);
                let reserved = Arc::clone(&reserved);
                let budget = &budget;
                let mailbox = &mailbox;
                threads.push(scope.spawn(move || {
                    start.wait();
                    let reservation = budget.try_reserve_message(mailbox, 0);
                    reserved.wait();
                    reservation.is_some()
                }));
            }
            start.wait();
            reserved.wait();
            threads
                .into_iter()
                .map(|thread| usize::from(thread.join().unwrap()))
                .sum::<usize>()
        });
        assert_eq!(successes, 1);
        assert_eq!(mailbox.load(Ordering::Acquire), MAX_MAILBOX_MESSAGES - 1);
        assert_eq!(budget.messages(), MAX_MAILBOX_MESSAGES - 1);

        for _ in 0..MAX_MAILBOX_MESSAGES - 1 {
            budget.release_message(&mailbox, 0);
        }
        assert_eq!(mailbox.load(Ordering::Acquire), 0);
        assert_eq!(budget.messages(), 0);
    }

    #[test]
    fn global_message_reservations_share_one_exact_limit() {
        let budget = Budget::new(0).unwrap();
        let mailboxes = (0..16).map(|_| AtomicUsize::new(0)).collect::<Vec<_>>();
        for mailbox in &mailboxes[..15] {
            for _ in 0..MAX_MAILBOX_MESSAGES {
                budget.try_reserve_message(mailbox, 0).unwrap().commit();
            }
        }
        for _ in 0..MAX_MAILBOX_MESSAGES - 1 {
            budget
                .try_reserve_message(&mailboxes[15], 0)
                .unwrap()
                .commit();
        }
        assert_eq!(budget.messages(), MAX_QUEUED_MESSAGES - 1);

        let contenders = (0..8).map(|_| AtomicUsize::new(0)).collect::<Vec<_>>();
        let start = Arc::new(Barrier::new(9));
        let reserved = Arc::new(Barrier::new(9));
        let successes = std::thread::scope(|scope| {
            let mut threads = Vec::new();
            for mailbox in &contenders {
                let start = Arc::clone(&start);
                let reserved = Arc::clone(&reserved);
                let budget = &budget;
                threads.push(scope.spawn(move || {
                    start.wait();
                    let reservation = budget.try_reserve_message(mailbox, 0);
                    reserved.wait();
                    reservation.is_some()
                }));
            }
            start.wait();
            reserved.wait();
            threads
                .into_iter()
                .map(|thread| usize::from(thread.join().unwrap()))
                .sum::<usize>()
        });
        assert_eq!(successes, 1);
        assert_eq!(budget.messages(), MAX_QUEUED_MESSAGES - 1);
        assert!(
            contenders
                .iter()
                .all(|mailbox| mailbox.load(Ordering::Acquire) == 0)
        );

        for mailbox in &mailboxes {
            let messages = mailbox.load(Ordering::Acquire);
            for _ in 0..messages {
                budget.release_message(mailbox, 0);
            }
        }
        assert_eq!(budget.messages(), 0);
    }

    #[test]
    fn message_failure_rolls_back_mailbox_global_and_bytes() {
        let budget = Budget::new(MAX_RETAINED_BYTES).unwrap();
        let mailbox = AtomicUsize::new(0);
        assert!(budget.try_reserve_message(&mailbox, 1).is_none());
        assert_eq!(mailbox.load(Ordering::Acquire), 0);
        assert_eq!(budget.messages(), 0);
        assert_eq!(budget.retained(), MAX_RETAINED_BYTES);

        mailbox.store(MAX_MAILBOX_MESSAGES, Ordering::Release);
        assert!(budget.try_reserve_message(&mailbox, 0).is_none());
        assert_eq!(budget.messages(), 0);
        assert_eq!(mailbox.load(Ordering::Acquire), MAX_MAILBOX_MESSAGES);
    }

    #[test]
    fn checked_release_rejects_a_second_release() {
        let budget = Budget::new(0).unwrap();
        budget.try_charge(1).unwrap().commit();
        budget.release_bytes(1);
        assert!(std::panic::catch_unwind(|| budget.release_bytes(1)).is_err());

        budget.try_reserve_actor(1).unwrap().commit();
        budget.release_actor(1);
        assert!(std::panic::catch_unwind(|| budget.release_actor(1)).is_err());

        let mailbox = AtomicUsize::new(0);
        budget.try_reserve_message(&mailbox, 1).unwrap().commit();
        budget.release_message(&mailbox, 1);
        assert!(std::panic::catch_unwind(|| budget.release_message(&mailbox, 1)).is_err());
    }
}
