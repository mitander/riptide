//! Content discovery service for finding and ranking available sources.

use std::collections::HashMap;

use tracing::{debug, info, warn};

use crate::domain::content::{ContentSource, SourceHealth};
use crate::domain::errors::DomainError;
use crate::domain::media::{Media, MediaId};

/// Errors that can occur during content discovery operations.
#[derive(Debug, thiserror::Error)]
pub enum DiscoveryError {
    #[error("Search failed for query '{query}': {reason}")]
    SearchFailed { query: String, reason: String },

    #[error("No sources found for media ID {media_id}")]
    NoSourcesFound { media_id: MediaId },

    #[error("Source validation failed")]
    SourceValidation(#[from] DomainError),

    #[error("Provider communication failed: {provider}")]
    ProviderFailed { provider: String },
}

/// Service for discovering and managing content sources.
pub struct ContentDiscovery {
    provider_clients: HashMap<String, Box<dyn SourceProvider>>,
    source_cache: HashMap<MediaId, Vec<ContentSource>>,
    #[allow(dead_code)] // TODO: Implement cache TTL functionality
    cache_ttl_seconds: u64,
}

impl ContentDiscovery {
    /// Creates a new content discovery service.
    pub fn new() -> Self {
        Self {
            provider_clients: HashMap::new(),
            source_cache: HashMap::new(),
            cache_ttl_seconds: 300, // 5 minutes
        }
    }

    /// Registers a content source provider.
    pub fn register_provider(&mut self, name: String, provider: Box<dyn SourceProvider>) {
        debug!("Registering content provider: {}", name);
        self.provider_clients.insert(name, provider);
    }

    /// Discovers all available sources for a media item.
    ///
    /// Searches across all registered providers and returns sources ranked by quality.
    /// Results are cached to avoid repeated API calls.
    ///
    /// # Errors
    /// - `DiscoveryError::NoSourcesFound` - No sources found for the media
    /// - `DiscoveryError::ProviderFailed` - Provider communication failed
    /// - `DiscoveryError::SourceValidation` - Source data validation failed
    pub async fn discover_sources(
        &mut self,
        media: &Media,
    ) -> Result<Vec<ContentSource>, DiscoveryError> {
        info!("Discovering sources for media: {}", media.title());

        // Check cache first
        if let Some(cached_sources) = self.source_cache.get(&media.id()) {
            debug!("Using cached sources for media: {}", media.title());
            return Ok(cached_sources.clone());
        }

        let mut all_sources = Vec::new();

        // Search across all providers
        for (provider_name, provider) in &self.provider_clients {
            match provider.search_sources(media).await {
                Ok(mut sources) => {
                    debug!("Found {} sources from {}", sources.len(), provider_name);
                    all_sources.append(&mut sources);
                }
                Err(e) => {
                    warn!("Provider {} failed: {}", provider_name, e);
                    // Continue with other providers
                }
            }
        }

        if all_sources.is_empty() {
            return Err(DiscoveryError::NoSourcesFound {
                media_id: media.id(),
            });
        }

        // Validate and filter sources
        let mut valid_sources = Vec::new();
        for source in all_sources {
            match source.validate() {
                Ok(_) => valid_sources.push(source),
                Err(e) => {
                    warn!("Invalid source found: {}", e);
                    continue;
                }
            }
        }

        // Rank sources by quality
        self.rank_sources(&mut valid_sources);

        // Cache results
        self.source_cache.insert(media.id(), valid_sources.clone());

        info!(
            "Discovered {} valid sources for media: {}",
            valid_sources.len(),
            media.title()
        );

        Ok(valid_sources)
    }

    /// Finds the best source for streaming a media item.
    ///
    /// Returns the highest quality source that is suitable for streaming.
    ///
    /// # Errors
    /// - `DiscoveryError::NoSourcesFound` - No streamable sources available
    pub async fn find_best_streaming_source(
        &mut self,
        media: &Media,
    ) -> Result<ContentSource, DiscoveryError> {
        let sources = self.discover_sources(media).await?;

        // Find best streamable source
        for source in sources {
            if source.is_streamable() && source.is_healthy() {
                info!(
                    "Selected best streaming source: {} ({})",
                    source.name(),
                    source.quality()
                );
                return Ok(source);
            }
        }

        Err(DiscoveryError::NoSourcesFound {
            media_id: media.id(),
        })
    }

