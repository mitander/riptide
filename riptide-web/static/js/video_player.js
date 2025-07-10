//! Video player integration - uses EnhancedVideoPlayer from streaming_readiness.js
//!
//! This file provides compatibility and fallback initialization for the video player

document.addEventListener("DOMContentLoaded", function () {
  // Check if EnhancedVideoPlayer is available from streaming_readiness.js
  if (typeof EnhancedVideoPlayer === "undefined") {
    console.error(
      "EnhancedVideoPlayer not found - streaming_readiness.js may not be loaded",
    );
    showFallbackError();
    return;
  }

  // Check if already initialized by streaming_readiness.js
  if (window.riptideVideoPlayer) {
    console.log("Video player already initialized by streaming_readiness.js");
    return;
  }

  const player = document.getElementById("video-player");
  if (!player) {
    console.warn("Video player element not found");
    return;
  }

  // Get info hash from data attribute or URL
  const infoHash = player.dataset.infoHash || extractInfoHashFromUrl();
  if (!infoHash) {
    console.error("Info hash not found");
    showFallbackError("Configuration error: Video identifier not found");
    return;
  }

  console.log("Initializing video player for:", infoHash);

  try {
    // Initialize enhanced video player using the class from streaming_readiness.js
    const enhancedPlayer = new EnhancedVideoPlayer(player, infoHash);

    // Store globally for debugging
    window.riptideVideoPlayer = enhancedPlayer;

    // Start loading process
    enhancedPlayer.loadVideo();
  } catch (error) {
    console.error("Failed to initialize video player:", error);
    showFallbackError("Failed to initialize video player");
  }
});

/**
 * Extract info hash from current URL
 */
function extractInfoHashFromUrl() {
  const pathParts = window.location.pathname.split("/");
  return pathParts[pathParts.length - 1];
}

/**
 * Show fallback error message when video player fails to initialize
 */
function showFallbackError(message = "Video player initialization failed") {
  console.error("Video player error:", message);

  const player = document.getElementById("video-player");
  if (player && player.parentElement) {
    // Ensure parent is positioned for absolute positioning
    const container = player.parentElement;
    if (getComputedStyle(container).position === "static") {
      container.style.position = "relative";
    }

    const errorDiv = document.createElement("div");
    errorDiv.style.cssText = `
      position: absolute;
      top: 0;
      left: 0;
      right: 0;
      bottom: 0;
      background: rgba(0, 0, 0, 0.9);
      color: white;
      display: flex;
      align-items: center;
      justify-content: center;
      z-index: 1001;
      font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
    `;

    errorDiv.innerHTML = `
      <div style="text-align: center; padding: 20px;">
        <div style="font-size: 48px; margin-bottom: 20px;">⚠️</div>
        <div style="font-size: 18px; margin-bottom: 10px; font-weight: 500;">${message}</div>
        <div style="font-size: 14px; color: rgba(255, 255, 255, 0.7);">
          Please try refreshing the page or check your browser console for more details.
        </div>
      </div>
    `;

    container.appendChild(errorDiv);
  }
}
