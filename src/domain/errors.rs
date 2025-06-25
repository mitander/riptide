//! Domain-specific errors for business rule violations.

/// Top-level domain error encompassing all domain validation failures.
#[derive(Debug, thiserror::Error)]
pub enum DomainError {
    #[error("Media validation failed")]
    MediaValidation(#[from] crate::domain::media::MediaValidationError),

    #[error("Content validation failed")]
    ContentValidation(#[from] crate::domain::content::ContentValidationError),

    #[error("Library validation failed")]
    LibraryValidation(#[from] crate::domain::library::LibraryValidationError),

    #[error("Stream validation failed")]
    StreamValidation(#[from] crate::domain::streaming::StreamValidationError),
}

impl DomainError {
    /// Checks if this error is due to invalid input data.
    pub fn is_validation_error(&self) -> bool {
        matches!(
            self,
            DomainError::MediaValidation(_)
                | DomainError::ContentValidation(_)
                | DomainError::LibraryValidation(_)
                | DomainError::StreamValidation(_)
        )
    }

    /// Returns a user-friendly error message.
    pub fn user_message(&self) -> String {
        match self {
            DomainError::MediaValidation(e) => match e {
                crate::domain::media::MediaValidationError::EmptyTitle => {
                    "Movie or show title cannot be empty".to_string()
                }
                crate::domain::media::MediaValidationError::TitleTooLong { .. } => {
                    "Title is too long (maximum 200 characters)".to_string()
                }
                crate::domain::media::MediaValidationError::InvalidReleaseYear { year } => {
                    format!("Invalid release year: {year}")
                }
                crate::domain::media::MediaValidationError::InvalidRating { rating } => {
                    format!("Invalid rating: {rating} (must be between 0.0 and 10.0)")
                }
            },
            DomainError::ContentValidation(e) => match e {
                crate::domain::content::ContentValidationError::EmptyName => {
                    "Content name cannot be empty".to_string()
                }
                crate::domain::content::ContentValidationError::ZeroSize => {
                    "Content size must be greater than zero".to_string()
                }
                crate::domain::content::ContentValidationError::EmptySourceUrl => {
                    "Content source URL cannot be empty".to_string()
                }
                crate::domain::content::ContentValidationError::InvalidTorrentUrl => {
                    "Invalid torrent URL (must be magnet link or .torrent file)".to_string()
                }
                crate::domain::content::ContentValidationError::InvalidDirectUrl => {
                    "Invalid direct URL (must start with http:// or https://)".to_string()
                }
            },
            DomainError::LibraryValidation(e) => match e {
                crate::domain::library::LibraryValidationError::EmptyFilePath => {
                    "File path cannot be empty".to_string()
                }
                crate::domain::library::LibraryValidationError::ZeroFileSize => {
                    "File size must be greater than zero".to_string()
                }
                crate::domain::library::LibraryValidationError::InvalidProgress { progress } => {
                    format!("Invalid progress: {progress}% (must be between 0% and 100%)")
                }
            },
            DomainError::StreamValidation(e) => match e {
                crate::domain::streaming::StreamValidationError::EmptyUserAgent => {
                    "Client information is incomplete".to_string()
                }
                crate::domain::streaming::StreamValidationError::BitrateThresholdTooLow {
                    ..
                } => "Quality settings are too low for streaming".to_string(),
                crate::domain::streaming::StreamValidationError::SeekBeyondDuration { .. } => {
                    "Cannot seek beyond the end of the content".to_string()
                }
            },
        }
    }
}
