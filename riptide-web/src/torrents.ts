import { ApiClient } from './shared/api-client.js';
import { TorrentInfo, formatBytes, formatSpeed, formatProgress } from './shared/types.js';

export class TorrentManager {
    private apiClient: ApiClient;
    private refreshInterval: number | null = null;

    constructor() {
        this.apiClient = new ApiClient();
    }

    public async initialize(): Promise<void> {
        this.setupEventListeners();
        await this.refreshTorrents();
        this.startAutoRefresh();
    }

    private setupEventListeners(): void {
        const refreshButton = document.getElementById('refresh-torrents') as HTMLButtonElement;
        if (refreshButton) {
            refreshButton.addEventListener('click', () => this.refreshTorrents());
        }
    }

    public async refreshTorrents(): Promise<void> {
        try {
            const torrents = await this.apiClient.getTorrents();
            this.renderTorrents(torrents);
        } catch (error) {
            console.error('Failed to refresh torrents:', error);
            this.showError('Failed to load torrents');
        }
    }

    private renderTorrents(torrents: TorrentInfo[]): void {
        const tbody = document.getElementById('torrents-tbody');
        if (!tbody) return;

        tbody.innerHTML = '';

        if (torrents.length === 0) {
            tbody.innerHTML = `
                <tr>
                    <td colspan="9" class="no-torrents">
                        No torrents found. <a href="/add-torrent">Add your first torrent</a>
                    </td>
                </tr>
            `;
            return;
        }

        torrents.forEach(torrent => {
            const row = document.createElement('tr');
            row.className = `torrent-row torrent-${torrent.status}`;

            const statusClass = this.getStatusClass(torrent.status);
            const progressBar = this.createProgressBar(torrent.progress);

            row.innerHTML = `
                <td class="torrent-name" title="${this.escapeHtml(torrent.name)}">
                    ${this.escapeHtml(this.truncateText(torrent.name, 40))}
                </td>
                <td class="torrent-status">
                    <span class="status-badge ${statusClass}">${this.capitalizeFirst(torrent.status)}</span>
                </td>
                <td class="torrent-progress">
                    ${progressBar}
                    <span class="progress-text">${formatProgress(torrent.progress)}</span>
                </td>
                <td class="torrent-download-speed">${formatSpeed(torrent.download_speed)}</td>
                <td class="torrent-upload-speed">${formatSpeed(torrent.upload_speed)}</td>
                <td class="torrent-size">${formatBytes(torrent.size)}</td>
                <td class="torrent-ratio">${torrent.ratio.toFixed(2)}</td>
                <td class="torrent-peers">${torrent.peer_count}</td>
                <td class="torrent-actions">
                    ${this.createActionButtons(torrent)}
                </td>
            `;

            // Add event listeners for action buttons
            this.addActionListeners(row, torrent);

            tbody.appendChild(row);
        });
    }

    private getStatusClass(status: string): string {
        const statusClasses: Record<string, string> = {
            'downloading': 'status-downloading',
            'seeding': 'status-seeding',
            'paused': 'status-paused',
            'error': 'status-error',
        };
        return statusClasses[status] || 'status-unknown';
    }

    private createProgressBar(progress: number): string {
        const percentage = Math.round(progress * 100);
        return `
            <div class="progress-bar">
                <div class="progress-fill" style="width: ${percentage}%"></div>
            </div>
        `;
    }

    private createActionButtons(torrent: TorrentInfo): string {
        const buttons = [];

        if (torrent.status === 'paused') {
            buttons.push('<button class="btn btn-sm btn-success" data-action="resume">Resume</button>');
        } else if (torrent.status === 'downloading' || torrent.status === 'seeding') {
            buttons.push('<button class="btn btn-sm btn-warning" data-action="pause">Pause</button>');
        }

        if (torrent.progress >= 1.0) {
            buttons.push('<button class="btn btn-sm btn-primary" data-action="stream">Stream</button>');
        }

        buttons.push('<button class="btn btn-sm btn-danger" data-action="delete">Delete</button>');

        return buttons.join(' ');
    }

    private addActionListeners(row: HTMLTableRowElement, torrent: TorrentInfo): void {
        const actionButtons = row.querySelectorAll('[data-action]');
        
        actionButtons.forEach(button => {
            button.addEventListener('click', async (e) => {
                const action = (e.target as HTMLElement).getAttribute('data-action');
                await this.handleTorrentAction(torrent, action);
            });
        });
    }

    private async handleTorrentAction(torrent: TorrentInfo, action: string | null): Promise<void> {
        if (!action) return;

        try {
            switch (action) {
                case 'pause':
                    await this.apiClient.pauseTorrent(torrent.id);
                    await this.refreshTorrents();
                    break;
                case 'resume':
                    await this.apiClient.resumeTorrent(torrent.id);
                    await this.refreshTorrents();
                    break;
                case 'delete':
                    if (confirm(`Delete torrent "${torrent.name}"?`)) {
                        await this.apiClient.deleteTorrent(torrent.id);
                        await this.refreshTorrents();
                    }
                    break;
                case 'stream':
                    // Create stream URL - this would need to be provided by the API
                    const streamUrl = `/stream/${torrent.id}`;
                    window.open(streamUrl, '_blank');
                    break;
                default:
                    console.warn('Unknown action:', action);
            }
        } catch (error) {
            const errorMessage = error instanceof Error ? error.message : 'Unknown error';
            alert(`Failed to ${action} torrent: ${errorMessage}`);
        }
    }

    private startAutoRefresh(): void {
        // Refresh every 2 seconds
        this.refreshInterval = window.setInterval(() => {
            this.refreshTorrents();
        }, 2000);
    }

    public stopAutoRefresh(): void {
        if (this.refreshInterval) {
            clearInterval(this.refreshInterval);
            this.refreshInterval = null;
        }
    }

    private showError(message: string): void {
        const tbody = document.getElementById('torrents-tbody');
        if (tbody) {
            tbody.innerHTML = `
                <tr>
                    <td colspan="9" class="error-message">
                        ${this.escapeHtml(message)}
                        <button class="btn btn-sm btn-primary" onclick="location.reload()">Retry</button>
                    </td>
                </tr>
            `;
        }
    }

    private truncateText(text: string, maxLength: number): string {
        if (text.length <= maxLength) return text;
        return text.substring(0, maxLength - 3) + '...';
    }

    private capitalizeFirst(text: string): string {
        return text.charAt(0).toUpperCase() + text.slice(1);
    }

    private escapeHtml(text: string): string {
        const div = document.createElement('div');
        div.textContent = text;
        return div.innerHTML;
    }
}

// Expose refresh function globally for backward compatibility
(window as any).refreshTorrents = () => {
    const manager = (window as any).torrentManager;
    if (manager) {
        manager.refreshTorrents();
    }
};

// Initialize when DOM is loaded
document.addEventListener('DOMContentLoaded', async () => {
    const manager = new TorrentManager();
    (window as any).torrentManager = manager; // Store globally for refresh function
    await manager.initialize();
});

// Clean up on page unload
window.addEventListener('beforeunload', () => {
    const manager = (window as any).torrentManager;
    if (manager) {
        manager.stopAutoRefresh();
    }
});