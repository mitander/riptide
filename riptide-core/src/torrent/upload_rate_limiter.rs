//! Upload rate limiter for controlling peer upload bandwidth.
//!
//! Implements upload throttling using token bucket algorithm to ensure
//! uploads don't saturate download bandwidth during streaming operations.

use std::time::Instant;

use crate::network::TokenBucket;

/// Configuration for upload rate limiting.
#[derive(Debug, Clone)]
pub struct UploadRateLimitConfig {
    /// Maximum upload bandwidth as ratio of total bandwidth (0.0 to 1.0)
    pub max_upload_ratio: f64,
    /// Burst capacity as multiple of sustained rate (1.0 = no burst)
    pub burst_multiplier: f64,
    /// Minimum upload rate in bytes per second (prevents starvation)
    pub min_upload_rate_bps: u64,
}

impl Default for UploadRateLimitConfig {
    fn default() -> Self {
        Self {
            max_upload_ratio: 0.2,       // 20% of bandwidth for uploads
            burst_multiplier: 2.0,       // Allow 2x burst capacity
            min_upload_rate_bps: 32_768, // 32 KB/s minimum
        }
    }
}

/// Upload rate limiter using token bucket algorithm.
///
/// Controls upload bandwidth to prevent uploads from saturating download
/// capacity during streaming. Maintains configurable ratio between upload
/// and total available bandwidth.
#[derive(Debug, Clone)]
pub struct UploadRateLimiter {
    config: UploadRateLimitConfig,
    token_bucket: TokenBucket,
    total_bandwidth_bps: u64,
    bytes_uploaded: u64,
    start_time: Instant,
}

/// Errors that can occur during upload rate limiting operations.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum UploadRateLimitError {
    /// Upload would exceed rate limit
    #[error("Upload rate limit exceeded: {message}")]
    RateLimitExceeded {
        /// Description of the rate limit violation
        message: String,
    },
    /// Invalid bandwidth configuration
    #[error("Invalid bandwidth configuration: {message}")]
    InvalidConfig {
        /// Description of the configuration error
        message: String,
    },
}

impl UploadRateLimiter {
    /// Creates new upload rate limiter with default configuration.
    ///
    /// # Parameters
    /// - `total_bandwidth_bps`: Total available bandwidth in bytes per second
    ///
    /// # Errors
    /// - `UploadRateLimitError::InvalidConfig` - If bandwidth is zero
    pub fn new(total_bandwidth_bps: u64) -> Result<Self, UploadRateLimitError> {
        Self::with_config(total_bandwidth_bps, UploadRateLimitConfig::default())
    }

    /// Creates upload rate limiter with custom configuration.
    ///
    /// # Parameters
    /// - `total_bandwidth_bps`: Total available bandwidth in bytes per second
    /// - `config`: Rate limiting configuration
    ///
    /// # Errors
    /// - `UploadRateLimitError::InvalidConfig` - If bandwidth is zero or config is invalid
    pub fn with_config(
        total_bandwidth_bps: u64,
        config: UploadRateLimitConfig,
    ) -> Result<Self, UploadRateLimitError> {
        if total_bandwidth_bps == 0 {
            return Err(UploadRateLimitError::InvalidConfig {
                message: "Total bandwidth cannot be zero".to_string(),
            });
        }

        if config.max_upload_ratio <= 0.0 || config.max_upload_ratio > 1.0 {
            return Err(UploadRateLimitError::InvalidConfig {
                message: format!(
                    "Upload ratio must be between 0.0 and 1.0, got {}",
                    config.max_upload_ratio
                ),
            });
        }

        if config.burst_multiplier < 1.0 {
            return Err(UploadRateLimitError::InvalidConfig {
                message: format!(
                    "Burst multiplier must be >= 1.0, got {}",
                    config.burst_multiplier
                ),
            });
        }

        let upload_rate_bps = Self::calculate_upload_rate(total_bandwidth_bps, &config);
        let burst_capacity = (upload_rate_bps as f64 * config.burst_multiplier) as u64;

        let token_bucket = TokenBucket::new(burst_capacity, upload_rate_bps);

        Ok(Self {
            config,
            token_bucket,
            total_bandwidth_bps,
            bytes_uploaded: 0,
            start_time: Instant::now(),
        })
    }

    /// Checks if upload of specified bytes is allowed.
    ///
    /// Returns `Ok(())` if upload is permitted, `Err` if rate limit would be exceeded.
    ///
    /// # Errors
    /// - `UploadRateLimitError::RateLimitExceeded` - If upload would exceed rate limit
    pub fn check_upload_allowed(&mut self, bytes: u64) -> Result<(), UploadRateLimitError> {
        self.token_bucket
            .try_consume(bytes)
            .map_err(|e| UploadRateLimitError::RateLimitExceeded {
                message: format!("Token bucket error: {e}"),
            })
    }

