# Progressive Streaming Truncation Reproduction

This script reproduces the progressive streaming truncation issue described in the problem statement, where only ~1min 15sec of a 16min 20sec movie is served due to progressive streaming stopping prematurely at approximately 12MB.

## Problem Description

The server shows "only serving 1min 15sec movie" instead of the full 16min 20sec duration. This happens because:

1. Progressive streaming starts correctly and begins feeding data to FFmpeg
2. The pipeline stops growing at approximately 12MB 
3. FFmpeg finalizes the MP4 with only partial content
4. Users see a truncated video instead of the full movie

## How This Script Works

1. **Server Startup**: Starts Riptide server in development mode with simulated torrents
2. **UI Simulation**: Makes the exact same API calls that the web UI makes:
   - `/api/torrents` to get torrent list
   - `/stream/{info_hash}/ready` to check readiness
   - HEAD and range requests to `/stream/{info_hash}` 
3. **Monitoring**: Continuously monitors progressive streaming growth by checking stream size
4. **Detection**: Identifies when streaming stops growing (indicating truncation)
5. **Analysis**: Provides detailed analysis of the truncation point and missing data

## Usage

```bash
# From the truncation_repro directory
cargo run
```

## Expected Output

The script should detect truncation at approximately:
- **File size**: ~12MB (out of ~135MB total)
- **Percentage**: ~9% of the original file
- **Duration**: ~1min 15sec (out of 16min 20sec total)

## Output Files

- `truncation_measurements.json`: Detailed measurements of streaming growth over time

## Key Insights

This reproduction script helps identify:

1. **When**: The exact time progressive streaming stops
2. **Where**: The specific byte position where truncation occurs  
3. **How much**: The percentage of content that gets served vs. missing
4. **Performance**: Response times and growth rates during streaming

## Connection to Code

The issue likely originates in the progressive streaming pipeline at:
- `riptide-core/src/streaming/progressive.rs` - The core streaming pump logic
- Progressive streaming stops early, causing FFmpeg to finalize with partial content
- The UI shows a complete but truncated video file

## Debugging

The script includes debug endpoints to monitor internal state:
- `/stream/stats` - Overall streaming statistics
- `/debug/stream/{info_hash}/status` - Per-torrent debug information

This helps understand what's happening inside the streaming pipeline when truncation occurs.