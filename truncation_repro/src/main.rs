use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use serde::{Deserialize, Serialize};
use tokio::time::timeout;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Torrent {
    #[serde(default)]
    info_hash: Option<String>,
    name: String,
    progress: f32,
}

#[derive(Debug, Serialize, Deserialize)]
struct TorrentApiResponse {
    torrents: Vec<Torrent>,
}

#[derive(Debug, Serialize)]
struct Measurement {
    elapsed_seconds: u64,
    stream_size_bytes: u64,
    growth_bytes: u64,
    response_time_ms: u64,
    http_status: u16,
}

async fn start_server() -> Result<std::process::Child, Box<dyn std::error::Error>> {
    println!("Starting Riptide server in development mode...");

    let child = Command::new("cargo")
        .args(["run", "--", "server", "--mode", "development"])
        .current_dir("/Users/mitander/c/p/riptide")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;

    // Wait for server to be ready with more thorough checks
    for i in 1..=60 {
        tokio::time::sleep(Duration::from_secs(1)).await;

        // Check both API endpoints the UI uses
        async fn health_check() -> Result<(), &'static str> {
            let torrents_response = reqwest::get("http://127.0.0.1:3000/api/torrents").await.map_err(|_| "torrents failed")?;
            let stats_response = reqwest::get("http://127.0.0.1:3000/api/dashboard/stats").await.map_err(|_| "stats failed")?;

            if torrents_response.status().is_success() && stats_response.status().is_success() {
                Ok(())
            } else {
                Err("Server not ready")
            }
        }

        if let Ok(Ok(_)) = timeout(Duration::from_secs(2), health_check()).await {
            println!("Server is fully responding (attempt {})", i);
            tokio::time::sleep(Duration::from_secs(3)).await; // Extra stabilization for development mode
            return Ok(child);
        }

        if i % 10 == 0 {
            println!("Still waiting for server... (attempt {})", i);
        }

        if i == 60 {
            return Err("Server failed to start within 60 seconds".into());
        }
    }

    Ok(child)
}

