import { ApiClient } from './shared/api-client.js';
import { LibraryItem, formatBytes } from './shared/types.js';

export class LibraryManager {
    private libraryItems: LibraryItem[] = [];
    private apiClient: ApiClient;

    constructor() {
        this.apiClient = new ApiClient();
    }

    public async initialize(items: LibraryItem[]): Promise<void> {
        this.libraryItems = items;
        this.setupEventListeners();
        this.renderLibraryItems(this.libraryItems);
    }

    private setupEventListeners(): void {
        const searchInput = document.getElementById('search') as HTMLInputElement;
        const typeFilter = document.getElementById('type-filter') as HTMLSelectElement;

        if (searchInput) {
            searchInput.addEventListener('input', (e) => {
                const query = (e.target as HTMLInputElement).value.toLowerCase();
                const filtered = this.libraryItems.filter(item =>
                    item.title.toLowerCase().includes(query)
                );
                this.renderLibraryItems(filtered);
            });
        }

        if (typeFilter) {
            typeFilter.addEventListener('change', (e) => {
                const type = (e.target as HTMLSelectElement).value;
                const filtered = type ?
                    this.libraryItems.filter(item => item.media_type === type) :
                    this.libraryItems;
                this.renderLibraryItems(filtered);
            });
        }
    }

    private renderLibraryItems(items: LibraryItem[]): void {
        const mediaGrid = document.getElementById('media-grid');
        if (!mediaGrid) return;

        mediaGrid.innerHTML = '';

        items.forEach(item => {
            const div = document.createElement('div');
            div.className = 'media-card';

            const posterContent = item.thumbnail_url ?
                `<img src="${item.thumbnail_url}" alt="${item.title}" loading="lazy">` :
                '<div class="placeholder-poster"></div>';

            const durationContent = item.duration ?
                `<p class="media-duration">${Math.floor(item.duration / 60)}m</p>` :
                '';

            div.innerHTML = `
                <div class="media-poster">
                    ${posterContent}
                </div>
                <div class="media-info">
                    <h3 class="media-title">${this.escapeHtml(item.title)}</h3>
                    <p class="media-type">${this.escapeHtml(item.media_type)}</p>
                    <p class="media-size">${formatBytes(item.size)}</p>
                    ${durationContent}
                    <div class="media-actions">
                        <a href="${item.stream_url}" class="btn btn-primary">Stream</a>
                        <button class="btn btn-secondary" data-media-id="${item.id}">Details</button>
                    </div>
                </div>
            `;

            // Add event listener for details button
            const detailsButton = div.querySelector('[data-media-id]') as HTMLButtonElement;
            if (detailsButton) {
                detailsButton.addEventListener('click', () => {
                    this.showMediaDetails(item.id);
                });
            }

            mediaGrid.appendChild(div);
        });
    }

    private async showMediaDetails(mediaId: string): Promise<void> {
        try {
            const details = await this.apiClient.getMediaDetails(mediaId);
            // Create and show modal with details
            this.createDetailsModal(details);
        } catch (error) {
            console.error('Failed to load media details:', error);
        }
    }

    private createDetailsModal(details: LibraryItem): void {
        // Remove existing modal if present
        const existingModal = document.getElementById('media-details-modal');
        if (existingModal) {
            existingModal.remove();
        }

        const modal = document.createElement('div');
        modal.id = 'media-details-modal';
        modal.className = 'modal';
        modal.innerHTML = `
            <div class="modal-content">
                <div class="modal-header">
                    <h2>${this.escapeHtml(details.title)}</h2>
                    <button class="modal-close">&times;</button>
                </div>
                <div class="modal-body">
                    <div class="media-details">
                        <div class="media-poster">
                            ${details.thumbnail_url ?
                                `<img src="${details.thumbnail_url}" alt="${details.title}">` :
                                '<div class="placeholder-poster"></div>'
                            }
                        </div>
                        <div class="media-info">
                            <p><strong>Type:</strong> ${this.escapeHtml(details.media_type)}</p>
                            <p><strong>Size:</strong> ${formatBytes(details.size)}</p>
                            ${details.duration ? `<p><strong>Duration:</strong> ${Math.floor(details.duration / 60)}m</p>` : ''}
                            <div class="media-actions">
                                <a href="${details.stream_url}" class="btn btn-primary">Stream Now</a>
                            </div>
                        </div>
                    </div>
                </div>
            </div>
        `;

        // Add event listeners
        const closeButton = modal.querySelector('.modal-close') as HTMLButtonElement;
        closeButton.addEventListener('click', () => modal.remove());

        modal.addEventListener('click', (e) => {
            if (e.target === modal) {
                modal.remove();
            }
        });

        document.body.appendChild(modal);
    }

    private escapeHtml(text: string): string {
        const div = document.createElement('div');
        div.textContent = text;
        return div.innerHTML;
    }
}

// Initialize when DOM is loaded
document.addEventListener('DOMContentLoaded', () => {
    // Get library items from global variable injected by template
    const libraryItems = (window as any).libraryItems || [];
    const library = new LibraryManager();
    library.initialize(libraryItems);
});