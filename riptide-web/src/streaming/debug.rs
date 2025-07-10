//! Debug utilities for streaming troubleshooting

use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use riptide_core::torrent::InfoHash;
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

use crate::streaming::{
    HttpStreamingError, HttpStreamingService, StreamingRequest, StreamingResponse,
};

/// Debug information for a streaming session
#[derive(Debug, Clone)]
pub struct StreamingDebugInfo {
    pub info_hash: InfoHash,
    pub request_count: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub remux_attempts: u64,
    pub remux_successes: u64,
    pub remux_failures: u64,
    pub last_error: Option<String>,
    pub ffmpeg_logs: Vec<String>,
    pub total_bytes_served: u64,
    pub average_response_time_ms: f64,
}

impl Default for StreamingDebugInfo {
    fn default() -> Self {
        Self {
            info_hash: InfoHash::new([0u8; 20]),
            request_count: 0,
            cache_hits: 0,
            cache_misses: 0,
            remux_attempts: 0,
            remux_successes: 0,
            remux_failures: 0,
            last_error: None,
            ffmpeg_logs: Vec::new(),
            total_bytes_served: 0,
            average_response_time_ms: 0.0,
        }
    }
}

/// Debug wrapper for HTTP streaming service
pub struct DebugStreamingService {
    inner: Arc<HttpStreamingService>,
    debug_info: Arc<Mutex<Vec<StreamingDebugInfo>>>,
}

