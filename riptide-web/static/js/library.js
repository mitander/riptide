async function loadLibrary() {
    try {
        const data = await apiCall('/api/library');
        displayLibrary(data.items || []);
    } catch (error) {
        document.getElementById('library-content').innerHTML = 
            '<div class="card"><h3>Library Empty</h3><p>No media found. Start by <a href="/search">searching for content</a> to download.</p></div>';
    }
}

function displayLibrary(items) {
    const contentDiv = document.getElementById('library-content');
    
    if (items.length === 0) {
        contentDiv.innerHTML = `
            <div class="card">
                <h3>No Media Found</h3>
                <p>Your library is empty. Start by <a href="/search">searching for media</a> to download.</p>
            </div>
        `;
        return;
    }
    
    let html = '<div class="grid">';
    for (let i = 0; i < items.length; i++) {
        const item = items[i];
        html += '<div class="grid-item">';
        html += '<h4>' + (item.title || 'Unknown Title') + '</h4>';
        html += '<p>Type: ' + (item.type || 'Unknown') + '</p>';
        html += '<p>Size: ' + (item.size || 'Unknown') + '</p>';
        html += '<p>Added: ' + (item.added_date || 'Unknown') + '</p>';
        html += '<button class="btn btn-small" onclick="streamMedia(\'' + (item.info_hash || item.id || '') + '\', ' + (item.is_local || false) + ')">Stream</button>';
        html += '</div>';
    }
    html += '</div>';
    
    contentDiv.innerHTML = html;
}

function streamMedia(infoHash, isLocal = false) {
    if (!infoHash) {
        alert('Cannot stream this item');
        return;
    }
    
    // Open video player page
    const playerUrl = isLocal ? `/player/${infoHash}?local=true` : `/player/${infoHash}`;
    window.open(playerUrl, '_blank');
}

// Load library on page load
document.addEventListener('DOMContentLoaded', loadLibrary);