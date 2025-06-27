import { ApiClient } from './shared/api-client.js';
import { SearchResult, TorrentItem, formatBytes } from './shared/types.js';

export class SearchManager {
    private apiClient: ApiClient;

    constructor() {
        this.apiClient = new ApiClient();
    }

    public initialize(): void {
        this.setupEventListeners();
        this.autoFocusSearchInput();
    }

    private setupEventListeners(): void {
        const searchForm = document.querySelector('.search-input-form') as HTMLFormElement;
        if (searchForm) {
            searchForm.addEventListener('submit', (e) => this.performSearch(e));
        }
    }

    private autoFocusSearchInput(): void {
        const searchInput = document.getElementById('search-query') as HTMLInputElement;
        if (searchInput) {
            searchInput.focus();
        }
    }

    private async performSearch(event: Event): Promise<void> {
        event.preventDefault();

        const queryInput = document.getElementById('search-query') as HTMLInputElement;
        const categoryInput = document.querySelector('input[name="category"]:checked') as HTMLInputElement;

        if (!queryInput || !categoryInput) return;

        const query = queryInput.value.trim();
        const category = categoryInput.value;

        if (!query) return;

        const loadingDiv = document.getElementById('search-loading');
        const resultsDiv = document.getElementById('search-results');
        const resultsGrid = document.getElementById('results-grid');

        if (!loadingDiv || !resultsDiv || !resultsGrid) return;

        // Show loading state
        loadingDiv.style.display = 'block';
        resultsDiv.style.display = 'none';

        try {
            const results = await this.apiClient.searchMedia(query, category);

            // Hide loading
            loadingDiv.style.display = 'none';

            if (results && results.length > 0) {
                this.renderSearchResults(results);
                resultsDiv.style.display = 'block';
            } else {
                resultsGrid.innerHTML = '<p class="no-results">No results found.</p>';
                resultsDiv.style.display = 'block';
            }
        } catch (error) {
            loadingDiv.style.display = 'none';
            const errorMessage = error instanceof Error ? error.message : 'Unknown error';
            resultsGrid.innerHTML = `<p class="error">Search failed: ${this.escapeHtml(errorMessage)}</p>`;
            resultsDiv.style.display = 'block';
        }
    }

    private renderSearchResults(results: SearchResult[]): void {
        const grid = document.getElementById('results-grid');
        if (!grid) return;

        grid.innerHTML = '';

        results.forEach(result => {
            const resultDiv = document.createElement('div');
            resultDiv.className = 'search-result-card';

            const posterImg = result.poster_url ?
                `<img src="${result.poster_url}" alt="${result.title}" class="result-poster" loading="lazy">` :
                '<div class="result-poster-placeholder"></div>';

            const year = result.year ? ` (${result.year})` : '';
            const rating = result.rating ? `<span class="rating">★ ${result.rating}</span>` : '';
            const genre = result.genre ? `<span class="genre">${this.escapeHtml(result.genre)}</span>` : '';

            resultDiv.innerHTML = `
                <div class="result-poster-container">
                    ${posterImg}
                </div>
                <div class="result-info">
                    <h3 class="result-title">${this.escapeHtml(result.title)}${year}</h3>
                    <div class="result-meta">
                        ${rating}
                        ${genre}
                        <span class="media-type">${this.escapeHtml(result.media_type)}</span>
                    </div>
                    <p class="result-plot">${this.escapeHtml(result.plot || 'No description available.')}</p>
                    <div class="result-torrents">
                        <h4>Available Downloads (${result.torrents.length})</h4>
                        <div class="torrent-list">
                            ${this.renderTorrentItems(result.torrents.slice(0, 3))}
                            ${result.torrents.length > 3 ? 
                                `<p class="more-torrents">... and ${result.torrents.length - 3} more</p>` : 
                                ''
                            }
                        </div>
                    </div>
                </div>
            `;

            // Add event listeners for download buttons
            const downloadButtons = resultDiv.querySelectorAll('[data-magnet-link]');
            downloadButtons.forEach(button => {
                button.addEventListener('click', (e) => {
                    const magnetLink = (e.target as HTMLElement).getAttribute('data-magnet-link');
                    if (magnetLink) {
                        this.downloadTorrent(magnetLink);
                    }
                });
            });

            grid.appendChild(resultDiv);
        });
    }

    private renderTorrentItems(torrents: TorrentItem[]): string {
        return torrents.map(torrent => `
            <div class="torrent-item">
                <div class="torrent-info">
                    <span class="torrent-quality">${this.escapeHtml(torrent.quality)}</span>
                    <span class="torrent-size">${formatBytes(torrent.size)}</span>
                    <span class="torrent-seeds">🌱 ${torrent.seeders}</span>
                </div>
                <button class="btn btn-sm btn-primary" data-magnet-link="${this.escapeHtml(torrent.magnet_link)}">
                    Download
                </button>
            </div>
        `).join('');
    }

    private async downloadTorrent(magnetLink: string): Promise<void> {
        try {
            const result = await this.apiClient.addTorrent(magnetLink);

            if (result.success) {
                alert('Torrent added successfully! ' + result.message);
                if (result.stream_url) {
                    if (confirm('Would you like to start streaming now?')) {
                        window.open(result.stream_url, '_blank');
                    }
                }
            } else {
                alert('Failed to add torrent: ' + result.message);
            }
        } catch (error) {
            const errorMessage = error instanceof Error ? error.message : 'Unknown error';
            alert('Error adding torrent: ' + errorMessage);
        }
    }

    private escapeHtml(text: string): string {
        const div = document.createElement('div');
        div.textContent = text;
        return div.innerHTML;
    }
}

// Initialize when DOM is loaded
document.addEventListener('DOMContentLoaded', () => {
    const search = new SearchManager();
    search.initialize();
});