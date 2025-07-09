document.addEventListener("DOMContentLoaded", function () {
  const player = document.getElementById("video-player");

  if (!player) return;

  let lastBufferCheck = 0;
  let streamReadyRetries = 0;
  let streamReadinessCheckInProgress = false;
  const maxRetries = 15; // Maximum retries for stream readiness
  let retryInterval = null;
  const baseRetryDelay = 1000; // Base delay in milliseconds (1 second)
  const maxRetryDelay = 30000; // Maximum delay in milliseconds (30 seconds)
  let originalVideoSrc = player.src || player.querySelector("source")?.src;

  // Function to check if stream is ready
  function checkStreamReadiness() {
    if (streamReadinessCheckInProgress) {
      console.log("Stream readiness check already in progress, skipping");
      return;
    }
    
    const streamUrl = originalVideoSrc || player.src || player.currentSrc;
    if (!streamUrl) return;
    
    streamReadinessCheckInProgress = true;

    fetch(streamUrl, { method: "HEAD" })
      .then((response) => {
        if (
          response.status === 425 ||
          response.status === 503 ||
          response.status === 429
        ) {
          // Stream not ready yet - retry with exponential backoff
          streamReadyRetries++;
          if (streamReadyRetries < maxRetries) {
            // Calculate exponential backoff delay
            const delay = Math.min(
              baseRetryDelay * Math.pow(2, streamReadyRetries - 1),
              maxRetryDelay,
            );

            console.log(
              `Stream not ready (${response.status}), retrying in ${delay}ms... (${streamReadyRetries}/${maxRetries})`,
            );

            const statusMessage =
              response.status === 425
                ? "Stream preparation in progress"
                : response.status === 429
                  ? "Service temporarily overloaded"
                  : "Content being processed for streaming";

            showStreamingIndicator(
              `${statusMessage}... (${streamReadyRetries}/${maxRetries})`,
              `Downloading torrent pieces and preparing stream. This may take 30-60 seconds.`,
              delay,
            );
            setTimeout(() => {
              streamReadinessCheckInProgress = false;
              checkStreamReadiness();
            }, delay);
          } else {
            streamReadinessCheckInProgress = false;
            showError(
              "Stream Preparation Failed",
              "The stream could not be prepared after multiple attempts. The torrent may need more time to download sufficient pieces. Try refreshing the page in a few minutes.",
            );
          }
        } else if (response.ok) {
          // Stream is ready
          console.log("Stream ready, starting playback");
          streamReadinessCheckInProgress = false;
          hideStreamingIndicator();
        } else {
          // Other error
          streamReadinessCheckInProgress = false;
          showError(
            "Stream Error",
            `Server returned status: ${response.status}`,
          );
        }
      })
      .catch((error) => {
        console.error("Stream readiness check failed:", error);

        // Handle network errors with retry logic
        streamReadyRetries++;
        if (streamReadyRetries < maxRetries) {
          const delay = Math.min(
            baseRetryDelay * Math.pow(2, streamReadyRetries - 1),
            maxRetryDelay,
          );

          console.log(
            `Network error, retrying in ${delay}ms... (${streamReadyRetries}/${maxRetries})`,
          );
          showStreamingIndicator(
            `Connection error, retrying... (${streamReadyRetries}/${maxRetries})`,
            `Network issue detected. Retrying connection.`,
            delay,
          );
          setTimeout(() => {
            streamReadinessCheckInProgress = false;
            checkStreamReadiness();
          }, delay);
        } else {
          streamReadinessCheckInProgress = false;
          showError(
            "Network Error",
            "Failed to check stream readiness after multiple attempts. Please check your connection and try refreshing the page.",
          );
        }
      });
  }

  // Function to show streaming indicator
  function showStreamingIndicator(message, subtitle = "", nextRetryIn = 0) {
    const container = document.getElementById("video-container");
    const progressPercent = Math.round((streamReadyRetries / maxRetries) * 100);
    const nextRetryText =
      nextRetryIn > 0 ? `Next retry in ${Math.ceil(nextRetryIn / 1000)}s` : "";

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
                <div style="margin: 20px 0;">
                    <div style="background: #555; height: 8px; border-radius: 4px; margin: 10px 0;">
                        <div style="background: #007bff; height: 100%; width: ${progressPercent}%; border-radius: 4px; transition: width 0.3s ease;"></div>
                    </div>
                    <p style="color: #ccc; font-size: 0.9em; margin: 5px 0;">
                        Progress: ${progressPercent}% • ${nextRetryText}
                    </p>
                </div>
                <p style="color: #aaa; margin-top: 10px; font-size: 0.9em;">
                    BitTorrent streaming preparation in progress:<br>
                    • Downloading torrent pieces needed for streaming<br>
                    • Processing video format for browser compatibility<br>
                    • Stream will start automatically once ready</p>
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
    // Check if video player already exists
    let currentPlayer = document.getElementById("video-player");

    if (!currentPlayer) {
      // Recreate video element only if it doesn't exist
      container.innerHTML = `
            <video id="video-player" controls style="background: #000;">
                <source src="${originalVideoSrc}">
                Your browser does not support the video tag or this video format.
            </video>
        `;
      currentPlayer = document.getElementById("video-player");
      if (currentPlayer) {
        setupVideoEventListeners(currentPlayer);
      }
    } else {
      // Video player exists, just ensure it's visible and properly set up
      currentPlayer.style.display = "block";
      currentPlayer.src = originalVideoSrc;
    }
  }

  // Function to show error messages
  function showError(title, message) {
    const container = document.getElementById("video-container");
    container.innerHTML = `
            <div style="background: #333; padding: 40px; text-align: center; border-radius: 8px; border-left: 4px solid #dc3545;">
                <div style="color: #dc3545; font-size: 48px; margin-bottom: 20px;">⚠</div>
                <h3 style="color: #dc3545; margin-bottom: 15px;">${title}</h3>
                <p style="margin-bottom: 20px;">${message}</p>
                <button onclick="location.reload()" style="
                    background: #007bff;
                    color: white;
                    border: none;
                    padding: 10px 20px;
                    border-radius: 5px;
                    cursor: pointer;
                    margin-right: 10px;
                ">Retry</button>
                <button onclick="history.back()" style="
                    background: #6c757d;
                    color: white;
                    border: none;
                    padding: 10px 20px;
                    border-radius: 5px;
                    cursor: pointer;
                ">Go Back</button>
                <p style="color: #aaa; margin-top: 20px; font-size: 0.9em;">
                    For best compatibility, use MP4 format with H.264 video and AAC audio.
                </p>
            </div>
        `;
  }

  // Function to setup video event listeners
  function setupVideoEventListeners(videoElement) {
    // Remove existing listeners to prevent duplicates
    const newVideoElement = videoElement.cloneNode(true);
    videoElement.parentNode.replaceChild(newVideoElement, videoElement);

    // Enhanced error handling with format-specific messages
    newVideoElement.addEventListener("error", function (e) {
      console.error("Video error:", e);
      const videoSrc = newVideoElement.currentSrc || newVideoElement.src;
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
    newVideoElement.addEventListener("waiting", function () {
      console.log("Video buffering - waiting for more data");
    });

    newVideoElement.addEventListener("playing", function () {
      console.log("Video playing");
    });

    newVideoElement.addEventListener("progress", function () {
      const currentTime = Date.now();
      if (currentTime - lastBufferCheck > 1000) {
        // Check every second
        const buffered = newVideoElement.buffered;
        if (buffered.length > 0) {
          const bufferedEnd = buffered.end(buffered.length - 1);
          const currentTime = newVideoElement.currentTime;
          const bufferHealth = bufferedEnd - currentTime;

          console.log(`Buffer health: ${bufferHealth.toFixed(1)}s ahead`);

          // If buffer is getting low, we could show a loading indicator
          if (bufferHealth < 5 && !newVideoElement.paused) {
            console.log(
              "Low buffer - torrent may need to download more pieces",
            );
          }
        }
        lastBufferCheck = currentTime;
      }
    });

    newVideoElement.addEventListener("loadstart", function () {
      console.log("Video loading started");
    });

    newVideoElement.addEventListener("canplay", function () {
      console.log("Video can start playing");
    });

    // Ensure controls are enabled and clickable
    newVideoElement.addEventListener("loadedmetadata", function () {
      console.log("Video metadata loaded, controls should be responsive");
      newVideoElement.controls = true;
    });

    return newVideoElement;
  }

  // Store original src and clear it to prevent immediate loading
  if (!originalVideoSrc) {
    originalVideoSrc = player.src || player.querySelector("source")?.src;
  }

  // Clear the src to prevent browser from trying to load before stream is ready
  if (originalVideoSrc) {
    player.src = "";
    if (player.querySelector("source")) {
      player.querySelector("source").src = "";
    }
  }

  // Initial setup
  const initialPlayer = setupVideoEventListeners(player);

  // Start checking stream readiness with a small delay to allow DOM to settle
  setTimeout(() => {
    checkStreamReadiness();
  }, 100);
});
