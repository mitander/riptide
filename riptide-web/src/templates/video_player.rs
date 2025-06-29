//! Video player page template

/// Generates the video player page content
pub fn video_player_content(info_hash: &str, is_local: bool) -> String {
    format!(
        r#"
        <div class="page-header">
            <h1>Video Player</h1>
            <p>Streaming content via BitTorrent</p>
        </div>
        
        <div class="card">
            <div id="video-container">
                <video id="video-player" controls width="100%" style="max-width: 800px; background: #000;">
                    <source src="/api/stream/{info_hash}{local_param}" type="video/mp4">
                    <source src="/api/stream/{info_hash}{local_param}" type="video/x-matroska">
                    Your browser does not support the video tag.
                </video>
            </div>
            
            <div style="margin-top: 20px;">
                <p><strong>Info Hash:</strong> {info_hash}</p>
                <p><strong>Source:</strong> {source_type}</p>
                <p style="color: #aaa; font-size: 14px; margin-top: 10px;">
                    If video doesn't play, ensure the file is in MP4 format for best browser compatibility.
                </p>
            </div>
        </div>
        
        <script>
            const player = document.getElementById('video-player');
            
            player.addEventListener('error', function(e) {{
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
            }});
            
            player.addEventListener('loadstart', function() {{
                console.log('Video loading started');
            }});
            
            player.addEventListener('canplay', function() {{
                console.log('Video can start playing');
            }});
        </script>
        "#,
        info_hash = info_hash,
        local_param = if is_local { "?local=true" } else { "" },
        source_type = if is_local {
            "Local File"
        } else {
            "BitTorrent Stream"
        }
    )
}
