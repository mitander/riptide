/**
 * Torrents page functionality
 */

class TorrentsManager {
    constructor() {
        this.torrents = [];
        this.init();
    }

    init() {
        this.loadTorrents();
        this.setupAutoRefresh();
    }

    loadTorrents() {
        // This will be populated by the template with server data
        if (window.torrents) {
            this.torrents = window.torrents;
            this.renderTorrents();
        }
    }

    setupAutoRefresh() {
        // Refresh torrent list every 10 seconds
        setInterval(() => {
            this.refreshTorrents();
        }, 10000);
    }

    async refreshTorrents() {
        try {
            const response = await fetch('/api/torrents');
            const data = await response.json();
            this.torrents = data.torrents || [];
            this.renderTorrents();
        } catch (error) {
            console.error('Failed to refresh torrents:', error);
        }
    }

    renderTorrents() {
        const tbody = document.getElementById('torrents-tbody');
        if (!tbody) return;

        tbody.innerHTML = '';
        
        this.torrents.forEach(torrent => {
            const tr = document.createElement('tr');
            tr.innerHTML = `
                <td class="torrent-name" title="${this.escapeHtml(torrent.name)}">${this.truncateText(torrent.name, 50)}</td>
                <td><span class="status ${torrent.status.toLowerCase()}">${this.escapeHtml(torrent.status)}</span></td>
                <td>
                    <div class="progress-bar">
                        <div class="progress-fill" style="width: ${torrent.progress}%"></div>
                        <span class="progress-text">${torrent.progress.toFixed(1)}%</span>
                    </div>
                </td>
                <td>${this.formatSpeed(torrent.download_speed)}</td>
                <td>${this.formatSpeed(torrent.upload_speed)}</td>
                <td>${this.formatFileSize(torrent.size)}</td>
                <td>${torrent.ratio.toFixed(2)}</td>
                <td>${torrent.peers}/${torrent.seeds}</td>
                <td class="torrent-actions">
                    <button class="btn btn-sm" onclick="window.torrentsManager.toggleTorrent('${torrent.info_hash}')">
                        ${torrent.status === 'Downloading' ? 'Pause' : 'Resume'}
                    </button>
                    <button class="btn btn-sm btn-danger" onclick="window.torrentsManager.removeTorrent('${torrent.info_hash}')">Remove</button>
                </td>
            `;
            tbody.appendChild(tr);
        });

        if (this.torrents.length === 0) {
            tbody.innerHTML = '<tr><td colspan="9" class="no-torrents">No torrents found. <a href="/add-torrent">Add one now</a></td></tr>';
        }
    }

    async toggleTorrent(infoHash) {
        const torrent = this.torrents.find(t => t.info_hash === infoHash);
        if (!torrent) return;

        const action = torrent.status === 'Downloading' ? 'pause' : 'resume';
        
        try {
            const response = await fetch(`/api/torrents/${infoHash}/${action}`, {
                method: 'POST'
            });
            
            if (response.ok) {
                // Update local state immediately for better UX
                torrent.status = action === 'pause' ? 'Paused' : 'Downloading';
                this.renderTorrents();
                
                // Refresh from server to get accurate state
                setTimeout(() => this.refreshTorrents(), 1000);
            } else {
                const error = await response.text();
                this.showError(`Failed to ${action} torrent: ${error}`);
            }
        } catch (error) {
            this.showError(`Failed to ${action} torrent: ${error.message}`);
        }
    }

    async removeTorrent(infoHash) {
        const torrent = this.torrents.find(t => t.info_hash === infoHash);
        if (!torrent) return;

        if (!confirm(`Are you sure you want to remove "${torrent.name}"?`)) {
            return;
        }

        try {
            const response = await fetch(`/api/torrents/${infoHash}`, {
                method: 'DELETE'
            });
            
            if (response.ok) {
                // Remove from local state immediately
                this.torrents = this.torrents.filter(t => t.info_hash !== infoHash);
                this.renderTorrents();
                this.showSuccess('Torrent removed successfully');
            } else {
                const error = await response.text();
                this.showError(`Failed to remove torrent: ${error}`);
            }
        } catch (error) {
            this.showError(`Failed to remove torrent: ${error.message}`);
        }
    }

    formatSpeed(bytesPerSecond) {
        if (bytesPerSecond < 1024) return `${bytesPerSecond} B/s`;
        if (bytesPerSecond < 1048576) return `${(bytesPerSecond / 1024).toFixed(0)} KB/s`;
        return `${(bytesPerSecond / 1048576).toFixed(1)} MB/s`;
    }

    formatFileSize(bytes) {
        if (!bytes) return 'Unknown';
        
        const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
        const i = Math.floor(Math.log(bytes) / Math.log(1024));
        
        if (i === 0) return `${bytes} ${sizes[i]}`;
        return `${(bytes / Math.pow(1024, i)).toFixed(1)} ${sizes[i]}`;
    }

    truncateText(text, maxLength) {
        if (text.length <= maxLength) return this.escapeHtml(text);
        return this.escapeHtml(text.substring(0, maxLength)) + '...';
    }

    showError(message) {
        this.showNotification(message, 'error');
    }

    showSuccess(message) {
        this.showNotification(message, 'success');
    }

    showNotification(message, type) {
        // Create notification element
        const notification = document.createElement('div');
        notification.className = `notification ${type}`;
        notification.textContent = message;
        
        // Add to page
        document.body.appendChild(notification);
        
        // Remove after 3 seconds
        setTimeout(() => {
            notification.remove();
        }, 3000);
    }

    escapeHtml(text) {
        const div = document.createElement('div');
        div.textContent = text;
        return div.innerHTML;
    }
}

// Initialize torrents manager when DOM is ready
document.addEventListener('DOMContentLoaded', () => {
    window.torrentsManager = new TorrentsManager();
});