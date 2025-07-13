# Progressive Streaming V2: Radically Simple Design

## Problem Statement

The current progressive streaming system is over-engineered with too many abstractions, async tasks, and state machines. This makes it:
- Hard to test in isolation
- Nearly impossible to debug
- Prone to race conditions and timing issues
- Difficult to reason about

## Core Insight

Progressive streaming is fundamentally simple:
1. Read torrent data sequentially as pieces arrive
2. Feed that data to FFmpeg's stdin
3. Serve FFmpeg's stdout as HTTP response

That's it. No complex coordination needed.

## Design Principles

1. **Synchronous Core**: The core feeding logic should be synchronous and linear
2. **Single Writer**: Only one thread writes to FFmpeg's stdin
3. **Backpressure**: Let FFmpeg's stdin buffer provide natural backpressure
4. **No Timeouts**: Never timeout while data is still arriving
5. **Clear Ownership**: Each component has a single, clear responsibility

## Architecture

```
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│                 │     │                 │     │                 │
│  DataSource     │────▶│  StreamPump     │────▶│    FFmpeg       │
│  (Torrent)      │     │  (Synchronous)  │     │    Process      │
│                 │     │                 │     │                 │
└─────────────────┘     └─────────────────┘     └─────────────────┘
                              │
                              ▼
                        ┌─────────────────┐
                        │                 │
                        │  Simple State   │
                        │  - bytes_read   │
                        │  - last_error   │
                        │                 │
                        └─────────────────┘
```

## Implementation

### 1. StreamPump (The Core)

```rust
pub struct StreamPump {
    source: Arc<dyn DataSource>,
    info_hash: InfoHash,
    file_size: u64,
    bytes_pumped: AtomicU64,
}

impl StreamPump {
    /// Synchronously pump data from source to writer
    /// This is the ENTIRE progressive streaming logic
    pub fn pump_to<W: Write>(&self, mut writer: W) -> io::Result<u64> {
        let mut offset = 0u64;
        let chunk_size = 1024 * 1024; // 1MB chunks

        while offset < self.file_size {
            // Calculate next chunk
            let end = (offset + chunk_size).min(self.file_size);

            // Try to read data
            match self.read_chunk(offset, end) {
                Ok(data) => {
                    writer.write_all(&data)?;
                    offset += data.len() as u64;
                    self.bytes_pumped.store(offset, Ordering::Relaxed);
                }
                Err(_) => {
                    // Data not available yet, wait a bit
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        }

        Ok(offset)
    }

    fn read_chunk(&self, start: u64, end: u64) -> Result<Vec<u8>, DataError> {
        // Simple blocking read with timeout
        let rt = tokio::runtime::Handle::current();
        rt.block_on(async {
            tokio::time::timeout(
                Duration::from_secs(5),
                self.source.read_range(self.info_hash, start..end)
            )
            .await
            .map_err(|_| DataError::Timeout)?
        })
    }
}
```

### 2. FFmpeg Runner (Simple Process Management)

```rust
pub struct FfmpegRunner {
    input_path: PathBuf,
    output_path: PathBuf,
}

impl FfmpegRunner {
    /// Run FFmpeg with progressive input
    /// Returns a handle to write input data
    pub fn run_progressive(&self) -> io::Result<FfmpegHandle> {
        let mut cmd = Command::new("ffmpeg");

        // Simple, proven FFmpeg args for AVI
        cmd.args(&[
            "-y",
            "-f", "avi",           // Explicit format
            "-i", "pipe:0",        // Read from stdin
            "-c:v", "libx264",     // Reliable codec
            "-c:a", "aac",         // Standard audio
            "-movflags", "frag_keyframe+empty_moov", // For streaming
            "-f", "mp4",           // Output format
            self.output_path.to_str().unwrap(),
        ]);

        cmd.stdin(Stdio::piped())
           .stdout(Stdio::null())
           .stderr(Stdio::piped());

        let mut child = cmd.spawn()?;
        let stdin = child.stdin.take().unwrap();

        Ok(FfmpegHandle {
            child,
            stdin: Some(stdin),
        })
    }
}

pub struct FfmpegHandle {
    child: Child,
    stdin: Option<ChildStdin>,
}

impl FfmpegHandle {
    /// Get the stdin to write data
    pub fn stdin(&mut self) -> Option<ChildStdin> {
        self.stdin.take()
    }

    /// Wait for FFmpeg to complete
    pub fn wait(mut self) -> io::Result<ExitStatus> {
        self.child.wait()
    }
}
```