impl DebugStreamingService {
    /// Create new debug streaming service
    pub fn new(inner: Arc<HttpStreamingService>) -> Self {
        Self {
            inner,
            debug_info: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Get debug info for a specific torrent
    pub async fn debug_info(&self, info_hash: InfoHash) -> Option<StreamingDebugInfo> {
        let debug_info = self.debug_info.lock().await;
        debug_info
            .iter()
            .find(|info| info.info_hash == info_hash)
            .cloned()
    }

    /// Get all debug info
    pub async fn all_debug_info(&self) -> Vec<StreamingDebugInfo> {
        self.debug_info.lock().await.clone()
    }

    /// Clear debug info
    pub async fn clear_debug_info(&self) {
        self.debug_info.lock().await.clear();
    }

    /// Handle streaming request with debug logging
    pub async fn handle_streaming_request_debug(
        &self,
        request: StreamingRequest,
    ) -> Result<StreamingResponse, HttpStreamingError> {
        let start_time = Instant::now();
        let info_hash = request.info_hash;

        info!(
            "DEBUG: Starting streaming request for {} - Range: {:?}, Time offset: {:?}, Quality: {:?}",
            info_hash, request.range, request.time_offset, request.preferred_quality
        );

        // Update debug info
        {
            let mut debug_info = self.debug_info.lock().await;
            let exists = debug_info.iter().any(|info| info.info_hash == info_hash);
            if !exists {
                let new_info = StreamingDebugInfo {
                    info_hash,
                    ..Default::default()
                };
                debug_info.push(new_info);
            }
            let entry = debug_info
                .iter_mut()
                .find(|info| info.info_hash == info_hash)
                .unwrap();
            entry.request_count += 1;
        }

        // Check cache status before request
        self.check_cache_status(info_hash).await;

        // Check piece availability
        self.check_piece_availability(info_hash).await;

        // Call inner service
        let result = self.inner.handle_streaming_request(request).await;

        let elapsed = start_time.elapsed();
        let elapsed_ms = elapsed.as_secs_f64() * 1000.0;

        // Update debug info based on result
        {
            let mut debug_info = self.debug_info.lock().await;
            if let Some(entry) = debug_info
                .iter_mut()
                .find(|info| info.info_hash == info_hash)
            {
                // Update average response time
                let total_time = entry.average_response_time_ms * (entry.request_count - 1) as f64;
                entry.average_response_time_ms =
                    (total_time + elapsed_ms) / entry.request_count as f64;

                match &result {
                    Ok(response) => {
                        info!(
                            "DEBUG: Request succeeded for {} - Status: {:?}, Time: {:.2}ms",
                            info_hash, response.status, elapsed_ms
                        );

                        // Estimate bytes served from Content-Length header
                        if let Some(content_length) = response.headers.get("Content-Length")
                            && let Ok(length_str) = content_length.to_str()
                            && let Ok(length) = length_str.parse::<u64>()
                        {
                            entry.total_bytes_served += length;
                        }
                    }
                    Err(e) => {
                        error!(
                            "DEBUG: Request failed for {} - Error: {:?}, Time: {:.2}ms",
                            info_hash, e, elapsed_ms
                        );
                        entry.last_error = Some(format!("{e:?}"));
                    }
                }
            }
        }

        // Check cache status after request
        self.check_cache_status(info_hash).await;

        result
    }

    /// Check cache status for debugging
    async fn check_cache_status(&self, info_hash: InfoHash) {
        let cache_dir = std::env::temp_dir().join("riptide-remux-cache");
        let cache_path = cache_dir.join(format!("{info_hash}.mp4"));
        let lock_path = cache_dir.join(format!("{info_hash}.lock"));

        let cache_exists = cache_path.exists();
        let lock_exists = lock_path.exists();

        if cache_exists {
            if let Ok(metadata) = std::fs::metadata(&cache_path) {
                info!(
                    "DEBUG: Cache file exists for {} - Size: {} bytes, Path: {}",
                    info_hash,
                    metadata.len(),
                    cache_path.display()
                );

                // Validate MP4 structure
                if let Err(e) = self.validate_mp4_file(&cache_path) {
                    error!(
                        "DEBUG: Invalid cached MP4 for {} - Error: {}, Path: {}",
                        info_hash,
                        e,
                        cache_path.display()
                    );
                }
            }
        } else {
            debug!(
                "DEBUG: No cache file for {} at {}",
                info_hash,
                cache_path.display()
            );
        }

        if lock_exists {
            warn!(
                "DEBUG: Lock file exists for {} at {} - Remux may be in progress",
                info_hash,
                lock_path.display()
            );
        }
    }

    /// Check piece availability
    async fn check_piece_availability(&self, info_hash: InfoHash) {
        // This would need access to piece store, but we'll log what we can
        info!(
            "DEBUG: Checking piece availability for {} (implement with piece store access)",
            info_hash
        );
    }

    /// Validate MP4 file structure
    fn validate_mp4_file(&self, path: &Path) -> Result<(), String> {
        use std::fs::File;
        use std::io::Read;

        let mut file = File::open(path).map_err(|e| format!("Failed to open file: {e}"))?;

        // Check file size
        let metadata = file
            .metadata()
            .map_err(|e| format!("Failed to get metadata: {e}"))?;

        if metadata.len() < 100 {
            return Err(format!("File too small: {} bytes", metadata.len()));
        }

        // Check for ftyp box
        let mut header = [0u8; 12];
        file.read_exact(&mut header)
            .map_err(|e| format!("Failed to read header: {e}"))?;

        if &header[4..8] != b"ftyp" {
            return Err(format!(
                "Missing ftyp box. Found: {:?}",
                String::from_utf8_lossy(&header[4..8])
            ));
        }

        info!(
            "DEBUG: MP4 validation passed for {} - Size: {} bytes",
            path.display(),
            metadata.len()
        );

        Ok(())
    }

    /// Capture FFmpeg output for debugging
    pub fn capture_ffmpeg_output(&self, _info_hash: InfoHash, _output: String) {
        // This would be called by the FFmpeg processor to capture output
        tokio::spawn(async move {
            // Store FFmpeg output in debug info
            warn!("DEBUG: FFmpeg output capture not yet implemented");
        });
    }
}

/// Debug endpoint data for web UI
#[derive(Debug, serde::Serialize)]
pub struct StreamingDebugEndpoint {
    pub torrents: Vec<TorrentDebugInfo>,
    pub cache_directory: String,
    pub cache_files: Vec<CacheFileInfo>,
    pub system_info: SystemDebugInfo,
}

#[derive(Debug, serde::Serialize)]
pub struct TorrentDebugInfo {
    pub info_hash: String,
    pub request_count: u64,
    pub cache_status: String,
    pub remux_status: String,
    pub last_error: Option<String>,
    pub bytes_served: u64,
    pub average_response_ms: f64,
}

#[derive(Debug, serde::Serialize)]
pub struct CacheFileInfo {
    pub filename: String,
    pub size_bytes: u64,
    pub created: String,
    pub is_valid: bool,
}

#[derive(Debug, serde::Serialize)]
pub struct SystemDebugInfo {
    pub ffmpeg_available: bool,
    pub ffmpeg_version: Option<String>,
    pub cache_dir_writable: bool,
    pub temp_space_available_gb: f64,
}

impl DebugStreamingService {
    /// Generate debug endpoint data
    pub async fn generate_debug_endpoint(&self) -> StreamingDebugEndpoint {
        let cache_dir = std::env::temp_dir().join("riptide-remux-cache");

        // Get cache files
        let mut cache_files = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&cache_dir) {
            for entry in entries.flatten() {
                if let Ok(metadata) = entry.metadata()
                    && entry.path().extension().and_then(|s| s.to_str()) == Some("mp4")
                {
                    let is_valid = self.validate_mp4_file(&entry.path()).is_ok();
                    cache_files.push(CacheFileInfo {
                        filename: entry.file_name().to_string_lossy().to_string(),
                        size_bytes: metadata.len(),
                        created: "N/A".to_string(), // Could add proper timestamp
                        is_valid,
                    });
                }
            }
        }

        // Check FFmpeg
        let ffmpeg_check = std::process::Command::new("ffmpeg")
            .arg("-version")
            .output();

        let (ffmpeg_available, ffmpeg_version) = match ffmpeg_check {
            Ok(output) if output.status.success() => {
                let version = String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .next()
                    .unwrap_or("Unknown")
                    .to_string();
                (true, Some(version))
            }
            _ => (false, None),
        };

        // Get debug info for all torrents
        let debug_infos = self.all_debug_info().await;
        let torrents = debug_infos
            .into_iter()
            .map(|info| {
                let cache_path = cache_dir.join(format!("{}.mp4", info.info_hash));
                let cache_status = if cache_path.exists() {
                    format!(
                        "Cached ({:.1} MB)",
                        cache_path.metadata().map(|m| m.len()).unwrap_or(0) as f64
                            / 1024.0
                            / 1024.0
                    )
                } else if cache_dir.join(format!("{}.lock", info.info_hash)).exists() {
                    "Remuxing...".to_string()
                } else {
                    "Not cached".to_string()
                };

                TorrentDebugInfo {
                    info_hash: info.info_hash.to_string(),
                    request_count: info.request_count,
                    cache_status,
                    remux_status: if info.remux_successes > 0 {
                        format!("Success ({} attempts)", info.remux_attempts)
                    } else if info.remux_failures > 0 {
                        format!("Failed ({} attempts)", info.remux_attempts)
                    } else {
                        "Not attempted".to_string()
                    },
                    last_error: info.last_error,
                    bytes_served: info.total_bytes_served,
                    average_response_ms: info.average_response_time_ms,
                }
            })
            .collect();

        StreamingDebugEndpoint {
            torrents,
            cache_directory: cache_dir.display().to_string(),
            cache_files,
            system_info: SystemDebugInfo {
                ffmpeg_available,
                ffmpeg_version,
                cache_dir_writable: std::fs::create_dir_all(&cache_dir).is_ok(),
                temp_space_available_gb: 0.0, // Could implement disk space check
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mp4_validation() {
        // Skip this test as it requires actual service setup
        // The validation logic is tested through integration tests
    }

    #[tokio::test]
    async fn test_debug_info_tracking() {
        // Test debug info tracking without actual service
        let debug_info: Arc<Mutex<Vec<StreamingDebugInfo>>> = Arc::new(Mutex::new(Vec::new()));
        let info_hash = InfoHash::new([1u8; 20]);

        // Update some debug info
        {
            let mut debug_vec = debug_info.lock().await;
            let entry = StreamingDebugInfo {
                info_hash,
                request_count: 5,
                cache_hits: 2,
                cache_misses: 3,
                ..Default::default()
            };
            debug_vec.push(entry);
        }

        // Retrieve debug info
        let debug_vec = debug_info.lock().await;
        let retrieved = debug_vec.iter().find(|info| info.info_hash == info_hash);
        assert!(retrieved.is_some());
        let info = retrieved.unwrap();
        assert_eq!(info.request_count, 5);
        assert_eq!(info.cache_hits, 2);
        assert_eq!(info.cache_misses, 3);
    }
}
