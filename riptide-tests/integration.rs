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

#[path = "integration/debug_remux_test.rs"]
mod debug_remux_test;
#[path = "integration/development_mode_debug.rs"]
mod development_mode_debug;
#[path = "integration/ffmpeg_validation_test.rs"]
mod ffmpeg_validation_test;
#[path = "integration/progressive_remuxing_test.rs"]
mod progressive_remuxing_test;
#[path = "integration/smart_header_footer_test.rs"]
mod smart_header_footer_test;

#[path = "integration/peer_communication.rs"]
mod peer_communication;
// TODO: Fix API compatibility after streaming redesign
// #[path = "integration/progressive_streaming_broken_test.rs"]
// mod progressive_streaming_broken_test;

#[path = "integration/torrent_integration.rs"]
mod torrent_integration;
#[path = "integration/torrent_protocol.rs"]
mod torrent_protocol;
#[path = "integration/upload_rate_limiting.rs"]
mod upload_rate_limiting;

// TODO: Fix API compatibility after streaming redesign
// #[path = "integration/range_validation.rs"]
// mod range_validation;
