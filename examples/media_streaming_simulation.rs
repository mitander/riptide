//! Media-aware streaming simulation example
//!
//! Demonstrates testing BitTorrent streaming with real movie folders containing
//! videos, subtitles, and metadata to identify real-world streaming issues.

use std::path::PathBuf;
use std::time::Duration;

use riptide::simulation::MediaStreamingSimulation;
use tokio::io::AsyncWriteExt;

/// Simulates streaming a real movie folder through BitTorrent.
///
/// This example shows how to test streaming algorithms against actual
/// media files to catch subtitle sync issues, buffering problems, and
/// piece prioritization bugs.
async fn simulate_movie_streaming(movie_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!("Media Streaming Simulation: {movie_path}");
    println!("{:-<60}", "");

    let movie_folder = PathBuf::from(movie_path);

    // Verify folder exists
    if !movie_folder.exists() {
        println!("Movie folder not found: {movie_path}");
        println!(
            "Create test folder with: mkdir -p '{movie_path}' && touch '{movie_path}/test.mp4' '{movie_path}/test.en.srt'"
        );
        return Ok(());
    }

    // Create media streaming simulation with deterministic seed
    let seed = 0x1337CAFE;
    let piece_size = 262144; // 256KB pieces

    println!("Creating simulation with seed: 0x{:X}", seed);
    println!("Piece size: {} KB", piece_size / 1024);

    let mut simulation =
        MediaStreamingSimulation::from_movie_folder(&movie_folder, seed, piece_size).await?;

    // Display movie folder analysis
    let folder_info = simulation.movie_folder();
    println!("\nMovie Folder Analysis:");
    println!("  Name: {}", folder_info.name);
    println!("  Total files: {}", folder_info.files.len());
    println!(
        "  Total size: {:.1} GB",
        folder_info.total_size as f64 / 1_073_741_824.0
    );

    if let Some(video_index) = folder_info.primary_video {
        let video = &folder_info.files[video_index];
        println!(
            "  Primary video: {} ({:.1} MB)",
            video.path.file_name().unwrap().to_string_lossy(),
            video.size as f64 / 1_048_576.0
        );

        if let riptide::simulation::media::MediaFileType::Video {
            bitrate, duration, ..
        } = &video.file_type
        {
            println!("    Bitrate: {:.1} Mbps", *bitrate as f64 / 1_000_000.0);
            println!("    Duration: {:?}", duration);
        }
    }

    println!("  Subtitle files: {}", folder_info.subtitle_files.len());
    for &sub_index in &folder_info.subtitle_files {
        let sub = &folder_info.files[sub_index];
        if let riptide::simulation::media::MediaFileType::Subtitle { language, .. } = &sub.file_type
        {
            println!(
                "    {}: {} ({} KB)",
                language,
                sub.path.file_name().unwrap().to_string_lossy(),
                sub.size / 1024
            );
        }
    }

    // Start streaming simulation
    println!("\nStarting streaming simulation...");
    simulation.start_streaming_simulation();

    // Run simulation for 5 minutes of streaming
    let result = simulation.run_streaming_simulation(Duration::from_secs(300));

    // Print detailed analysis
    println!("\nStreaming Results:");
    result.print_analysis(simulation.movie_folder());

    // Specific streaming insights
    println!("\nStreaming Insights:");
    if result.streaming_efficiency < 0.9 {
        println!(
            "  Warning: Low streaming efficiency ({:.1}%)",
            result.streaming_efficiency * 100.0
        );
        println!("  Recommendation: Improve piece prioritization or add more peers");
    }

    if result.video_pieces_completed == 0 {
        println!("  Error: No video pieces completed - streaming would fail");
    } else {
        println!(
            "  Video streaming: {:.1}% success rate",
            result.video_success_rate() * 100.0
        );
    }

    if result.subtitle_pieces_completed > 0 {
        println!(
            "  Subtitle availability: {:.1}%",
            result.subtitle_availability() * 100.0
        );
        if result.subtitle_sync_issues > 0 {
            println!(
                "  Subtitle sync issues detected: {}",
                result.subtitle_sync_issues
            );
        }
    }

    if result.buffering_events > 10 {
        println!(
            "  High buffering activity: {} events",
            result.buffering_events
        );
        println!("  Consider increasing buffer size or improving piece selection");
    }

    Ok(())
}

