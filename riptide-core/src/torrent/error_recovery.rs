//! Error recovery and retry logic for piece downloads
//!
//! Implements robust error handling for network failures, peer disconnections,
//! piece verification failures, and timeout scenarios with exponential backoff.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use crate::torrent::{PieceIndex, TorrentError};

/// Maximum number of retry attempts for piece downloads
const MAX_RETRY_ATTEMPTS: u32 = 5;

/// Base delay for exponential backoff (in milliseconds)
const BASE_RETRY_DELAY_MS: u64 = 500;

/// Maximum backoff delay (in milliseconds)
const MAX_RETRY_DELAY_MS: u64 = 30_000;

/// Maximum consecutive failures before blacklisting a peer temporarily
const MAX_CONSECUTIVE_FAILURES: u32 = 3;

/// Temporary blacklist duration for problematic peers
const PEER_BLACKLIST_DURATION: Duration = Duration::from_secs(300); // 5 minutes

/// Retry strategy for failed operations
#[derive(Debug, Clone)]
pub enum RetryStrategy {
    /// Retry with exponential backoff
    ExponentialBackoff {
        base_delay: Duration,
        max_delay: Duration,
    },
    /// Retry with fixed delay
    FixedDelay { delay: Duration },
    /// Retry immediately
    Immediate,
    /// Do not retry
    NoRetry,
}

/// Categorizes different types of errors for appropriate retry strategies
#[derive(Debug, Clone, PartialEq)]
pub enum ErrorCategory {
    /// Network connection issues (should retry with different peer)
    NetworkError,
    /// Timeout waiting for response (should retry with backoff)
    TimeoutError,
    /// Piece hash mismatch (should retry from different peer)
    VerificationError,
    /// Peer protocol violation (should blacklist peer)
    ProtocolError,
    /// Storage/disk issues (should retry with delay)
    StorageError,
    /// Fatal errors that should not be retried
    FatalError,
}

/// Tracks retry attempts and failure patterns for piece downloads
#[derive(Debug, Clone)]
pub struct PieceRetryTracker {
    pub piece_index: PieceIndex,
    pub attempt_count: u32,
    pub first_attempt: Instant,
    pub last_attempt: Instant,
    pub failed_peers: HashMap<SocketAddr, PeerFailureInfo>,
    pub retry_strategy: RetryStrategy,
}

/// Information about peer failures for blacklisting decisions
#[derive(Debug, Clone)]
pub struct PeerFailureInfo {
    pub consecutive_failures: u32,
    pub last_failure: Instant,
    pub error_categories: Vec<ErrorCategory>,
    pub total_timeouts: u32,
    pub blacklisted_until: Option<Instant>,
}

/// Manages error recovery and retry logic for torrent downloads
#[derive(Debug)]
pub struct ErrorRecoveryManager {
    piece_retries: HashMap<PieceIndex, PieceRetryTracker>,
    peer_failures: HashMap<SocketAddr, PeerFailureInfo>,
}

impl Default for RetryStrategy {
    fn default() -> Self {
        Self::ExponentialBackoff {
            base_delay: Duration::from_millis(BASE_RETRY_DELAY_MS),
            max_delay: Duration::from_millis(MAX_RETRY_DELAY_MS),
        }
    }
}

impl RetryStrategy {
    /// Calculate delay for the given attempt number
    pub fn calculate_delay(&self, attempt: u32) -> Duration {
        match self {
            Self::ExponentialBackoff {
                base_delay,
                max_delay,
            } => {
                let exponential_delay =
                    base_delay.as_millis() as u64 * 2_u64.pow(attempt.saturating_sub(1));
                let capped_delay = exponential_delay.min(max_delay.as_millis() as u64);
                Duration::from_millis(capped_delay)
            }
            Self::FixedDelay { delay } => *delay,
            Self::Immediate => Duration::ZERO,
            Self::NoRetry => Duration::ZERO,
        }
    }

