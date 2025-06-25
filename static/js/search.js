/**
 * Search page functionality
 */

class MediaSearch {
    constructor() {
        this.searchResults = [];
        this.init();
    }

    init() {
        this.setupForm();
        this.setupKeyboardShortcuts();
    }

    setupForm() {
        const form = document.querySelector('.search-input-form');
        if (form) {
            form.addEventListener('submit', (e) => this.handleSearch(e));
        }

        // Auto-focus search input
        const searchInput = document.getElementById('search-query');
        if (searchInput) {
            searchInput.focus();
        }
    }

    setupKeyboardShortcuts() {
        document.addEventListener('keydown', (e) => {
            // Focus search on '/' key
            if (e.key === '/' && !this.isInputFocused()) {
                e.preventDefault();
                const searchInput = document.getElementById('search-query');
                if (searchInput) searchInput.focus();
            }
        });
    }

    async handleSearch(event) {
        event.preventDefault();
        
        const queryInput = document.getElementById('search-query');
        const categoryInput = document.querySelector('input[name="category"]:checked');
        
        if (!queryInput || !categoryInput) return;

        const query = queryInput.value.trim();
        const category = categoryInput.value;
        
        if (!query) {
            this.showError('Please enter a search query');
            return;
        }

        this.showLoading(true);
        this.hideResults();
        
        try {
            let url = '/api/search';
            if (category === 'movie') {
                url = '/api/search/movies';
            } else if (category === 'tv') {
                url = '/api/search/tv';
            }
            
            const response = await fetch(`${url}?q=${encodeURIComponent(query)}`);
            const data = await response.json();
            
            this.showLoading(false);
            
            if (data.results && data.results.length > 0) {
                this.searchResults = data.results;
                this.renderSearchResults();
                this.showResults();
            } else {
                this.showNoResults();
            }
        } catch (error) {
            this.showLoading(false);
            this.showError(`Search failed: ${error.message}`);
        }
    }

    renderSearchResults() {
        const grid = document.getElementById('results-grid');
        if (!grid) return;

        grid.innerHTML = '';
        
        this.searchResults.forEach(result => {
            const resultDiv = document.createElement('div');
            resultDiv.className = 'search-result-card';
            
            const posterImg = result.poster_url ? 
                `<img src="${this.escapeHtml(result.poster_url)}" alt="${this.escapeHtml(result.title)}" class="result-poster" loading="lazy">` :
                '<div class="result-poster-placeholder"></div>';
            
            const year = result.year ? ` (${result.year})` : '';
            const rating = result.rating ? `<span class="rating">★ ${result.rating}</span>` : '';
            const genre = result.genre ? `<span class="genre">${this.escapeHtml(result.genre)}</span>` : '';
            
            resultDiv.innerHTML = `
                <div class="result-poster-container">
                    ${posterImg}
                </div>
                <div class="result-info">
                    <h3 class="result-title">${this.escapeHtml(result.title)}${this.escapeHtml(year)}</h3>
                    <div class="result-meta">
                        ${rating}
                        ${genre}
                        <span class="media-type">${this.escapeHtml(result.media_type)}</span>
                    </div>
                    <p class="result-plot">${this.escapeHtml(result.plot || 'No description available.')}</p>
                    <div class="result-torrents">
                        <h4>Available Downloads (${result.torrents.length})</h4>
                        <div class="torrent-list">
                            ${this.renderTorrentList(result.torrents)}
                        </div>
                    </div>
                </div>
            `;
            
            grid.appendChild(resultDiv);
        });
    }

