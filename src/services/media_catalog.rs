//! Media catalog service for search and metadata enrichment.

use std::collections::HashMap;

use async_trait::async_trait;

use crate::domain::{Media, MediaType};
use crate::media_search::MediaSearchService;

/// Deep module for media catalog operations.
///
/// Hides complexity of search, ranking, metadata enrichment, and caching
/// behind a simple interface. Coordinates between domain entities and
/// external search providers.
#[derive(Clone)]
pub struct MediaCatalog {
    search_service: MediaSearchService,
    // Future: IMDb client, metadata cache, search index
}

impl MediaCatalog {
    /// Creates new media catalog with default search provider.
    pub fn new(search_service: MediaSearchService) -> Self {
        Self { search_service }
    }

    /// Searches for media by query string.
    ///
    /// Handles search ranking, deduplication, and basic metadata enrichment.
    /// Returns results sorted by relevance and quality.
    ///
    /// # Errors
    /// - `CatalogError::SearchFailed` - External search provider failed
    /// - `CatalogError::InvalidQuery` - Query string is invalid
    pub async fn search(&self, query: &str) -> Result<Vec<Media>, CatalogError> {
        // Validate query
        if query.trim().is_empty() {
            return Err(CatalogError::InvalidQuery {
                reason: "Query cannot be empty".to_string(),
            });
        }

        if query.len() > 200 {
            return Err(CatalogError::InvalidQuery {
                reason: "Query too long (maximum 200 characters)".to_string(),
            });
        }

        // Search using external provider
        let search_results = self.search_service.search_all(query).await.map_err(|e| {
            CatalogError::SearchFailed {
                query: query.to_string(),
                reason: e.to_string(),
            }
        })?;

        // Convert to domain entities
        let mut media_list = Vec::new();
        for result in search_results {
            let media_type = match result.media_type {
                crate::media_search::MediaType::Movie => MediaType::Movie,
                crate::media_search::MediaType::TvShow => MediaType::TvShow,
                crate::media_search::MediaType::Music => MediaType::Documentary, // Map music to documentary for now
                crate::media_search::MediaType::Other => MediaType::Documentary,
            };

            match Media::with_metadata(
                result.title,
                media_type,
                result.year,
                result.imdb_id,
                result.plot,
                result.genre,
                result.rating,
                result.poster_url,
            ) {
                Ok(media) => media_list.push(media),
                Err(e) => {
                    // Log validation error but continue with other results
                    tracing::warn!("Failed to create media from search result: {}", e);
                }
            }
        }

        // Sort by search relevance
        self.rank_search_results(&mut media_list, query);

        Ok(media_list)
    }

    /// Searches specifically for movies.
    ///
    /// # Errors
    /// - `CatalogError::SearchFailed` - External search provider failed
    /// - `CatalogError::InvalidQuery` - Query string is invalid
    pub async fn search_movies(&self, query: &str) -> Result<Vec<Media>, CatalogError> {
        let all_results = self.search(query).await?;
        Ok(all_results
            .into_iter()
            .filter(|media| media.media_type() == MediaType::Movie)
            .collect())
    }

    /// Searches specifically for TV shows.
    ///
    /// # Errors
    /// - `CatalogError::SearchFailed` - External search provider failed
    /// - `CatalogError::InvalidQuery` - Query string is invalid
    pub async fn search_tv_shows(&self, query: &str) -> Result<Vec<Media>, CatalogError> {
        let all_results = self.search(query).await?;
        Ok(all_results
            .into_iter()
            .filter(|media| media.media_type() == MediaType::TvShow)
            .collect())
    }

    /// Enriches media with additional metadata from external sources.
    ///
    /// Currently placeholder for future IMDb integration.
    ///
    /// # Errors
    /// - `CatalogError::EnrichmentFailed` - Failed to fetch additional metadata
    pub async fn enrich_metadata(&self, media: &Media) -> Result<EnrichedMedia, CatalogError> {
        // Future: IMDb API integration, poster downloads, etc.
        Ok(EnrichedMedia {
            media: media.clone(),
            additional_metadata: HashMap::new(),
        })
    }