async fn get_torrent_info() -> Result<Torrent, Box<dyn std::error::Error>> {
    println!("Getting torrent information from development server...");

    // Simulate the UI's behavior - check multiple times as development mode sets up torrents
    for attempt in 1..=10 {
        let response = reqwest::get("http://127.0.0.1:3000/api/torrents").await?;
        let body = response.text().await?;

        println!("API Response (attempt {}): {}", attempt, body);

        // Parse the response - development mode returns array of torrents
        let torrents: Vec<Torrent> = if let Ok(direct_array) = serde_json::from_str::<Vec<Torrent>>(&body) {
            direct_array
        } else if let Ok(wrapper) = serde_json::from_str::<TorrentApiResponse>(&body) {
            wrapper.torrents
        } else if let Ok(obj) = serde_json::from_str::<serde_json::Value>(&body) {
            if let Some(torrents_array) = obj.get("torrents").and_then(|v| v.as_array()) {
                serde_json::from_value(serde_json::Value::Array(torrents_array.clone()))?
            } else {
                println!("Unexpected response format: {}", body);
                return Err("No torrents found in response".into());
            }
        } else {
            println!("Could not parse response: {}", body);
            return Err("Could not parse torrents response".into());
        };

        if !torrents.is_empty() {
            let mut torrent = torrents[0].clone();

            // Handle development mode fallback data that doesn't have info_hash
            if torrent.info_hash.is_none() {
                // Generate deterministic info_hash for development mode based on movie name
                let hash = generate_development_info_hash(&torrent.name);
                torrent.info_hash = Some(hash.clone());
                println!("Development mode: Generated info_hash {} for {}", hash, torrent.name);
            }

            if let Some(ref hash) = torrent.info_hash {
                println!("Found torrent: {} ({})", torrent.name, hash);
                println!("Progress: {:.1}%", torrent.progress);
                return Ok(torrent);
            }
        }

        if attempt < 10 {
            println!("No torrents yet, waiting for development mode setup... (attempt {})", attempt);
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    }

    Err("No torrents available after waiting".into())
}

async fn make_initial_requests(torrent: &Torrent) -> Result<(), Box<dyn std::error::Error>> {
    let info_hash = torrent.info_hash.as_ref().ok_or("Missing info_hash")?;
    let stream_url = format!("http://127.0.0.1:3000/stream/{}", info_hash);
    let client = reqwest::Client::new();

    println!("=== SIMULATING UI VIDEO PLAYER INITIALIZATION ===");

    // 1. Readiness check (simulates UI checking if stream is ready)
    println!("1. Checking stream readiness...");
    let readiness_url = format!("http://127.0.0.1:3000/stream/{}/ready", info_hash);
    if let Ok(response) = client.get(&readiness_url).send().await {
        println!("   Readiness Status: {}", response.status());
        if let Ok(body) = response.text().await {
            println!("   Readiness Response: {}", body);
        }
    }

    // 2. HEAD request (browser often does this to check content length)
    println!("2. Making HEAD request to check file size...");
    let head_response = client.head(&stream_url).send().await?;
    println!("   Status: {}", head_response.status());

    if let Some(content_length) = head_response.headers().get("content-length") {
        println!("   Content-Length: {:?}", content_length);
    }
    if let Some(accept_ranges) = head_response.headers().get("accept-ranges") {
        println!("   Accept-Ranges: {:?}", accept_ranges);
    }

    // 3. Initial range request for start of file (simulates browser starting playback)
    println!("3. Making initial range request (bytes=0-1048575) for MP4 headers...");
    let range_response = client
        .get(&stream_url)
        .header("Range", "bytes=0-1048575") // First 1MB for MP4 headers
        .send()
        .await?;

    println!("   Status: {}", range_response.status());

    if let Some(content_range) = range_response.headers().get("content-range") {
        println!("   Content-Range: {:?}", content_range);
    }
    if let Some(content_type) = range_response.headers().get("content-type") {
        println!("   Content-Type: {:?}", content_type);
    }

    // Read the initial chunk to trigger streaming setup
    let bytes = range_response.bytes().await?;
    println!("   Received: {} bytes of MP4 header data", bytes.len());

    // 4. Check streaming health (debug endpoint)
    println!("4. Checking streaming service health...");
    let health_response = client.get("http://127.0.0.1:3000/stream/health").send().await?;
    if let Ok(health_body) = health_response.text().await {
        println!("   Health: {}", health_body);
    }

    println!("=== UI SIMULATION COMPLETE - READY FOR MONITORING ===\n");

    Ok(())
}

async fn monitor_progressive_growth(torrent: &Torrent, duration_minutes: u64) -> Result<Vec<Measurement>, Box<dyn std::error::Error>> {
    let info_hash = torrent.info_hash.as_ref().ok_or("Missing info_hash")?;
    let stream_url = format!("http://127.0.0.1:3000/stream/{}", info_hash);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?;
    let start_time = Instant::now();
    let end_time = start_time + Duration::from_secs(duration_minutes * 60);

    println!("=== MONITORING PROGRESSIVE STREAMING GROWTH ===");
    println!("Duration: {} minutes", duration_minutes);
    println!("This simulates continuous browser requests during video playback\n");
    println!("Time  | Stream Size    | Growth     | Response | Status | Details");
    println!("------|----------------|------------|----------|---------|------------------");

    let mut measurements = Vec::new();
    let mut last_size = 0u64;
    let mut consecutive_no_growth = 0;
    let max_consecutive_no_growth = 24; // 2 minutes at 5-second intervals
    let check_interval = Duration::from_secs(5);

    while Instant::now() < end_time {
        let request_start = Instant::now();
        let range_header = format!("bytes={}-", last_size);

        match client
            .head(&stream_url)
            .header("Range", &range_header)
            .send()
            .await
        {
            Ok(response) => {
                let request_time = request_start.elapsed().as_millis() as u64;
                let elapsed = start_time.elapsed().as_secs();
                let status = response.status().as_u16();

                if let Some(content_range) = response.headers().get("content-range") {
                    let content_range_str = content_range.to_str().unwrap_or("");

                    // Parse "bytes start-end/total" or "bytes start-end/*"
                    if let Some(current_size) = parse_content_range_end(content_range_str) {
                        if current_size > last_size {
                            let growth = current_size - last_size;
                            println!("{:>4}s | {:>13} | {:>9} | {:>7}ms | {:>3} | Progressive growth",
                                     elapsed, format_number(current_size), format_bytes(growth), request_time, status);

                            measurements.push(Measurement {
                                elapsed_seconds: elapsed,
                                stream_size_bytes: current_size,
                                growth_bytes: growth,
                                response_time_ms: request_time,
                                http_status: status,
                            });

                            last_size = current_size;
                            consecutive_no_growth = 0;
                        } else {
                            consecutive_no_growth += 1;

                            if consecutive_no_growth <= 5 {
                                println!("{:>4}s | {:>13} | {:>9} | {:>7}ms | {:>3} | No growth ({})",
                                         elapsed, format_number(current_size), "0 B", request_time, status, consecutive_no_growth);
                            } else if consecutive_no_growth == 6 {
                                println!("{:>4}s | {:>13} | {:>9} | {:>7}ms | {:>3} | Continuing...",
                                         elapsed, format_number(current_size), "0 B", request_time, status);
                            }

                            if consecutive_no_growth >= max_consecutive_no_growth {
                                println!("\n*** TRUNCATION DETECTED ***");
                                println!("No growth detected for {} checks ({} seconds)",
                                         max_consecutive_no_growth, max_consecutive_no_growth * 5);
                                println!("Progressive streaming appears to have stopped at {} bytes",
                                         format_number(current_size));
                                break;
                            }
                        }

                        // Also check debug endpoints periodically to understand internal state
                        if elapsed % 30 == 0 && elapsed > 0 {
                            check_debug_status(&client, info_hash).await;
                        }
                    }
                }
            }
            Err(e) => {
                let request_time = request_start.elapsed().as_millis() as u64;
                let elapsed = start_time.elapsed().as_secs();
                println!("{:>4}s | {:>13} | {:>9} | {:>7}ms | ERR | Request failed: {}",
                         elapsed, "N/A", "N/A", request_time, e);
            }
        }

        tokio::time::sleep(check_interval).await;
    }

    println!("\n=== MONITORING COMPLETE ===\n");
    Ok(measurements)
}

async fn check_debug_status(client: &reqwest::Client, info_hash: &str) {
    println!("    [DEBUG] Checking internal streaming state...");

    // Check streaming statistics
    if let Ok(response) = client.get("http://127.0.0.1:3000/stream/stats").send().await {
        if let Ok(body) = response.text().await {
            println!("    [DEBUG] Streaming stats: {}", body);
        }
    }

    // Check specific torrent debug status
    let debug_url = format!("http://127.0.0.1:3000/debug/stream/{}/status", info_hash);
    if let Ok(response) = client.get(&debug_url).send().await {
        if let Ok(body) = response.text().await {
            println!("    [DEBUG] Torrent status: {}", body);
        }
    }
}

fn parse_content_range_end(content_range: &str) -> Option<u64> {
    // Parse "bytes start-end/total" format
    if content_range.starts_with("bytes ") {
        let range_part = &content_range[6..];
        if let Some(slash_pos) = range_part.find('/') {
            let range_only = &range_part[..slash_pos];
            if let Some(dash_pos) = range_only.find('-') {
                let end_str = &range_only[dash_pos + 1..];
                if let Ok(end_byte) = end_str.parse::<u64>() {
                    return Some(end_byte + 1); // +1 because range is inclusive
                }
            }
        }
    }
    None
}

fn format_number(n: u64) -> String {
    let n_str = n.to_string();
    let mut result = String::new();
    let chars: Vec<char> = n_str.chars().collect();

    for (i, ch) in chars.iter().enumerate() {
        if i > 0 && (chars.len() - i) % 3 == 0 {
            result.push(',');
        }
        result.push(*ch);
    }
    result
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{} B", bytes)
    }
}

