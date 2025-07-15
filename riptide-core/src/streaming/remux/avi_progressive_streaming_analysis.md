# AVI Progressive Streaming Analysis

## Issue Identified

The user reported that AVI files show "MIME errors until 85%" completion and incorrect duration (11 minutes instead of 16 minutes). Through systematic testing, we identified the root cause.

## Root Cause

The progressive streaming system was failing because:

1. **File-based serving crashes when output file doesn't exist**: The `RemuxStreamStrategy::serve_range()` method was calling `tokio::fs::metadata()` on the output file during remuxing, but the file doesn't exist yet.

2. **Missing graceful fallback**: When the chunk server fails to serve data (which happens frequently), the system falls back to file-based serving, but this crashes if the file doesn't exist.

## The Fix

In `riptide-core/src/streaming/remux/strategy.rs`, lines 193-196:

**Before (broken):**
```rust
// Get current remux file size
let current_remux_size = tokio::fs::metadata(&output_path)
    .await
    .map_err(|e| StrategyError::RemuxingFailed {
        reason: format!("Failed to get remux file metadata: {e}"),
    })?
    .len();
```

**After (fixed):**
```rust
// Check if output file exists before trying to read it
let metadata_result = tokio::fs::metadata(&output_path).await;
if let Ok(metadata) = metadata_result {
    let current_remux_size = metadata.len();
    // ... proceed with file-based serving
} else {
    // File doesn't exist yet during remuxing - this is the core issue
    tracing::debug!(
        "Remux output file {} doesn't exist yet, remuxing still in progress",
        output_path.display()
    );
    // Fall through to return streaming not ready error
}
```

## Impact

This fix changes the behavior from:
- **Before**: Crash with "No such file or directory" → Browser sees as MIME error
- **After**: Return "Remuxing output not available yet" → Browser can handle gracefully

## Testing Results

The test `test_avi_progressive_streaming_systematic_debug` demonstrates:

1. **10% completion**: `WaitingForData` (expected)
2. **30% completion**: `Processing` → `StreamingNotReady` (now graceful)
3. **50% completion**: `Processing` → `StreamingNotReady` (now graceful)
4. **85% completion**: `Processing` → `StreamingNotReady` (now graceful)
5. **100% completion**: `Processing` → `StreamingNotReady` (still needs chunk server work)

## Next Steps

1. **Chunk Server Implementation**: The real-time remuxing with chunk servers needs to be fully implemented to provide data during the `Processing` state.

2. **Duration Preservation**: The 11-minute vs 16-minute duration issue is likely related to FFmpeg configuration for AVI files.

3. **Progressive Headers**: The HTTP response headers need to properly indicate progressive streaming to prevent browsers from showing MIME errors.

## Key Insight

The "85% completion" threshold mentioned by the user likely corresponds to when the batch remuxing process finally completes and creates the output file. Before this fix, all requests before 85% would crash with file-not-found errors, appearing as MIME errors in the browser.