    /// Check if retry should be attempted based on strategy
    pub fn should_retry(&self, attempt: u32) -> bool {
        match self {
            Self::ExponentialBackoff { .. } | Self::FixedDelay { .. } | Self::Immediate => {
                attempt <= MAX_RETRY_ATTEMPTS
            }
            Self::NoRetry => false,
        }
    }
}

impl ErrorCategory {
    /// Determine error category from TorrentError
    pub fn from_torrent_error(error: &TorrentError) -> Self {
        match error {
            TorrentError::PeerConnectionError { .. } => Self::NetworkError,
            TorrentError::PieceHashMismatch { .. } => Self::VerificationError,
            TorrentError::ProtocolError { .. } => Self::ProtocolError,
            TorrentError::Storage(_) => Self::StorageError,
            TorrentError::TrackerConnectionFailed { .. } | TorrentError::TrackerTimeout { .. } => {
                Self::NetworkError
            }
            TorrentError::InvalidTorrentFile { .. } | TorrentError::EngineShutdown => {
                Self::FatalError
            }
            _ => Self::NetworkError, // Default to network error for retryable issues
        }
    }

    /// Get appropriate retry strategy for this error category
    pub fn default_retry_strategy(&self) -> RetryStrategy {
        match self {
            Self::NetworkError => RetryStrategy::ExponentialBackoff {
                base_delay: Duration::from_millis(1000),
                max_delay: Duration::from_millis(10_000),
            },
            Self::TimeoutError => RetryStrategy::ExponentialBackoff {
                base_delay: Duration::from_millis(2000),
                max_delay: Duration::from_millis(15_000),
            },
            Self::VerificationError => RetryStrategy::FixedDelay {
                delay: Duration::from_millis(500),
            },
            Self::ProtocolError => RetryStrategy::ExponentialBackoff {
                base_delay: Duration::from_millis(5000),
                max_delay: Duration::from_millis(30_000),
            },
            Self::StorageError => RetryStrategy::ExponentialBackoff {
                base_delay: Duration::from_millis(1000),
                max_delay: Duration::from_millis(5000),
            },
            Self::FatalError => RetryStrategy::NoRetry,
        }
    }
}

impl PieceRetryTracker {
    /// Create new retry tracker for a piece
    pub fn new(piece_index: PieceIndex) -> Self {
        let now = Instant::now();
        Self {
            piece_index,
            attempt_count: 0,
            first_attempt: now,
            last_attempt: now,
            failed_peers: HashMap::new(),
            retry_strategy: RetryStrategy::default(),
        }
    }

    /// Record a failed attempt with specific peer and error
    pub fn record_failure(&mut self, peer_addr: SocketAddr, error: &TorrentError) {
        self.attempt_count += 1;
        self.last_attempt = Instant::now();

        let error_category = ErrorCategory::from_torrent_error(error);

        let peer_info = self
            .failed_peers
            .entry(peer_addr)
            .or_insert_with(|| PeerFailureInfo {
                consecutive_failures: 0,
                last_failure: Instant::now(),
                error_categories: Vec::new(),
                total_timeouts: 0,
                blacklisted_until: None,
            });

        peer_info.consecutive_failures += 1;
        peer_info.last_failure = Instant::now();
        peer_info.error_categories.push(error_category.clone());

        if matches!(error_category, ErrorCategory::TimeoutError) {
            peer_info.total_timeouts += 1;
        }

        // Update retry strategy based on error patterns
        self.retry_strategy = error_category.default_retry_strategy();
    }

    /// Check if retry should be attempted
    pub fn should_retry(&self) -> bool {
        self.retry_strategy.should_retry(self.attempt_count)
    }

    /// Calculate delay before next retry attempt
    pub fn next_retry_delay(&self) -> Duration {
        self.retry_strategy.calculate_delay(self.attempt_count)
    }

