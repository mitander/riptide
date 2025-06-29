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
    
    let html = '<div class="card"><h3>Search Results</h3><div class="grid">';
    for (const result of results) {
        html += '<div class="grid-item">';
        html += '<h4>' + (result.title || 'Unknown Title') + '</h4>';
        html += '<p>Quality: ' + (result.quality || 'Unknown') + '</p>';
        html += '<p>Size: ' + (result.size || 'Unknown') + '</p>';
        html += '<p>Seeds: ' + (result.seeds || '0') + '</p>';
        html += '<button class="btn btn-small" onclick="downloadTorrent(\'' + (result.magnet_link || '') + '\')">Download</button>';
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
        const result = await apiCall(`/api/torrents/add?magnet=${encodeURIComponent(magnetLink)}`);
        if (result.success) {
            alert('Download started! Check the Torrents page for progress.');
        } else {
            alert('Failed to start download: ' + result.message);
        }
    } catch (error) {
        alert('Failed to start download');
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