//! Torrents page template

/// Generates the torrents page content
pub fn torrents_content() -> String {
    r#"
        <div class="page-header">
            <h1>Torrent Management</h1>
            <p>View and manage your active downloads</p>
        </div>
        
        <div class="card">
            <h3>Add New Torrent</h3>
            <div class="search-form">
                <input type="text" id="magnet-input" placeholder="Paste magnet link here...">
                <button onclick="addMagnetLink()">Add Torrent</button>
            </div>
        </div>
        
        <div class="card">
            <h3>Active Torrents</h3>
            <div id="torrents-list">
                <div class="loading">Loading torrents...</div>
            </div>
        </div>
        
        <script>
            async function loadTorrents() {
                try {
                    const data = await apiCall('/api/torrents');
                    displayTorrents(data.torrents || []);
                } catch (error) {
                    document.getElementById('torrents-list').innerHTML = 
                        '<p>No active torrents</p>';
                }
            }
            
            function displayTorrents(torrents) {
                const listDiv = document.getElementById('torrents-list');
                
                if (torrents.length === 0) {
                    listDiv.innerHTML = '<p>No active torrents</p>';
                    return;
                }
                
                let tableRows = '';
                for (const torrent of torrents) {
                    const isDisabled = torrent.progress < 5 && !torrent.is_local ? 'disabled' : '';
                    tableRows += `
                        <tr>
                            <td>` + (torrent.name || 'Unknown') + `</td>
                            <td>` + (torrent.progress || '0') + `%</td>
                            <td>` + (torrent.speed || '0') + ` KB/s</td>
                            <td>` + (torrent.size || 'Unknown') + `</td>
                            <td>
                                <button class="btn btn-small" onclick="streamTorrent('` + (torrent.info_hash || '') + `', ` + (torrent.is_local || false) + `)" ` + isDisabled + `>Stream</button>
                                <button class="btn btn-small">Pause</button>
                                <button class="btn btn-small">Remove</button>
                            </td>
                        </tr>
                    `;
                }
                
                const html = `
                    <table class="table">
                        <thead>
                            <tr>
                                <th>Name</th>
                                <th>Progress</th>
                                <th>Speed</th>
                                <th>Size</th>
                                <th>Actions</th>
                            </tr>
                        </thead>
                        <tbody>` + tableRows + `
                        </tbody>
                    </table>
                `;
                
                listDiv.innerHTML = html;
            }
            
            async function addMagnetLink() {
                const magnetInput = document.getElementById('magnet-input');
                const magnetLink = magnetInput.value.trim();
                
                if (!magnetLink) {
                    alert('Please enter a magnet link');
                    return;
                }
                
                try {
                    const result = await apiCall(`/api/torrents/add?magnet=${encodeURIComponent(magnetLink)}`);
                    if (result.success) {
                        alert('Torrent added successfully!');
                        magnetInput.value = '';
                        loadTorrents(); // Refresh the list
                    } else {
                        alert('Failed to add torrent: ' + result.message);
                    }
                } catch (error) {
                    alert('Failed to add torrent');
                }
            }
            
            function streamTorrent(infoHash, isLocal = false) {
                if (!infoHash) {
                    alert('Cannot stream: no torrent info hash');
                    return;
                }
                
                // Open video player page in new window
                const playerUrl = isLocal ? `/player/${infoHash}?local=true` : `/player/${infoHash}`;
                window.open(playerUrl, '_blank');
            }
            
            // Load torrents on page load
            loadTorrents();
            
            // Auto-refresh every 10 seconds
            setInterval(loadTorrents, 10000);
        </script>
    "#.to_string()
}
