//! Enhanced video player with streaming readiness integration
//!
//! Uses the new readiness API to provide better UX during stream preparation

document.addEventListener("DOMContentLoaded", function () {
  const player = document.getElementById("video-player");
  if (!player) {
    console.warn("Video player element not found");
    return;
  }

  // Get info hash from data attribute or URL
  const infoHash = player.dataset.infoHash || extractInfoHashFromUrl();
  if (!infoHash) {
    console.error("Info hash not found");
    showError("Configuration error: Video identifier not found");
    return;
  }

  console.log("Initializing video player for:", infoHash);

  // Initialize enhanced video player
  const enhancedPlayer = new EnhancedVideoPlayer(player, infoHash);

  // Store globally for debugging
  window.riptidePlayer = enhancedPlayer;

  // Start loading process
  enhancedPlayer.loadVideo();
});

/**
 * Enhanced video player with readiness checking
 */
class EnhancedVideoPlayer {
  constructor(videoElement, infoHash) {
    this.video = videoElement;
    this.infoHash = infoHash;
    this.statusOverlay = null;
    this.isLoading = false;
    this.loadAttempts = 0;
    this.maxLoadAttempts = 3;

    this.setupStatusOverlay();
    this.setupVideoEventListeners();
  }

  /**
   * Setup status overlay for loading states
   */
  setupStatusOverlay() {
    const container = this.video.parentElement;
    if (!container) return;

    // Ensure container is relatively positioned
    if (getComputedStyle(container).position === "static") {
      container.style.position = "relative";
    }

    // Create overlay
    this.statusOverlay = document.createElement("div");
    this.statusOverlay.className = "streaming-status-overlay";
    this.statusOverlay.innerHTML = `
            <div class="status-content">
                <div class="status-icon">
                    <svg class="spinner" width="48" height="48" viewBox="0 0 24 24">
                        <circle cx="12" cy="12" r="10" stroke="currentColor" stroke-width="2"
                                fill="none" stroke-linecap="round" stroke-dasharray="32" stroke-dashoffset="32">
                            <animateTransform attributeName="transform" type="rotate" dur="1s"
                                            values="0 12 12;360 12 12" repeatCount="indefinite"/>
                            <animate attributeName="stroke-dashoffset" dur="1s"
                                   values="32;0;32" repeatCount="indefinite"/>
                        </circle>
                    </svg>
                </div>
                <div class="status-message">Checking stream readiness...</div>
                <div class="status-progress">
                    <div class="progress-bar">
                        <div class="progress-fill"></div>
                    </div>
                    <div class="progress-text">0%</div>
                </div>
                <div class="status-details"></div>
            </div>
        `;

    // Add CSS styles
    this.statusOverlay.style.cssText = `
            position: absolute;
            top: 0;
            left: 0;
            right: 0;
            bottom: 0;
            background: rgba(0, 0, 0, 0.9);
            display: flex;
            align-items: center;
            justify-content: center;
            z-index: 1000;
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
            color: white;
        `;

    // Add internal styles
    const style = document.createElement("style");
    style.textContent = `
            .status-content {
                text-align: center;
                max-width: 400px;
                padding: 20px;
            }
            .status-icon {
                margin-bottom: 20px;
            }
            .spinner {
                color: #007bff;
            }
            .status-message {
                font-size: 18px;
                font-weight: 500;
                margin-bottom: 20px;
                line-height: 1.4;
            }
            .progress-bar {
                width: 300px;
                height: 6px;
                background: rgba(255, 255, 255, 0.2);
                border-radius: 3px;
                margin: 0 auto 10px;
                overflow: hidden;
            }
            .progress-fill {
                height: 100%;
                background: linear-gradient(90deg, #007bff, #0056b3);
                width: 0%;
                transition: width 0.3s ease;
                border-radius: 3px;
            }
            .progress-text {
                font-size: 14px;
                color: rgba(255, 255, 255, 0.8);
                margin-bottom: 10px;
            }
            .status-details {
                font-size: 14px;
                color: rgba(255, 255, 255, 0.7);
                line-height: 1.3;
            }
            .status-error {
                color: #dc3545;
            }
            .status-error .spinner {
                color: #dc3545;
            }
        `;
    document.head.appendChild(style);

    container.appendChild(this.statusOverlay);
  }