/// Demonstrates creating a test movie folder structure.
async fn create_test_movie_folder() -> Result<(), Box<dyn std::error::Error>> {
    use tokio::fs;
    use tokio::io::AsyncWriteExt;

    let test_dir = "test_movies/Big.Buck.Bunny.2008";

    println!("Creating test movie folder: {}", test_dir);
    fs::create_dir_all(test_dir).await?;

    // Create test video file (empty but realistic size)
    let video_path = format!("{}/Big.Buck.Bunny.2008.1080p.h264.mp4", test_dir);
    let video_file = fs::File::create(&video_path).await?;
    // Simulate 1GB movie file
    video_file.set_len(1_073_741_824).await?;

    // Create English subtitles
    let en_sub_path = format!("{}/Big.Buck.Bunny.2008.en.srt", test_dir);
    let mut en_sub = fs::File::create(&en_sub_path).await?;
    en_sub.write_all(b"1\n00:00:01,000 --> 00:00:05,000\nBig Buck Bunny\n\n2\n00:00:06,000 --> 00:00:10,000\nA short film by Blender Foundation\n").await?;

    // Create Spanish subtitles
    let es_sub_path = format!("{}/Big.Buck.Bunny.2008.es.srt", test_dir);
    let mut es_sub = fs::File::create(&es_sub_path).await?;
    es_sub.write_all(b"1\n00:00:01,000 --> 00:00:05,000\nGran Conejo Buck\n\n2\n00:00:06,000 --> 00:00:10,000\nUn cortometraje de Blender Foundation\n").await?;

    // Create metadata file
    let nfo_path = format!("{}/Big.Buck.Bunny.2008.nfo", test_dir);
    let mut nfo_file = fs::File::create(&nfo_path).await?;
    nfo_file.write_all(b"<movie>\n  <title>Big Buck Bunny</title>\n  <year>2008</year>\n  <plot>A large rabbit deals with three small bullies.</plot>\n</movie>").await?;

    // Create poster image
    let poster_path = format!("{}/poster.jpg", test_dir);
    let mut poster = fs::File::create(&poster_path).await?;
    poster.write_all(b"JPEG_PLACEHOLDER_DATA").await?;

    println!("Test movie folder created with:");
    println!("  Video: Big.Buck.Bunny.2008.1080p.h264.mp4 (1GB)");
    println!("  Subtitles: English, Spanish");
    println!("  Metadata: NFO file");
    println!("  Poster: poster.jpg");

    Ok(())
}

/// Compares streaming performance across different movie types.
async fn compare_movie_types() -> Result<(), Box<dyn std::error::Error>> {
    println!("\nComparing Streaming Performance Across Movie Types");
    println!("{:-<60}", "");

    let test_scenarios = [
        ("Small movie (500MB)", 500_000_000u64),
        ("Standard movie (2GB)", 2_000_000_000u64),
        ("Large movie (8GB)", 8_000_000_000u64),
    ];

    for (name, size) in &test_scenarios {
        println!("\nTesting {}", name);

        // Create temporary test folder
        let temp_dir = format!("temp_test_{}", size);
        tokio::fs::create_dir_all(&temp_dir).await?;

        // Create test video file
        let video_path = format!("{}/test_movie.mp4", temp_dir);
        let video_file = tokio::fs::File::create(&video_path).await?;
        video_file.set_len(*size).await?;

        // Create basic subtitle
        let sub_path = format!("{}/test_movie.en.srt", temp_dir);
        let mut sub_file = tokio::fs::File::create(&sub_path).await?;
        sub_file
            .write_all(b"1\n00:00:01,000 --> 00:00:03,000\nTest subtitle\n")
            .await?;

        // Run simulation
        let mut simulation =
            MediaStreamingSimulation::from_movie_folder(&PathBuf::from(&temp_dir), 0x12345, 262144)
                .await?;

        simulation.start_streaming_simulation();
        let result = simulation.run_streaming_simulation(Duration::from_secs(120));

        println!(
            "  Streaming efficiency: {:.1}%",
            result.streaming_efficiency * 100.0
        );
        println!(
            "  Video pieces completed: {}",
            result.video_pieces_completed
        );
        println!("  Buffering events: {}", result.buffering_events);

        // Clean up
        tokio::fs::remove_dir_all(&temp_dir).await?;
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Media-Aware BitTorrent Streaming Simulation");
    println!("==========================================\n");

    // Example 1: Create test movie folder if needed
    if let Err(_e) = simulate_movie_streaming("test_movies/Big.Buck.Bunny.2008").await {
        println!("Creating test movie folder...");
        create_test_movie_folder().await?;
        println!();

        // Try simulation again with test folder
        simulate_movie_streaming("test_movies/Big.Buck.Bunny.2008").await?;
    }

    // Example 2: Compare different movie sizes
    compare_movie_types().await?;

    println!("{}", "\n".to_string() + &"=".repeat(60));
    println!("Media Streaming Simulation Complete");
    println!("\nThis simulation enables:");
    println!("• Testing with real movie folder structures");
    println!("• Identifying subtitle synchronization issues");
    println!("• Validating piece prioritization for streaming");
    println!("• Debugging buffering and startup delays");
    println!("• Performance testing with various content sizes");
    println!("\nNext steps:");
    println!("• Integrate with magneto project for magnet link testing");
    println!("• Add mock tracker interaction simulation");
    println!("• Implement realistic network condition simulation");

    Ok(())
}