### 3. Progressive Streaming Service (Thin Coordination Layer)

```rust
pub struct ProgressiveStreamer {
    pump: StreamPump,
    runner: FfmpegRunner,
}

impl ProgressiveStreamer {
    /// Start progressive streaming in a background thread
    pub fn start(&self) -> JoinHandle<Result<(), StreamError>> {
        let pump = self.pump.clone();
        let runner = self.runner.clone();

        thread::spawn(move || {
            // Start FFmpeg
            let mut handle = runner.run_progressive()
                .map_err(|e| StreamError::FfmpegStart(e))?;

            // Get stdin handle
            let stdin = handle.stdin()
                .ok_or(StreamError::NoStdin)?;

            // Pump data (this blocks until complete or error)
            pump.pump_to(stdin)
                .map_err(|e| StreamError::PumpFailed(e))?;

            // Wait for FFmpeg to finish
            let status = handle.wait()
                .map_err(|e| StreamError::FfmpegWait(e))?;

            if !status.success() {
                return Err(StreamError::FfmpegFailed(status));
            }

            Ok(())
        })
    }

    /// Check if output is ready for streaming
    pub fn is_ready(&self) -> bool {
        // Simple check: has initial MP4 header
        self.check_mp4_header()
    }

    fn check_mp4_header(&self) -> bool {
        // Check if we have ftyp + moov atoms (usually ~1KB)
        std::fs::metadata(&self.runner.output_path)
            .map(|m| m.len() > 1024)
            .unwrap_or(false)
    }
}
```

## Testing Strategy

Each component is independently testable:

1. **StreamPump**: Test with mock DataSource that controls data availability
2. **FfmpegRunner**: Test with known input files
3. **ProgressiveStreamer**: Integration test with controlled data flow

Example test:

```rust
#[test]
fn test_pump_with_delayed_data() {
    let mock_source = MockDataSource::new()
        .with_delay(0..1024*1024, Duration::from_millis(100))
        .with_data(1024*1024..2048*1024, vec![0xFF; 1024*1024]);

    let pump = StreamPump::new(mock_source, info_hash, 2048*1024);
    let mut output = Vec::new();

    let bytes = pump.pump_to(&mut output).unwrap();
    assert_eq!(bytes, 2048*1024);
    assert_eq!(output.len(), 2048*1024);
}
```

## Benefits

1. **Debuggable**: Linear flow, easy to add logging at each step
2. **Testable**: Each component has a single responsibility
3. **Robust**: No complex async coordination, no race conditions
4. **Simple**: The entire core logic fits in ~50 lines
5. **Flexible**: Easy to add features like progress tracking, bandwidth limiting

## Migration Path

1. Keep existing system in place
2. Implement `progressive_v2` module alongside
3. Add feature flag to switch between implementations
4. Test thoroughly with real torrents
5. Remove old implementation once stable

## Open Questions

1. Should we add bandwidth throttling in StreamPump?
2. Do we need to handle seeking for partial content requests?
3. Should pump_to be async for better tokio integration?

## Conclusion

By embracing simplicity and avoiding premature abstraction, we can build a progressive streaming system that actually works and is maintainable. The key insight is that **FFmpeg's stdin provides all the coordination we need** - we don't need to build our own complex state machine on top.
