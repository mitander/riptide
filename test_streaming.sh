#!/bin/bash
#
# Comprehensive streaming test runner for Riptide
# This script tests the streaming functionality end-to-end without manual browser interaction
#
# Usage: ./test_streaming.sh [options]
# Options:
#   --skip-build      Skip building the project
#   --keep-server     Keep server running after tests
#   --verbose         Enable verbose logging
#   --test-dir PATH   Directory containing test video files

set -euo pipefail

# Configuration
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR" && pwd)"
TEST_DIR="${TEST_DIR:-/tmp/riptide-test-videos}"
CACHE_DIR="/tmp/riptide-remux-cache"
SERVER_PORT="${SERVER_PORT:-3000}"
SERVER_URL="http://127.0.0.1:${SERVER_PORT}"
LOG_DIR="${LOG_DIR:-/tmp/riptide-test-logs}"
RUST_LOG="${RUST_LOG:-riptide_web::streaming=debug,riptide_core::streaming=debug}"

# Command line options
SKIP_BUILD=0
KEEP_SERVER=0
VERBOSE=0

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Parse command line arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --skip-build)
            SKIP_BUILD=1
            shift
            ;;
        --keep-server)
            KEEP_SERVER=1
            shift
            ;;
        --verbose)
            VERBOSE=1
            shift
            ;;
        --test-dir)
            TEST_DIR="$2"
            shift 2
            ;;
        *)
            echo "Unknown option: $1"
            echo "Usage: $0 [--skip-build] [--keep-server] [--verbose] [--test-dir PATH]"
            exit 1
            ;;
    esac
done

# Logging functions
log_info() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

log_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

log_warning() {
    echo -e "${YELLOW}[WARNING]${NC} $1"
}

# Cleanup function
cleanup() {
    local exit_code=$?

    if [[ $KEEP_SERVER -eq 0 ]]; then
        log_info "Cleaning up..."

        # Kill server if running
        if [[ -n "${SERVER_PID:-}" ]] && kill -0 "$SERVER_PID" 2>/dev/null; then
            log_info "Stopping server (PID: $SERVER_PID)"
            kill "$SERVER_PID" 2>/dev/null || true
            wait "$SERVER_PID" 2>/dev/null || true
        fi
    else
        log_info "Server kept running at PID: ${SERVER_PID:-unknown}"
    fi

    # Clean up test files unless explicitly kept
    if [[ "${KEEP_TEST_FILES:-0}" -eq 0 ]]; then
        rm -rf "$TEST_DIR" 2>/dev/null || true
    fi

    exit $exit_code
}

trap cleanup EXIT INT TERM

# Check dependencies
check_dependencies() {
    log_info "Checking dependencies..."

    local missing_deps=0

    # Check for required commands
    for cmd in cargo ffmpeg ffprobe python3 curl; do
        if ! command -v "$cmd" &> /dev/null; then
            log_error "$cmd is not installed"
            ((missing_deps++))
        fi
    done

    # Check Python packages
    if command -v python3 &> /dev/null; then
        if ! python3 -c "import requests" 2>/dev/null; then
            log_warning "Python 'requests' module not installed. Installing..."
            pip3 install requests || {
                log_error "Failed to install Python requests module"
                ((missing_deps++))
            }
        fi
    fi

    # Check FFmpeg version
    if command -v ffmpeg &> /dev/null; then
        local ffmpeg_version=$(ffmpeg -version | head -1)
        log_info "FFmpeg version: $ffmpeg_version"
    fi

    if [[ $missing_deps -gt 0 ]]; then
        log_error "Missing $missing_deps dependencies. Please install them first."
        exit 1
    fi

    log_success "All dependencies satisfied"
}

# Build the project
build_project() {
    if [[ $SKIP_BUILD -eq 1 ]]; then
        log_info "Skipping build (--skip-build specified)"
        return 0
    fi

    log_info "Building Riptide..."

    cd "$PROJECT_ROOT"

    if [[ $VERBOSE -eq 1 ]]; then
        cargo build --release --bin riptide
    else
        cargo build --release --bin riptide 2>&1 | grep -E "(Compiling|Finished)" || true
    fi

    if [[ ! -f "target/release/riptide" ]]; then
        log_error "Build failed - riptide binary not found"
        exit 1
    fi

    log_success "Build completed"
}

