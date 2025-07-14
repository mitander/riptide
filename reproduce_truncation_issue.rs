#!/usr/bin/env -S cargo +nightly -Zscript
//! ```cargo
//! [dependencies]
//! tokio = { version = "1.0", features = ["full"] }
//! reqwest = { version = "0.11", features = ["json"] }
//! serde_json = "1.0"
//! serde = { version = "1.0", features = ["derive"] }
//! ctrlc = "3.0"
//! chrono = { version = "0.4", features = ["serde"] }
//! ```

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use std::thread;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct Torrent {
    info_hash: String,
    name: String,
    progress: f32,
}

#[derive(Debug, Serialize)]
struct Measurement {
    elapsed_seconds: u64,
    stream_size_bytes: u64,
    growth_bytes: u64,
}

#[derive(Debug, Serialize)]
struct TestResults {
    test_start: String,
    original_file_size: u64,
    measurements: Vec<Measurement>,
    final_analysis: String,
}

async fn start_server() -> Result<std::process::Child, Box<dyn std::error::Error>> {
    println!("Starting Riptide server...");
    
    let child = Command::new("cargo")
        .args(["run", "--", "server"])
        .current_dir("/Users/mitander/c/p/riptide")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    
    // Wait for server to be ready
    for i in 1..=30 {
        tokio::time::sleep(Duration::from_secs(1)).await;
        
        if let Ok(response) = reqwest::get("http://127.0.0.1:3000/api/torrents").await {
            if response.status().is_success() {
                println!("Server is responding (attempt {})", i);
                tokio::time::sleep(Duration::from_secs(2)).await; // Stabilization
                return Ok(child);
            }
        }
        
        if i == 30 {
            return Err("Server failed to start within 30 seconds".into());
        }
    }
    
    Ok(child)
}

async fn get_torrent_info() -> Result<Torrent, Box<dyn std::error::Error>> {
    println!("Getting torrent information...");
    
    let response = reqwest::get("http://127.0.0.1:3000/api/torrents").await?;
    let body = response.text().await?;
    
    println!("Response: {}", body);
    
    // Try to parse as array first, then as object with torrents field
    let torrents: Vec<Torrent> = if let Ok(direct_array) = serde_json::from_str::<Vec<Torrent>>(&body) {
        direct_array
    } else if let Ok(obj) = serde_json::from_str::<serde_json::Value>(&body) {
        if let Some(torrents_array) = obj.get("torrents").and_then(|v| v.as_array()) {
            serde_json::from_value(serde_json::Value::Array(torrents_array.clone()))?
        } else {
            return Err("No torrents found in response".into());
        }
    } else {
        return Err("Could not parse torrents response".into());
    };
    
    if torrents.is_empty() {
        return Err("No torrents available".into());
    }
    
    let torrent = &torrents[0];
    println!("Info hash: {}", torrent.info_hash);
    println!("Progress: {:.1}%", torrent.progress);
    
    Ok(torrent.clone())
}

async fn make_initial_requests(info_hash: &str) -> Result<(), Box<dyn std::error::Error>> {
    let stream_url = format!("http://127.0.0.1:3000/stream/{}", info_hash);
    
    // HEAD request
    println!("📡 Making HEAD request...");
    let head_response = reqwest::Client::new().head(&stream_url).send().await?;
    println!("   Status: {}", head_response.status());
    
    if let Some(content_length) = head_response.headers().get("content-length") {
        println!("   Content-Length: {:?}", content_length);
    }
    
    // Initial range request
    println!("📡 Making initial range request (bytes=0-)...");
    let range_response = reqwest::Client::new()
        .get(&stream_url)
        .header("Range", "bytes=0-")
        .send()
        .await?;
    
    println!("   Status: {}", range_response.status());
    
    if let Some(content_range) = range_response.headers().get("content-range") {
        println!("   Content-Range: {:?}", content_range);
    }
    
    // Read first chunk to trigger streaming
    let bytes = range_response.bytes().await?;
    println!("   📦 Received: {} bytes", bytes.len());
    
    Ok(())
}

