#!/bin/bash

# Test script for verifying remuxing functionality in Riptide
# This tests that AVI and MKV files can be properly remuxed to MP4 for browser streaming

set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Test configuration
TEST_DIR="/tmp/riptide-remux-test"
CACHE_DIR="/tmp/riptide-remux-cache"

# Functions
log_info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

log_warning() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

cleanup() {
    log_info "Cleaning up test files..."
    rm -rf "$TEST_DIR"
    rm -rf "$CACHE_DIR"
}

check_dependencies() {
    log_info "Checking dependencies..."

    if ! command -v ffmpeg &> /dev/null; then
        log_error "ffmpeg is not installed. Please install ffmpeg to run this test."
        exit 1
    fi

    if ! command -v ffprobe &> /dev/null; then
        log_error "ffprobe is not installed. Please install ffmpeg to run this test."
        exit 1
    fi

    log_info "All dependencies found."
}

create_test_videos() {
    log_info "Creating test video files..."

    mkdir -p "$TEST_DIR"

    # Create a simple test pattern video with audio
    # AVI file
    log_info "Creating test AVI file..."
    ffmpeg -f lavfi -i testsrc=duration=10:size=320x240:rate=30 \
           -f lavfi -i sine=frequency=1000:duration=10 \
           -c:v mpeg4 -c:a mp3 -b:v 1M -b:a 128k \
           -y "$TEST_DIR/test.avi" 2>/dev/null

    # MKV file
    log_info "Creating test MKV file..."
    ffmpeg -f lavfi -i testsrc=duration=10:size=320x240:rate=30 \
           -f lavfi -i sine=frequency=1000:duration=10 \
           -c:v libx264 -c:a aac -b:v 1M -b:a 128k \
           -y "$TEST_DIR/test.mkv" 2>/dev/null

    # MP4 file (for comparison)
    log_info "Creating test MP4 file..."
    ffmpeg -f lavfi -i testsrc=duration=10:size=320x240:rate=30 \
           -f lavfi -i sine=frequency=1000:duration=10 \
           -c:v libx264 -c:a aac -b:v 1M -b:a 128k \
           -movflags +faststart \
           -y "$TEST_DIR/test.mp4" 2>/dev/null
}

test_remux_avi() {
    log_info "Testing AVI remux to MP4..."

    local input="$TEST_DIR/test.avi"
    local output="$TEST_DIR/test_avi_remuxed.mp4"

    # Run the same remux command that Riptide would use
    ffmpeg -y -i "$input" \
           -fflags +genpts+igndts \
           -avoid_negative_ts make_zero \
           -max_muxing_queue_size 9999 \
           -err_detect ignore_err \
           -copy_unknown \
           -c:v copy \
           -c:a aac -b:a 192k \
           -movflags +faststart \
           -f mp4 \
           "$output" 2>/dev/null

    if [ ! -f "$output" ]; then
        log_error "AVI remux failed - output file not created"
        return 1
    fi

    # Validate the output
    validate_mp4 "$output" "AVI remux"
}

test_remux_mkv() {
    log_info "Testing MKV remux to MP4..."

    local input="$TEST_DIR/test.mkv"
    local output="$TEST_DIR/test_mkv_remuxed.mp4"

    # Run the same remux command that Riptide would use
    ffmpeg -y -i "$input" \
           -map 0:v -map 0:a? \
           -c:v copy \
           -c:a copy \
           -movflags +faststart \
           -f mp4 \
           "$output" 2>/dev/null

    if [ ! -f "$output" ]; then
        log_error "MKV remux failed - output file not created"
        return 1
    fi

    # Validate the output
    validate_mp4 "$output" "MKV remux"
}