    /// Check if peer should be avoided for this piece
    pub fn should_avoid_peer(&self, peer_addr: &SocketAddr) -> bool {
        if let Some(peer_info) = self.failed_peers.get(peer_addr) {
            // Check if peer is temporarily blacklisted
            if let Some(blacklist_expiry) = peer_info.blacklisted_until
                && Instant::now() < blacklist_expiry
            {
                return true;
            }

            // Avoid peers with too many consecutive failures
            peer_info.consecutive_failures >= MAX_CONSECUTIVE_FAILURES
        } else {
            false
        }
    }

    /// Get list of peers to avoid for this piece
    pub fn peers_to_avoid(&self) -> Vec<SocketAddr> {
        self.failed_peers
            .iter()
            .filter(|(addr, _)| self.should_avoid_peer(addr))
            .map(|(addr, _)| *addr)
            .collect()
    }
}

impl PeerFailureInfo {
    /// Check if peer should be temporarily blacklisted
    pub fn should_blacklist(&self) -> bool {
        self.consecutive_failures >= MAX_CONSECUTIVE_FAILURES
            || self.total_timeouts >= 5
            || self
                .error_categories
                .iter()
                .filter(|cat| matches!(cat, ErrorCategory::ProtocolError))
                .count()
                >= 2
    }

    /// Calculate blacklist duration based on failure pattern
    pub fn calculate_blacklist_duration(&self) -> Duration {
        let base_duration = PEER_BLACKLIST_DURATION;

        // Extend blacklist for repeated offenders
        let multiplier = match self.consecutive_failures {
            0..=3 => 1,
            4..=6 => 2,
            7..=10 => 4,
            _ => 8,
        };

        base_duration * multiplier
    }
}

impl ErrorRecoveryManager {
    /// Create new error recovery manager
    pub fn new() -> Self {
        Self {
            piece_retries: HashMap::new(),
            peer_failures: HashMap::new(),
        }
    }

    /// Record a piece download failure
    pub fn record_piece_failure(
        &mut self,
        piece_index: PieceIndex,
        peer_addr: SocketAddr,
        error: &TorrentError,
    ) {
        // Update piece-specific retry tracker
        let piece_tracker = self
            .piece_retries
            .entry(piece_index)
            .or_insert_with(|| PieceRetryTracker::new(piece_index));

        piece_tracker.record_failure(peer_addr, error);

        // Update global peer failure tracking
        let peer_info = self
            .peer_failures
            .entry(peer_addr)
            .or_insert_with(|| PeerFailureInfo {
                consecutive_failures: 0,
                last_failure: Instant::now(),
                error_categories: Vec::new(),
                total_timeouts: 0,
                blacklisted_until: None,
            });

        peer_info.consecutive_failures += 1;
        peer_info.last_failure = Instant::now();
        peer_info
            .error_categories
            .push(ErrorCategory::from_torrent_error(error));

        // Check if peer should be blacklisted
        if peer_info.should_blacklist() {
            let blacklist_duration = peer_info.calculate_blacklist_duration();
            peer_info.blacklisted_until = Some(Instant::now() + blacklist_duration);

            tracing::warn!(
                "Temporarily blacklisting peer {} for {} seconds due to repeated failures",
                peer_addr,
                blacklist_duration.as_secs()
            );
        }
    }

    /// Record successful piece download (reset failure counters)
    pub fn record_piece_success(&mut self, piece_index: PieceIndex, peer_addr: SocketAddr) {
        // Remove piece from retry tracking
        self.piece_retries.remove(&piece_index);

        // Reset peer failure counter on success
        if let Some(peer_info) = self.peer_failures.get_mut(&peer_addr) {
            peer_info.consecutive_failures = 0;
            peer_info.blacklisted_until = None;
        }
    }

    /// Check if piece should be retried
    pub fn should_retry_piece(&self, piece_index: PieceIndex) -> bool {
        self.piece_retries
            .get(&piece_index)
            .map(|tracker| tracker.should_retry())
            .unwrap_or(true) // Allow first attempt
    }

