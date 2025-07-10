//! Streaming readiness checker for video player
//!
//! Checks if a torrent file is ready for streaming before attempting to load video.
//! Provides better user experience with loading states and progress indicators.

class StreamingReadinessChecker {
    constructor(infoHash, options = {}) {
        this.infoHash = infoHash;
        this.options = {
            checkInterval: options.checkInterval || 2000, // 2 seconds
            maxRetries: options.maxRetries || 150, // 5 minutes total
            onReady: options.onReady || (() => {}),
            onProgress: options.onProgress || (() => {}),
            onError: options.onError || (() => {}),
            onTimeout: options.onTimeout || (() => {}),
            ...options
        };

        this.retryCount = 0;
        this.isChecking = false;
        this.checkInterval = null;
    }

    /**
     * Start checking readiness
     */
    async startChecking() {
        if (this.isChecking) {
            return;
        }

        this.isChecking = true;
        this.retryCount = 0;

        console.log(`Starting readiness check for ${this.infoHash}`);

        // Do initial check
        await this.checkReadiness();

        // Set up periodic checking
        this.checkInterval = setInterval(() => {
            this.checkReadiness();
        }, this.options.checkInterval);
    }

    /**
     * Stop checking readiness
     */
    stopChecking() {
        this.isChecking = false;
        if (this.checkInterval) {
            clearInterval(this.checkInterval);
            this.checkInterval = null;
        }
    }

    /**
     * Check if streaming is ready
     */
    async checkReadiness() {
        if (!this.isChecking) {
            return;
        }

        this.retryCount++;

        if (this.retryCount > this.options.maxRetries) {
            this.stopChecking();
            this.options.onTimeout();
            return;
        }

        try {
            const response = await fetch(`/stream/${this.infoHash}/ready`);

            if (!response.ok) {
                throw new Error(`HTTP ${response.status}: ${response.statusText}`);
            }

            const readiness = await response.json();

            console.log(`Readiness check ${this.retryCount}:`, readiness);

            if (readiness.ready) {
                this.stopChecking();
                this.options.onReady(readiness);
            } else {
                this.options.onProgress(readiness, this.retryCount, this.options.maxRetries);
            }

        } catch (error) {
            console.error('Readiness check failed:', error);
            this.options.onError(error, this.retryCount);
        }
    }
}

/**
 * Enhanced video player with readiness checking
 */
class EnhancedVideoPlayer {
    constructor(videoElement, infoHash) {
        this.video = videoElement;
        this.infoHash = infoHash;
        this.readinessChecker = null;
        this.statusElement = null;
        this.progressElement = null;

        this.setupStatusDisplay();
    }

    /**
     * Setup status display elements
     */
    setupStatusDisplay() {
        // Create status container
        const statusContainer = document.createElement('div');
        statusContainer.className = 'streaming-status';
        statusContainer.style.cssText = `
            position: absolute;
            top: 50%;
            left: 50%;
            transform: translate(-50%, -50%);
            background: rgba(0, 0, 0, 0.8);
            color: white;
            padding: 20px;
            border-radius: 8px;
            text-align: center;
            z-index: 1000;
            font-family: -apple-system, BlinkMacSystemFont, sans-serif;
        `;

        // Status message
        this.statusElement = document.createElement('div');
        this.statusElement.className = 'status-message';
        this.statusElement.style.cssText = 'margin-bottom: 10px; font-size: 16px;';

        // Progress bar
        this.progressElement = document.createElement('div');
        this.progressElement.className = 'progress-bar';
        this.progressElement.style.cssText = `
            width: 200px;
            height: 4px;
            background: rgba(255, 255, 255, 0.3);
            border-radius: 2px;
            overflow: hidden;
            margin: 10px auto;
        `;

        const progressFill = document.createElement('div');
        progressFill.className = 'progress-fill';
        progressFill.style.cssText = `
            height: 100%;
            background: #007bff;
            width: 0%;
            transition: width 0.3s ease;
        `;

        this.progressElement.appendChild(progressFill);

        statusContainer.appendChild(this.statusElement);
        statusContainer.appendChild(this.progressElement);

        // Add to video container
        const videoContainer = this.video.parentElement;
        if (videoContainer) {
            videoContainer.style.position = 'relative';
            videoContainer.appendChild(statusContainer);
        }

        this.statusContainer = statusContainer;
    }

    /**
     * Start loading video with readiness check
     */
    async loadVideo() {
        this.showStatus('Checking stream readiness...', 0);

        this.readinessChecker = new StreamingReadinessChecker(this.infoHash, {
            onReady: (readiness) => this.onStreamReady(readiness),
            onProgress: (readiness, retry, maxRetries) => this.onStreamProgress(readiness, retry, maxRetries),
            onError: (error, retry) => this.onStreamError(error, retry),
            onTimeout: () => this.onStreamTimeout()
        });

        await this.readinessChecker.startChecking();
    }

