//! Diagnostic tools for progressive streaming issues

use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use crate::storage::DataSource;
use crate::torrent::InfoHash;

/// Diagnostic information about progressive streaming
#[derive(Debug, Clone)]
pub struct StreamingDiagnostics {
    /// When diagnostics started
    pub start_time: Instant,
    /// File size being streamed
    pub file_size: u64,
    /// Bytes that are currently available
    pub available_bytes: u64,
    /// Number of pieces downloaded
    pub pieces_downloaded: usize,
    /// Total number of pieces
    pub total_pieces: usize,
    /// Download rate (bytes per second)
    pub download_rate: f64,
    /// Whether data availability is keeping up with streaming needs
    pub is_data_keeping_up: bool,
    /// Streaming position that would be needed now
    pub required_streaming_position: u64,
}

/// Progressive streaming diagnostics collector
pub struct StreamingDiagnosticsCollector {
    info_hash: InfoHash,
    data_source: Arc<dyn DataSource>,
    diagnostics: Arc<RwLock<StreamingDiagnostics>>,
    piece_size: u64,
}

impl StreamingDiagnosticsCollector {
    /// Create a new diagnostics collector
    pub fn new(
        info_hash: InfoHash,
        data_source: Arc<dyn DataSource>,
        file_size: u64,
        piece_size: u64,
    ) -> Self {
        let total_pieces = file_size.div_ceil(piece_size) as usize;

        let diagnostics = Arc::new(RwLock::new(StreamingDiagnostics {
            start_time: Instant::now(),
            file_size,
            available_bytes: 0,
            pieces_downloaded: 0,
            total_pieces,
            download_rate: 0.0,
            is_data_keeping_up: false,
            required_streaming_position: 0,
        }));

        Self {
            info_hash,
            data_source,
            diagnostics,
            piece_size,
        }
    }

    /// Start collecting diagnostics
    pub async fn start_monitoring(&self) {
        let data_source = Arc::clone(&self.data_source);
        let diagnostics = Arc::clone(&self.diagnostics);
        let info_hash = self.info_hash;
        let piece_size = self.piece_size;

        tokio::spawn(async move {
            let mut check_interval = tokio::time::interval(Duration::from_secs(1));
            let mut last_available_bytes = 0u64;
            let mut last_check_time = Instant::now();

            loop {
                check_interval.tick().await;

                if let Err(e) = Self::collect_diagnostics(
                    &data_source,
                    &diagnostics,
                    info_hash,
                    piece_size,
                    &mut last_available_bytes,
                    &mut last_check_time,
                )
                .await
                {
                    warn!("Failed to collect diagnostics for {}: {}", info_hash, e);
                }
            }
        });
    }

    /// Collect current diagnostics
    async fn collect_diagnostics(
        data_source: &Arc<dyn DataSource>,
        diagnostics: &Arc<RwLock<StreamingDiagnostics>>,
        info_hash: InfoHash,
        piece_size: u64,
        last_available_bytes: &mut u64,
        last_check_time: &mut Instant,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let file_size = data_source.file_size(info_hash).await?;
        let now = Instant::now();

        // Calculate how much data is available by checking sequential availability
        let mut available_bytes = 0u64;
        let chunk_size = piece_size;

        // Check availability in chunks from the beginning
        while available_bytes < file_size {
            let chunk_end = (available_bytes + chunk_size).min(file_size);
            match data_source
                .check_range_availability(info_hash, available_bytes..chunk_end)
                .await
            {
                Ok(availability) if availability.available => {
                    available_bytes = chunk_end;
                }
                Ok(_) => {
                    // First unavailable chunk found
                    break;
                }
                Err(_) => break,
            }
        }

        // Calculate download rate
        let time_elapsed = now.duration_since(*last_check_time).as_secs_f64();
        let bytes_downloaded = available_bytes.saturating_sub(*last_available_bytes);
        let download_rate = if time_elapsed > 0.0 {
            bytes_downloaded as f64 / time_elapsed
        } else {
            0.0
        };

        // Calculate streaming requirements
        let elapsed_since_start = now
            .duration_since(diagnostics.read().await.start_time)
            .as_secs_f64();
        let streaming_bitrate = 5_000_000.0; // Assume 5 Mbps streaming bitrate
        let required_streaming_position = (elapsed_since_start * streaming_bitrate / 8.0) as u64;

        let is_data_keeping_up = available_bytes >= required_streaming_position;

        // Update diagnostics
        {
            let mut diag = diagnostics.write().await;
            diag.available_bytes = available_bytes;
            diag.pieces_downloaded = (available_bytes / piece_size) as usize;
            diag.download_rate = download_rate;
            diag.is_data_keeping_up = is_data_keeping_up;
            diag.required_streaming_position = required_streaming_position;
        }

        // Log interesting findings
        if !is_data_keeping_up {
            warn!(
                "Data availability falling behind for {}: available={}, required={}, rate={:.1} KB/s",
                info_hash,
                available_bytes,
                required_streaming_position,
                download_rate / 1024.0
            );
        }

        if available_bytes > *last_available_bytes {
            debug!(
                "Progress for {}: +{} bytes, total={}/{}, rate={:.1} KB/s",
                info_hash,
                bytes_downloaded,
                available_bytes,
                file_size,
                download_rate / 1024.0
            );
        }

        *last_available_bytes = available_bytes;
        *last_check_time = now;

        Ok(())
    }