    /// Get delay before retrying piece download
    pub fn get_retry_delay(&self, piece_index: PieceIndex) -> Duration {
        self.piece_retries
            .get(&piece_index)
            .map(|tracker| tracker.next_retry_delay())
            .unwrap_or(Duration::ZERO)
    }

    /// Check if peer is currently blacklisted
    pub fn is_peer_blacklisted(&self, peer_addr: &SocketAddr) -> bool {
        if let Some(peer_info) = self.peer_failures.get(peer_addr) {
            if let Some(blacklist_expiry) = peer_info.blacklisted_until {
                Instant::now() < blacklist_expiry
            } else {
                false
            }
        } else {
            false
        }
    }

    /// Get list of peers to avoid for a specific piece
    pub fn get_peers_to_avoid_for_piece(&self, piece_index: PieceIndex) -> Vec<SocketAddr> {
        let mut peers_to_avoid = Vec::new();

        // Add piece-specific failed peers
        if let Some(piece_tracker) = self.piece_retries.get(&piece_index) {
            peers_to_avoid.extend(piece_tracker.peers_to_avoid());
        }

        // Add globally blacklisted peers
        peers_to_avoid.extend(
            self.peer_failures
                .iter()
                .filter(|(_, info)| {
                    info.blacklisted_until
                        .map(|expiry| Instant::now() < expiry)
                        .unwrap_or(false)
                })
                .map(|(addr, _)| *addr),
        );

        peers_to_avoid.sort();
        peers_to_avoid.dedup();
        peers_to_avoid
    }

    /// Get statistics about error recovery
    pub fn get_statistics(&self) -> ErrorRecoveryStats {
        let total_pieces_with_failures = self.piece_retries.len();
        let total_blacklisted_peers = self
            .peer_failures
            .values()
            .filter(|info| {
                info.blacklisted_until
                    .map(|expiry| Instant::now() < expiry)
                    .unwrap_or(false)
            })
            .count();

        let total_retry_attempts = self
            .piece_retries
            .values()
            .map(|tracker| tracker.attempt_count)
            .sum();

        ErrorRecoveryStats {
            total_pieces_with_failures,
            total_blacklisted_peers,
            total_retry_attempts,
            active_piece_retries: self.piece_retries.len(),
        }
    }

    /// Clean up expired entries to prevent memory leaks
    pub fn cleanup_expired(&mut self) {
        let now = Instant::now();

        // Remove old piece retry trackers (older than 1 hour)
        self.piece_retries.retain(|_, tracker| {
            now.duration_since(tracker.last_attempt) < Duration::from_secs(3600)
        });

        // Remove expired blacklist entries
        self.peer_failures.retain(|_, peer_info| {
            if let Some(blacklist_expiry) = peer_info.blacklisted_until {
                now < blacklist_expiry || peer_info.consecutive_failures > 0
            } else {
                peer_info.consecutive_failures > 0
            }
        });
    }
}

/// Statistics about error recovery operations
#[derive(Debug, Clone)]
pub struct ErrorRecoveryStats {
    pub total_pieces_with_failures: usize,
    pub total_blacklisted_peers: usize,
    pub total_retry_attempts: u32,
    pub active_piece_retries: usize,
}