# Create test video files
create_test_videos() {
    log_info "Creating test video files..."

    mkdir -p "$TEST_DIR"
    cd "$TEST_DIR"

    # Create test AVI file
    log_info "Creating test AVI file..."
    ffmpeg -f lavfi -i testsrc2=duration=10:size=640x480:rate=25 \
           -f lavfi -i sine=frequency=1000:duration=10 \
           -c:v mpeg4 -c:a mp3 -b:v 2M -b:a 128k \
           -y "test-avi-file.avi" 2>/dev/null || {
        log_error "Failed to create AVI test file"
        return 1
    }

    # Create test MKV file
    log_info "Creating test MKV file..."
    ffmpeg -f lavfi -i testsrc2=duration=15:size=1280x720:rate=30 \
           -f lavfi -i sine=frequency=800:duration=15 \
           -c:v libx264 -preset fast -c:a aac -b:v 3M -b:a 192k \
           -y "test-mkv-file.mkv" 2>/dev/null || {
        log_error "Failed to create MKV test file"
        return 1
    }

    # Create test MP4 file for comparison
    log_info "Creating test MP4 file..."
    ffmpeg -f lavfi -i testsrc2=duration=10:size=640x480:rate=25 \
           -f lavfi -i sine=frequency=600:duration=10 \
           -c:v libx264 -preset fast -c:a aac -b:v 2M -b:a 128k \
           -movflags +faststart \
           -y "test-mp4-file.mp4" 2>/dev/null || {
        log_error "Failed to create MP4 test file"
        return 1
    }

    log_success "Test videos created in $TEST_DIR"
    ls -la "$TEST_DIR"
}

# Start the Riptide server
start_server() {
    log_info "Starting Riptide server..."

    # Create log directory
    mkdir -p "$LOG_DIR"

    # Clear cache directory
    rm -rf "$CACHE_DIR" 2>/dev/null || true

    # Start server in development mode with test directory
    cd "$PROJECT_ROOT"

    RUST_LOG="$RUST_LOG" \
    target/release/riptide server \
        --mode dev \
        --port "$SERVER_PORT" \
        --movies-dir "$TEST_DIR" \
        > "$LOG_DIR/server.log" 2>&1 &

    SERVER_PID=$!

    log_info "Server started with PID: $SERVER_PID"
    log_info "Server logs: $LOG_DIR/server.log"

    # Wait for server to be ready
    local max_attempts=30
    local attempt=0

    while [[ $attempt -lt $max_attempts ]]; do
        if curl -s "$SERVER_URL/health" >/dev/null 2>&1 || \
           curl -s "$SERVER_URL/" >/dev/null 2>&1; then
            log_success "Server is ready"
            return 0
        fi

        # Check if server process is still running
        if ! kill -0 "$SERVER_PID" 2>/dev/null; then
            log_error "Server process died unexpectedly"
            tail -20 "$LOG_DIR/server.log"
            return 1
        fi

        sleep 1
        ((attempt++))
    done

    log_error "Server failed to start within 30 seconds"
    tail -20 "$LOG_DIR/server.log"
    return 1
}

# Run unit tests
run_unit_tests() {
    log_info "Running unit tests..."

    cd "$PROJECT_ROOT"

    if [[ $VERBOSE -eq 1 ]]; then
        cargo test --workspace streaming -- --nocapture
    else
        cargo test --workspace streaming 2>&1 | grep -E "(test result:|passed)" || true
    fi

    log_success "Unit tests completed"
}

# Run integration tests
run_integration_tests() {
    log_info "Running integration tests..."

    # First, get the list of torrents from the server
    local torrents_response=$(curl -s "$SERVER_URL/api/torrents" || echo "[]")

    if [[ "$torrents_response" == "[]" ]]; then
        log_warning "No torrents found via API, checking HTML endpoint..."
        # Try to extract info hashes from HTML (simplified)
        curl -s "$SERVER_URL/torrents" | grep -oE '[0-9a-f]{40}' | head -5 > "$LOG_DIR/info_hashes.txt" || true
    fi

    # Run Python integration tests
    if [[ -f "$PROJECT_ROOT/tests/integration/test_streaming_http.py" ]]; then
        log_info "Running HTTP streaming tests..."

        python3 "$PROJECT_ROOT/tests/integration/test_streaming_http.py" \
            --server-url "$SERVER_URL" \
            $(if [[ $VERBOSE -eq 1 ]]; then echo "--verbose"; fi) \
            > "$LOG_DIR/http_tests.log" 2>&1 || {
            log_error "HTTP streaming tests failed"
            tail -20 "$LOG_DIR/http_tests.log"
            return 1
        }

        log_success "HTTP streaming tests passed"
    fi

    # Run FFmpeg validation tests
    if [[ -f "$PROJECT_ROOT/tests/integration/test_remux.sh" ]]; then
        log_info "Running FFmpeg remux tests..."

        "$PROJECT_ROOT/tests/integration/test_remux.sh" > "$LOG_DIR/remux_tests.log" 2>&1 || {
            log_error "Remux tests failed"
            tail -20 "$LOG_DIR/remux_tests.log"
            return 1
        }

        log_success "Remux tests passed"
    fi
}

