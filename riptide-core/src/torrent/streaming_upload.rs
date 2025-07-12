//! Streaming-optimized upload management for BitTorrent peers.
//!
//! Implements upload throttling designed specifically for media streaming scenarios
//! where download bandwidth is prioritized over upload bandwidth, but some uploading
//! is necessary for peer reciprocity and connection maintenance.

use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use super::{InfoHash, PieceIndex};

/// Configuration for streaming-optimized uploading.
#[derive(Debug, Clone)]
pub struct StreamingUploadConfig {
    /// Maximum upload bandwidth as percentage of total bandwidth (0.0 to 1.0)
    pub max_upload_bandwidth_ratio: f64,
    /// Maximum concurrent upload slots per torrent
    pub max_upload_slots: usize,
    /// Minimum reciprocity ratio before deprioritizing peer (received/sent bytes)
    pub min_reciprocity_ratio: f64,
    /// Time to wait for reciprocity before deprioritizing peer
    pub reciprocity_timeout: Duration,
    /// Size of upload queue per peer
    pub upload_queue_size: usize,
}

impl Default for StreamingUploadConfig {
    fn default() -> Self {
        Self {
            max_upload_bandwidth_ratio: 0.15, // 15% of bandwidth for uploads
            max_upload_slots: 8,
            min_reciprocity_ratio: 0.1, // Expect at least 10% reciprocity
            reciprocity_timeout: Duration::from_secs(30),
            upload_queue_size: 4,
        }
    }
}

/// Tracks peer reciprocity and upload prioritization.
#[derive(Debug, Clone)]
struct PeerUploadStats {
    bytes_uploaded_to_peer: u64,
    bytes_downloaded_from_peer: u64,
    last_download_activity: Instant,
    upload_queue: VecDeque<UploadRequest>,
    is_actively_uploading: bool,
}

impl PeerUploadStats {
    fn new() -> Self {
        Self {
            bytes_uploaded_to_peer: 0,
            bytes_downloaded_from_peer: 0,
            last_download_activity: Instant::now(),
            upload_queue: VecDeque::new(),
            is_actively_uploading: false,
        }
    }

    /// Calculate reciprocity ratio (received/sent). Higher is better for us.
    fn reciprocity_ratio(&self) -> f64 {
        if self.bytes_uploaded_to_peer == 0 {
            return f64::INFINITY; // Haven't uploaded anything yet
        }
        self.bytes_downloaded_from_peer as f64 / self.bytes_uploaded_to_peer as f64
    }

    /// Check if peer is worth uploading to based on reciprocity.
    fn is_worthy_peer(&self, config: &StreamingUploadConfig) -> bool {
        let now = Instant::now();
        let time_since_activity = now.duration_since(self.last_download_activity);

        // Recent downloaders get priority
        if time_since_activity < config.reciprocity_timeout {
            return true;
        }

        // Check reciprocity ratio for older connections
        self.reciprocity_ratio() >= config.min_reciprocity_ratio
    }
}

/// Upload request from a peer.
#[derive(Debug, Clone)]
pub struct UploadRequest {
    /// Index of piece being requested
    pub piece_index: PieceIndex,
    /// Byte offset within piece
    pub offset: u32,
    /// Number of bytes requested
    pub length: u32,
    /// Timestamp when request was made
    pub requested_at: Instant,
}

/// Manages streaming-optimized uploads across all peers.
///
/// Prioritizes download bandwidth while maintaining enough upload activity
/// to ensure good peer relationships and connection stability.
pub struct StreamingUpload {
    config: StreamingUploadConfig,
    peer_stats: HashMap<SocketAddr, PeerUploadStats>,
    available_bandwidth_bps: u64,
    current_streaming_position: HashMap<InfoHash, u64>, // Byte position per torrent
    active_upload_slots: usize,
    total_uploaded_bytes: u64,
    upload_start_time: Instant,
}

impl StreamingUpload {
    /// Creates new streaming upload manager with default configuration.
    pub fn new() -> Self {
        Self::with_config(StreamingUploadConfig::default())
    }

    /// Creates streaming upload manager with custom configuration.
    pub fn with_config(config: StreamingUploadConfig) -> Self {
        Self {
            config,
            peer_stats: HashMap::new(),
            available_bandwidth_bps: 10_000_000, // Default 10MB/s, should be configured
            current_streaming_position: HashMap::new(),
            active_upload_slots: 0,
            total_uploaded_bytes: 0,
            upload_start_time: Instant::now(),
        }
    }

