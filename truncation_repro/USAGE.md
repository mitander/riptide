# Usage Instructions

## Quick Start

```bash
# Navigate to the truncation reproduction directory
cd /Users/mitander/c/p/riptide/truncation_repro

# Run the reproduction script
cargo run
```

## What the Script Does

1. **Starts Riptide Server**: Launches the server in development mode with simulated torrents
2. **Waits for Setup**: Allows time for the development server to initialize and create torrents
3. **Simulates UI**: Makes the same HTTP requests a browser would make:
   - Checks torrent list via `/api/torrents`
   - Checks stream readiness via `/stream/{hash}/ready`
   - Makes HEAD request to get file size
   - Makes range requests to start progressive streaming
4. **Monitors Growth**: Tracks how the progressive stream grows over time
5. **Detects Truncation**: Identifies when streaming stops growing (indicating the bug)
6. **Analyzes Results**: Provides detailed analysis of the truncation

## Expected Timeline

- **0-60s**: Server startup and torrent initialization
- **60-90s**: UI simulation and streaming initiation
- **90-300s**: Progressive streaming monitoring
- **Detection**: Truncation should be detected within 2-3 minutes

## Expected Results

If the bug is present, you should see:
- Stream grows initially (progressive streaming starts)
- Growth stops at approximately 12MB
- Final analysis shows ~9% of file served
- Estimated truncated duration: ~1min 15sec

## Troubleshooting

**Server fails to start:**
- Ensure no other instances are running on port 3000
- Check that you have a valid movies directory for development mode

**No torrents found:**
- Development mode needs time to create simulated torrents
- The script waits up to 20 seconds for torrents to appear

**Network errors:**
- Check that port 3000 is available
- Verify reqwest dependencies are properly installed

## Output Files

- `truncation_measurements.json`: Detailed streaming measurements
- Console output: Real-time monitoring and final analysis

## Understanding the Output

The monitoring section shows:
- **Time**: Elapsed seconds since monitoring started
- **Stream Size**: Current size of the progressive stream
- **Growth**: Bytes added since last measurement
- **Response**: HTTP response time for the measurement
- **Status**: HTTP status code
- **Details**: What's happening (growth, no growth, etc.)

When truncation is detected, you'll see:
```
*** TRUNCATION DETECTED ***
No growth detected for 24 checks (120 seconds)
Progressive streaming appears to have stopped at 12,345,678 bytes
```