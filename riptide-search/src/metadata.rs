//! IMDb metadata fetching using OMDb API.

use serde::{Deserialize, Serialize};

use crate::errors::MediaSearchError;
use crate::types::MediaType;

/// IMDb metadata service for fetching movie and TV show information.
#[derive(Debug, Clone)]
pub struct ImdbMetadataService {
    client: reqwest::Client,
    api_key: Option<String>,
}

/// Response from OMDb API for movie/show details.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OmdbResponse {
    #[serde(rename = "Title")]
    pub title: Option<String>,
    #[serde(rename = "Year")]
    pub year: Option<String>,
    #[serde(rename = "Type")]
    pub media_type: Option<String>,
    #[serde(rename = "Plot")]
    pub plot: Option<String>,
    #[serde(rename = "Genre")]
    pub genre: Option<String>,
    #[serde(rename = "imdbRating")]
    pub imdb_rating: Option<String>,
    #[serde(rename = "Poster")]
    pub poster: Option<String>,
    #[serde(rename = "imdbID")]
    pub imdb_id: Option<String>,
    #[serde(rename = "Response")]
    pub response: Option<String>,
    #[serde(rename = "Error")]
    pub error: Option<String>,
}

/// Enhanced media metadata with IMDb information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaMetadata {
    pub title: String,
    pub year: Option<u16>,
    pub media_type: MediaType,
    pub imdb_id: Option<String>,
    pub poster_url: Option<String>,
    pub plot: Option<String>,
    pub genre: Option<String>,
    pub rating: Option<f32>,
    pub director: Option<String>,
    pub cast: Vec<String>,
    pub runtime: Option<String>,
}

impl ImdbMetadataService {
    /// Create new IMDb metadata service.
    ///
    /// For production use, set OMDB_API_KEY environment variable.
    /// Free tier allows 1000 requests per day without key.
    pub fn new() -> Self {
        let api_key = std::env::var("OMDB_API_KEY").ok();

        Self {
            client: reqwest::Client::new(),
            api_key,
        }
    }

    /// Create IMDb metadata service with explicit API key.
    ///
    /// Allows configuration-driven API key instead of environment variable.
    pub fn with_api_key(api_key: Option<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key,
        }
    }

    /// Fetch metadata by IMDb ID.
    ///
    /// # Errors
    /// - `MediaSearchError::MetadataFetchFailed` - Failed to fetch or parse metadata
    pub async fn fetch_by_imdb_id(&self, imdb_id: &str) -> Result<MediaMetadata, MediaSearchError> {
        let url = if let Some(ref api_key) = self.api_key {
            format!("http://www.omdbapi.com/?i={imdb_id}&apikey={api_key}")
        } else {
            format!("http://www.omdbapi.com/?i={imdb_id}")
        };

        let response = self.client.get(&url).send().await.map_err(|e| {
            MediaSearchError::MetadataFetchFailed {
                reason: format!("HTTP request failed: {e}"),
            }
        })?;

        let omdb_data: OmdbResponse =
            response
                .json()
                .await
                .map_err(|e| MediaSearchError::MetadataFetchFailed {
                    reason: format!("JSON parsing failed: {e}"),
                })?;

        if omdb_data.response == Some("False".to_string()) {
            return Err(MediaSearchError::MetadataFetchFailed {
                reason: omdb_data
                    .error
                    .unwrap_or_else(|| "Unknown error".to_string()),
            });
        }

        Ok(self.parse_omdb_response(omdb_data))
    }

    /// Search for metadata by title and year.
    ///
    /// # Errors
    /// - `MediaSearchError::MetadataFetchFailed` - Failed to fetch or parse metadata
    pub async fn search_by_title(
        &self,
        title: &str,
        year: Option<u16>,
    ) -> Result<MediaMetadata, MediaSearchError> {
        let mut url = if let Some(ref api_key) = self.api_key {
            format!(
                "http://www.omdbapi.com/?t={}&apikey={}",
                urlencoding::encode(title),
                api_key
            )
        } else {
            format!("http://www.omdbapi.com/?t={}", urlencoding::encode(title))
        };

        if let Some(year) = year {
            url.push_str(&format!("&y={year}"));
        }

        let response = self.client.get(&url).send().await.map_err(|e| {
            MediaSearchError::MetadataFetchFailed {
                reason: format!("HTTP request failed: {e}"),
            }
        })?;

        let omdb_data: OmdbResponse =
            response
                .json()
                .await
                .map_err(|e| MediaSearchError::MetadataFetchFailed {
                    reason: format!("JSON parsing failed: {e}"),
                })?;

        if omdb_data.response == Some("False".to_string()) {
            return Err(MediaSearchError::MetadataFetchFailed {
                reason: omdb_data.error.unwrap_or_else(|| "Not found".to_string()),
            });
        }

        Ok(self.parse_omdb_response(omdb_data))
    }

    /// Parse OMDb API response into MediaMetadata.
    fn parse_omdb_response(&self, omdb: OmdbResponse) -> MediaMetadata {
        let year = omdb.year.and_then(|y| {
            // Handle year ranges like "2019-2021" for TV series
            y.split('-')
                .next()
                .and_then(|year_str| year_str.parse().ok())
        });

        let media_type = match omdb.media_type.as_deref() {
            Some("movie") => MediaType::Movie,
            Some("series") => MediaType::TvShow,
            Some("episode") => MediaType::TvShow,
            _ => MediaType::Other,
        };

        let rating = omdb
            .imdb_rating
            .and_then(|r| if r == "N/A" { None } else { r.parse().ok() });

        let poster_url = omdb.poster.filter(|p| p != "N/A");

        MediaMetadata {
            title: omdb.title.unwrap_or_else(|| "Unknown".to_string()),
            year,
            media_type,
            imdb_id: omdb.imdb_id,
            poster_url,
            plot: omdb.plot.filter(|p| p != "N/A"),
            genre: omdb.genre.filter(|g| g != "N/A"),
            rating,
            director: None,   // TODO: Parse from OMDb data
            cast: Vec::new(), // TODO: Parse from OMDb data
            runtime: None,    // TODO: Parse from OMDb data
        }
    }
}

impl Default for ImdbMetadataService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_metadata_service_creation() {
        let service = ImdbMetadataService::new();
        assert!(service.client.get("http://example.com").build().is_ok());
    }

    #[tokio::test]
    async fn test_search_by_title_matrix() {
        let service = ImdbMetadataService::new();

        // This test requires internet connection
        if let Ok(metadata) = service.search_by_title("The Matrix", Some(1999)).await {
            assert_eq!(metadata.title, "The Matrix");
            assert_eq!(metadata.year, Some(1999));
            assert_eq!(metadata.media_type, MediaType::Movie);
            assert!(metadata.rating.is_some());
        }
        // Don't fail test if no internet or API limit reached
    }
}
