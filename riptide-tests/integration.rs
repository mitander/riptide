//! Integration tests for Riptide
//!
//! These tests verify the integration between different components of the system.
//! They test component interactions, data flow, and interface contracts.
//!
//! Integration tests are designed to test the interaction between modules and
//! ensure that the system works correctly as a whole.

// TODO: Re-enable tests one by one after fixing API imports

#[path = "style.rs"]
mod style;

#[path = "integration/avi_ffmpeg_duration_test.rs"]
mod avi_ffmpeg_duration_test;

#[path = "integration/dev_simulation.rs"]
mod dev_simulation;
#[path = "integration/engine_integration.rs"]
mod engine_integration;
#[path = "integration/mp4_validation.rs"]
mod mp4_validation;

#[path = "integration/peer_communication.rs"]
mod peer_communication;
#[path = "integration/progressive_streaming_test.rs"]
mod progressive_streaming_test;
#[path = "integration/remux_pipeline.rs"]
mod remux_pipeline;
#[path = "integration/sim_streaming_integration.rs"]
mod sim_streaming_integration;

#[path = "integration/streaming_integration.rs"]
mod streaming_integration;

#[path = "integration/torrent_integration.rs"]
mod torrent_integration;
#[path = "integration/torrent_protocol.rs"]
mod torrent_protocol;
#[path = "integration/upload_rate_limiting.rs"]
mod upload_rate_limiting;

#[path = "integration/range_validation.rs"]
mod range_validation;
