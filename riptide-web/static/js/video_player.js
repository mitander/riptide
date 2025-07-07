document.addEventListener("DOMContentLoaded", function () {
  const player = document.getElementById("video-player");

  if (!player) return;

  let lastBufferCheck = 0;
  let streamReadyRetries = 0;
  const maxRetries = 30; // Maximum retries for stream readiness
  let retryInterval = null;

  // Function to check if stream is ready
  function checkStreamReadiness() {
    const streamUrl = player.src || player.currentSrc;
    if (!streamUrl) return;

    fetch(streamUrl, { method: "HEAD" })
      .then((response) => {
        if (response.status === 425) {
          // Stream not ready yet - retry
          streamReadyRetries++;
          if (streamReadyRetries < maxRetries) {
            console.log(
              `Stream not ready, retrying... (${streamReadyRetries}/${maxRetries})`,
            );

            // Determine file type and required percentage
            const streamUrl = player.src || player.currentSrc;
            const isAvi = streamUrl.includes(".avi");
            const isMkv = streamUrl.includes(".mkv");

            let requiredPercent = "10%";
            let estimatedTime = "1-2 minutes";

            if (isAvi) {
              requiredPercent = "70%";
              estimatedTime = "5-10 minutes";
            } else if (isMkv) {
              requiredPercent = "15%";
              estimatedTime = "2-3 minutes";
            }

            showStreamingIndicator(
              `Downloading ${requiredPercent} of file for conversion... (${streamReadyRetries}/${maxRetries})`,
              `Estimated time: ${estimatedTime}`,
            );
            setTimeout(checkStreamReadiness, 3000); // Retry every 3 seconds for longer waits
          } else {
            showError(
              "Stream Preparation Failed",
              "The stream could not be prepared after multiple attempts. The file may need more time to download. Try refreshing the page.",
            );
          }
        } else if (response.ok) {
          // Stream is ready
          console.log("Stream ready, starting playback");
          hideStreamingIndicator();
          player.load(); // Reload the video element
        } else {
          // Other error
          showError(
            "Stream Error",
            `Server returned status: ${response.status}`,
          );
        }
      })
      .catch((error) => {
        console.error("Stream readiness check failed:", error);
        showError(
          "Network Error",
          "Failed to check stream readiness. Please check your connection.",
        );
      });
  }

  // Function to show streaming indicator
  function showStreamingIndicator(message, subtitle = "") {
    const container = document.getElementById("video-container");
    container.innerHTML = `
            <div style="background: #333; padding: 40px; text-align: center; border-radius: 8px;">
                <div class="spinner" style="
                    border: 4px solid #f3f3f3;
                    border-top: 4px solid #007bff;
                    border-radius: 50%;
                    width: 40px;
                    height: 40px;
                    animation: spin 1s linear infinite;
                    margin: 0 auto 20px;
                "></div>
                <h3>Preparing Stream</h3>
                <p>${message}</p>
                ${subtitle ? `<p style="color: #007bff; margin-top: 10px;">${subtitle}</p>` : ""}
                <p style="color: #aaa; margin-top: 10px; font-size: 0.9em;">
                    Different file formats require different amounts of data:<br>
                    • AVI files: 70% download required<br>
                    • MKV files: 15% download required<br>
                    • Other formats: 10% download required
                </p>
            </div>
            <style>
                @keyframes spin {
                    0% { transform: rotate(0deg); }
                    100% { transform: rotate(360deg); }
                }
            </style>
        `;
  }

  // Function to hide streaming indicator and restore video player
  function hideStreamingIndicator() {
    const container = document.getElementById("video-container");
    container.innerHTML = `
            <video id="video-player" controls style="background: #000;">
                <source src="${player.src}">
                Your browser does not support the video tag or this video format.
            </video>
        `;
    // Re-get the player element after DOM update
    const newPlayer = document.getElementById("video-player");
    if (newPlayer) {
      setupVideoEventListeners(newPlayer);
    }
  }

  // Function to show error messages
  function showError(title, message) {
    const container = document.getElementById("video-container");
    container.innerHTML = `
            <div style="background: #333; padding: 40px; text-align: center; border-radius: 8px;">
                <h3>${title}</h3>
                <p>${message}</p>
                <p style="color: #aaa; margin-top: 10px;">
                    For best compatibility, use MP4 format with H.264 video and AAC audio.
                </p>
            </div>
        `;
  }

  // Function to setup video event listeners
  function setupVideoEventListeners(videoElement) {
    // Enhanced error handling with format-specific messages
    videoElement.addEventListener("error", function (e) {
      console.error("Video error:", e);
      const videoSrc = videoElement.currentSrc || videoElement.src;
      const isAvi = videoSrc.includes(".avi");
      const isMkv = videoSrc.includes(".mkv");

      let message = "Video Playback Error";
      let details = "The video format may not be supported by your browser.";

      if (isAvi) {
        message = "AVI Format Not Supported";
        details =
          "AVI files cannot be played directly in browsers. Please convert to MP4 format.";
      } else if (isMkv) {
        message = "MKV Format Limited Support";
        details =
          "MKV files have limited browser support. MP4 or WebM work best.";
      }

      // Check if this is a 425 error by trying to load the stream
      checkStreamReadiness();
    });

    // Streaming buffer management
    videoElement.addEventListener("waiting", function () {
      console.log("Video buffering - waiting for more data");
    });

    videoElement.addEventListener("playing", function () {
      console.log("Video playing");
    });

    videoElement.addEventListener("progress", function () {
      const currentTime = Date.now();
      if (currentTime - lastBufferCheck > 1000) {
        // Check every second
        const buffered = videoElement.buffered;
        if (buffered.length > 0) {
          const bufferedEnd = buffered.end(buffered.length - 1);
          const currentTime = videoElement.currentTime;
          const bufferHealth = bufferedEnd - currentTime;

          console.log(`Buffer health: ${bufferHealth.toFixed(1)}s ahead`);

          // If buffer is getting low, we could show a loading indicator
          if (bufferHealth < 5 && !videoElement.paused) {
            console.log(
              "Low buffer - torrent may need to download more pieces",
            );
          }
        }
        lastBufferCheck = currentTime;
      }
    });

    videoElement.addEventListener("loadstart", function () {
      console.log("Video loading started");
    });

    videoElement.addEventListener("canplay", function () {
      console.log("Video can start playing");
    });
  }

  // Initial setup
  setupVideoEventListeners(player);

  // Start checking stream readiness immediately
  checkStreamReadiness();
});