impl Default for ErrorRecoveryManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};

    use super::*;

    fn create_test_peer() -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 6881)
    }

    fn create_test_error() -> TorrentError {
        TorrentError::PeerConnectionError {
            reason: "Test connection failure".to_string(),
        }
    }

    #[test]
    fn test_retry_strategy_calculation() {
        let strategy = RetryStrategy::ExponentialBackoff {
            base_delay: Duration::from_millis(100),
            max_delay: Duration::from_millis(1000),
        };

        assert_eq!(strategy.calculate_delay(1), Duration::from_millis(100));
        assert_eq!(strategy.calculate_delay(2), Duration::from_millis(200));
        assert_eq!(strategy.calculate_delay(3), Duration::from_millis(400));
        assert_eq!(strategy.calculate_delay(4), Duration::from_millis(800));
        assert_eq!(strategy.calculate_delay(5), Duration::from_millis(1000)); // Capped
    }

    #[test]
    fn test_error_categorization() {
        let network_error = TorrentError::PeerConnectionError {
            reason: "Connection failed".to_string(),
        };
        assert_eq!(
            ErrorCategory::from_torrent_error(&network_error),
            ErrorCategory::NetworkError
        );

        let hash_error = TorrentError::PieceHashMismatch {
            index: PieceIndex::new(0),
        };
        assert_eq!(
            ErrorCategory::from_torrent_error(&hash_error),
            ErrorCategory::VerificationError
        );

        let protocol_error = TorrentError::ProtocolError {
            message: "Invalid message".to_string(),
        };
        assert_eq!(
            ErrorCategory::from_torrent_error(&protocol_error),
            ErrorCategory::ProtocolError
        );
    }

    #[test]
    fn test_piece_retry_tracker() {
        let mut tracker = PieceRetryTracker::new(PieceIndex::new(0));
        let peer = create_test_peer();
        let error = create_test_error();

        assert!(tracker.should_retry());
        assert_eq!(tracker.attempt_count, 0);

        tracker.record_failure(peer, &error);
        assert_eq!(tracker.attempt_count, 1);
        assert!(tracker.should_retry());

        // Record multiple failures
        for _ in 1..MAX_RETRY_ATTEMPTS {
            tracker.record_failure(peer, &error);
        }

        assert_eq!(tracker.attempt_count, MAX_RETRY_ATTEMPTS);
        assert!(tracker.should_retry());

        // One more failure should exceed limit
        tracker.record_failure(peer, &error);
        assert!(!tracker.should_retry());
    }

    #[test]
    fn test_peer_blacklisting() {
        let mut manager = ErrorRecoveryManager::new();
        let peer = create_test_peer();
        let piece = PieceIndex::new(0);
        let error = create_test_error();

        assert!(!manager.is_peer_blacklisted(&peer));

        // Record failures below blacklist threshold
        for _ in 0..MAX_CONSECUTIVE_FAILURES - 1 {
            manager.record_piece_failure(piece, peer, &error);
        }
        assert!(!manager.is_peer_blacklisted(&peer));

        // One more failure should trigger blacklist
        manager.record_piece_failure(piece, peer, &error);
        assert!(manager.is_peer_blacklisted(&peer));
    }

    #[test]
    fn test_peer_success_recovery() {
        let mut manager = ErrorRecoveryManager::new();
        let peer = create_test_peer();
        let piece = PieceIndex::new(0);
        let error = create_test_error();

        // Record failures to trigger blacklist
        for _ in 0..MAX_CONSECUTIVE_FAILURES {
            manager.record_piece_failure(piece, peer, &error);
        }
        assert!(manager.is_peer_blacklisted(&peer));

        // Success should clear blacklist
        manager.record_piece_success(piece, peer);
        assert!(!manager.is_peer_blacklisted(&peer));
    }

    #[test]
    fn test_peers_to_avoid() {
        let mut manager = ErrorRecoveryManager::new();
        let peer1 = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 6881);
        let peer2 = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 6882);
        let piece = PieceIndex::new(0);
        let error = create_test_error();

        // Blacklist peer1
        for _ in 0..MAX_CONSECUTIVE_FAILURES {
            manager.record_piece_failure(piece, peer1, &error);
        }

        let avoided_peers = manager.get_peers_to_avoid_for_piece(piece);
        assert!(avoided_peers.contains(&peer1));
        assert!(!avoided_peers.contains(&peer2));
    }

    #[test]
    fn test_cleanup_expired() {
        let mut manager = ErrorRecoveryManager::new();
        let peer = create_test_peer();
        let piece = PieceIndex::new(0);
        let error = create_test_error();

        manager.record_piece_failure(piece, peer, &error);
        assert!(manager.piece_retries.contains_key(&piece));

        // Cleanup should not remove recent entries
        manager.cleanup_expired();
        assert!(manager.piece_retries.contains_key(&piece));
    }
}
