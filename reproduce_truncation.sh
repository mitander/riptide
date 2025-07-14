#!/bin/bash

# Automated reproduction script for progressive streaming truncation issue
# This script replicates the exact browser behavior to identify where and why
# progressive streaming stops serving the full movie length.

set -e

BASE_URL="http://127.0.0.1:3000"
LOG_FILE="truncation_test.log"
RESULTS_FILE="truncation_results.json"

echo "🎬 PROGRESSIVE STREAMING TRUNCATION REPRODUCTION"
echo "================================================================"

# Function to cleanup background processes
cleanup() {
    if [[ -n "$SERVER_PID" ]]; then
        echo "🔄 Stopping server (PID: $SERVER_PID)..."
        kill $SERVER_PID 2>/dev/null || true
        wait $SERVER_PID 2>/dev/null || true
    fi
}

# Setup cleanup trap
trap cleanup EXIT

# Start the server
echo "🚀 Starting Riptide server..."
cd /Users/mitander/c/p/riptide
cargo run > "$LOG_FILE" 2>&1 &
SERVER_PID=$!

echo "⏳ Waiting for server to start..."
for i in {1..30}; do
    if curl -s "$BASE_URL/api/torrents" > /dev/null 2>&1; then
        echo "✅ Server is responding (attempt $i)"
        sleep 2  # Additional stabilization
        break
    fi
    if [[ $i -eq 30 ]]; then
        echo "❌ Server failed to start within 30 seconds"
        exit 1
    fi
    sleep 1
done

# Get torrent info
echo "📁 Getting torrent information..."
TORRENT_RESPONSE=$(curl -s "$BASE_URL/api/torrents")
echo "Response: $TORRENT_RESPONSE"

