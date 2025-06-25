//! Streaming coordination service for managing playback sessions.

use std::collections::HashMap;
use std::time::Duration;

use tracing::{debug, error, info, warn};

use crate::domain::errors::DomainError;
use crate::domain::library::{LibraryContent, LibraryId};
use crate::domain::streaming::{
    BufferHealth, ClientInfo, QualitySettings, StreamId, StreamSession, StreamValidationError,
};

/// Errors that can occur during streaming operations.
#[derive(Debug, thiserror::Error)]
pub enum StreamingError {
    #[error("Stream not found: {stream_id}")]
    StreamNotFound { stream_id: StreamId },

    #[error("Content not streamable: {library_id}")]
    ContentNotStreamable { library_id: LibraryId },

    #[error("Streaming service failed: {reason}")]
    StreamingServiceFailed { reason: String },

    #[error("Domain validation failed")]
    DomainValidation(#[from] DomainError),

    #[error("Stream validation failed")]
    StreamValidation(#[from] StreamValidationError),

    #[error("Client not compatible: {reason}")]
    ClientIncompatible { reason: String },

    #[error("Bandwidth insufficient for requested quality")]
    InsufficientBandwidth,

    #[error("Content file not accessible: {path}")]
    FileNotAccessible { path: String },
}

/// Service for coordinating streaming sessions and managing playback.
pub struct StreamingCoordinator {
    streaming_service: Box<dyn StreamingService>,
    active_sessions: HashMap<StreamId, StreamSession>,
    #[allow(dead_code)] // TODO: Implement transcoding functionality
    transcoding_service: Option<Box<dyn TranscodingService>>,
    session_timeout: Duration,
    max_concurrent_streams: usize,
}

impl StreamingCoordinator {
    /// Creates a new streaming coordinator.
    pub fn new(
        streaming_service: Box<dyn StreamingService>,
        transcoding_service: Option<Box<dyn TranscodingService>>,
    ) -> Self {
        Self {
            streaming_service,
            active_sessions: HashMap::new(),
            transcoding_service,
            session_timeout: Duration::from_secs(1800), // 30 minutes
            max_concurrent_streams: 10,
        }
    }

    /// Starts a new streaming session for library content.
    ///
    /// Validates content availability, client compatibility, and creates
    /// a streaming session with appropriate quality settings.
    ///
    /// # Errors
    /// - `StreamingError::ContentNotStreamable` - Content cannot be streamed
    /// - `StreamingError::ClientIncompatible` - Client doesn't support required formats
    /// - `StreamingError::FileNotAccessible` - Content file is not available
    /// - `StreamingError::DomainValidation` - Invalid session parameters
    pub async fn start_stream(
        &mut self,
        library_content: &LibraryContent,
        client_info: ClientInfo,
        quality_settings: QualitySettings,
    ) -> Result<StreamSession, StreamingError> {
        info!(
            "Starting stream for library content: {} (client: {})",
            library_content.id(),
            client_info.user_agent
        );

        // Validate content is streamable
        if !library_content.is_streamable() {
            return Err(StreamingError::ContentNotStreamable {
                library_id: library_content.id(),
            });
        }

        // Check concurrent stream limit
        if self.active_sessions.len() >= self.max_concurrent_streams {
            return Err(StreamingError::StreamingServiceFailed {
                reason: "Maximum concurrent streams reached".to_string(),
            });
        }

        // Validate file accessibility
        let file_path = library_content.file_path().to_string_lossy().to_string();
        if !self
            .streaming_service
            .is_file_accessible(library_content.file_path())
            .await
        {
            return Err(StreamingError::FileNotAccessible { path: file_path });
        }

        // Check client compatibility
        self.validate_client_compatibility(&client_info, &quality_settings)?;

        // Create streaming session
        let mut session = StreamSession::new(library_content.id(), client_info, quality_settings)?;

        // Initialize streaming service
        let stream_info = self
            .streaming_service
            .prepare_stream(library_content, session.quality_settings())
            .await
            .map_err(|e| StreamingError::StreamingServiceFailed {
                reason: e.to_string(),
            })?;

        // Set total duration if known
        if let Some(duration) = stream_info.total_duration {
            session.set_total_duration(duration);
        }

        // Track active session
        let stream_id = session.id();
        self.active_sessions.insert(stream_id, session.clone());

        info!("Stream session started: {}", stream_id);
        Ok(session)
    }

    /// Updates streaming session with new position and metrics.
    pub async fn update_stream_progress(
        &mut self,
        stream_id: StreamId,
        position: Duration,
        buffer_health: BufferHealth,
        bitrate: u64,
    ) -> Result<(), StreamingError> {
        let session = self
            .active_sessions
            .get_mut(&stream_id)
            .ok_or(StreamingError::StreamNotFound { stream_id })?;

        session.update_position(position);
        session.update_buffer_health(buffer_health);
        session.update_bitrate(bitrate);

        debug!(
            "Stream progress updated: {} - {}% complete",
            stream_id,
            session.progress_percent()
        );

        // Handle adaptive quality adjustments
        if session.quality_settings().adaptive_quality {
            Self::adjust_quality_if_needed_static(session).await?;
        }

        Ok(())
    }

    /// Pauses a streaming session.
    pub async fn pause_stream(&mut self, stream_id: StreamId) -> Result<(), StreamingError> {
        let session = self
            .active_sessions
            .get_mut(&stream_id)
            .ok_or(StreamingError::StreamNotFound { stream_id })?;

        session.pause();
        info!("Stream paused: {}", stream_id);
        Ok(())
    }

    /// Resumes a paused streaming session.
    pub async fn resume_stream(&mut self, stream_id: StreamId) -> Result<(), StreamingError> {
        let session = self
            .active_sessions
            .get_mut(&stream_id)
            .ok_or(StreamingError::StreamNotFound { stream_id })?;

        session.resume();
        info!("Stream resumed: {}", stream_id);
        Ok(())
    }

    /// Seeks to a specific position in the stream.
    pub async fn seek_stream(
        &mut self,
        stream_id: StreamId,
        position: Duration,
    ) -> Result<(), StreamingError> {
        let session = self
            .active_sessions
            .get_mut(&stream_id)
            .ok_or(StreamingError::StreamNotFound { stream_id })?;

        session.seek(position)?;

        // Notify streaming service of seek operation
        self.streaming_service
            .seek_to_position(stream_id, position)
            .await
            .map_err(|e| StreamingError::StreamingServiceFailed {
                reason: e.to_string(),
            })?;

        info!(
            "Stream seeked to {} seconds: {}",
            position.as_secs(),
            stream_id
        );
        Ok(())
    }

    /// Stops a streaming session and cleans up resources.
    pub async fn stop_stream(&mut self, stream_id: StreamId) -> Result<(), StreamingError> {
        if let Some(mut session) = self.active_sessions.remove(&stream_id) {
            session.stop();

            // Clean up streaming service resources
            self.streaming_service
                .cleanup_stream(stream_id)
                .await
                .map_err(|e| StreamingError::StreamingServiceFailed {
                    reason: e.to_string(),
                })?;

            info!("Stream stopped and cleaned up: {}", stream_id);
        }

        Ok(())
    }

    /// Gets information about an active streaming session.
    pub fn get_stream_session(&self, stream_id: StreamId) -> Option<&StreamSession> {
        self.active_sessions.get(&stream_id)
    }

    /// Lists all active streaming sessions.
    pub fn list_active_sessions(&self) -> Vec<&StreamSession> {
        self.active_sessions.values().collect()
    }

    /// Cleans up idle and expired streaming sessions.
    pub async fn cleanup_idle_sessions(&mut self) -> Result<usize, StreamingError> {
        let mut sessions_to_remove = Vec::new();

        for (stream_id, session) in &self.active_sessions {
            if session.is_idle(self.session_timeout) {
                sessions_to_remove.push(*stream_id);
            }
        }

        let cleanup_count = sessions_to_remove.len();
        for stream_id in sessions_to_remove {
            warn!("Cleaning up idle session: {}", stream_id);
            self.stop_stream(stream_id).await?;
        }

        info!("Cleaned up {} idle streaming sessions", cleanup_count);
        Ok(cleanup_count)
    }

    /// Sets maximum concurrent streams.
    pub fn set_max_concurrent_streams(&mut self, max: usize) {
        self.max_concurrent_streams = max;
        info!("Maximum concurrent streams set to: {}", max);
    }

    /// Sets session timeout duration.
    pub fn set_session_timeout(&mut self, timeout: Duration) {
        self.session_timeout = timeout;
        info!("Session timeout set to: {} seconds", timeout.as_secs());
    }

    /// Gets the number of active streaming sessions.
    pub fn active_session_count(&self) -> usize {
        self.active_sessions.len()
    }

    /// Validates client compatibility with streaming requirements.
    fn validate_client_compatibility(
        &self,
        client_info: &ClientInfo,
        quality_settings: &QualitySettings,
    ) -> Result<(), StreamingError> {
        // Check if client supports required codecs
        if !client_info.supports_codec("h264") && !client_info.supports_codec("h265") {
            return Err(StreamingError::ClientIncompatible {
                reason: "Client doesn't support h264 or h265 codecs".to_string(),
            });
        }

        // Check resolution compatibility
        if let (Some(target_res), Some(max_res)) = (
            &quality_settings.target_resolution,
            &client_info.max_resolution,
        ) {
            if target_res.is_higher_than(max_res) {
                return Err(StreamingError::ClientIncompatible {
                    reason: format!(
                        "Target resolution {target_res} exceeds client maximum {max_res}"
                    ),
                });
            }
        }

        Ok(())
    }

    /// Adjusts streaming quality based on current performance.
    async fn adjust_quality_if_needed_static(
        session: &mut StreamSession,
    ) -> Result<(), StreamingError> {
        let buffer_health = session.buffer_health();

        // Reduce quality if buffer is starving
        if buffer_health.is_starving() && session.bitrate() > 1_000_000 {
            // Reduce bitrate by 25%
            let new_bitrate = (session.bitrate() as f64 * 0.75) as u64;
            session.update_bitrate(new_bitrate);

            warn!(
                "Reducing stream quality due to buffer starvation: {} -> {} kbps",
                session.bitrate() / 1000,
                new_bitrate / 1000
            );
        }
        // Increase quality if buffer is healthy and we have headroom
        else if buffer_health.is_healthy()
            && session.bitrate() < session.quality_settings().max_bitrate
        {
            let new_bitrate = std::cmp::min(
                (session.bitrate() as f64 * 1.1) as u64,
                session.quality_settings().max_bitrate,
            );
            session.update_bitrate(new_bitrate);

            debug!(
                "Increasing stream quality: {} -> {} kbps",
                session.bitrate() / 1000,
                new_bitrate / 1000
            );
        }

        Ok(())
    }
}

/// Trait for streaming service implementations.
#[async_trait::async_trait]
pub trait StreamingService: Send + Sync {
    /// Checks if a file is accessible for streaming.
    async fn is_file_accessible(&self, file_path: &std::path::Path) -> bool;

    /// Prepares a stream for the given content and quality settings.
    async fn prepare_stream(
        &self,
        content: &LibraryContent,
        quality_settings: &QualitySettings,
    ) -> Result<StreamInfo, StreamingServiceError>;

    /// Seeks to a specific position in the stream.
    async fn seek_to_position(
        &self,
        stream_id: StreamId,
        position: Duration,
    ) -> Result<(), StreamingServiceError>;

    /// Cleans up resources for a finished stream.
    async fn cleanup_stream(&self, stream_id: StreamId) -> Result<(), StreamingServiceError>;
}

/// Trait for transcoding service implementations.
#[async_trait::async_trait]
pub trait TranscodingService: Send + Sync {
    /// Checks if content needs transcoding for the client.
    async fn needs_transcoding(
        &self,
        content: &LibraryContent,
        client_info: &ClientInfo,
    ) -> Result<bool, TranscodingError>;

    /// Starts transcoding a file for streaming.
    async fn start_transcode(
        &self,
        content: &LibraryContent,
        quality_settings: &QualitySettings,
    ) -> Result<Box<dyn TranscodeHandle>, TranscodingError>;
}

/// Information about a prepared stream.
#[derive(Debug, Clone)]
pub struct StreamInfo {
    pub stream_url: String,
    pub total_duration: Option<Duration>,
    pub available_qualities: Vec<QualitySettings>,
}

/// Handle for managing transcoding operations.
#[async_trait::async_trait]
pub trait TranscodeHandle: Send + Sync {
    /// Gets transcoding progress.
    async fn get_progress(&self) -> Result<TranscodeProgress, TranscodingError>;

    /// Cancels the transcoding operation.
    async fn cancel(&mut self) -> Result<(), TranscodingError>;
}

/// Transcoding progress information.
#[derive(Debug, Clone)]
pub struct TranscodeProgress {
    pub percent_complete: f32,
    pub output_path: std::path::PathBuf,
    pub estimated_completion: Option<Duration>,
}

/// Errors that can occur in streaming services.
#[derive(Debug, thiserror::Error)]
pub enum StreamingServiceError {
    #[error("File access error: {reason}")]
    FileAccessError { reason: String },

    #[error("Stream preparation failed: {reason}")]
    PreparationFailed { reason: String },

    #[error("Seek operation failed: {reason}")]
    SeekFailed { reason: String },

    #[error("Cleanup failed: {reason}")]
    CleanupFailed { reason: String },
}

/// Errors that can occur during transcoding.
#[derive(Debug, thiserror::Error)]
pub enum TranscodingError {
    #[error("Transcoding failed: {reason}")]
    TranscodingFailed { reason: String },

    #[error("Codec not supported: {codec}")]
    UnsupportedCodec { codec: String },

    #[error("Insufficient resources for transcoding")]
    InsufficientResources,
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::domain::content::{ContentSource, SourceHealth, SourceType};
    use crate::domain::library::LibraryContent;
    use crate::domain::media::{Media, MediaType, VideoQuality};
    use crate::domain::streaming::{ClientInfo, QualitySettings, Resolution, StreamStatus};

    /// Mock streaming service for testing.
    struct MockStreamingService {
        file_accessible: bool,
    }

    impl MockStreamingService {
        fn new(file_accessible: bool) -> Self {
            Self { file_accessible }
        }
    }

    #[async_trait::async_trait]
    impl StreamingService for MockStreamingService {
        async fn is_file_accessible(&self, _file_path: &std::path::Path) -> bool {
            self.file_accessible
        }

        async fn prepare_stream(
            &self,
            _content: &LibraryContent,
            _quality_settings: &QualitySettings,
        ) -> Result<StreamInfo, StreamingServiceError> {
            Ok(StreamInfo {
                stream_url: "http://localhost:8080/stream/test".to_string(),
                total_duration: Some(Duration::from_secs(3600)), // 1 hour
                available_qualities: vec![QualitySettings::default()],
            })
        }

        async fn seek_to_position(
            &self,
            _stream_id: StreamId,
            _position: Duration,
        ) -> Result<(), StreamingServiceError> {
            Ok(())
        }

        async fn cleanup_stream(&self, _stream_id: StreamId) -> Result<(), StreamingServiceError> {
            Ok(())
        }
    }

    fn create_test_library_content() -> LibraryContent {
        let media = Media::new("Test Movie".to_string(), MediaType::Movie);
        let source = ContentSource::new(
            media.id(),
            SourceType::Torrent,
            "Test.Movie.1080p.mkv".to_string(),
            2_000_000_000,
            VideoQuality::BluRay1080p,
            SourceHealth::from_swarm_stats(50, 10),
            "magnet:?xt=urn:btih:test".to_string(),
        )
        .unwrap();

        let mut content = LibraryContent::new(
            media.id(),
            source.id(),
            PathBuf::from("/movies/test_movie.mkv"),
            2_000_000_000,
        )
        .unwrap();

        // Make it streamable by setting progress
        content.update_progress(100.0, 0, 0).unwrap();
        content
    }

    #[tokio::test]
    async fn test_streaming_coordinator_creation() {
        let streaming_service = Box::new(MockStreamingService::new(true));
        let coordinator = StreamingCoordinator::new(streaming_service, None);

        assert_eq!(coordinator.active_session_count(), 0);
    }

    #[tokio::test]
    async fn test_start_stream_success() {
        let streaming_service = Box::new(MockStreamingService::new(true));
        let mut coordinator = StreamingCoordinator::new(streaming_service, None);

        let content = create_test_library_content();
        let client_info =
            ClientInfo::new("TestPlayer/1.0".to_string(), "192.168.1.100".to_string());
        let quality_settings = QualitySettings::default();

        let session = coordinator
            .start_stream(&content, client_info, quality_settings)
            .await
            .unwrap();

        assert_eq!(session.library_id(), content.id());
        assert_eq!(coordinator.active_session_count(), 1);
    }

    #[tokio::test]
    async fn test_start_stream_file_not_accessible() {
        let streaming_service = Box::new(MockStreamingService::new(false)); // File not accessible
        let mut coordinator = StreamingCoordinator::new(streaming_service, None);

        let content = create_test_library_content();
        let client_info =
            ClientInfo::new("TestPlayer/1.0".to_string(), "192.168.1.100".to_string());
        let quality_settings = QualitySettings::default();

        let result = coordinator
            .start_stream(&content, client_info, quality_settings)
            .await;

        assert!(matches!(
            result,
            Err(StreamingError::FileNotAccessible { .. })
        ));
    }

    #[tokio::test]
    async fn test_start_stream_content_not_streamable() {
        let streaming_service = Box::new(MockStreamingService::new(true));
        let mut coordinator = StreamingCoordinator::new(streaming_service, None);

        let media = Media::new("Test Movie".to_string(), MediaType::Movie);
        let source = ContentSource::new(
            media.id(),
            SourceType::Torrent,
            "Test.Movie.1080p.mkv".to_string(),
            2_000_000_000,
            VideoQuality::BluRay1080p,
            SourceHealth::from_swarm_stats(50, 10),
            "magnet:?xt=urn:btih:test".to_string(),
        )
        .unwrap();

        // Create content with 0% progress (not streamable)
        let content = LibraryContent::new(
            media.id(),
            source.id(),
            PathBuf::from("/movies/test_movie.mkv"),
            2_000_000_000,
        )
        .unwrap();

        let client_info =
            ClientInfo::new("TestPlayer/1.0".to_string(), "192.168.1.100".to_string());
        let quality_settings = QualitySettings::default();

        let result = coordinator
            .start_stream(&content, client_info, quality_settings)
            .await;

        assert!(matches!(
            result,
            Err(StreamingError::ContentNotStreamable { .. })
        ));
    }

    #[tokio::test]
    async fn test_client_compatibility_validation() {
        let streaming_service = Box::new(MockStreamingService::new(true));
        let mut coordinator = StreamingCoordinator::new(streaming_service, None);

        let content = create_test_library_content();

        // Create client with no codec support
        let mut client_info = ClientInfo::new(
            "IncompatiblePlayer/1.0".to_string(),
            "192.168.1.100".to_string(),
        );
        client_info.supported_codecs.clear(); // Remove all codec support

        let quality_settings = QualitySettings::default();

        let result = coordinator
            .start_stream(&content, client_info, quality_settings)
            .await;

        assert!(matches!(
            result,
            Err(StreamingError::ClientIncompatible { .. })
        ));
    }

    #[tokio::test]
    async fn test_resolution_compatibility_validation() {
        let streaming_service = Box::new(MockStreamingService::new(true));
        let mut coordinator = StreamingCoordinator::new(streaming_service, None);

        let content = create_test_library_content();

        // Create client with limited resolution support
        let mut client_info =
            ClientInfo::new("LimitedPlayer/1.0".to_string(), "192.168.1.100".to_string());
        client_info.max_resolution = Some(Resolution::hd_720p());

        // Request 4K streaming
        let quality_settings = QualitySettings {
            target_resolution: Some(Resolution::uhd_4k()),
            max_bitrate: 25_000_000,
            adaptive_quality: false,
            hardware_acceleration: true,
        };

        let result = coordinator
            .start_stream(&content, client_info, quality_settings)
            .await;

        assert!(matches!(
            result,
            Err(StreamingError::ClientIncompatible { .. })
        ));
    }

    #[tokio::test]
    async fn test_stream_progress_updates() {
        let streaming_service = Box::new(MockStreamingService::new(true));
        let mut coordinator = StreamingCoordinator::new(streaming_service, None);

        let content = create_test_library_content();
        let client_info =
            ClientInfo::new("TestPlayer/1.0".to_string(), "192.168.1.100".to_string());
        let quality_settings = QualitySettings {
            target_resolution: Some(Resolution::hd_1080p()),
            max_bitrate: 8_000_000,
            adaptive_quality: false, // Disable adaptive quality for predictable test
            hardware_acceleration: true,
        };

        let session = coordinator
            .start_stream(&content, client_info, quality_settings)
            .await
            .unwrap();

        let stream_id = session.id();

        // Update progress
        let buffer_health = BufferHealth::new(Duration::from_secs(8), Duration::from_secs(10));
        coordinator
            .update_stream_progress(
                stream_id,
                Duration::from_secs(300), // 5 minutes
                buffer_health,
                5_000_000, // 5 Mbps
            )
            .await
            .unwrap();

        let updated_session = coordinator.get_stream_session(stream_id).unwrap();
        assert_eq!(updated_session.current_position(), Duration::from_secs(300));
        assert_eq!(updated_session.bitrate(), 5_000_000);
    }

    #[tokio::test]
    async fn test_stream_pause_resume() {
        let streaming_service = Box::new(MockStreamingService::new(true));
        let mut coordinator = StreamingCoordinator::new(streaming_service, None);

        let content = create_test_library_content();
        let client_info =
            ClientInfo::new("TestPlayer/1.0".to_string(), "192.168.1.100".to_string());
        let quality_settings = QualitySettings::default();

        let session = coordinator
            .start_stream(&content, client_info, quality_settings)
            .await
            .unwrap();

        let stream_id = session.id();

        // First start playing by updating position
        let buffer_health = BufferHealth::new(Duration::from_secs(8), Duration::from_secs(10));
        coordinator
            .update_stream_progress(stream_id, Duration::from_secs(30), buffer_health, 5_000_000)
            .await
            .unwrap();

        let session = coordinator.get_stream_session(stream_id).unwrap();
        assert!(matches!(session.stream_status(), StreamStatus::Playing));

        // Pause stream
        coordinator.pause_stream(stream_id).await.unwrap();
        let session = coordinator.get_stream_session(stream_id).unwrap();
        assert!(matches!(session.stream_status(), StreamStatus::Paused));

        // Resume stream
        coordinator.resume_stream(stream_id).await.unwrap();
        let session = coordinator.get_stream_session(stream_id).unwrap();
        assert!(matches!(session.stream_status(), StreamStatus::Playing));
    }

    #[tokio::test]
    async fn test_stream_seek() {
        let streaming_service = Box::new(MockStreamingService::new(true));
        let mut coordinator = StreamingCoordinator::new(streaming_service, None);

        let content = create_test_library_content();
        let client_info =
            ClientInfo::new("TestPlayer/1.0".to_string(), "192.168.1.100".to_string());
        let quality_settings = QualitySettings::default();

        let mut session = coordinator
            .start_stream(&content, client_info, quality_settings)
            .await
            .unwrap();

        session.set_total_duration(Duration::from_secs(3600)); // 1 hour
        let stream_id = session.id();

        // Seek to 30 minutes
        coordinator
            .seek_stream(stream_id, Duration::from_secs(1800))
            .await
            .unwrap();

        let updated_session = coordinator.get_stream_session(stream_id).unwrap();
        assert_eq!(
            updated_session.current_position(),
            Duration::from_secs(1800)
        );
    }

    #[tokio::test]
    async fn test_stop_stream() {
        let streaming_service = Box::new(MockStreamingService::new(true));
        let mut coordinator = StreamingCoordinator::new(streaming_service, None);

        let content = create_test_library_content();
        let client_info =
            ClientInfo::new("TestPlayer/1.0".to_string(), "192.168.1.100".to_string());
        let quality_settings = QualitySettings::default();

        let session = coordinator
            .start_stream(&content, client_info, quality_settings)
            .await
            .unwrap();

        let stream_id = session.id();
        assert_eq!(coordinator.active_session_count(), 1);

        // Stop stream
        coordinator.stop_stream(stream_id).await.unwrap();
        assert_eq!(coordinator.active_session_count(), 0);
        assert!(coordinator.get_stream_session(stream_id).is_none());
    }

    #[tokio::test]
    async fn test_concurrent_stream_limit() {
        let streaming_service = Box::new(MockStreamingService::new(true));
        let mut coordinator = StreamingCoordinator::new(streaming_service, None);
        coordinator.set_max_concurrent_streams(1); // Only allow 1 stream

        let content1 = create_test_library_content();
        let content2 = create_test_library_content();

        let client_info =
            ClientInfo::new("TestPlayer/1.0".to_string(), "192.168.1.100".to_string());
        let quality_settings = QualitySettings::default();

        // First stream should succeed
        let _session1 = coordinator
            .start_stream(&content1, client_info.clone(), quality_settings.clone())
            .await
            .unwrap();

        // Second stream should fail
        let result2 = coordinator
            .start_stream(&content2, client_info, quality_settings)
            .await;

        assert!(matches!(
            result2,
            Err(StreamingError::StreamingServiceFailed { .. })
        ));
    }
}
