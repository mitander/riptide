//! Integration tests for upload rate limiting functionality.
//!
//! Verifies that upload bandwidth throttling works correctly to prevent
//! uploads from saturating download bandwidth during streaming operations.

use std::time::Duration;

use riptide_core::torrent::{
    InfoHash, PeerId, PeerManager, TcpPeers, UploadRateLimitConfig, UploadRateLimiter,
};

#[tokio::test]
async fn test_upload_rate_limiter_basic_functionality() {
    let mut rate_limiter = UploadRateLimiter::new(1_000_000).unwrap(); // 1 MB/s total

    // Should allow small uploads within rate limit (20% of 1 MB/s = 200 KB/s)
    assert!(rate_limiter.check_upload_allowed(50_000).is_ok()); // 50 KB
    rate_limiter.record_upload(50_000);

    let (uploaded, _current_rate, allowed_rate) = rate_limiter.upload_stats();
    assert_eq!(uploaded, 50_000);
    assert_eq!(allowed_rate, 200_000); // 20% of 1 MB/s
}

#[tokio::test]
async fn test_upload_rate_limiter_with_burst() {
    let config = UploadRateLimitConfig {
        max_upload_ratio: 0.1, // 10%
        burst_multiplier: 2.0, // 2x burst
        min_upload_rate_bps: 0,
    };

    let mut rate_limiter = UploadRateLimiter::with_config(1_000_000, config).unwrap();

    // Should allow burst upload (2x the sustained rate)
    // Sustained: 100 KB/s, Burst: 200 KB
    assert!(rate_limiter.check_upload_allowed(150_000).is_ok()); // 150 KB within burst
    rate_limiter.record_upload(150_000);

    // But not excessive burst
    assert!(rate_limiter.check_upload_allowed(100_000).is_err()); // Would exceed remaining capacity
}

#[tokio::test]
async fn test_upload_rate_limiter_bandwidth_update() {
    let mut rate_limiter = UploadRateLimiter::new(1_000_000).unwrap(); // 1 MB/s

    // Update to higher bandwidth
    rate_limiter.update_bandwidth(2_000_000).unwrap(); // 2 MB/s

    let (_, _, allowed_rate) = rate_limiter.upload_stats();
    assert_eq!(allowed_rate, 400_000); // 20% of 2 MB/s = 400 KB/s

    // Should now allow larger uploads
    assert!(rate_limiter.check_upload_allowed(300_000).is_ok()); // 300 KB
}

#[tokio::test]
async fn test_upload_rate_limiter_minimum_rate_enforced() {
    let config = UploadRateLimitConfig {
        max_upload_ratio: 0.01,       // 1% of bandwidth
        min_upload_rate_bps: 100_000, // 100 KB/s minimum
        ..Default::default()
    };

    let rate_limiter = UploadRateLimiter::with_config(1_000_000, config).unwrap(); // 1 MB/s
    let (_, _, allowed_rate) = rate_limiter.clone().upload_stats();

    // Should be minimum rate, not 1% of 1 MB/s (10 KB/s)
    assert_eq!(allowed_rate, 100_000);
}

#[tokio::test]
async fn test_upload_rate_limiter_invalid_configurations() {
    // Zero bandwidth should fail
    assert!(UploadRateLimiter::new(0).is_err());

    // Invalid upload ratio should fail
    let config = UploadRateLimitConfig {
        max_upload_ratio: 1.5, // > 1.0
        ..Default::default()
    };
    assert!(UploadRateLimiter::with_config(1_000_000, config).is_err());

    // Invalid burst multiplier should fail
    let config = UploadRateLimitConfig {
        burst_multiplier: 0.5, // < 1.0
        ..Default::default()
    };
    assert!(UploadRateLimiter::with_config(1_000_000, config).is_err());
}

#[tokio::test]
async fn test_tcp_peers_upload_configuration() {
    let peer_id = PeerId::generate();
    let mut tcp_peers = TcpPeers::new(peer_id, 10);

    let info_hash = InfoHash::new([1u8; 20]);
    let total_bandwidth = 2_000_000; // 2 MB/s

    // Configure upload rate limiting
    let result = tcp_peers
        .configure_upload(info_hash, 1024, total_bandwidth)
        .await;

    assert!(result.is_ok(), "Upload configuration should succeed");
}

