import { api } from '../api/client';
import type { TorrentProgress } from '../../types/api';
import { formatBytes } from '../utils/formatting';

/**
 * Torrent management component
 */
export class TorrentManager {
  private tableBody: HTMLTableSectionElement;
  private refreshInterval: number | null = null;

  constructor() {
    this.tableBody = document.getElementById('torrents-tbody') as HTMLTableSectionElement;
    if (!this.tableBody) {
      console.error('Torrents table body not found');
    }
  }

  /**
   * Initialize torrent manager
   */
  async initialize(): Promise<void> {
    try {
      await this.loadTorrents();
      this.startAutoRefresh();
    } catch (error) {
      console.error('Failed to initialize torrent manager:', error);
    }
  }

  /**
   * Load torrents from server and render table
   */
  async loadTorrents(): Promise<void> {
    try {
      const torrents = await api.getTorrents();
      this.renderTorrents(torrents);
    } catch (error) {
      console.error('Failed to load torrents:', error);
      this.showError('Failed to load torrent list');
    }
  }

  /**
   * Render torrents in table
   */
  private renderTorrents(torrents: TorrentProgress[]): void {
    if (!this.tableBody) return;

    // Clear existing rows
    this.tableBody.innerHTML = '';

    torrents.forEach((torrent) => {
      const row = this.createTorrentRow(torrent);
      this.tableBody.appendChild(row);
    });
  }

  /**
   * Create table row for torrent
   */
  private createTorrentRow(torrent: TorrentProgress): HTMLTableRowElement {
    const tr = document.createElement('tr');
    tr.dataset.infoHash = torrent.info_hash.hash;
    
    tr.innerHTML = `
      <td class="torrent-name">${this.escapeHtml(torrent.name)}</td>
      <td><span class="status ${torrent.status.toLowerCase()}">${torrent.status}</span></td>
      <td>
        <div class="progress-bar">
          <div class="progress-fill" style="width: ${torrent.progress}%"></div>
          <span class="progress-text">${torrent.progress.toFixed(1)}%</span>
        </div>
      </td>
      <td>${(torrent.download_speed / 1024).toFixed(0)} KB/s</td>
      <td>${(torrent.upload_speed / 1024).toFixed(0)} KB/s</td>
      <td>${formatBytes(torrent.size)}</td>
      <td>${torrent.ratio.toFixed(2)}</td>
      <td>${torrent.peers}/${torrent.seeds}</td>
      <td class="torrent-actions">
        <button class="btn btn-sm" onclick="torrentManager.pauseTorrent('${torrent.info_hash.hash}')">Pause</button>
        <button class="btn btn-sm btn-danger" onclick="torrentManager.removeTorrent('${torrent.info_hash.hash}')">Remove</button>
      </td>
    `;
    
    return tr;
  }

  /**
   * Pause a torrent
   */
  async pauseTorrent(infoHash: string): Promise<void> {
    try {
      await api.pauseTorrent(infoHash);
      await this.loadTorrents(); // Refresh list
    } catch (error) {
      console.error('Failed to pause torrent:', error);
      this.showError('Failed to pause torrent');
    }
  }

  /**
   * Remove a torrent
   */
  async removeTorrent(infoHash: string): Promise<void> {
    if (!confirm('Are you sure you want to remove this torrent?')) {
      return;
    }

    try {
      await api.removeTorrent(infoHash);
      await this.loadTorrents(); // Refresh list
    } catch (error) {
      console.error('Failed to remove torrent:', error);
      this.showError('Failed to remove torrent');
    }
  }

  /**
   * Refresh torrent list
   */
  async refreshTorrents(): Promise<void> {
    await this.loadTorrents();
  }

  /**
   * Start auto-refresh interval
   */
  private startAutoRefresh(): void {
    // Refresh every 5 seconds
    this.refreshInterval = window.setInterval(() => {
      this.loadTorrents();
    }, 5000);
  }

  /**
   * Stop auto-refresh interval
   */
  private stopAutoRefresh(): void {
    if (this.refreshInterval) {
      clearInterval(this.refreshInterval);
      this.refreshInterval = null;
    }
  }

  /**
   * Show error message
   */
  private showError(message: string): void {
    // You could implement a toast notification system here
    console.error(message);
    alert(message); // Simple fallback
  }

  /**
   * Escape HTML to prevent XSS
   */
  private escapeHtml(text: string): string {
    const div = document.createElement('div');
    div.textContent = text;
    return div.innerHTML;
  }

  /**
   * Cleanup when component is destroyed
   */
  destroy(): void {
    this.stopAutoRefresh();
  }
}

/**
 * Initialize torrent manager when DOM is ready
 */
export function initializeTorrentManager(): void {
  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', () => {
      const manager = new TorrentManager();
      manager.initialize();
      
      // Make available globally for onclick handlers
      (window as any).torrentManager = manager;
    });
  } else {
    const manager = new TorrentManager();
    manager.initialize();
    (window as any).torrentManager = manager;
  }
}

/**
 * Global function for refresh button
 */
(window as any).refreshTorrents = () => {
  const manager = (window as any).torrentManager as TorrentManager;
  if (manager) {
    manager.refreshTorrents();
  }
};