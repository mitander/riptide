//! Streaming-related simulation utilities and test data.

pub mod mock_piece_provider;
pub mod test_data;

pub use mock_piece_provider::MockPieceProvider;
pub use test_data::{
    create_realistic_video_data, create_sequential_pieces, create_simple_mock_data,
    split_into_pieces,
};