  /**
   * Setup video event listeners
   */
  setupVideoEventListeners() {
    this.video.addEventListener("loadstart", () => {
      console.log("Video loading started");
    });

    this.video.addEventListener("canplay", () => {
      console.log("Video can start playing");
      this.hideStatusOverlay();
    });

    this.video.addEventListener("error", (event) => {
      console.error("Video error:", event);
      const error = this.video.error;
      if (error) {
        this.handleVideoError(error);
      }
    });

    this.video.addEventListener("stalled", () => {
      console.warn("Video stalled");
      this.showStatus("Buffering...", null, "Video is buffering data");
    });

    this.video.addEventListener("waiting", () => {
      console.log("Video waiting for data");
    });

    this.video.addEventListener("playing", () => {
      console.log("Video started playing");
      this.hideStatusOverlay();
    });
  }

  /**
   * Start the video loading process
   */
  async loadVideo() {
    if (this.isLoading) return;

    this.isLoading = true;
    this.loadAttempts++;

    console.log(`Starting video load attempt ${this.loadAttempts}`);

    this.showStatus(
      "Checking stream readiness...",
      0,
      "Connecting to streaming service",
    );

    try {
      // Check if stream is ready
      const readiness = await this.checkStreamReadiness();

      if (readiness.ready) {
        this.startVideoPlayback();
      } else {
        await this.waitForStreamReady(readiness);
      }
    } catch (error) {
      console.error("Failed to load video:", error);
      this.handleLoadError(error);
    }
  }

  /**
   * Check current stream readiness
   */
  async checkStreamReadiness() {
    try {
      const response = await fetch(`/stream/${this.infoHash}/ready`);

      if (!response.ok) {
        throw new Error(`HTTP ${response.status}: ${response.statusText}`);
      }

      return await response.json();
    } catch (error) {
      console.error("Readiness check failed:", error);
      throw new Error(`Failed to check stream readiness: ${error.message}`);
    }
  }

  /**
   * Wait for stream to become ready
   */
  async waitForStreamReady(initialReadiness) {
    return new Promise((resolve, reject) => {
      const maxWaitTime = 5 * 60 * 1000; // 5 minutes
      const checkInterval = 2000; // 2 seconds
      const maxRetries = Math.floor(maxWaitTime / checkInterval);
      let retryCount = 0;

      // Show initial status
      this.updateStatusFromReadiness(initialReadiness, retryCount, maxRetries);

      const checkTimer = setInterval(async () => {
        retryCount++;

        if (retryCount > maxRetries) {
          clearInterval(checkTimer);
          reject(new Error("Stream readiness timeout"));
          return;
        }

        try {
          const readiness = await this.checkStreamReadiness();

          if (readiness.ready) {
            clearInterval(checkTimer);
            resolve(readiness);
            this.startVideoPlayback();
          } else {
            this.updateStatusFromReadiness(readiness, retryCount, maxRetries);
          }
        } catch (error) {
          console.error(`Readiness check ${retryCount} failed:`, error);

          if (retryCount > 5) {
            clearInterval(checkTimer);
            reject(error);
          }
          // Otherwise continue retrying
        }
      }, checkInterval);
    });
  }

  /**
   * Update status display based on readiness response
   */
  updateStatusFromReadiness(readiness, retry, maxRetries) {
    let message = readiness.message || "Preparing stream...";
    let details = "";
    let progress = null;

    if (readiness.requiresRemuxing) {
      if (readiness.progress) {
        progress = readiness.progress * 100;
        message = `Remuxing for browser compatibility: ${Math.round(progress)}%`;
        details = "Converting video format for web streaming";
      } else {
        message = "Remuxing video for browser compatibility...";
        details = "This may take a few minutes for large files";
      }
    } else if (retry && maxRetries) {
      // Show wait progress
      progress = (retry / maxRetries) * 100;
      details = `Attempt ${retry} of ${maxRetries}`;
    }

    this.showStatus(message, progress, details);
  }

