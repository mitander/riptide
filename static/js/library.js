/**
 * Library page functionality
 */

class Library {
    constructor() {
        this.libraryItems = [];
        this.filteredItems = [];
        this.init();
    }

    init() {
        this.loadLibraryItems();
        this.setupEventListeners();
    }

    loadLibraryItems() {
        // This will be populated by the template with server data
        if (window.libraryItems) {
            this.libraryItems = window.libraryItems;
            this.filteredItems = [...this.libraryItems];
            this.renderLibraryItems();
        }
    }

    setupEventListeners() {
        const searchInput = document.getElementById('search');
        const typeFilter = document.getElementById('type-filter');

        if (searchInput) {
            searchInput.addEventListener('input', (e) => {
                this.handleSearch(e.target.value);
            });
        }

        if (typeFilter) {
            typeFilter.addEventListener('change', (e) => {
                this.handleTypeFilter(e.target.value);
            });
        }
    }

    handleSearch(query) {
        const searchTerm = query.toLowerCase();
        this.filteredItems = this.libraryItems.filter(item => 
            item.title.toLowerCase().includes(searchTerm)
        );
        this.renderLibraryItems();
    }

    handleTypeFilter(selectedType) {
        this.filteredItems = selectedType ? 
            this.libraryItems.filter(item => item.media_type === selectedType) :
            [...this.libraryItems];
        this.renderLibraryItems();
    }

    renderLibraryItems() {
        const mediaGrid = document.getElementById('media-grid');
        if (!mediaGrid) return;

        mediaGrid.innerHTML = '';
        
        this.filteredItems.forEach(item => {
            const div = document.createElement('div');
            div.className = 'media-card';
            div.innerHTML = `
                <div class="media-poster">
                    ${item.thumbnail_url ? 
                        `<img src="${this.escapeHtml(item.thumbnail_url)}" alt="${this.escapeHtml(item.title)}" loading="lazy">` :
                        '<div class="placeholder-poster"></div>'
                    }
                </div>
                <div class="media-info">
                    <h3 class="media-title">${this.escapeHtml(item.title)}</h3>
                    <p class="media-type">${this.escapeHtml(item.media_type)}</p>
                    <p class="media-size">${this.formatFileSize(item.size)}</p>
                    ${item.duration ? `<p class="media-duration">${this.formatDuration(item.duration)}</p>` : ''}
                    <div class="media-actions">
                        <a href="${this.escapeHtml(item.stream_url)}" class="btn btn-primary">Stream</a>
                        <button class="btn btn-secondary" onclick="window.library.showMediaDetails('${item.id}')">Details</button>
                    </div>
                </div>
            `;
            mediaGrid.appendChild(div);
        });

        if (this.filteredItems.length === 0) {
            mediaGrid.innerHTML = '<p class="no-results">No media found matching your criteria.</p>';
        }
    }

    showMediaDetails(mediaId) {
        const item = this.libraryItems.find(item => item.id === mediaId);
        if (!item) return;

        // Create modal or navigate to details page
        alert(`Details for: ${item.title}\n\nType: ${item.media_type}\nSize: ${this.formatFileSize(item.size)}`);
    }

    formatFileSize(bytes) {
        if (!bytes) return 'Unknown';
        
        const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
        const i = Math.floor(Math.log(bytes) / Math.log(1024));
        
        if (i === 0) return `${bytes} ${sizes[i]}`;
        return `${(bytes / Math.pow(1024, i)).toFixed(1)} ${sizes[i]}`;
    }

    formatDuration(seconds) {
        const hours = Math.floor(seconds / 3600);
        const minutes = Math.floor((seconds % 3600) / 60);
        
        if (hours > 0) {
            return `${hours}h ${minutes}m`;
        }
        return `${minutes}m`;
    }

    escapeHtml(text) {
        const div = document.createElement('div');
        div.textContent = text;
        return div.innerHTML;
    }
}

// Initialize library when DOM is ready
document.addEventListener('DOMContentLoaded', () => {
    window.library = new Library();
});