    /// Updates health metrics for known sources.
    pub async fn refresh_source_health(&mut self, media_id: MediaId) -> Result<(), DiscoveryError> {
        if let Some(sources) = self.source_cache.get_mut(&media_id) {
            for source in sources.iter_mut() {
                // Refresh health from providers
                for provider in self.provider_clients.values() {
                    if let Ok(health) = provider.get_source_health(source).await {
                        source.update_health(health.seeders(), health.leechers());
                        break;
                    }
                }
            }
        }
        Ok(())
    }

    /// Clears cached sources for a specific media item.
    pub fn clear_cache(&mut self, media_id: MediaId) {
        self.source_cache.remove(&media_id);
    }

    /// Returns the number of cached source entries.
    pub fn cache_size(&self) -> usize {
        self.source_cache.len()
    }

    /// Ranks sources by quality score (highest first).
    fn rank_sources(&self, sources: &mut [ContentSource]) {
        sources.sort_by(|a, b| {
            b.quality_score()
                .partial_cmp(&a.quality_score())
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }
}

impl Default for ContentDiscovery {
    fn default() -> Self {
        Self::new()
    }
}

/// Trait for content source providers (torrent sites, direct links, etc.).
#[async_trait::async_trait]
pub trait SourceProvider: Send + Sync {
    /// Searches for sources of a specific media item.
    async fn search_sources(&self, media: &Media) -> Result<Vec<ContentSource>, ProviderError>;

    /// Gets current health metrics for a source.
    async fn get_source_health(
        &self,
        source: &ContentSource,
    ) -> Result<SourceHealth, ProviderError>;

    /// Returns the provider's name.
    fn provider_name(&self) -> &str;

    /// Checks if the provider is currently available.
    async fn is_available(&self) -> bool {
        true
    }
}

/// Errors that can occur in source providers.
#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("Network request failed: {reason}")]
    NetworkFailed { reason: String },

    #[error("Authentication failed")]
    AuthenticationFailed,

    #[error("Rate limited by provider")]
    RateLimited,

    #[error("Parsing response failed: {reason}")]
    ParseFailed { reason: String },

    #[error("Provider temporarily unavailable")]
    Unavailable,
}

/// Mock provider for testing and development.
pub struct MockProvider {
    name: String,
    mock_sources: Vec<ContentSource>,
}

impl MockProvider {
    pub fn new(name: String) -> Self {
        Self {
            name,
            mock_sources: Vec::new(),
        }
    }

    pub fn add_mock_source(&mut self, source: ContentSource) {
        self.mock_sources.push(source);
    }
}

#[async_trait::async_trait]
impl SourceProvider for MockProvider {
    async fn search_sources(&self, media: &Media) -> Result<Vec<ContentSource>, ProviderError> {
        // Return mock sources that match the media
        let matching_sources: Vec<ContentSource> = self
            .mock_sources
            .iter()
            .filter(|source| source.media_id() == media.id())
            .cloned()
            .collect();

        Ok(matching_sources)
    }

    async fn get_source_health(
        &self,
        _source: &ContentSource,
    ) -> Result<SourceHealth, ProviderError> {
        // Return mock health data
        Ok(SourceHealth::from_swarm_stats(50, 10))
    }

    fn provider_name(&self) -> &str {
        &self.name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::content::{ContentSource, SourceHealth, SourceType};
    use crate::domain::media::{Media, MediaType, VideoQuality};

    #[tokio::test]
    async fn test_content_discovery_creation() {
        let discovery = ContentDiscovery::new();
        assert_eq!(discovery.cache_size(), 0);
    }

    #[tokio::test]
    async fn test_source_discovery_with_mock_provider() {
        let mut discovery = ContentDiscovery::new();
        let media = Media::new("Test Movie".to_string(), MediaType::Movie);

        // Create mock provider with test source
        let mut mock_provider = MockProvider::new("TestProvider".to_string());
        let test_source = ContentSource::new(
            media.id(),
            SourceType::Torrent,
            "Test.Movie.2024.1080p".to_string(),
            1_500_000_000,
            VideoQuality::BluRay1080p,
            SourceHealth::from_swarm_stats(100, 20),
            "magnet:?xt=urn:btih:test".to_string(),
        )
        .unwrap();
        mock_provider.add_mock_source(test_source);

        discovery.register_provider("test".to_string(), Box::new(mock_provider));

        // Discover sources
        let sources = discovery.discover_sources(&media).await.unwrap();
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].name(), "Test.Movie.2024.1080p");
    }

