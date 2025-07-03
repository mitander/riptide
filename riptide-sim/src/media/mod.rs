//! Media-aware simulation for testing streaming with real movie files
//!
//! Enables testing streaming algorithms against real media content including
//! movies, subtitles, and metadata to catch real-world streaming issues.

mod analysis;
mod results;
mod simulation;
mod types;

pub use analysis::MediaAnalyzer;
pub use results::StreamingResult;
pub use simulation::MediaStreamingSimulation;
pub use types::{
    MediaFile, MediaFileType, MovieFolder, StreamingBuffer, StreamingPriority, StreamingProfile,
};

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tempfile::{TempDir, tempdir};
    use tokio::fs::{self, File};
    use tokio::io::AsyncWriteExt;

    use super::*;

    async fn create_test_movie_folder() -> (std::path::PathBuf, TempDir) {
        let temp_dir = tempdir().unwrap();
        let movie_dir = temp_dir.path().join("Test.Movie.2024");
        fs::create_dir(&movie_dir).await.unwrap();

        // Create test video file
        let mut video_file = File::create(movie_dir.join("Test.Movie.2024.1080p.mp4"))
            .await
            .unwrap();
        video_file.write_all(&vec![0; 1_000_000]).await.unwrap(); // 1MB test file

        // Create subtitle files
        let mut sub_en = File::create(movie_dir.join("Test.Movie.2024.en.srt"))
            .await
            .unwrap();
        sub_en
            .write_all(b"1\n00:00:01,000 --> 00:00:03,000\nTest subtitle\n")
            .await
            .unwrap();

        let mut sub_es = File::create(movie_dir.join("Test.Movie.2024.es.srt"))
            .await
            .unwrap();
        sub_es
            .write_all(b"1\n00:00:01,000 --> 00:00:03,000\nSubtitulo de prueba\n")
            .await
            .unwrap();

        // Create metadata file
        let mut nfo_file = File::create(movie_dir.join("Test.Movie.2024.nfo"))
            .await
            .unwrap();
        nfo_file
            .write_all(b"<movie><title>Test Movie</title></movie>")
            .await
            .unwrap();

        (movie_dir.to_path_buf(), temp_dir)
    }

    #[tokio::test]
    async fn test_movie_folder_analysis() {
        let (movie_dir, _temp_dir) = create_test_movie_folder().await;
        let movie_folder = MediaAnalyzer::analyze_movie_folder(&movie_dir)
            .await
            .unwrap();

        assert_eq!(movie_folder.name, "Test.Movie.2024");
        assert!(movie_folder.files.len() >= 3); // Video, 2 subs, metadata
        assert!(movie_folder.primary_video.is_some());
        assert_eq!(movie_folder.subtitle_files.len(), 2);
    }

    #[tokio::test]
    async fn test_media_file_classification() {
        let (movie_dir, _temp_dir) = create_test_movie_folder().await;
        let video_path = movie_dir.join("Test.Movie.2024.1080p.mp4");

        let media_file = analysis::MediaAnalyzer::classify_media_file(&video_path, 1_000_000)
            .await
            .unwrap()
            .unwrap();

        assert!(matches!(media_file.file_type, MediaFileType::Video { .. }));
        assert_eq!(media_file.priority, StreamingPriority::Critical);
    }

    #[tokio::test]
    async fn test_streaming_simulation() {
        let (movie_dir, _temp_dir) = create_test_movie_folder().await;
        let mut simulation = MediaStreamingSimulation::from_movie_folder(&movie_dir, 42, 262144)
            .await
            .unwrap();

        simulation.start_streaming_simulation();
        let result = simulation
            .execute_streaming_simulation(Duration::from_secs(30))
            .expect("Streaming simulation should complete successfully");

        assert!(result.total_events > 0);
        assert!(result.piece_requests > 0);
    }
}
