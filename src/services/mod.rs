//! Application service layer implementing use cases.
//!
//! Contains deep modules that hide complex implementation details behind
//! simple interfaces. Each service coordinates domain entities and
//! infrastructure concerns to implement business use cases.

pub mod content_discovery;
pub mod content_manager;
pub mod media_catalog;
pub mod streaming_coordinator;

// Re-export main service types
pub use content_discovery::{ContentDiscovery, DiscoveryError, SourceProvider};
pub use content_manager::{ContentManager, ContentManagerError, DownloadClient, StorageService};
pub use media_catalog::{CatalogError, MediaCatalog};
pub use streaming_coordinator::{StreamingCoordinator, StreamingError, StreamingService};