    /**
     * Stream is ready to play
     */
    onStreamReady(readiness) {
        console.log('Stream ready:', readiness);
        this.showStatus('Stream ready, loading video...', 100);

        // Set video source and load
        this.video.src = `/stream/${this.infoHash}`;
        this.video.load();

        // Hide status after a short delay
        setTimeout(() => {
            this.hideStatus();
        }, 1000);
    }

    /**
     * Stream preparation in progress
     */
    onStreamProgress(readiness, retry, maxRetries) {
        const progress = (retry / maxRetries) * 100;
        let message = readiness.message || 'Preparing stream...';

        if (readiness.requiresRemuxing) {
            message = readiness.progress
                ? `Remuxing: ${Math.round(readiness.progress * 100)}%`
                : 'Remuxing video for browser compatibility...';
        }

        this.showStatus(message, Math.min(progress, 90));
    }

    /**
     * Stream check error
     */
    onStreamError(error, retry) {
        console.error('Stream error:', error);
        if (retry < 5) {
            this.showStatus('Connection error, retrying...', 0);
        } else {
            this.showStatus('Stream temporarily unavailable', 0);
        }
    }

    /**
     * Stream check timeout
     */
    onStreamTimeout() {
        this.showStatus('Stream preparation timed out. Please try again later.', 0);
        console.error('Stream readiness check timed out');
    }

    /**
     * Show status message with progress
     */
    showStatus(message, progress = 0) {
        if (this.statusElement) {
            this.statusElement.textContent = message;
        }

        if (this.progressElement) {
            const progressFill = this.progressElement.querySelector('.progress-fill');
            if (progressFill) {
                progressFill.style.width = `${progress}%`;
            }
            this.progressElement.style.display = progress > 0 ? 'block' : 'none';
        }

        if (this.statusContainer) {
            this.statusContainer.style.display = 'block';
        }
    }

    /**
     * Hide status display
     */
    hideStatus() {
        if (this.statusContainer) {
            this.statusContainer.style.display = 'none';
        }
    }

    /**
     * Cleanup resources
     */
    destroy() {
        if (this.readinessChecker) {
            this.readinessChecker.stopChecking();
        }

        if (this.statusContainer && this.statusContainer.parentElement) {
            this.statusContainer.parentElement.removeChild(this.statusContainer);
        }
    }
}

/**
 * Initialize enhanced video player for current page
 */
function initEnhancedVideoPlayer() {
    const videoElement = document.getElementById('video-player');
    if (!videoElement) {
        console.warn('Video player element not found');
        return null;
    }

    // Extract info hash from URL or data attribute
    const pathParts = window.location.pathname.split('/');
    const infoHash = pathParts[pathParts.length - 1] || videoElement.dataset.infoHash;

    if (!infoHash) {
        console.error('Info hash not found');
        return null;
    }

    console.log('Initializing enhanced video player for:', infoHash);
    return new EnhancedVideoPlayer(videoElement, infoHash);
}

/**
 * Simple readiness check function for existing code
 */
async function checkStreamingReadiness(infoHash) {
    try {
        const response = await fetch(`/stream/${infoHash}/ready`);
        if (!response.ok) {
            throw new Error(`HTTP ${response.status}`);
        }
        return await response.json();
    } catch (error) {
        console.error('Failed to check streaming readiness:', error);
        return null;
    }
}

/**
 * Wait for stream to be ready with timeout
 */
async function waitForStreamReady(infoHash, timeoutMs = 300000) {
    return new Promise((resolve, reject) => {
        const checker = new StreamingReadinessChecker(infoHash, {
            checkInterval: 2000,
            maxRetries: Math.floor(timeoutMs / 2000),
            onReady: (readiness) => resolve(readiness),
            onTimeout: () => reject(new Error('Stream readiness timeout')),
            onError: (error) => {
                if (checker.retryCount > 10) {
                    reject(error);
                }
                // Otherwise continue retrying
            }
        });

        checker.startChecking();
    });
}

// Export for module usage
if (typeof module !== 'undefined' && module.exports) {
    module.exports = {
        StreamingReadinessChecker,
        EnhancedVideoPlayer,
        initEnhancedVideoPlayer,
        checkStreamingReadiness,
        waitForStreamReady
    };
}

// Auto-initialize on DOM ready
if (typeof document !== 'undefined') {
    document.addEventListener('DOMContentLoaded', () => {
        // Only auto-initialize if we're on a video player page
        if (window.location.pathname.includes('/player/')) {
            const player = initEnhancedVideoPlayer();
            if (player) {
                // Store globally for access from other scripts
                window.riptideVideoPlayer = player;

                // Auto-start loading
                player.loadVideo();
            }
        }
    });
}
