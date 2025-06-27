import type {
  TorrentAddResult,
  TorrentProgress,
  MediaItem,
  SearchResult,
  ServerStats,
  ActivityItem,
  ServerSettings,
} from '../../types/api';

/**
 * Type-safe API client for Riptide server
 */
export class RiptideApi {
  // Future: private baseUrl for WebSocket connections, etc.
  
  constructor(_baseUrl = '') {
    // TODO: Store baseUrl when needed for WebSocket connections
  }

  /**
   * Add a new torrent via magnet link
   */
  async addTorrent(magnetLink: string): Promise<TorrentAddResult> {
    const response = await fetch(
      `/api/torrents/add?magnet=${encodeURIComponent(magnetLink)}`
    );
    const data = await response.json();
    return data.result as TorrentAddResult;
  }

  /**
   * Get list of active torrents
   */
  async getTorrents(): Promise<TorrentProgress[]> {
    const response = await fetch('/api/torrents');
    if (!response.ok) {
      throw new Error(`Failed to fetch torrents: ${response.statusText}`);
    }
    const data = await response.json();
    return data.torrents as TorrentProgress[];
  }

  /**
   * Pause a torrent
   */
  async pauseTorrent(infoHash: string): Promise<void> {
    const response = await fetch(`/api/torrents/${infoHash}/pause`, {
      method: 'POST',
    });
    if (!response.ok) {
      throw new Error(`Failed to pause torrent: ${response.statusText}`);
    }
  }

  /**
   * Remove a torrent
   */
  async removeTorrent(infoHash: string): Promise<void> {
    const response = await fetch(`/api/torrents/${infoHash}`, {
      method: 'DELETE',
    });
    if (!response.ok) {
      throw new Error(`Failed to remove torrent: ${response.statusText}`);
    }
  }

  /**
   * Get media library items
   */
  async getLibraryItems(): Promise<MediaItem[]> {
    const response = await fetch('/api/library');
    if (!response.ok) {
      throw new Error(`Failed to fetch library: ${response.statusText}`);
    }
    const data = await response.json();
    return data.items as MediaItem[];
  }

  /**
   * Search for media
   */
  async searchMedia(query: string, category: 'all' | 'movie' | 'tv' = 'all'): Promise<SearchResult[]> {
    let url = '/api/search';
    if (category === 'movie') {
      url = '/api/search/movies';
    } else if (category === 'tv') {
      url = '/api/search/tv';
    }

    const response = await fetch(`${url}?q=${encodeURIComponent(query)}`);
    if (!response.ok) {
      throw new Error(`Search failed: ${response.statusText}`);
    }
    const data = await response.json();
    return data.results as SearchResult[];
  }

  /**
   * Get server statistics
   */
  async getServerStats(): Promise<ServerStats> {
    const response = await fetch('/api/stats');
    if (!response.ok) {
      throw new Error(`Failed to fetch stats: ${response.statusText}`);
    }
    const data = await response.json();
    return data.stats as ServerStats;
  }

  /**
   * Get recent activity
   */
  async getRecentActivity(): Promise<ActivityItem[]> {
    const response = await fetch('/api/activity');
    if (!response.ok) {
      throw new Error(`Failed to fetch activity: ${response.statusText}`);
    }
    const data = await response.json();
    return data.activity as ActivityItem[];
  }

  /**
   * Get server settings
   */
  async getSettings(): Promise<ServerSettings> {
    const response = await fetch('/api/settings');
    if (!response.ok) {
      throw new Error(`Failed to fetch settings: ${response.statusText}`);
    }
    const data = await response.json();
    return data.settings as ServerSettings;
  }

  /**
   * Save server settings
   */
  async saveSettings(settings: ServerSettings): Promise<void> {
    const response = await fetch('/api/settings', {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
      },
      body: JSON.stringify(settings),
    });
    if (!response.ok) {
      throw new Error(`Failed to save settings: ${response.statusText}`);
    }
  }
}

// Global API client instance
export const api = new RiptideApi();