fn analyze_results(measurements: &[Measurement], original_file_size: u64) {
    if measurements.is_empty() {
        println!("ERROR: No measurements recorded - progressive streaming may have failed to start");
        return;
    }

    let final_size = measurements.last().unwrap().stream_size_bytes;
    let percentage_served = (final_size as f64 / original_file_size as f64) * 100.0;

    println!("=== PROGRESSIVE STREAMING TRUNCATION ANALYSIS ===");
    println!();
    println!("FILE SIZE ANALYSIS:");
    println!("  Original file size: {} bytes ({:.2} MB)",
             format_number(original_file_size), original_file_size as f64 / (1024.0 * 1024.0));
    println!("  Final stream size:  {} bytes ({:.2} MB)",
             format_number(final_size), final_size as f64 / (1024.0 * 1024.0));
    println!("  Percentage served:  {:.2}%", percentage_served);
    println!();

    if percentage_served < 95.0 {
        println!("*** TRUNCATION CONFIRMED ***");
        println!("Only {:.1}% of the file was progressively streamed", percentage_served);

        // Calculate duration estimates
        let original_duration_seconds = 16 * 60 + 20; // Known from logs: 16min 20s movie
        let served_duration_seconds = (percentage_served / 100.0) * original_duration_seconds as f64;
        let served_minutes = (served_duration_seconds / 60.0) as u64;
        let served_seconds = (served_duration_seconds % 60.0) as u64;

        println!();
        println!("DURATION ANALYSIS:");
        println!("  Original duration:  16:20 (16 minutes, 20 seconds)");
        println!("  Truncated to:       {}:{:02} ({} minutes, {} seconds)",
                 served_minutes, served_seconds, served_minutes, served_seconds);

        let missing_bytes = original_file_size - final_size;
        println!();
        println!("MISSING DATA:");
        println!("  Missing bytes:      {} bytes ({:.2} MB)",
                 format_number(missing_bytes), missing_bytes as f64 / (1024.0 * 1024.0));

        // Performance analysis
        println!();
        println!("STREAMING PERFORMANCE:");
        let total_growth: u64 = measurements.iter().map(|m| m.growth_bytes).sum();
        let growth_measurements = measurements.iter().filter(|m| m.growth_bytes > 0).count();
        if growth_measurements > 0 {
            let avg_growth = total_growth / growth_measurements as u64;
            let avg_response_time: u64 = measurements.iter().map(|m| m.response_time_ms).sum::<u64>() / measurements.len() as u64;
            println!("  Average growth per measurement: {}", format_bytes(avg_growth));
            println!("  Average response time: {}ms", avg_response_time);
        }

        // Find when growth stopped
        if let Some(last_growth) = measurements.iter().filter(|m| m.growth_bytes > 0).last() {
            println!("  Growth stopped after: {} seconds", last_growth.elapsed_seconds);
        }

        println!();
        println!("ROOT CAUSE HYPOTHESIS:");
        println!("  The progressive streaming pipeline starts correctly and begins");
        println!("  feeding data to FFmpeg for remuxing. However, it stops prematurely");
        println!("  at approximately {} MB, which corresponds to the truncation",
                 final_size as f64 / (1024.0 * 1024.0));
        println!("  issue described in the problem statement.");
        println!();
        println!("  This explains why users see a '{}:{:02} movie' instead of the",
                 served_minutes, served_seconds);
        println!("  full 16:20 duration - the progressive streaming stops early,");
        println!("  causing FFmpeg to finalize the MP4 with only partial content.");

    } else {
        println!("*** SUCCESS: FULL FILE STREAMED ***");
        println!("Progressive streaming completed successfully!");

        let total_time = measurements.last().unwrap().elapsed_seconds;
        let avg_rate = final_size as f64 / total_time as f64;
        println!("Streaming completed in {} seconds", total_time);
        println!("Average streaming rate: {:.2} KB/s", avg_rate / 1024.0);
    }

    println!();
    println!("=== ANALYSIS COMPLETE ===");
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🔍 PROGRESSIVE STREAMING TRUNCATION REPRODUCTION");
    println!("=============================================");
    println!();
    println!("This script reproduces the progressive streaming truncation issue by:");
    println!("1. Starting the Riptide server in development mode");
    println!("2. Simulating the exact UI interactions (API calls and streaming requests)");
    println!("3. Monitoring progressive streaming growth over time");
    println!("4. Detecting when streaming stops growing (indicating truncation)");
    println!();
    println!("Expected result: ~12MB truncation at approximately 1min 15sec of video");
    println!("=============================================");
    println!();

    // Start server
    let mut server_process = start_server().await?;

    let result = async {
        // Get torrent info from development server
        let torrent = get_torrent_info().await?;

        // Make initial requests that simulate UI behavior
        make_initial_requests(&torrent).await?;

        // Monitor progressive growth over time
        println!("Starting progressive streaming monitoring...");
        println!("This will run for up to 5 minutes or until truncation is detected.\n");
        let measurements = monitor_progressive_growth(&torrent, 5).await?;

        // Analyze results and provide detailed diagnosis
        let original_file_size = 135046574; // From previous logs: "Final file size determined: 135046574 bytes"
        analyze_results(&measurements, original_file_size);

        // Save measurements for further analysis
        if !measurements.is_empty() {
            let json_output = serde_json::to_string_pretty(&measurements)?;
            tokio::fs::write("truncation_measurements.json", json_output).await?;
            println!("📊 Detailed measurements saved to truncation_measurements.json");
        }

        Ok::<(), Box<dyn std::error::Error>>(())
    }.await;

    // Cleanup
    println!("\n🧹 Cleaning up server process...");
    let _ = server_process.kill();

    if let Err(e) = result {
        println!("\n❌ Reproduction failed: {}", e);
        std::process::exit(1);
    } else {
        println!("\n✅ Reproduction completed successfully");
    }

    Ok(())
}

/// Generate a deterministic info_hash for development mode based on torrent name
fn generate_development_info_hash(name: &str) -> String {
    let mut hasher = DefaultHasher::new();
    name.hash(&mut hasher);
    let hash = hasher.finish();

    // Convert to 40-character hex string (SHA1 hash format)
    format!("{:040x}", hash)
}
