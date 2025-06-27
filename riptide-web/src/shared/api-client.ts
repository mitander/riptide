import { 
    TorrentInfo, 
    ServerStats, 
    AddTorrentRequest, 
    AddTorrentResult, 
    LibraryItem, 
    SearchResult, 
    Settings 
} from './types.js';

export class ApiClient {
    private baseUrl: string;

    constructor(baseUrl: string = '/api') {
        this.baseUrl = baseUrl;
    }

    // Torrent management
    public async getTorrents(): Promise<TorrentInfo[]> {
        const response = await fetch(`${this.baseUrl}/torrents`);
        if (!response.ok) {
            throw new Error(`Failed to fetch torrents: ${response.statusText}`);
        }
        const data = await response.json();
        return data.torrents || [];
    }

    public async addTorrent(magnetLink: string, startImmediately: boolean = true): Promise<AddTorrentResult> {
        const request: AddTorrentRequest = {
            magnet: magnetLink,
            start_immediately: startImmediately,
        };

        const response = await fetch(`${this.baseUrl}/torrents/add`, {
            method: 'POST',
            headers: {
                'Content-Type': 'application/json',
            },
            body: JSON.stringify(request),
        });

        if (!response.ok) {
            throw new Error(`Failed to add torrent: ${response.statusText}`);
        }

        const data = await response.json();
        return data.result;
    }

    public async pauseTorrent(torrentId: string): Promise<void> {
        const response = await fetch(`${this.baseUrl}/torrents/${torrentId}/pause`, {
            method: 'POST',
        });

        if (!response.ok) {
            throw new Error(`Failed to pause torrent: ${response.statusText}`);
        }
    }

    public async resumeTorrent(torrentId: string): Promise<void> {
        const response = await fetch(`${this.baseUrl}/torrents/${torrentId}/resume`, {
            method: 'POST',
        });

        if (!response.ok) {
            throw new Error(`Failed to resume torrent: ${response.statusText}`);
        }
    }

    public async deleteTorrent(torrentId: string): Promise<void> {
        const response = await fetch(`${this.baseUrl}/torrents/${torrentId}`, {
            method: 'DELETE',
        });

        if (!response.ok) {
            throw new Error(`Failed to delete torrent: ${response.statusText}`);
        }
    }

    // Server stats
    public async getServerStats(): Promise<ServerStats> {
        const response = await fetch(`${this.baseUrl}/stats`);
        if (!response.ok) {
            throw new Error(`Failed to fetch stats: ${response.statusText}`);
        }
        const data = await response.json();
        return data.stats;
    }

    // Library management
    public async getLibraryItems(): Promise<LibraryItem[]> {
        const response = await fetch(`${this.baseUrl}/library`);
        if (!response.ok) {
            throw new Error(`Failed to fetch library: ${response.statusText}`);
        }
        const data = await response.json();
        return data.items || [];
    }

    public async getMediaDetails(mediaId: string): Promise<LibraryItem> {
        const response = await fetch(`${this.baseUrl}/library/${mediaId}`);
        if (!response.ok) {
            throw new Error(`Failed to fetch media details: ${response.statusText}`);
        }
        const data = await response.json();
        return data.item;
    }

    // Search functionality
    public async searchMedia(query: string, category: string = 'all'): Promise<SearchResult[]> {
        let url = `${this.baseUrl}/search`;
        if (category === 'movie') {
            url = `${this.baseUrl}/search/movies`;
        } else if (category === 'tv') {
            url = `${this.baseUrl}/search/tv`;
        }

        const response = await fetch(`${url}?q=${encodeURIComponent(query)}`);
        if (!response.ok) {
            throw new Error(`Failed to search: ${response.statusText}`);
        }

        const data = await response.json();
        return data.results || [];
    }

    // Settings management
    public async getSettings(): Promise<Settings> {
        const response = await fetch(`${this.baseUrl}/settings`);
        if (!response.ok) {
            throw new Error(`Failed to fetch settings: ${response.statusText}`);
        }
        const data = await response.json();
        return data.settings;
    }

    public async updateSettings(settings: Settings): Promise<void> {
        const response = await fetch(`${this.baseUrl}/settings`, {
            method: 'PUT',
            headers: {
                'Content-Type': 'application/json',
            },
            body: JSON.stringify(settings),
        });

        if (!response.ok) {
            throw new Error(`Failed to update settings: ${response.statusText}`);
        }
    }

    public async resetSettings(): Promise<void> {
        const response = await fetch(`${this.baseUrl}/settings/reset`, {
            method: 'POST',
        });

        if (!response.ok) {
            throw new Error(`Failed to reset settings: ${response.statusText}`);
        }
    }
}