    /// Finds similar media based on genre, year, or other attributes.
    ///
    /// # Errors
    /// - `CatalogError::SearchFailed` - External search provider failed
    pub async fn find_similar(&self, media: &Media) -> Result<Vec<Media>, CatalogError> {
        // Build similarity search query
        let mut query_parts = Vec::new();

        if let Some(genre) = media.genre() {
            query_parts.push(genre.to_string());
        }

        if let Some(year) = media.release_year() {
            // Search for media from nearby years
            query_parts.push(format!("{year}"));
        }

        if query_parts.is_empty() {
            return Ok(Vec::new());
        }

        let query = query_parts.join(" ");
        let mut results = self.search(&query).await?;

        // Remove the original media from results
        results.retain(|result| result.id() != media.id());

        // Limit to reasonable number of suggestions
        results.truncate(10);

        Ok(results)
    }

    /// Ranks search results by relevance to the query.
    fn rank_search_results(&self, media_list: &mut [Media], query: &str) {
        media_list.sort_by(|a, b| {
            let relevance_a = a.search_relevance(query);
            let relevance_b = b.search_relevance(query);

            // Sort by relevance (descending)
            relevance_b
                .partial_cmp(&relevance_a)
                .unwrap_or(std::cmp::Ordering::Equal)
                // Then by release year (descending)
                .then_with(|| b.release_year().cmp(&a.release_year()))
                // Then by title (ascending)
                .then_with(|| a.title().cmp(b.title()))
        });
    }
}

/// Media with additional enriched metadata.
#[derive(Debug, Clone)]
pub struct EnrichedMedia {
    pub media: Media,
    pub additional_metadata: HashMap<String, String>,
}

impl EnrichedMedia {
    /// Returns the core media entity.
    pub fn media(&self) -> &Media {
        &self.media
    }

    /// Gets additional metadata by key.
    pub fn get_metadata(&self, key: &str) -> Option<&str> {
        self.additional_metadata.get(key).map(|s| s.as_str())
    }
}

/// Errors that can occur in media catalog operations.
#[derive(Debug, thiserror::Error)]
pub enum CatalogError {
    #[error("Search failed for query '{query}': {reason}")]
    SearchFailed { query: String, reason: String },

    #[error("Invalid search query: {reason}")]
    InvalidQuery { reason: String },

    #[error("Metadata enrichment failed for media '{title}': {reason}")]
    EnrichmentFailed { title: String, reason: String },

    #[error("External service error: {reason}")]
    ExternalService { reason: String },
}

/// Trait for metadata enrichment providers (future IMDb integration).
#[async_trait]
pub trait MetadataProvider: Send + Sync {
    /// Enriches media with additional metadata.
    async fn enrich_media(&self, media: &Media) -> Result<HashMap<String, String>, String>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media_search::MediaSearchService;

    #[tokio::test]
    async fn test_media_catalog_search() {
        let search_service = MediaSearchService::new_with_mock();
        let catalog = MediaCatalog::new(search_service);

        let results = catalog.search("test movie").await.unwrap();
        assert!(!results.is_empty());
        assert!(results.iter().any(|m| m.media_type() == MediaType::Movie));
    }

    #[tokio::test]
    async fn test_search_validation() {
        let search_service = MediaSearchService::new_with_mock();
        let catalog = MediaCatalog::new(search_service);

        // Empty query
        let result = catalog.search("").await;
        assert!(matches!(result, Err(CatalogError::InvalidQuery { .. })));

        // Too long query
        let long_query = "x".repeat(201);
        let result = catalog.search(&long_query).await;
        assert!(matches!(result, Err(CatalogError::InvalidQuery { .. })));
    }

    #[tokio::test]
    async fn test_media_type_filtering() {
        let search_service = MediaSearchService::new_with_mock();
        let catalog = MediaCatalog::new(search_service);

        let movie_results = catalog.search_movies("test").await.unwrap();
        assert!(
            movie_results
                .iter()
                .all(|m| m.media_type() == MediaType::Movie)
        );
    }

    #[tokio::test]
    async fn test_enrichment_placeholder() {
        let search_service = MediaSearchService::new_with_mock();
        let catalog = MediaCatalog::new(search_service);

        let media = Media::new("Test Movie".to_string(), MediaType::Movie);
        let enriched = catalog.enrich_metadata(&media).await.unwrap();

        assert_eq!(enriched.media().title(), "Test Movie");
        assert!(enriched.additional_metadata.is_empty()); // Placeholder implementation
    }

    #[tokio::test]
    async fn test_find_similar_empty_attributes() {
        let search_service = MediaSearchService::new_with_mock();
        let catalog = MediaCatalog::new(search_service);

        let media = Media::new("Test Movie".to_string(), MediaType::Movie);
        let similar = catalog.find_similar(&media).await.unwrap();

        // Should return empty since media has no genre or year
        assert!(similar.is_empty());
    }
}