    /// Updates available bandwidth estimate for throttling calculations.
    pub fn update_available_bandwidth(&mut self, bandwidth_bps: u64) {
        self.available_bandwidth_bps = bandwidth_bps;
    }

    /// Updates current streaming position for a torrent.
    ///
    /// Only pieces behind this position will be eligible for upload,
    /// ensuring we don't upload pieces needed for future streaming.
    pub fn update_streaming_position(&mut self, info_hash: InfoHash, byte_position: u64) {
        self.current_streaming_position
            .insert(info_hash, byte_position);
    }

    /// Records bytes downloaded from a peer to track reciprocity.
    pub fn record_download_from_peer(&mut self, peer_address: SocketAddr, bytes: u64) {
        let stats = self
            .peer_stats
            .entry(peer_address)
            .or_insert_with(PeerUploadStats::new);
        stats.bytes_downloaded_from_peer += bytes;
        stats.last_download_activity = Instant::now();
    }

    /// Records bytes uploaded to a peer.
    pub fn record_upload_to_peer(&mut self, peer_address: SocketAddr, bytes: u64) {
        let stats = self
            .peer_stats
            .entry(peer_address)
            .or_insert_with(PeerUploadStats::new);
        stats.bytes_uploaded_to_peer += bytes;
        self.total_uploaded_bytes += bytes;
    }

    /// Queues an upload request from a peer.
    ///
    /// Returns true if request was queued, false if rejected due to throttling.
    #[allow(clippy::too_many_arguments)]
    pub fn queue_upload_request(
        &mut self,
        peer_address: SocketAddr,
        info_hash: InfoHash,
        piece_index: PieceIndex,
        offset: u32,
        length: u32,
        piece_size: u32,
    ) -> bool {
        // Check if we should accept uploads from this peer
        let stats = self
            .peer_stats
            .entry(peer_address)
            .or_insert_with(PeerUploadStats::new);

        if !stats.is_worthy_peer(&self.config) {
            return false; // Reject upload from non-reciprocating peer
        }

        // Check if piece is behind streaming position (safe to upload)
        if let Some(&streaming_position) = self.current_streaming_position.get(&info_hash) {
            let piece_start_byte = piece_index.as_u32() as u64 * piece_size as u64;
            if piece_start_byte >= streaming_position {
                return false; // Don't upload pieces we might need soon
            }
        }

        // Check upload queue capacity
        if stats.upload_queue.len() >= self.config.upload_queue_size {
            return false; // Queue full
        }

        // Check global upload slot limit
        if self.active_upload_slots >= self.config.max_upload_slots {
            return false; // Too many active uploads
        }

        // Queue the request
        let request = UploadRequest {
            piece_index,
            offset,
            length,
            requested_at: Instant::now(),
        };

        stats.upload_queue.push_back(request);
        true
    }

    /// Gets next upload request to process, prioritizing reciprocating peers.
    ///
    /// Returns None if upload bandwidth is exhausted or no worthy requests available.
    pub fn next_upload_request(&mut self) -> Option<(SocketAddr, UploadRequest)> {
        // Check bandwidth throttling
        let max_upload_bps =
            (self.available_bandwidth_bps as f64 * self.config.max_upload_bandwidth_ratio) as u64;
        let current_upload_speed = self.current_upload_speed();

        if current_upload_speed >= max_upload_bps {
            return None; // Upload bandwidth exhausted
        }

        // Find best peer to upload to (highest reciprocity ratio)
        let mut best_peer: Option<(SocketAddr, f64)> = None;

        for (&peer_address, stats) in &self.peer_stats {
            if stats.upload_queue.is_empty() || stats.is_actively_uploading {
                continue;
            }

            if !stats.is_worthy_peer(&self.config) {
                continue;
            }

            let reciprocity = stats.reciprocity_ratio();
            if best_peer.is_none_or(|(_, best_ratio)| reciprocity > best_ratio) {
                best_peer = Some((peer_address, reciprocity));
            }
        }

        // Get next request from best peer
        if let Some((peer_address, _)) = best_peer
            && let Some(stats) = self.peer_stats.get_mut(&peer_address)
            && let Some(request) = stats.upload_queue.pop_front()
        {
            stats.is_actively_uploading = true;
            self.active_upload_slots += 1;
            return Some((peer_address, request));
        }

        None
    }

    /// Marks upload as completed for a peer.
    pub fn complete_upload(&mut self, peer_address: SocketAddr, bytes_uploaded: u64) {
        if let Some(stats) = self.peer_stats.get_mut(&peer_address) {
            stats.is_actively_uploading = false;
            stats.bytes_uploaded_to_peer += bytes_uploaded;
            self.total_uploaded_bytes += bytes_uploaded;
        }

        if self.active_upload_slots > 0 {
            self.active_upload_slots -= 1;
        }
    }