  /**
   * Start video playback
   */
  startVideoPlayback() {
    console.log("Starting video playback");

    this.showStatus("Loading video...", 95, "Stream is ready");

    // Set video source
    const streamUrl = `/stream/${this.infoHash}`;
    this.video.src = streamUrl;

    // Load and attempt to play
    this.video.load();

    // Hide overlay after a short delay
    setTimeout(() => {
      this.hideStatusOverlay();
    }, 1000);
  }

  /**
   * Handle video loading errors
   */
  handleLoadError(error) {
    console.error("Video load error:", error);

    if (this.loadAttempts < this.maxLoadAttempts) {
      const retryDelay = this.loadAttempts * 2000; // Increasing delay
      this.showStatus(
        `Load failed, retrying in ${retryDelay / 1000}s...`,
        null,
        error.message,
        true,
      );

      setTimeout(() => {
        this.isLoading = false;
        this.loadVideo();
      }, retryDelay);
    } else {
      this.showStatus(
        "Failed to load video",
        null,
        "Please try refreshing the page or check your connection",
        true,
      );
    }
  }

  /**
   * Handle video element errors
   */
  handleVideoError(error) {
    console.error("Video playback error:", error);

    let message = "Video playback error";
    let details = "";

    switch (error.code) {
      case MediaError.MEDIA_ERR_ABORTED:
        message = "Video loading aborted";
        details = "Playback was aborted by user or network";
        break;
      case MediaError.MEDIA_ERR_NETWORK:
        message = "Network error";
        details = "Check your internet connection";
        break;
      case MediaError.MEDIA_ERR_DECODE:
        message = "Video decode error";
        details = "Browser cannot decode this video format";
        break;
      case MediaError.MEDIA_ERR_SRC_NOT_SUPPORTED:
        message = "Video format not supported";
        details = "This video format is not supported by your browser";
        break;
      default:
        details = error.message || "Unknown error occurred";
    }

    this.showStatus(message, null, details, true);
  }

  /**
   * Show status overlay with message and progress
   */
  showStatus(message, progress = null, details = "", isError = false) {
    if (!this.statusOverlay) return;

    const messageEl = this.statusOverlay.querySelector(".status-message");
    const progressBar = this.statusOverlay.querySelector(".progress-fill");
    const progressText = this.statusOverlay.querySelector(".progress-text");
    const detailsEl = this.statusOverlay.querySelector(".status-details");
    const contentEl = this.statusOverlay.querySelector(".status-content");

    if (messageEl) messageEl.textContent = message;
    if (detailsEl) detailsEl.textContent = details;

    // Update progress
    if (progress !== null && progressBar && progressText) {
      progressBar.style.width = `${Math.max(0, Math.min(100, progress))}%`;
      progressText.textContent = `${Math.round(progress)}%`;
      progressText.parentElement.style.display = "block";
    } else if (progressText) {
      progressText.parentElement.style.display = "none";
    }

    // Apply error styling
    if (contentEl) {
      if (isError) {
        contentEl.classList.add("status-error");
      } else {
        contentEl.classList.remove("status-error");
      }
    }

    this.statusOverlay.style.display = "flex";
  }

  /**
   * Hide status overlay
   */
  hideStatusOverlay() {
    if (this.statusOverlay) {
      this.statusOverlay.style.display = "none";
    }
  }

  /**
   * Cleanup resources
   */
  destroy() {
    if (this.statusOverlay && this.statusOverlay.parentElement) {
      this.statusOverlay.parentElement.removeChild(this.statusOverlay);
    }
  }
}

/**
 * Extract info hash from current URL
 */
function extractInfoHashFromUrl() {
  const pathParts = window.location.pathname.split("/");
  return pathParts[pathParts.length - 1];
}

/**
 * Show error message (fallback function)
 */
function showError(message) {
  console.error("Video player error:", message);

  const player = document.getElementById("video-player");
  if (player && player.parentElement) {
    const errorDiv = document.createElement("div");
    errorDiv.style.cssText = `
            position: absolute;
            top: 50%;
            left: 50%;
            transform: translate(-50%, -50%);
            background: rgba(220, 53, 69, 0.9);
            color: white;
            padding: 20px;
            border-radius: 8px;
            text-align: center;
            z-index: 1001;
            font-family: -apple-system, BlinkMacSystemFont, sans-serif;
        `;
    errorDiv.textContent = message;
    player.parentElement.appendChild(errorDiv);
  }
}