# Monitor cache directory
monitor_cache() {
    log_info "Monitoring cache directory..."

    if [[ -d "$CACHE_DIR" ]]; then
        log_info "Cache directory contents:"
        ls -la "$CACHE_DIR" 2>/dev/null || log_warning "Cache directory is empty"

        # Validate cached MP4 files
        for mp4_file in "$CACHE_DIR"/*.mp4; do
            if [[ -f "$mp4_file" ]]; then
                local size=$(stat -f%z "$mp4_file" 2>/dev/null || stat -c%s "$mp4_file" 2>/dev/null)
                log_info "Cached file: $(basename "$mp4_file") - Size: $((size / 1024 / 1024)) MB"

                # Quick validation with ffprobe
                if ffprobe -v error -show_format "$mp4_file" >/dev/null 2>&1; then
                    log_success "Valid MP4: $(basename "$mp4_file")"
                else
                    log_error "Invalid MP4: $(basename "$mp4_file")"
                fi
            fi
        done
    else
        log_warning "Cache directory not found at $CACHE_DIR"
    fi
}

# Check server logs for errors
check_server_logs() {
    log_info "Checking server logs for errors..."

    if [[ -f "$LOG_DIR/server.log" ]]; then
        local error_count=$(grep -iE "(error|panic|failed)" "$LOG_DIR/server.log" | wc -l)
        local warning_count=$(grep -iE "warn" "$LOG_DIR/server.log" | wc -l)

        if [[ $error_count -gt 0 ]]; then
            log_warning "Found $error_count errors in server logs"
            if [[ $VERBOSE -eq 1 ]]; then
                grep -iE "(error|panic|failed)" "$LOG_DIR/server.log" | tail -10
            fi
        fi

        if [[ $warning_count -gt 0 ]]; then
            log_info "Found $warning_count warnings in server logs"
        fi

        # Check for successful remux operations
        local remux_success=$(grep -c "Successfully remuxed to MP4" "$LOG_DIR/server.log" || echo "0")
        if [[ $remux_success -gt 0 ]]; then
            log_success "Found $remux_success successful remux operations"
        fi
    fi
}

# Generate test report
generate_report() {
    log_info "Generating test report..."

    local report_file="$LOG_DIR/test_report.txt"

    {
        echo "Riptide Streaming Test Report"
        echo "============================="
        echo "Date: $(date)"
        echo "Server URL: $SERVER_URL"
        echo "Test Directory: $TEST_DIR"
        echo "Cache Directory: $CACHE_DIR"
        echo ""
        echo "Test Results:"
        echo "-------------"

        # Add test results here
        if [[ -f "$LOG_DIR/http_tests.log" ]]; then
            echo "HTTP Tests: $(grep -c SUCCESS "$LOG_DIR/http_tests.log" || echo 0) passed"
        fi

        if [[ -f "$LOG_DIR/remux_tests.log" ]]; then
            echo "Remux Tests: $(grep -c "tests passed" "$LOG_DIR/remux_tests.log" || echo 0) passed"
        fi

        echo ""
        echo "Cache Statistics:"
        echo "-----------------"
        if [[ -d "$CACHE_DIR" ]]; then
            echo "MP4 files: $(find "$CACHE_DIR" -name "*.mp4" | wc -l)"
            echo "Lock files: $(find "$CACHE_DIR" -name "*.lock" | wc -l)"
            echo "Total size: $(du -sh "$CACHE_DIR" | cut -f1)"
        else
            echo "Cache directory not found"
        fi

    } > "$report_file"

    log_success "Test report saved to: $report_file"

    if [[ $VERBOSE -eq 1 ]]; then
        cat "$report_file"
    fi
}

# Main test execution
main() {
    log_info "Starting Riptide streaming tests"
    log_info "Test directory: $TEST_DIR"
    log_info "Cache directory: $CACHE_DIR"
    log_info "Log directory: $LOG_DIR"
    echo

    # Check dependencies
    check_dependencies

    # Build project
    build_project

    # Create test videos
    create_test_videos

    # Run unit tests first
    run_unit_tests

    # Start server
    if ! start_server; then
        log_error "Failed to start server"
        exit 1
    fi

    # Give server time to initialize
    sleep 2

    # Run integration tests
    run_integration_tests

    # Monitor cache
    monitor_cache

    # Check server logs
    check_server_logs

    # Generate report
    generate_report

    echo
    log_success "All streaming tests completed! ✅"
    log_info "Logs saved to: $LOG_DIR"

    if [[ $KEEP_SERVER -eq 1 ]]; then
        log_info "Server is still running at $SERVER_URL (PID: $SERVER_PID)"
        log_info "Stop it with: kill $SERVER_PID"
    fi
}

# Run main function
main