    /// Calculates current upload speed in bytes per second.
    fn current_upload_speed(&self) -> u64 {
        let elapsed = self.upload_start_time.elapsed();
        if elapsed.as_secs() > 0 {
            self.total_uploaded_bytes / elapsed.as_secs()
        } else {
            0
        }
    }

    /// Removes disconnected peer from tracking.
    pub fn remove_peer(&mut self, peer_address: SocketAddr) {
        if let Some(stats) = self.peer_stats.remove(&peer_address)
            && stats.is_actively_uploading
            && self.active_upload_slots > 0
        {
            self.active_upload_slots -= 1;
        }
    }

    /// Gets upload statistics summary.
    pub fn upload_stats(&self) -> (u64, u64) {
        (self.total_uploaded_bytes, self.current_upload_speed())
    }

    /// Gets peer upload statistics for debugging.
    pub fn peer_upload_stats(&self, peer_address: SocketAddr) -> Option<(u64, u64, f64)> {
        self.peer_stats.get(&peer_address).map(|stats| {
            (
                stats.bytes_uploaded_to_peer,
                stats.bytes_downloaded_from_peer,
                stats.reciprocity_ratio(),
            )
        })
    }

    /// Cleans up old upload requests and inactive peers.
    pub fn cleanup_stale_requests(&mut self) {
        let now = Instant::now();
        let request_timeout = Duration::from_secs(60);

        for stats in self.peer_stats.values_mut() {
            // Remove old requests from queue
            stats
                .upload_queue
                .retain(|req| now.duration_since(req.requested_at) < request_timeout);
        }

        // Remove peers with no activity for a long time
        let peer_timeout = Duration::from_secs(300); // 5 minutes
        self.peer_stats
            .retain(|_, stats| now.duration_since(stats.last_download_activity) < peer_timeout);
    }
}

impl Default for StreamingUpload {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};

    use super::*;

    #[test]
    fn test_streaming_upload_creation() {
        let upload = StreamingUpload::new();
        let (total_uploaded, upload_speed) = upload.upload_stats();
        assert_eq!(total_uploaded, 0);
        assert_eq!(upload_speed, 0);
    }

    #[test]
    fn test_reciprocity_tracking() {
        let mut upload = StreamingUpload::new();
        let peer_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 6881);

        // Record some activity
        upload.record_download_from_peer(peer_addr, 1000);
        upload.record_upload_to_peer(peer_addr, 500);

        let (uploaded, downloaded, ratio) = upload.peer_upload_stats(peer_addr).unwrap();
        assert_eq!(uploaded, 500);
        assert_eq!(downloaded, 1000);
        assert_eq!(ratio, 2.0); // Downloaded 2x what we uploaded
    }

    #[test]
    fn test_streaming_position_filtering() {
        let mut upload = StreamingUpload::new();
        let peer_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 6881);
        let info_hash = InfoHash::new([1u8; 20]);

        // Set streaming position to piece 10 (byte 10240 with 1024-byte pieces)
        upload.update_streaming_position(info_hash, 10240);

        // Make peer worthy by recording download activity
        upload.record_download_from_peer(peer_addr, 1000);

        // Should accept upload for piece 5 (behind streaming position)
        let accepted =
            upload.queue_upload_request(peer_addr, info_hash, PieceIndex::new(5), 0, 1024, 1024);
        assert!(accepted);

        // Should reject upload for piece 15 (ahead of streaming position)
        let rejected =
            upload.queue_upload_request(peer_addr, info_hash, PieceIndex::new(15), 0, 1024, 1024);
        assert!(!rejected);
    }

    #[test]
    fn test_bandwidth_throttling() {
        let config = StreamingUploadConfig {
            max_upload_bandwidth_ratio: 0.1, // 10% of bandwidth
            ..Default::default()
        };
        let mut upload = StreamingUpload::with_config(config);
        upload.update_available_bandwidth(1000); // 1KB/s total, so 100 B/s for uploads

        // After uploading more than the limit, should throttle
        upload.total_uploaded_bytes = 200; // Simulate past uploads
        upload.upload_start_time = Instant::now() - Duration::from_secs(1); // 1 second ago

        // Current speed is 200 B/s, which exceeds 100 B/s limit
        let request = upload.next_upload_request();
        assert!(request.is_none()); // Should be throttled
    }
}
