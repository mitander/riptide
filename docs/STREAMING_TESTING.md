# Riptide Streaming Testing Guide

## Overview

This document describes the comprehensive testing infrastructure for Riptide's streaming functionality. The tests are designed to validate that AVI, MKV, and other video formats are properly streamed to browsers without requiring manual interaction.

## Testing Philosophy

Instead of manual browser testing, which is:
- Time-consuming and error-prone
- Difficult to reproduce consistently
- Hard for automated systems (including LLMs) to perform

We've implemented automated tests that:
- Simulate browser behavior programmatically
- Validate MP4 structure and streaming compatibility
- Test cache functionality and performance
- Verify concurrent request handling
- Check error conditions and edge cases

## Test Components

### 1. Unit Tests (`tests/browser_streaming_test.rs`)

**Purpose**: Test streaming logic without a running server

**What it tests**:
- Container format detection
- MP4 remuxing for AVI/MKV files
- Cache behavior
- Concurrent request handling
- Range request support

**Run with**:
```bash
cargo test browser_streaming_test
```

### 2. Integration Test Script (`test_streaming.sh`)

**Purpose**: End-to-end testing with a real server

**What it does**:
1. Creates test video files (AVI, MKV, MP4)
2. Starts Riptide server in dev mode
3. Runs HTTP streaming tests
4. Monitors cache directory
5. Validates remuxed files
6. Generates test report

**Run with**:
```bash
./test_streaming.sh [options]

Options:
  --skip-build      Skip building the project
  --keep-server     Keep server running after tests
  --verbose         Enable verbose logging
  --test-dir PATH   Directory containing test video files
```

### 3. HTTP Streaming Tests (`tests/integration/test_streaming_http.py`)

**Purpose**: Test real HTTP streaming behavior

**What it tests**:
- Initial streaming requests
- Range requests (seeking)
- MP4 structure validation
- Cache hit/miss behavior
- Concurrent request handling
- Error responses

**Run with**:
```bash
python3 tests/integration/test_streaming_http.py --server-url http://127.0.0.1:3000
```

### 4. FFmpeg Remux Tests (`tests/integration/test_remux.sh`)

**Purpose**: Validate FFmpeg remuxing commands

**What it tests**:
- AVI to MP4 remuxing
- MKV to MP4 remuxing
- Output file validation
- Cache functionality
- Performance metrics

**Run with**:
```bash
./tests/integration/test_remux.sh
```

## Running All Tests

The easiest way to run all tests is:

```bash
# Run complete test suite
./test_streaming.sh

# Keep server running for manual inspection
./test_streaming.sh --keep-server

# Verbose output for debugging
./test_streaming.sh --verbose
```

## Understanding Test Results

### Successful Test Output

```
[INFO] Starting Riptide streaming tests
[SUCCESS] All dependencies satisfied
[SUCCESS] Build completed
[SUCCESS] Test videos created
[SUCCESS] Server is ready
[SUCCESS] HTTP streaming tests passed
[SUCCESS] Remux tests passed
[SUCCESS] Valid MP4: c1f839325f14bf485761059f23061b273d5c820f.mp4
[SUCCESS] All streaming tests completed! ✅
```

### Common Issues and Solutions

#### Issue: FFmpeg Not Found
```
[ERROR] ffmpeg is not installed
```
**Solution**: Install FFmpeg
```bash
# macOS
brew install ffmpeg

# Ubuntu/Debian
sudo apt-get install ffmpeg

# Fedora
sudo dnf install ffmpeg
```

#### Issue: Server Fails to Start
```
[ERROR] Server process died unexpectedly
```
**Solution**: Check server logs
```bash
tail -f /tmp/riptide-test-logs/server.log
```

#### Issue: Remuxing Fails
```
[ERROR] Invalid MP4: Missing moov atom
```
**Solution**: Check FFmpeg logs and ensure input files are valid

#### Issue: Cache Not Working
```
[WARNING] Cache directory not found
```
**Solution**: Check permissions on `/tmp/riptide-remux-cache/`

## Test Scenarios

### 1. AVI Streaming Test
- Creates a test AVI file with MPEG-4 video and MP3 audio
- Requests streaming URL
- Validates that file is remuxed to MP4
- Checks for valid MP4 structure (ftyp and moov atoms)
- Verifies cache is populated
- Tests range requests

### 2. MKV Seeking Test
- Creates a test MKV file with H.264 video and AAC audio
- Tests multiple seek positions (0%, 25%, 50%, 75%)
- Validates Content-Range headers
- Ensures seeking doesn't restart playback

### 3. Concurrent Request Test
- Sends 5 simultaneous requests for the same file
- Verifies lock mechanism prevents multiple remuxing
- Checks that some requests get "Stream being prepared" response
- Validates eventual consistency

### 4. Cache Performance Test
- Makes initial request (triggers remux)
- Makes second request (should hit cache)
- Compares response times
- Cache hit should be >50% faster

## Debugging Failed Tests

### Enable Debug Logging

```bash
export RUST_LOG=riptide_web::streaming=trace,riptide_core::streaming=trace
./test_streaming.sh --verbose
```

### Check Cache Directory

```bash
# List cache contents
ls -la /tmp/riptide-remux-cache/

# Validate cached MP4 files
for f in /tmp/riptide-remux-cache/*.mp4; do
    ffprobe -v error -show_format "$f" && echo "$f is valid" || echo "$f is invalid"
done
```

### Monitor Server Logs

```bash
# Watch server logs in real-time
tail -f /tmp/riptide-test-logs/server.log | grep -E "(Remuxing|cache|error)"
```

### Test Individual Components

```bash
# Test only FFmpeg remuxing
./tests/integration/test_remux.sh

# Test only HTTP endpoints
python3 tests/integration/test_streaming_http.py --info-hash c1f839325f14bf485761059f23061b273d5c820f

# Test only unit tests
cargo test streaming -- --nocapture
```

## Performance Expectations

- **Initial AVI/MKV Request**: 1-5 seconds (includes remuxing)
- **Cached Requests**: <100ms
- **MP4 Direct Streaming**: <50ms
- **Remuxing Speed**: 50-100 MB/s on modern hardware

## Continuous Integration

Add to your CI pipeline:

```yaml
# Example GitHub Actions workflow
test-streaming:
  runs-on: ubuntu-latest
  steps:
    - uses: actions/checkout@v2
    - name: Install FFmpeg
      run: sudo apt-get update && sudo apt-get install -y ffmpeg
    - name: Install Rust
      uses: actions-rs/toolchain@v1
    - name: Run streaming tests
      run: ./test_streaming.sh
    - name: Upload test logs
      if: failure()
      uses: actions/upload-artifact@v2
      with:
        name: test-logs
        path: /tmp/riptide-test-logs/
```

## Adding New Tests

To add new test cases:

1. **Unit Tests**: Add to `tests/browser_streaming_test.rs`
2. **HTTP Tests**: Extend `tests/integration/test_streaming_http.py`
3. **FFmpeg Tests**: Modify `tests/integration/test_remux.sh`

Example:
```rust
#[tokio::test]
async fn test_webm_streaming() {
    // Your test implementation
}
```

## Summary

The streaming test suite provides comprehensive validation without manual browser testing. It catches common issues like:
- Invalid MP4 structure from remuxing
- Cache failures
- Concurrent request problems
- Seeking/range request issues
- Performance regressions

Run tests frequently during development to ensure streaming functionality remains robust and browser-compatible.
