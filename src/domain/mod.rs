//! Core domain models and business logic.
//!
//! Contains the fundamental entities and value objects that represent
//! the core concepts of the media streaming domain.

pub mod content;
pub mod errors;
pub mod library;
pub mod media;
pub mod streaming;

// Re-export core domain types
pub use content::{ContentId, ContentSource, SourceHealth, SourceType};
pub use errors::DomainError;
pub use library::{DownloadStatus, LibraryContent, LibraryId};
pub use media::{Media, MediaId, MediaType, VideoQuality};
pub use streaming::{StreamId, StreamSession, StreamStatus};
