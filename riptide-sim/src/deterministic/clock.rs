//! Time control and random number generation for deterministic simulations.

use std::time::{Duration, Instant};

use rand::{RngCore, SeedableRng};
use rand_chacha::ChaCha8Rng;

use crate::SimulationError;

/// Maximum time that can be advanced in a single operation (24 hours).
const MAX_TIME_ADVANCE: Duration = Duration::from_secs(86400);

/// Deterministic clock for simulation time control.
///
/// Provides controlled time advancement for reproducible test scenarios.
/// Time can only move forward and is independent of wall-clock time.
#[derive(Debug, Clone)]
pub struct DeterministicClock {
    current_time: Instant,
    start_time: Instant,
}

impl Default for DeterministicClock {
    fn default() -> Self {
        Self::new()
    }
}

impl DeterministicClock {
    /// Creates new deterministic clock starting at simulation time zero.
    pub fn new() -> Self {
        let start = Instant::now();
        Self {
            current_time: start,
            start_time: start,
        }
    }

    /// Returns current simulation time.
    pub fn now(&self) -> Instant {
        self.current_time
    }

    /// Returns elapsed time since simulation start.
    pub fn elapsed(&self) -> Duration {
        self.current_time.duration_since(self.start_time)
    }

    /// Advances simulation time by specified duration.
    ///
    /// # Panics
    ///
    /// Panics if duration exceeds 24 hours (MAX_TIME_ADVANCE).
    pub fn advance(&mut self, duration: Duration) {
        assert!(
            duration <= MAX_TIME_ADVANCE,
            "Cannot advance time by more than 24 hours"
        );
        self.current_time += duration;
    }

    /// Advances simulation time to specific instant.
    ///
    /// # Errors
    ///
    /// - `SimulationError` - If target time is in the past
    pub fn advance_to(&mut self, target: Instant) -> Result<(), SimulationError> {
        if target < self.current_time {
            return Err(SimulationError::InvalidEventScheduling {
                reason: "Cannot advance time backwards".to_string(),
            });
        }
        self.current_time = target;
        Ok(())
    }

    /// Resets clock to initial state.
    pub fn reset(&mut self) {
        self.current_time = self.start_time;
    }
}

/// Deterministic random number generator for reproducible simulations.
///
/// Uses ChaCha8 algorithm for fast, high-quality pseudorandom numbers
/// with deterministic seed-based generation.
#[derive(Debug)]
pub struct DeterministicRng {
    rng: ChaCha8Rng,
    seed: u64,
}

impl DeterministicRng {
    /// Creates deterministic RNG from seed value.
    pub fn from_seed(seed: u64) -> Self {
        Self {
            rng: ChaCha8Rng::seed_from_u64(seed),
            seed,
        }
    }

    /// Returns the seed used for this RNG.
    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// Generates random number in range [0, 1).
    pub fn random_f64(&mut self) -> f64 {
        self.rng.next_u64() as f64 / u64::MAX as f64
    }

    /// Generates random number in range [min, max).
    pub fn random_range(&mut self, min: u64, max: u64) -> u64 {
        if min >= max {
            return min;
        }
        min + (self.rng.next_u64() % (max - min))
    }

    /// Generates random boolean with given probability.
    pub fn random_bool(&mut self, probability: f64) -> bool {
        self.random_f64() < probability
    }

    /// Shuffles a mutable slice in-place.
    pub fn shuffle<T>(&mut self, slice: &mut [T]) {
        use rand::seq::SliceRandom;
        slice.shuffle(&mut self.rng);
    }

    /// Selects random element from slice.
    pub fn choose<'a, T>(&mut self, slice: &'a [T]) -> Option<&'a T> {
        if slice.is_empty() {
            None
        } else {
            let index = self.random_range(0, slice.len() as u64) as usize;
            Some(&slice[index])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deterministic_clock_advancement() {
        let mut clock = DeterministicClock::new();
        let start = clock.now();

        clock.advance(Duration::from_secs(10));
        assert_eq!(clock.elapsed(), Duration::from_secs(10));

        clock.advance(Duration::from_secs(5));
        assert_eq!(clock.elapsed(), Duration::from_secs(15));

        // Verify time has advanced from start
        assert!(clock.now() > start);
    }

    #[test]
    fn test_clock_cannot_go_backwards() {
        let mut clock = DeterministicClock::new();
        clock.advance(Duration::from_secs(10));

        let past = clock.now() - Duration::from_secs(5);
        let result = clock.advance_to(past);

        assert!(result.is_err());
    }

    #[test]
    #[should_panic(expected = "Cannot advance time by more than 24 hours")]
    fn test_clock_max_advance_limit() {
        let mut clock = DeterministicClock::new();
        clock.advance(Duration::from_secs(86401)); // 24 hours + 1 second
    }

    #[test]
    fn test_deterministic_rng_reproducibility() {
        let seed = 12345;
        let mut rng1 = DeterministicRng::from_seed(seed);
        let mut rng2 = DeterministicRng::from_seed(seed);

        // Generate some random values
        let values1: Vec<u64> = (0..10).map(|_| rng1.random_range(0, 100)).collect();
        let values2: Vec<u64> = (0..10).map(|_| rng2.random_range(0, 100)).collect();

        // Same seed should produce same sequence
        assert_eq!(values1, values2);
    }

    #[test]
    fn test_rng_shuffle_determinism() {
        let seed = 42;
        let mut rng1 = DeterministicRng::from_seed(seed);
        let mut rng2 = DeterministicRng::from_seed(seed);

        let mut data1 = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        let mut data2 = data1.clone();

        rng1.shuffle(&mut data1);
        rng2.shuffle(&mut data2);

        // Same seed produces same shuffle
        assert_eq!(data1, data2);
        // But different from original
        assert_ne!(data1, vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
    }
}