    renderTorrentList(torrents) {
        // Sort torrents by quality and health
        const sortedTorrents = [...torrents]
            .sort((a, b) => {
                // Quality priority: 4K > 1080p > 720p > etc
                const qualityOrder = { '4K': 4, '1080p': 3, '720p': 2, 'DVD': 1 };
                const aQuality = qualityOrder[a.quality] || 0;
                const bQuality = qualityOrder[b.quality] || 0;
                
                if (aQuality !== bQuality) return bQuality - aQuality;
                
                // Then by seeders
                return b.seeders - a.seeders;
            })
            .slice(0, 5); // Show top 5 torrents

        const torrentItems = sortedTorrents.map(torrent => `
            <div class="torrent-item">
                <div class="torrent-info">
                    <span class="torrent-quality ${torrent.quality.toLowerCase()}">${this.escapeHtml(torrent.quality)}</span>
                    <span class="torrent-size">${this.formatFileSize(torrent.size)}</span>
                    <span class="torrent-seeds">🌱 ${torrent.seeders}</span>
                    <span class="torrent-health ${this.getTorrentHealthClass(torrent)}"></span>
                </div>
                <button class="btn btn-sm btn-primary" onclick="window.mediaSearch.downloadTorrent('${this.escapeHtml(torrent.magnet_link)}')">
                    Download
                </button>
            </div>
        `).join('');

        const moreCount = torrents.length - sortedTorrents.length;
        const moreText = moreCount > 0 ? `<p class="more-torrents">... and ${moreCount} more</p>` : '';

        return torrentItems + moreText;
    }

    async downloadTorrent(magnetLink) {
        try {
            const response = await fetch(`/api/torrents/add?magnet=${encodeURIComponent(magnetLink)}`, {
                method: 'POST'
            });
            
            const data = await response.json();
            
            if (data.result?.success) {
                this.showNotification('Torrent added successfully! ' + data.result.message, 'success');
                
                if (data.result.stream_url) {
                    if (confirm('Would you like to start streaming now?')) {
                        window.open(data.result.stream_url, '_blank');
                    }
                }
            } else {
                this.showNotification('Failed to add torrent: ' + (data.result?.message || 'Unknown error'), 'error');
            }
        } catch (error) {
            this.showNotification('Error adding torrent: ' + error.message, 'error');
        }
    }

    getTorrentHealthClass(torrent) {
        const ratio = torrent.seeders / (torrent.seeders + torrent.leechers || 1);
        if (ratio > 0.8) return 'excellent';
        if (ratio > 0.5) return 'good';
        if (ratio > 0.2) return 'fair';
        return 'poor';
    }

    formatFileSize(bytes) {
        if (!bytes) return 'Unknown';
        
        const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
        const i = Math.floor(Math.log(bytes) / Math.log(1024));
        
        if (i === 0) return `${bytes} ${sizes[i]}`;
        return `${(bytes / Math.pow(1024, i)).toFixed(1)} ${sizes[i]}`;
    }

    showLoading(show) {
        const loadingDiv = document.getElementById('search-loading');
        if (loadingDiv) {
            loadingDiv.style.display = show ? 'block' : 'none';
        }
    }

    showResults() {
        const resultsDiv = document.getElementById('search-results');
        if (resultsDiv) {
            resultsDiv.style.display = 'block';
        }
    }

    hideResults() {
        const resultsDiv = document.getElementById('search-results');
        if (resultsDiv) {
            resultsDiv.style.display = 'none';
        }
    }

    showNoResults() {
        const grid = document.getElementById('results-grid');
        if (grid) {
            grid.innerHTML = '<p class="no-results">No results found. Try different search terms.</p>';
        }
        this.showResults();
    }

    showError(message) {
        const grid = document.getElementById('results-grid');
        if (grid) {
            grid.innerHTML = `<p class="error">${this.escapeHtml(message)}</p>`;
        }
        this.showResults();
    }

    showNotification(message, type) {
        // Create notification element
        const notification = document.createElement('div');
        notification.className = `notification ${type}`;
        notification.textContent = message;
        
        // Add to page
        document.body.appendChild(notification);
        
        // Remove after 5 seconds
        setTimeout(() => {
            notification.remove();
        }, 5000);
    }

    isInputFocused() {
        const activeElement = document.activeElement;
        return activeElement && (
            activeElement.tagName === 'INPUT' || 
            activeElement.tagName === 'TEXTAREA' ||
            activeElement.isContentEditable
        );
    }

    escapeHtml(text) {
        if (!text) return '';
        const div = document.createElement('div');
        div.textContent = text;
        return div.innerHTML;
    }
}

// Initialize search functionality when DOM is ready
document.addEventListener('DOMContentLoaded', () => {
    window.mediaSearch = new MediaSearch();
});