    /// Get current diagnostics
    pub async fn get_diagnostics(&self) -> StreamingDiagnostics {
        self.diagnostics.read().await.clone()
    }

    /// Print a summary of the current state
    pub async fn print_summary(&self) {
        let diag = self.get_diagnostics().await;

        info!(
            "Streaming Diagnostics for {}:\n\
             File Size: {} MB\n\
             Available: {} MB ({:.1}%)\n\
             Pieces: {}/{}\n\
             Download Rate: {:.1} KB/s\n\
             Required Position: {} MB\n\
             Keeping Up: {}",
            self.info_hash,
            diag.file_size / 1024 / 1024,
            diag.available_bytes / 1024 / 1024,
            (diag.available_bytes as f64 / diag.file_size as f64) * 100.0,
            diag.pieces_downloaded,
            diag.total_pieces,
            diag.download_rate / 1024.0,
            diag.required_streaming_position / 1024 / 1024,
            if diag.is_data_keeping_up { "YES" } else { "NO" }
        );
    }
}

/// Helper function to diagnose why progressive streaming is failing
///
/// # Errors
/// Returns an error if data source operations fail or file size cannot be determined
pub async fn diagnose_progressive_streaming_issue(
    info_hash: InfoHash,
    data_source: Arc<dyn DataSource>,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let file_size = data_source.file_size(info_hash).await?;
    let piece_size = 256 * 1024; // Assume 256KB pieces

    // Check first 10MB availability
    let initial_check_size = (10 * 1024 * 1024).min(file_size);
    let initial_availability = data_source
        .check_range_availability(info_hash, 0..initial_check_size)
        .await?;

    // Check how much sequential data is available
    let mut sequential_available = 0u64;
    let chunk_size = piece_size;

    while sequential_available < file_size {
        let chunk_end = (sequential_available + chunk_size).min(file_size);
        match data_source
            .check_range_availability(info_hash, sequential_available..chunk_end)
            .await
        {
            Ok(availability) if availability.available => {
                sequential_available = chunk_end;
            }
            Ok(_) => break,
            Err(_) => break,
        }
    }

    let mut diagnosis = String::new();
    diagnosis.push_str(&format!(
        "Progressive Streaming Diagnosis for {info_hash}:\n"
    ));
    diagnosis.push_str(&format!("File Size: {} MB\n", file_size / 1024 / 1024));
    diagnosis.push_str(&format!(
        "Initial 10MB Available: {}\n",
        initial_availability.available
    ));
    diagnosis.push_str(&format!(
        "Sequential Data Available: {} MB ({:.1}%)\n",
        sequential_available / 1024 / 1024,
        (sequential_available as f64 / file_size as f64) * 100.0
    ));

    if !initial_availability.available {
        diagnosis.push_str("❌ ISSUE: Initial 10MB not available - streaming cannot start\n");
    } else if sequential_available < file_size / 10 {
        diagnosis.push_str("❌ ISSUE: Very little sequential data available - will cause early FFmpeg termination\n");
    } else if sequential_available < file_size / 2 {
        diagnosis.push_str(
            "⚠️  WARNING: Less than 50% sequential data available - may cause truncation\n",
        );
    } else {
        diagnosis.push_str("✅ Sequential data looks good for progressive streaming\n");
    }

    Ok(diagnosis)
}