async fn monitor_progressive_growth(info_hash: &str, duration_minutes: u64) -> Result<Vec<Measurement>, Box<dyn std::error::Error>> {
    let stream_url = format!("http://127.0.0.1:3000/stream/{}", info_hash);
    let start_time = Instant::now();
    let end_time = start_time + Duration::from_secs(duration_minutes * 60);
    
    println!("⏱️  Monitoring progressive streaming for {} minutes...", duration_minutes);
    println!("📈 Tracking stream size growth:");
    
    let mut measurements = Vec::new();
    let mut last_size = 0u64;
    let mut consecutive_no_growth = 0;
    let max_consecutive_no_growth = 10;
    let check_interval = Duration::from_secs(5);
    
    while Instant::now() < end_time {
        let range_header = format!("bytes={}-", last_size);
        
        match reqwest::Client::new()
            .head(&stream_url)
            .header("Range", &range_header)
            .send()
            .await
        {
            Ok(response) => {
                let elapsed = start_time.elapsed().as_secs();
                
                if let Some(content_range) = response.headers().get("content-range") {
                    let content_range_str = content_range.to_str().unwrap_or("");
                    
                    // Parse "bytes start-end/total" or "bytes start-end/*"
                    if let Some(current_size) = parse_content_range_end(content_range_str) {
                        if current_size > last_size {
                            let growth = current_size - last_size;
                            println!("   {:>4}s: {:>10} bytes (+{})", elapsed, format_number(current_size), format_number(growth));
                            
                            measurements.push(Measurement {
                                elapsed_seconds: elapsed,
                                stream_size_bytes: current_size,
                                growth_bytes: growth,
                            });
                            
                            last_size = current_size;
                            consecutive_no_growth = 0;
                        } else {
                            consecutive_no_growth += 1;
                            
                            if consecutive_no_growth <= 3 {
                                println!("   {:>4}s: {:>10} bytes (no growth)", elapsed, format_number(current_size));
                            } else if consecutive_no_growth == 4 {
                                println!("   ...continuing to monitor...");
                            }
                            
                            if consecutive_no_growth >= max_consecutive_no_growth {
                                println!("🛑 Stopping: No growth detected for {} seconds", max_consecutive_no_growth * 5);
                                break;
                            }
                        }
                    }
                }
            }
            Err(e) => {
                println!("❌ Error during monitoring: {}", e);
            }
        }
        
        tokio::time::sleep(check_interval).await;
    }
    
    Ok(measurements)
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

fn analyze_results(measurements: &[Measurement], original_file_size: u64) -> String {
    if measurements.is_empty() {
        return "❌ No measurements to analyze".to_string();
    }
    
    let final_size = measurements.last().unwrap().stream_size_bytes;
    let percentage_served = (final_size as f64 / original_file_size as f64) * 100.0;
    
    let mut analysis = String::new();
    analysis.push_str(&format!("Original file size: {} bytes ({:.1} MB)\n", 
        format_number(original_file_size), original_file_size as f64 / 1024.0 / 1024.0));
    analysis.push_str(&format!("Final stream size:  {} bytes ({:.1} MB)\n", 
        format_number(final_size), final_size as f64 / 1024.0 / 1024.0));
    analysis.push_str(&format!("Percentage served:  {:.1}%\n", percentage_served));
    
    if percentage_served < 95.0 {
        analysis.push_str(&format!("❌ TRUNCATION DETECTED: Only {:.1}% of file was served\n", percentage_served));
        
        // Estimate served duration (original is 16min 20s = 980 seconds)
        let original_duration_seconds = 16 * 60 + 20;
        let served_duration_seconds = (percentage_served / 100.0) * original_duration_seconds as f64;
        let served_minutes = (served_duration_seconds / 60.0) as u64;
        let served_seconds = (served_duration_seconds % 60.0) as u64;
        analysis.push_str(&format!("📺 Estimated served duration: {}:{:02}\n", served_minutes, served_seconds));
    } else {
        analysis.push_str("✅ Full file appears to be served\n");
    }
    
    // Find when growth stopped
    if let Some(last_growth) = measurements.iter().filter(|m| m.growth_bytes > 0).last() {
        analysis.push_str(&format!("⏰ Streaming stopped growing after: {} seconds\n", last_growth.elapsed_seconds));
    }
    
    analysis
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🎬 PROGRESSIVE STREAMING TRUNCATION REPRODUCTION (Rust Edition)");
    println!("================================================================");
    
    // Start server
    let mut server_process = start_server().await?;
    
    // Setup cleanup
    let server_running = Arc::new(AtomicBool::new(true));
    let server_running_clone = server_running.clone();
    
    ctrlc::set_handler(move || {
        println!("\n⚠️  Received interrupt signal, cleaning up...");
        server_running_clone.store(false, Ordering::Relaxed);
    }).expect("Error setting Ctrl-C handler");
    
    let result = async {
        // Get torrent info
        let torrent = get_torrent_info().await?;
        
        // Make initial requests to trigger streaming
        make_initial_requests(&torrent.info_hash).await?;
        
        // Monitor progressive growth
        let measurements = monitor_progressive_growth(&torrent.info_hash, 3).await?;
        
        // Analyze results
        let original_file_size = 135046574; // From logs
        let analysis = analyze_results(&measurements, original_file_size);
        
        println!("\n📊 ANALYSIS RESULTS:");
        println!("================================================================");
        println!("{}", analysis);
        
        // Save results
        let results = TestResults {
            test_start: chrono::Utc::now().to_rfc3339(),
            original_file_size,
            measurements,
            final_analysis: analysis,
        };
        
        let results_json = serde_json::to_string_pretty(&results)?;
        std::fs::write("truncation_results_rust.json", results_json)?;
        println!("📄 Detailed results saved to: truncation_results_rust.json");
        
        Ok::<(), Box<dyn std::error::Error>>(())
    }.await;
    
    // Cleanup
    server_running.store(false, Ordering::Relaxed);
    if let Err(_) = server_process.kill() {
        // Process might have already exited
    }
    
    result
}