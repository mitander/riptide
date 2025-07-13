//! Token bucket rate limiter for bandwidth control.
//!
//! Implements the classic token bucket algorithm for smooth rate limiting
//! with configurable burst capacity and sustained throughput rates.

use std::time::{Duration, Instant};

/// Token bucket rate limiter for controlling bandwidth usage.
///
/// Implements token bucket algorithm where tokens are added at a fixed rate
/// and consumed when bandwidth is used. Allows bursts up to bucket capacity
/// while maintaining average rate over time.
#[derive(Debug, Clone)]
pub struct TokenBucket {
    /// Maximum number of tokens the bucket can hold
    capacity: u64,
    /// Current number of tokens in the bucket
    tokens: u64,
    /// Rate at which tokens are added (tokens per second)
    refill_rate: u64,
    /// Timestamp of last refill operation
    last_refill: Instant,
}

/// Errors that can occur during token bucket operations.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum TokenBucketError {
    /// Not enough tokens available for the requested operation
    #[error("Insufficient tokens: requested {requested}, available {available}")]
    InsufficientTokens {
        /// Number of tokens requested
        requested: u64,
        /// Number of tokens currently available
        available: u64,
    },
}

impl TokenBucket {
    /// Creates new token bucket with specified capacity and refill rate.
    ///
    /// # Parameters
    /// - `capacity`: Maximum tokens the bucket can hold (burst size)
    /// - `refill_rate`: Tokens added per second (sustained rate)
    ///
    /// # Panics
    ///
    /// Panics if capacity or refill_rate is zero.
    pub fn new(capacity: u64, refill_rate: u64) -> Self {
        assert!(
            capacity > 0,
            "Token bucket capacity must be greater than zero"
        );
        assert!(
            refill_rate > 0,
            "Token bucket refill rate must be greater than zero"
        );

        Self {
            capacity,
            tokens: capacity, // Start with full bucket
            refill_rate,
            last_refill: Instant::now(),
        }
    }

    /// Attempts to consume specified number of tokens.
    ///
    /// Returns `Ok(())` if tokens were consumed successfully,
    /// or `Err(TokenBucketError::InsufficientTokens)` if not enough tokens available.
    ///
    /// # Errors
    ///
    /// - `TokenBucketError::InsufficientTokens` - If requested tokens exceed available tokens
    pub fn try_consume(&mut self, tokens: u64) -> Result<(), TokenBucketError> {
        self.refill();

        if self.tokens >= tokens {
            self.tokens -= tokens;
            Ok(())
        } else {
            Err(TokenBucketError::InsufficientTokens {
                requested: tokens,
                available: self.tokens,
            })
        }
    }

    /// Consumes tokens if available, returns number of tokens actually consumed.
    ///
    /// This method never fails - it consumes up to the requested amount
    /// based on token availability.
    pub fn consume(&mut self, requested_tokens: u64) -> u64 {
        self.refill();

        let consumed = requested_tokens.min(self.tokens);
        self.tokens -= consumed;
        consumed
    }

    /// Returns current number of available tokens.
    pub fn available_tokens(&mut self) -> u64 {
        self.refill();
        self.tokens
    }

    /// Returns bucket capacity.
    pub fn capacity(&self) -> u64 {
        self.capacity
    }

    /// Returns refill rate in tokens per second.
    pub fn refill_rate(&self) -> u64 {
        self.refill_rate
    }

