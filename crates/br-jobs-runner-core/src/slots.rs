use std::collections::HashSet;
use std::num::NonZeroUsize;

use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaimDecision {
    Claimed,
    AtCapacity,
    Draining,
    AlreadyInFlight,
}

#[derive(Debug)]
pub struct SlotPool {
    capacity: NonZeroUsize,
    in_flight: HashSet<Uuid>,
    draining: bool,
}

impl SlotPool {
    pub fn new(capacity: NonZeroUsize) -> Self {
        Self {
            capacity,
            in_flight: HashSet::new(),
            draining: false,
        }
    }

    pub fn try_claim(&mut self, run_id: Uuid) -> ClaimDecision {
        if self.draining {
            return ClaimDecision::Draining;
        }
        if self.in_flight.contains(&run_id) {
            return ClaimDecision::AlreadyInFlight;
        }
        if self.in_flight.len() >= self.capacity.get() {
            return ClaimDecision::AtCapacity;
        }
        self.in_flight.insert(run_id);
        ClaimDecision::Claimed
    }

    pub fn release(&mut self, run_id: Uuid) -> bool {
        self.in_flight.remove(&run_id)
    }

    pub fn begin_drain(&mut self) {
        self.draining = true;
    }

    pub fn has_free_slot(&self) -> bool {
        !self.draining && self.in_flight.len() < self.capacity.get()
    }

    pub fn in_flight(&self) -> usize {
        self.in_flight.len()
    }

    pub fn is_idle(&self) -> bool {
        self.in_flight.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pool(capacity: usize) -> SlotPool {
        SlotPool::new(NonZeroUsize::new(capacity).expect("non-zero capacity"))
    }

    #[test]
    fn given_free_slot_when_claiming_then_claimed() {
        let mut pool = pool(1);
        assert!(pool.has_free_slot());
        assert_eq!(pool.try_claim(Uuid::now_v7()), ClaimDecision::Claimed);
        assert_eq!(pool.in_flight(), 1);
    }

    #[test]
    fn given_full_pool_when_claiming_then_at_capacity() {
        let mut pool = pool(1);
        pool.try_claim(Uuid::now_v7());
        assert!(!pool.has_free_slot());
        assert_eq!(pool.try_claim(Uuid::now_v7()), ClaimDecision::AtCapacity);
    }

    #[test]
    fn given_released_slot_when_claiming_then_claimed_again() {
        let mut pool = pool(1);
        let run = Uuid::now_v7();
        pool.try_claim(run);
        assert!(pool.release(run));
        assert_eq!(pool.try_claim(Uuid::now_v7()), ClaimDecision::Claimed);
    }

    #[test]
    fn given_run_in_flight_when_claiming_same_run_then_rejected() {
        let mut pool = pool(2);
        let run = Uuid::now_v7();
        pool.try_claim(run);
        assert_eq!(pool.try_claim(run), ClaimDecision::AlreadyInFlight);
        assert_eq!(pool.in_flight(), 1);
    }

    #[test]
    fn given_draining_pool_when_claiming_then_refused_but_in_flight_kept() {
        let mut pool = pool(2);
        let run = Uuid::now_v7();
        pool.try_claim(run);
        pool.begin_drain();
        assert_eq!(pool.try_claim(Uuid::now_v7()), ClaimDecision::Draining);
        assert!(!pool.has_free_slot());
        assert_eq!(pool.in_flight(), 1);
        assert!(!pool.is_idle());
        pool.release(run);
        assert!(pool.is_idle());
    }

    #[test]
    fn given_unknown_run_when_releasing_then_false() {
        let mut pool = pool(1);
        assert!(!pool.release(Uuid::now_v7()));
    }

    #[test]
    fn given_multi_capacity_when_claiming_then_parallel_runs_coexist() {
        let mut pool = pool(3);
        assert_eq!(pool.try_claim(Uuid::now_v7()), ClaimDecision::Claimed);
        assert_eq!(pool.try_claim(Uuid::now_v7()), ClaimDecision::Claimed);
        assert!(pool.has_free_slot());
        assert_eq!(pool.try_claim(Uuid::now_v7()), ClaimDecision::Claimed);
        assert!(!pool.has_free_slot());
    }
}