# Extract info_hash (handle both array and object responses)
INFO_HASH=$(echo "$TORRENT_RESPONSE" | python3 -c "
import sys, json
data = json.load(sys.stdin)
if isinstance(data, list):
    torrents = data
elif isinstance(data, dict) and 'torrents' in data:
    torrents = data['torrents']
else:
    torrents = []

if torrents:
    print(torrents[0]['info_hash'])
else:
    sys.exit(1)
" 2>/dev/null || echo "")

if [[ -z "$INFO_HASH" ]]; then
    echo "❌ Could not extract info_hash from response"
    exit 1
fi

echo "🔗 Info hash: $INFO_HASH"

# Stream URL
STREAM_URL="$BASE_URL/stream/$INFO_HASH"

# Make initial HEAD request
echo "📡 Making HEAD request..."
HEAD_RESPONSE=$(curl -I -s "$STREAM_URL")
echo "HEAD Response:"
echo "$HEAD_RESPONSE"

# Make initial range request
echo "📡 Making initial range request (bytes=0-)..."
INITIAL_RESPONSE=$(curl -I -s -H "Range: bytes=0-" "$STREAM_URL")
echo "Initial Range Response:"
echo "$INITIAL_RESPONSE"

# Extract Content-Range to get initial size
CONTENT_RANGE=$(echo "$INITIAL_RESPONSE" | grep -i "content-range:" | cut -d' ' -f2- | tr -d '\r\n')
echo "📊 Content-Range: $CONTENT_RANGE"

# Monitor progressive growth
echo "⏱️  Monitoring progressive streaming growth..."
echo "📈 Tracking stream size over time:"

START_TIME=$(date +%s)
LAST_SIZE=0
NO_GROWTH_COUNT=0
MAX_NO_GROWTH=10
CHECK_INTERVAL=5
MAX_DURATION=300  # 5 minutes

# Create results file header
echo '{"test_start": "'$(date -Iseconds)'", "original_file_size": 135046574, "measurements": [' > "$RESULTS_FILE"
FIRST_MEASUREMENT=true

while true; do
    CURRENT_TIME=$(date +%s)
    ELAPSED=$((CURRENT_TIME - START_TIME))
    
    # Stop if we've run for too long
    if [[ $ELAPSED -gt $MAX_DURATION ]]; then
        echo "⏰ Stopping after ${MAX_DURATION} seconds"
        break
    fi
    
    # Make range request from current position
    RANGE_RESPONSE=$(curl -I -s -H "Range: bytes=${LAST_SIZE}-" "$STREAM_URL" 2>/dev/null || echo "")
    
    if [[ -n "$RANGE_RESPONSE" ]]; then
        # Extract current size from Content-Range
        NEW_CONTENT_RANGE=$(echo "$RANGE_RESPONSE" | grep -i "content-range:" | cut -d' ' -f2- | tr -d '\r\n' || echo "")
        
        if [[ -n "$NEW_CONTENT_RANGE" && "$NEW_CONTENT_RANGE" =~ bytes\ ([0-9]+)-([0-9]+)/ ]]; then
            END_BYTE=${BASH_REMATCH[2]}
            CURRENT_SIZE=$((END_BYTE + 1))
            
            if [[ $CURRENT_SIZE -gt $LAST_SIZE ]]; then
                GROWTH=$((CURRENT_SIZE - LAST_SIZE))
                printf "%4ds: %10s bytes (+%s)\n" "$ELAPSED" "$(printf "%'d" "$CURRENT_SIZE")" "$(printf "%'d" "$GROWTH")"
                
                # Add measurement to results file
                if [[ "$FIRST_MEASUREMENT" == "true" ]]; then
                    FIRST_MEASUREMENT=false
                else
                    echo "," >> "$RESULTS_FILE"
                fi
                echo "  {\"elapsed_seconds\": $ELAPSED, \"stream_size_bytes\": $CURRENT_SIZE, \"growth_bytes\": $GROWTH}" >> "$RESULTS_FILE"
                
                LAST_SIZE=$CURRENT_SIZE
                NO_GROWTH_COUNT=0
            else
                NO_GROWTH_COUNT=$((NO_GROWTH_COUNT + 1))
                if [[ $NO_GROWTH_COUNT -le 3 ]]; then
                    printf "%4ds: %10s bytes (no growth)\n" "$ELAPSED" "$(printf "%'d" "$CURRENT_SIZE")"
                elif [[ $NO_GROWTH_COUNT -eq 4 ]]; then
                    echo "   ...continuing to monitor..."
                fi
                
                if [[ $NO_GROWTH_COUNT -ge $MAX_NO_GROWTH ]]; then
                    echo "🛑 Stopping: No growth for $((MAX_NO_GROWTH * CHECK_INTERVAL)) seconds"
                    break
                fi
            fi
        else
            echo "⚠️  Could not parse Content-Range: '$NEW_CONTENT_RANGE'"
        fi
    else
        echo "⚠️  Failed to get response"
    fi
    
    sleep $CHECK_INTERVAL
done

# Close results file
echo "]}" >> "$RESULTS_FILE"

# Analysis
echo ""
echo "📊 ANALYSIS RESULTS:"
echo "================================================================"

ORIGINAL_SIZE=135046574  # From server logs
PERCENTAGE=$(echo "scale=1; ($LAST_SIZE * 100) / $ORIGINAL_SIZE" | bc)

echo "Original file size: $(printf "%'d" "$ORIGINAL_SIZE") bytes ($(echo "scale=1; $ORIGINAL_SIZE / 1024 / 1024" | bc) MB)"
echo "Final stream size:  $(printf "%'d" "$LAST_SIZE") bytes ($(echo "scale=1; $LAST_SIZE / 1024 / 1024" | bc) MB)"
echo "Percentage served:  ${PERCENTAGE}%"

# Check for truncation
if (( $(echo "$PERCENTAGE < 95" | bc -l) )); then
    echo "❌ TRUNCATION DETECTED: Only ${PERCENTAGE}% of file was served"
    
    # Calculate estimated served duration (original is 16min 20s = 980 seconds)
    ORIGINAL_DURATION=980
    SERVED_DURATION=$(echo "scale=0; ($PERCENTAGE / 100) * $ORIGINAL_DURATION" | bc)
    SERVED_MINUTES=$((SERVED_DURATION / 60))
    SERVED_SECONDS=$((SERVED_DURATION % 60))
    printf "📺 Estimated served duration: %d:%02d\n" "$SERVED_MINUTES" "$SERVED_SECONDS"
else
    echo "✅ Full file appears to be served"
fi

echo ""
echo "📄 Detailed results saved to: $RESULTS_FILE"
echo "📄 Server logs saved to: $LOG_FILE"

# Show the most relevant part of the server log
echo ""
echo "🔍 Key server log entries:"
if [[ -f "$LOG_FILE" ]]; then
    grep -E "(Duration:|Final file size|Partial fragmented MP4|FFmpeg.*completed)" "$LOG_FILE" | tail -10
fi