validate_mp4() {
    local file="$1"
    local test_name="$2"

    log_info "Validating $test_name output..."

    # Check file size
    local size=$(stat -f%z "$file" 2>/dev/null || stat -c%s "$file" 2>/dev/null)
    if [ "$size" -lt 1000 ]; then
        log_error "$test_name: Output file too small ($size bytes)"
        return 1
    fi

    # Check MP4 structure with ffprobe
    if ! ffprobe -v error -show_format -show_streams "$file" > /dev/null 2>&1; then
        log_error "$test_name: Invalid MP4 structure"
        return 1
    fi

    # Check for video and audio streams
    local video_streams=$(ffprobe -v error -select_streams v -show_entries stream=codec_type -of csv=p=0 "$file" 2>/dev/null | wc -l)
    local audio_streams=$(ffprobe -v error -select_streams a -show_entries stream=codec_type -of csv=p=0 "$file" 2>/dev/null | wc -l)

    if [ "$video_streams" -eq 0 ]; then
        log_error "$test_name: No video streams found"
        return 1
    fi

    if [ "$audio_streams" -eq 0 ]; then
        log_warning "$test_name: No audio streams found (this might be intentional)"
    fi

    # Check for moov atom position (should be at the beginning for faststart)
    local moov_pos=$(ffprobe -v error -show_entries format=format_name -of json "$file" 2>&1 | grep -q "moov" && echo "found" || echo "not found")

    # Verify the file can be played (check duration)
    local duration=$(ffprobe -v error -show_entries format=duration -of default=noprint_wrappers=1:nokey=1 "$file" 2>/dev/null)
    if [ -z "$duration" ] || [ "$duration" = "N/A" ]; then
        log_error "$test_name: Unable to determine duration"
        return 1
    fi

    log_info "$test_name: Valid MP4 created - Size: $size bytes, Duration: ${duration}s, Video: $video_streams, Audio: $audio_streams"
    return 0
}

test_cache_functionality() {
    log_info "Testing cache functionality..."

    # Create cache directory
    mkdir -p "$CACHE_DIR"

    # Simulate Riptide's cache file naming
    local test_hash="c1f839325f14bf485761059f23061b273d5c820f"
    local cache_file="$CACHE_DIR/${test_hash}.mp4"
    local lock_file="$CACHE_DIR/${test_hash}.lock"

    # Test lock file behavior
    touch "$lock_file"
    log_info "Created lock file: $lock_file"

    # Copy a remuxed file to cache
    cp "$TEST_DIR/test_avi_remuxed.mp4" "$cache_file" 2>/dev/null || true

    if [ -f "$cache_file" ]; then
        log_info "Cache file created successfully"
        rm -f "$lock_file"
        log_info "Lock file removed"
    else
        log_error "Failed to create cache file"
        return 1
    fi

    # Verify cached file is valid
    validate_mp4 "$cache_file" "Cached file"
}

run_performance_test() {
    log_info "Running performance test..."

    local start_time=$(date +%s)

    # Create a larger test file
    log_info "Creating larger test file for performance testing..."
    ffmpeg -f lavfi -i testsrc=duration=60:size=1920x1080:rate=30 \
           -f lavfi -i sine=frequency=1000:duration=60 \
           -c:v mpeg4 -c:a mp3 -b:v 5M -b:a 192k \
           -y "$TEST_DIR/test_large.avi" 2>/dev/null

    local file_size=$(stat -f%z "$TEST_DIR/test_large.avi" 2>/dev/null || stat -c%s "$TEST_DIR/test_large.avi" 2>/dev/null)
    log_info "Created test file: $(( file_size / 1024 / 1024 )) MB"

    # Time the remux operation
    local remux_start=$(date +%s)
    ffmpeg -y -i "$TEST_DIR/test_large.avi" \
           -fflags +genpts+igndts \
           -avoid_negative_ts make_zero \
           -max_muxing_queue_size 9999 \
           -err_detect ignore_err \
           -copy_unknown \
           -c:v copy \
           -c:a aac -b:a 192k \
           -movflags +faststart \
           -f mp4 \
           "$TEST_DIR/test_large_remuxed.mp4" 2>/dev/null
    local remux_end=$(date +%s)

    local remux_time=$((remux_end - remux_start))
    local throughput=$(( file_size / remux_time / 1024 / 1024 ))

    log_info "Remux performance: ${remux_time}s for $(( file_size / 1024 / 1024 )) MB (${throughput} MB/s)"
}

# Main execution
main() {
    log_info "Starting Riptide remux functionality tests..."

    # Set up error handling
    trap cleanup EXIT

    # Check dependencies
    check_dependencies

    # Create test videos
    create_test_videos

    # Run tests
    local failed=0

    if ! test_remux_avi; then
        ((failed++))
    fi

    if ! test_remux_mkv; then
        ((failed++))
    fi

    if ! test_cache_functionality; then
        ((failed++))
    fi

    # Optional performance test
    if [ "${RUN_PERF_TEST:-0}" = "1" ]; then
        run_performance_test
    fi

    # Summary
    echo
    if [ $failed -eq 0 ]; then
        log_info "All tests passed! ✅"
        exit 0
    else
        log_error "$failed tests failed! ❌"
        exit 1
    fi
}

# Run main function
main "$@"
