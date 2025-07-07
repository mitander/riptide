document.addEventListener('DOMContentLoaded', function() {
    const player = document.getElementById('video-player');
    
    if (!player) return;
    
    let lastBufferCheck = 0;
    
    // Enhanced error handling with format-specific messages
    player.addEventListener('error', function(e) {
        console.error('Video error:', e);
        const container = document.getElementById('video-container');
        const videoSrc = player.currentSrc || player.src;
        const isAvi = videoSrc.includes('.avi');
        const isMkv = videoSrc.includes('.mkv');
        
        let message = 'Video Playback Error';
        let details = 'The video format may not be supported by your browser.';
        
        if (isAvi) {
            message = 'AVI Format Not Supported';
            details = 'AVI files cannot be played directly in browsers. Please convert to MP4 format.';
        } else if (isMkv) {
            message = 'MKV Format Limited Support';
            details = 'MKV files have limited browser support. MP4 or WebM work best.';
        }
        
        container.innerHTML = `
            <div style="background: #333; padding: 40px; text-align: center; border-radius: 8px;">
                <h3>${message}</h3>
                <p>${details}</p>
                <p style="color: #aaa; margin-top: 10px;">
                    For best compatibility, use MP4 format with H.264 video and AAC audio.
                </p>
            </div>
        `;
    });
    
    // Streaming buffer management
    player.addEventListener('waiting', function() {
        console.log('Video buffering - waiting for more data');
    });
    
    player.addEventListener('playing', function() {
        console.log('Video playing');
    });
    
    player.addEventListener('progress', function() {
        const currentTime = Date.now();
        if (currentTime - lastBufferCheck > 1000) { // Check every second
            const buffered = player.buffered;
            if (buffered.length > 0) {
                const bufferedEnd = buffered.end(buffered.length - 1);
                const currentTime = player.currentTime;
                const bufferHealth = bufferedEnd - currentTime;
                
                console.log(`Buffer health: ${bufferHealth.toFixed(1)}s ahead`);
                
                // If buffer is getting low, we could show a loading indicator
                if (bufferHealth < 5 && !player.paused) {
                    console.log('Low buffer - torrent may need to download more pieces');
                }
            }
            lastBufferCheck = currentTime;
        }
    });
    
    player.addEventListener('loadstart', function() {
        console.log('Video loading started');
    });
    
    player.addEventListener('canplay', function() {
        console.log('Video can start playing');
    });
});