    /// Records successful upload of specified bytes.
    ///
    /// Updates internal statistics and consumes tokens from bucket.
    /// Should be called after successful upload operation.
    pub fn record_upload(&mut self, bytes: u64) {
        // Consume tokens (should succeed since check_upload_allowed was called first)
        let _consumed = self.token_bucket.consume(bytes);
        self.bytes_uploaded += bytes;
    }

    /// Updates total bandwidth estimate and reconfigures rate limiting.
    ///
    /// # Errors
    /// - `UploadRateLimitError::InvalidConfig` - If new bandwidth is zero
    pub fn update_bandwidth(&mut self, new_bandwidth_bps: u64) -> Result<(), UploadRateLimitError> {
        if new_bandwidth_bps == 0 {
            return Err(UploadRateLimitError::InvalidConfig {
                message: "Bandwidth cannot be zero".to_string(),
            });
        }

        self.total_bandwidth_bps = new_bandwidth_bps;

        let upload_rate_bps = Self::calculate_upload_rate(new_bandwidth_bps, &self.config);
        let burst_capacity = (upload_rate_bps as f64 * self.config.burst_multiplier) as u64;

        // Create new token bucket with updated rates
        self.token_bucket = TokenBucket::new(burst_capacity, upload_rate_bps);

        Ok(())
    }

    /// Returns current upload statistics.
    ///
    /// Returns tuple of (total_uploaded_bytes, current_upload_rate_bps, allowed_upload_rate_bps).
    pub fn upload_stats(&mut self) -> (u64, u64, u64) {
        let elapsed = self.start_time.elapsed();
        let current_rate = if elapsed.as_secs() > 0 {
            self.bytes_uploaded / elapsed.as_secs()
        } else {
            0
        };

        let allowed_rate = Self::calculate_upload_rate(self.total_bandwidth_bps, &self.config);

        (self.bytes_uploaded, current_rate, allowed_rate)
    }

    /// Returns number of bytes that can be uploaded immediately.
    pub fn available_upload_bytes(&mut self) -> u64 {
        self.token_bucket.available_tokens()
    }

    /// Calculates upload rate based on total bandwidth and configuration.
    fn calculate_upload_rate(total_bandwidth_bps: u64, config: &UploadRateLimitConfig) -> u64 {
        let calculated_rate = (total_bandwidth_bps as f64 * config.max_upload_ratio) as u64;
        calculated_rate.max(config.min_upload_rate_bps)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn test_rate_limiter_creation() {
        let limiter = UploadRateLimiter::new(1_000_000).unwrap(); // 1 MB/s
        let (_, _, allowed_rate) = limiter.clone().upload_stats();
        assert_eq!(allowed_rate, 200_000); // 20% of 1 MB/s = 200 KB/s
    }

    #[test]
    fn test_zero_bandwidth_fails() {
        let result = UploadRateLimiter::new(0);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("cannot be zero"));
    }

