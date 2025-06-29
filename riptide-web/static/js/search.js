async function performSearch() {
    const query = document.getElementById('search-query').value.trim();
    if (!query) {
        alert('Please enter a search term');
        return;
    }
    
    const resultsDiv = document.getElementById('search-results');
    resultsDiv.innerHTML = '<div class="loading">Searching...</div>';
    
    try {
        const data = await apiCall(`/api/search?q=${encodeURIComponent(query)}`);
        displaySearchResults(data.results || []);
    } catch (error) {
        resultsDiv.innerHTML = '<div class="card"><p>Search failed. Please try again.</p></div>';
    }
}

function displaySearchResults(results) {
    const resultsDiv = document.getElementById('search-results');
    
    if (results.length === 0) {
        resultsDiv.innerHTML = `
            <div class="card">
                <h3>No Results Found</h3>
                <p>Try different search terms or check the spelling.</p>
            </div>
        `;
        return;
    }
    
    let html = '<div class="card"><h3>Search Results (' + results.length + ' torrents found)</h3><div class="search-grid">';
    for (const result of results) {
        html += '<div class="torrent-item">';
        html += '<div class="torrent-header">';
        html += '<h4>' + (result.title || 'Unknown Title') + '</h4>';
        html += '<span class="torrent-source">' + (result.source || '') + '</span>';
        html += '</div>';
        html += '<div class="torrent-details">';
        html += '<span class="quality">' + (result.quality || 'Unknown') + '</span>';
        html += '<span class="size">' + (result.size || 'Unknown') + '</span>';
        html += '<span class="seeds">🌱 ' + (result.seeds || '0') + '</span>';
        html += '</div>';
        html += '<button class="btn btn-small download-btn" onclick="downloadTorrent(\'' + (result.magnet_link || '') + '\')">Download</button>';
        html += '</div>';
    }
    html += '</div></div>';
    
    resultsDiv.innerHTML = html;
}

async function downloadTorrent(magnetLink) {
    if (!magnetLink) {
        alert('No magnet link available');
        return;
    }
    
    try {
        const data = await apiCall('/api/download', {
            method: 'POST',
            headers: {
                'Content-Type': 'application/json',
            },
            body: JSON.stringify({ magnet_link: magnetLink })
        });
        
        if (data.success) {
            alert('Download started successfully!');
        } else {
            alert('Failed to start download: ' + (data.error || 'Unknown error'));
        }
    } catch (error) {
        console.error('Download error:', error);
        alert('Failed to start download. Please try again.');
    }
}

// Allow Enter key to trigger search
document.addEventListener('DOMContentLoaded', function() {
    const searchInput = document.getElementById('search-query');
    if (searchInput) {
        searchInput.addEventListener('keypress', function(e) {
            if (e.key === 'Enter') {
                performSearch();
            }
        });
    }
});