#[tokio::test]
async fn test_tcp_peers_streaming_position_update() {
    let peer_id = PeerId::generate();
    let mut tcp_peers = TcpPeers::new(peer_id, 10);

    let info_hash = InfoHash::new([2u8; 20]);

    // Update streaming position
    let result = tcp_peers.update_streaming_position(info_hash, 10240).await;

    assert!(result.is_ok(), "Streaming position update should succeed");
}

#[tokio::test]
async fn test_upload_rate_limiter_statistics_tracking() {
    let mut rate_limiter = UploadRateLimiter::new(1_000_000).unwrap();

    // Perform some uploads
    assert!(rate_limiter.check_upload_allowed(50_000).is_ok());
    rate_limiter.record_upload(50_000);

    assert!(rate_limiter.check_upload_allowed(30_000).is_ok());
    rate_limiter.record_upload(30_000);

    let (total_uploaded, _current_rate, allowed_rate) = rate_limiter.upload_stats();
    assert_eq!(total_uploaded, 80_000);
    assert_eq!(allowed_rate, 200_000); // 20% of 1 MB/s

    // Check available bytes
    let available = rate_limiter.available_upload_bytes();
    assert!(available > 0, "Should have some tokens available");
}

#[tokio::test]
async fn test_upload_rate_limiter_token_refill() {
    let config = UploadRateLimitConfig {
        max_upload_ratio: 0.1,
        burst_multiplier: 1.0,        // No burst for predictable behavior
        min_upload_rate_bps: 100_000, // 100 KB/s
    };

    let mut rate_limiter = UploadRateLimiter::with_config(1_000_000, config).unwrap();

    // Consume all available tokens
    let available = rate_limiter.available_upload_bytes();
    if available > 0 {
        assert!(rate_limiter.check_upload_allowed(available).is_ok());
        rate_limiter.record_upload(available);
    }

    assert_eq!(rate_limiter.available_upload_bytes(), 0);

    // Wait for token refill
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Should have some tokens back
    assert!(
        rate_limiter.available_upload_bytes() > 0,
        "Tokens should refill over time"
    );
}

#[tokio::test]
async fn test_upload_rate_calculation_with_different_ratios() {
    let bandwidth = 1_000_000; // 1 MB/s

    // Test 20% ratio (default)
    let rate_limiter_20 = UploadRateLimiter::new(bandwidth).unwrap();
    let (_, _, allowed_rate_20) = rate_limiter_20.clone().upload_stats();
    assert_eq!(allowed_rate_20, 200_000); // 200 KB/s

    // Test 10% ratio
    let config_10 = UploadRateLimitConfig {
        max_upload_ratio: 0.1,
        ..Default::default()
    };
    let rate_limiter_10 = UploadRateLimiter::with_config(bandwidth, config_10).unwrap();
    let (_, _, allowed_rate_10) = rate_limiter_10.clone().upload_stats();
    assert_eq!(allowed_rate_10, 100_000); // 100 KB/s

    // Test 5% ratio
    let config_5 = UploadRateLimitConfig {
        max_upload_ratio: 0.05,
        ..Default::default()
    };
    let rate_limiter_5 = UploadRateLimiter::with_config(bandwidth, config_5).unwrap();
    let (_, _, allowed_rate_5) = rate_limiter_5.clone().upload_stats();
    assert_eq!(allowed_rate_5, 50_000); // 50 KB/s
}

#[tokio::test]
async fn test_upload_rate_limiter_zero_bandwidth_update_fails() {
    let mut rate_limiter = UploadRateLimiter::new(1_000_000).unwrap();

    let result = rate_limiter.update_bandwidth(0);
    assert!(result.is_err(), "Updating to zero bandwidth should fail");
    assert!(result.unwrap_err().to_string().contains("cannot be zero"));
}