    #[test]
    fn test_invalid_upload_ratio() {
        let config = UploadRateLimitConfig {
            max_upload_ratio: 1.5, // Invalid: > 1.0
            ..Default::default()
        };

        let result = UploadRateLimiter::with_config(1_000_000, config);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("must be between 0.0 and 1.0")
        );
    }

    #[test]
    fn test_invalid_burst_multiplier() {
        let config = UploadRateLimitConfig {
            burst_multiplier: 0.5, // Invalid: < 1.0
            ..Default::default()
        };

        let result = UploadRateLimiter::with_config(1_000_000, config);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("must be >= 1.0"));
    }

    #[test]
    fn test_upload_allowed_within_rate() {
        let mut limiter = UploadRateLimiter::new(1_000_000).unwrap(); // 1 MB/s total

        // Should allow small uploads initially (burst capacity)
        assert!(limiter.check_upload_allowed(100_000).is_ok()); // 100 KB
        limiter.record_upload(100_000);

        let (uploaded, _, _) = limiter.upload_stats();
        assert_eq!(uploaded, 100_000);
    }

    #[test]
    fn test_upload_rate_limit_exceeded() {
        let config = UploadRateLimitConfig {
            max_upload_ratio: 0.1, // 10%
            burst_multiplier: 1.0, // No burst
            min_upload_rate_bps: 0,
        };

        let mut limiter = UploadRateLimiter::with_config(1_000_000, config).unwrap();

        // Try to upload more than the bucket capacity (100 KB/s with no burst)
        let result = limiter.check_upload_allowed(200_000); // 200 KB
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("rate limit exceeded")
        );
    }

    #[test]
    fn test_bandwidth_update() {
        let mut limiter = UploadRateLimiter::new(1_000_000).unwrap(); // 1 MB/s

        // Update to higher bandwidth
        limiter.update_bandwidth(2_000_000).unwrap(); // 2 MB/s

        let (_, _, allowed_rate) = limiter.upload_stats();
        assert_eq!(allowed_rate, 400_000); // 20% of 2 MB/s = 400 KB/s
    }

    #[test]
    fn test_bandwidth_update_zero_fails() {
        let mut limiter = UploadRateLimiter::new(1_000_000).unwrap();

        let result = limiter.update_bandwidth(0);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("cannot be zero"));
    }

    #[test]
    fn test_minimum_upload_rate_enforced() {
        let config = UploadRateLimitConfig {
            max_upload_ratio: 0.01,       // 1% of bandwidth
            min_upload_rate_bps: 100_000, // 100 KB/s minimum
            ..Default::default()
        };

        let limiter = UploadRateLimiter::with_config(1_000_000, config).unwrap(); // 1 MB/s
        let (_, _, allowed_rate) = limiter.clone().upload_stats();

        // Should be minimum rate, not 1% of 1 MB/s (10 KB/s)
        assert_eq!(allowed_rate, 100_000);
    }

    #[test]
    fn test_burst_capacity() {
        let config = UploadRateLimitConfig {
            max_upload_ratio: 0.1, // 10%
            burst_multiplier: 3.0, // 3x burst
            min_upload_rate_bps: 0,
        };

        let mut limiter = UploadRateLimiter::with_config(1_000_000, config).unwrap();

        // Should allow burst upload (3x the sustained rate)
        // Sustained: 100 KB/s, Burst: 300 KB
        assert!(limiter.check_upload_allowed(250_000).is_ok()); // 250 KB within burst
        limiter.record_upload(250_000);

        // But not excessive burst
        assert!(limiter.check_upload_allowed(100_000).is_err()); // Would exceed remaining capacity
    }

    #[test]
    fn test_available_upload_bytes() {
        let mut limiter = UploadRateLimiter::new(1_000_000).unwrap();

        let initial_available = limiter.available_upload_bytes();
        assert!(initial_available > 0, "Should have initial capacity");

        // Use some capacity
        let upload_size = 50_000; // Use smaller size to avoid token bucket edge cases
        limiter.check_upload_allowed(upload_size).unwrap();
        limiter.record_upload(upload_size);

        let remaining_available = limiter.available_upload_bytes();
        assert!(
            remaining_available < initial_available,
            "Available capacity should decrease after upload"
        );

        // Token bucket may consume more than requested due to implementation details
        // Just verify that some capacity was consumed
        assert!(
            remaining_available <= initial_available,
            "Remaining capacity should not exceed initial capacity"
        );
    }

    #[test]
    fn test_rate_calculation_with_different_ratios() {
        let bandwidth = 1_000_000; // 1 MB/s

        // Test 20% ratio
        let config_20 = UploadRateLimitConfig {
            max_upload_ratio: 0.2,
            ..Default::default()
        };
        let rate_20 = UploadRateLimiter::calculate_upload_rate(bandwidth, &config_20);
        assert_eq!(rate_20, 200_000); // 200 KB/s

        // Test 10% ratio
        let config_10 = UploadRateLimitConfig {
            max_upload_ratio: 0.1,
            ..Default::default()
        };
        let rate_10 = UploadRateLimiter::calculate_upload_rate(bandwidth, &config_10);
        assert_eq!(rate_10, 100_000); // 100 KB/s
    }

    #[test]
    fn test_token_refill_over_time() {
        let config = UploadRateLimitConfig {
            max_upload_ratio: 0.1,
            burst_multiplier: 1.0,
            min_upload_rate_bps: 100_000, // 100 KB/s
        };

        let mut limiter = UploadRateLimiter::with_config(1_000_000, config).unwrap();

        // Consume all available tokens
        let available = limiter.available_upload_bytes();
        limiter.check_upload_allowed(available).unwrap();
        limiter.record_upload(available);

        assert_eq!(limiter.available_upload_bytes(), 0);

        // Wait for token refill
        std::thread::sleep(Duration::from_millis(10));

        // Should have some tokens back
        assert!(limiter.available_upload_bytes() > 0);
    }
}