    /// Adds tokens to bucket based on elapsed time since last refill.
    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill);

        if elapsed >= Duration::from_millis(1) {
            // Calculate tokens to add based on elapsed time
            let tokens_to_add = (elapsed.as_secs_f64() * self.refill_rate as f64) as u64;

            if tokens_to_add > 0 {
                self.tokens = (self.tokens + tokens_to_add).min(self.capacity);
                self.last_refill = now;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn test_token_bucket_creation() {
        let bucket = TokenBucket::new(100, 10);
        assert_eq!(bucket.capacity(), 100);
        assert_eq!(bucket.refill_rate(), 10);
    }

    #[test]
    #[should_panic(expected = "Token bucket capacity must be greater than zero")]
    fn test_zero_capacity_panics() {
        TokenBucket::new(0, 10);
    }

    #[test]
    #[should_panic(expected = "Token bucket refill rate must be greater than zero")]
    fn test_zero_refill_rate_panics() {
        TokenBucket::new(100, 0);
    }

    #[test]
    fn test_consume_success() {
        let mut bucket = TokenBucket::new(100, 10);

        // Should start with full bucket
        assert_eq!(bucket.available_tokens(), 100);

        // Consume some tokens
        assert_eq!(bucket.consume(30), 30);
        assert_eq!(bucket.available_tokens(), 70);

        // Consume more tokens
        assert_eq!(bucket.consume(20), 20);
        assert_eq!(bucket.available_tokens(), 50);
    }

    #[test]
    fn test_consume_partial() {
        let mut bucket = TokenBucket::new(100, 10);

        // Consume most tokens
        assert_eq!(bucket.consume(90), 90);
        assert_eq!(bucket.available_tokens(), 10);

        // Try to consume more than available
        assert_eq!(bucket.consume(50), 10); // Only 10 available
        assert_eq!(bucket.available_tokens(), 0);
    }

    #[test]
    fn test_try_consume_success() {
        let mut bucket = TokenBucket::new(100, 10);

        assert!(bucket.try_consume(50).is_ok());
        assert_eq!(bucket.available_tokens(), 50);
    }

    #[test]
    fn test_try_consume_insufficient_tokens() {
        let mut bucket = TokenBucket::new(100, 10);

        // Consume most tokens
        assert!(bucket.try_consume(90).is_ok());

        // Try to consume more than available
        let result = bucket.try_consume(50);
        assert!(result.is_err());

        if let Err(TokenBucketError::InsufficientTokens {
            requested,
            available,
        }) = result
        {
            assert_eq!(requested, 50);
            assert_eq!(available, 10);
        } else {
            panic!("Expected InsufficientTokens error");
        }
    }

    #[test]
    fn test_token_refill() {
        let mut bucket = TokenBucket::new(100, 1000); // 1000 tokens/sec for faster testing

        // Consume all tokens
        assert_eq!(bucket.consume(100), 100);
        assert_eq!(bucket.available_tokens(), 0);

        // Wait a bit for refill
        std::thread::sleep(Duration::from_millis(10));

        // Should have some tokens back (at least a few)
        let available = bucket.available_tokens();
        assert!(
            available > 0,
            "Expected some tokens after refill, got {available}"
        );
        assert!(available <= 100, "Should not exceed capacity");
    }

    #[test]
    fn test_capacity_limit() {
        let mut bucket = TokenBucket::new(50, 1000); // Small capacity, fast refill

        // Wait for potential refill
        std::thread::sleep(Duration::from_millis(100));

        // Should not exceed capacity despite high refill rate
        assert!(bucket.available_tokens() <= 50);
    }

    #[test]
    fn test_zero_tokens_consume() {
        let mut bucket = TokenBucket::new(100, 10);

        // Consuming zero tokens should always succeed
        assert_eq!(bucket.consume(0), 0);
        assert!(bucket.try_consume(0).is_ok());
        assert_eq!(bucket.available_tokens(), 100);
    }

    #[test]
    fn test_refill_rate_calculation() {
        // Test that refill rate is approximately correct
        let mut bucket = TokenBucket::new(1000, 100); // 100 tokens/sec

        // Consume all tokens
        bucket.consume(1000);
        assert_eq!(bucket.available_tokens(), 0);

        // Wait 100ms (should get ~10 tokens at 100 tokens/sec)
        std::thread::sleep(Duration::from_millis(100));

        let available = bucket.available_tokens();
        // Allow some variance due to timing precision
        assert!(
            (8..=12).contains(&available),
            "Expected ~10 tokens after 100ms, got {available}"
        );
    }
}
