document.addEventListener('DOMContentLoaded', function() {
    const player = document.getElementById('video-player');
    
    if (!player) return;
    
    player.addEventListener('error', function(e) {
        console.error('Video error:', e);
        const container = document.getElementById('video-container');
        container.innerHTML = `
            <div style="background: #333; padding: 40px; text-align: center; border-radius: 8px;">
                <h3>Video Playback Error</h3>
                <p>The video format may not be supported by your browser.</p>
                <p style="color: #aaa; margin-top: 10px;">
                    Try converting the file to MP4 format for better compatibility.
                </p>
            </div>
        `;
    });
    
    player.addEventListener('loadstart', function() {
        console.log('Video loading started');
    });
    
    player.addEventListener('canplay', function() {
        console.log('Video can start playing');
    });
});