    #[tokio::test]
    async fn test_best_streaming_source_selection() {
        let mut discovery = ContentDiscovery::new();
        let media = Media::new("Test Movie".to_string(), MediaType::Movie);

        let mut mock_provider = MockProvider::new("TestProvider".to_string());

        // Add low quality source
        let low_quality_source = ContentSource::new(
            media.id(),
            SourceType::Torrent,
            "Test.Movie.CAM".to_string(),
            500_000_000,
            VideoQuality::CamRip,
            SourceHealth::from_swarm_stats(10, 5),
            "magnet:?xt=urn:btih:cam".to_string(),
        )
        .unwrap();
        mock_provider.add_mock_source(low_quality_source);

        // Add high quality source
        let high_quality_source = ContentSource::new(
            media.id(),
            SourceType::Torrent,
            "Test.Movie.1080p".to_string(),
            4_000_000_000,
            VideoQuality::BluRay1080p,
            SourceHealth::from_swarm_stats(100, 20),
            "magnet:?xt=urn:btih:1080p".to_string(),
        )
        .unwrap();
        mock_provider.add_mock_source(high_quality_source);

        discovery.register_provider("test".to_string(), Box::new(mock_provider));

        // Should select the high quality streamable source
        let best_source = discovery.find_best_streaming_source(&media).await.unwrap();
        assert_eq!(best_source.quality(), VideoQuality::BluRay1080p);
        assert!(best_source.is_streamable());
    }

    #[tokio::test]
    async fn test_source_ranking() {
        let mut discovery = ContentDiscovery::new();
        let media = Media::new("Test Movie".to_string(), MediaType::Movie);

        let mut mock_provider = MockProvider::new("TestProvider".to_string());

        // Add sources in random quality order
        let sources = vec![
            (VideoQuality::DVD, "DVD"),
            (VideoQuality::BluRay4K, "4K"),
            (VideoQuality::BluRay1080p, "1080p"),
            (VideoQuality::BluRay720p, "720p"),
        ];

        for (quality, name) in sources {
            let source = ContentSource::new(
                media.id(),
                SourceType::Torrent,
                format!("Test.Movie.{}", name),
                2_000_000_000,
                quality,
                SourceHealth::from_swarm_stats(50, 10),
                format!("magnet:?xt=urn:btih:{}", name.to_lowercase()),
            )
            .unwrap();
            mock_provider.add_mock_source(source);
        }

        discovery.register_provider("test".to_string(), Box::new(mock_provider));

        // Discover sources - should be ranked by quality
        let discovered_sources = discovery.discover_sources(&media).await.unwrap();
        assert_eq!(discovered_sources.len(), 4);

        // First source should be highest quality (4K)
        assert_eq!(discovered_sources[0].quality(), VideoQuality::BluRay4K);
        // Last source should be lowest quality (DVD)
        assert_eq!(discovered_sources[3].quality(), VideoQuality::DVD);
    }

    #[tokio::test]
    async fn test_cache_functionality() {
        let mut discovery = ContentDiscovery::new();
        let media = Media::new("Test Movie".to_string(), MediaType::Movie);

        let mut mock_provider = MockProvider::new("TestProvider".to_string());
        let test_source = ContentSource::new(
            media.id(),
            SourceType::Torrent,
            "Test.Movie.1080p".to_string(),
            2_000_000_000,
            VideoQuality::BluRay1080p,
            SourceHealth::from_swarm_stats(50, 10),
            "magnet:?xt=urn:btih:test".to_string(),
        )
        .unwrap();
        mock_provider.add_mock_source(test_source);

        discovery.register_provider("test".to_string(), Box::new(mock_provider));

        // First discovery should populate cache
        let _sources = discovery.discover_sources(&media).await.unwrap();
        assert_eq!(discovery.cache_size(), 1);

        // Clear cache
        discovery.clear_cache(media.id());
        assert_eq!(discovery.cache_size(), 0);
    }

    #[tokio::test]
    async fn test_no_sources_found_error() {
        let mut discovery = ContentDiscovery::new();
        let media = Media::new("Nonexistent Movie".to_string(), MediaType::Movie);

        // Register empty mock provider
        let mock_provider = MockProvider::new("EmptyProvider".to_string());
        discovery.register_provider("empty".to_string(), Box::new(mock_provider));

        // Should return error when no sources found
        let result = discovery.discover_sources(&media).await;
        assert!(matches!(result, Err(DiscoveryError::NoSourcesFound { .. })));
    }
}
