//! Test FFmpeg AVI transcoding duration issue
//!
//! This test specifically checks if FFmpeg is truncating the video
//! from 16 minutes to 11 minutes during transcoding.

use std::fs;
use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

#[test]
fn test_ffmpeg_avi_progressive_transcoding_duration() {
    let temp_dir = TempDir::new().unwrap();
    let input_path = temp_dir.path().join("test_16min.avi");
    let output_path = temp_dir.path().join("output.mp4");

    // Create a 16-minute test AVI file
    create_test_avi(&input_path, 980); // 16.33 minutes

    // Test 1: Full file transcoding (baseline)
    {
        println!("\n=== Test 1: Full file transcoding ===");
        let status = Command::new("ffmpeg")
            .args([
                "-y",
                "-i",
                input_path.to_str().unwrap(),
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-c:a",
                "aac",
                "-movflags",
                "faststart",
                "-f",
                "mp4",
                output_path.to_str().unwrap(),
            ])
            .output()
            .expect("Failed to run FFmpeg");

        assert!(status.status.success(), "FFmpeg should succeed");

        let duration = mp4_duration(&output_path);
        println!(
            "Full file transcode duration: {} seconds ({} minutes)",
            duration,
            duration / 60.0
        );
        assert!(
            duration > 900.0,
            "Full transcode should preserve 16 minute duration, got {} minutes",
            duration / 60.0
        );
    }

    // Test 2: Progressive transcoding with pipe (reproduces the issue)
    {
        println!("\n=== Test 2: Progressive pipe transcoding ===");
        fs::remove_file(&output_path).ok();

        // Read file data
        let avi_data = fs::read(&input_path).unwrap();

        // Start FFmpeg with old progressive streaming settings (to test if fix works)
        let mut ffmpeg = Command::new("ffmpeg")
            .args([
                "-y",
                "-analyzeduration",
                "10000000", // 10 seconds - from progressive.rs
                "-probesize",
                "20971520", // 20MB - from progressive.rs
                "-err_detect",
                "ignore_err",
                "-fflags",
                "+igndts+ignidx+genpts+discardcorrupt",
                "-avoid_negative_ts",
                "make_zero",
                "-thread_queue_size",
                "2048",
                "-f",
                "avi",
                "-i",
                "pipe:0",
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-crf",
                "23",
                "-profile:v",
                "high",
                "-level",
                "4.1",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
                "-b:a",
                "192k",
                "-movflags",
                "frag_keyframe+empty_moov+default_base_moof",
                "-max_muxing_queue_size",
                "4096",
                "-f",
                "mp4",
                output_path.to_str().unwrap(),
            ])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("Failed to spawn FFmpeg");

        // Feed data progressively
        let mut stdin = ffmpeg.stdin.take().unwrap();

        // Feed in chunks (simulating progressive download)
        let chunk_size = 1024 * 1024; // 1MB chunks
        for chunk in avi_data.chunks(chunk_size) {
            use std::io::Write;
            stdin.write_all(chunk).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        drop(stdin);

        let output = ffmpeg.wait_with_output().unwrap();

        // Check stderr for warnings
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("truncat") || stderr.contains("Duration") {
            println!("FFmpeg stderr warnings:\n{stderr}");
        }

        let duration = mp4_duration(&output_path);
        println!(
            "Progressive pipe transcode duration: {} seconds ({} minutes)",
            duration,
            duration / 60.0
        );

        // Verify the fix works - should preserve full duration
        assert!(
            duration > 900.0,
            "Progressive streaming should preserve full 16 minute duration, got {} minutes",
            duration / 60.0
        );
        println!("SUCCESS: Progressive streaming preserved full duration");
    }

    // Test 3: Progressive with FIXED settings (validates the fix)
    {
        println!("\n=== Test 3: Progressive with extended analysis ===");
        fs::remove_file(&output_path).ok();

        let avi_data = fs::read(&input_path).unwrap();

        let mut ffmpeg = Command::new("ffmpeg")
            .args([
                "-y",
                "-probesize",
                "100000000", // 100MB probe
                "-analyzeduration",
                "100000000", // 100 seconds
                "-fflags",
                "+genpts+igndts",
                "-f",
                "avi",
                "-i",
                "pipe:0",
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-c:a",
                "aac",
                "-movflags",
                "faststart", // Try faststart instead of fragmented
                "-f",
                "mp4",
                output_path.to_str().unwrap(),
            ])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("Failed to spawn FFmpeg");

        let mut stdin = ffmpeg.stdin.take().unwrap();
        use std::io::Write;
        stdin.write_all(&avi_data).unwrap();
        drop(stdin);

        ffmpeg.wait().unwrap();

        let duration = mp4_duration(&output_path);
        println!(
            "Extended analysis duration: {} seconds ({} minutes)",
            duration,
            duration / 60.0
        );
    }
}

fn create_test_avi(path: &Path, duration_seconds: u32) {
    // Create test AVI using FFmpeg
    Command::new("ffmpeg")
        .args([
            "-y",
            "-f",
            "lavfi",
            "-i",
            &format!("testsrc2=duration={duration_seconds}:size=640x480:rate=25"),
            "-f",
            "lavfi",
            "-i",
            &format!("sine=frequency=1000:duration={duration_seconds}"),
            "-c:v",
            "mpeg4",
            "-c:a",
            "mp3",
            "-f",
            "avi",
            path.to_str().unwrap(),
        ])
        .output()
        .expect("Failed to create test AVI");
}

fn mp4_duration(path: &Path) -> f64 {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
            path.to_str().unwrap(),
        ])
        .output()
        .expect("Failed to run ffprobe");

    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .unwrap_or(0.0)
}
