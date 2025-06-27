// Shared type definitions for Riptide Web API

export interface TorrentInfo {
    id: string;
    name: string;
    info_hash: string;
    status: 'downloading' | 'seeding' | 'paused' | 'error';
    progress: number;
    download_speed: number;
    upload_speed: number;
    size: number;
    downloaded: number;
    uploaded: number;
    ratio: number;
    peer_count: number;
    created_at: string;
    updated_at: string;
}

export interface ServerStats {
    total_torrents: number;
    active_streams: number;
    download_speed: number;
    upload_speed: number;
}

export interface AddTorrentRequest {
    magnet: string;
    start_immediately?: boolean;
}

export interface AddTorrentResult {
    success: boolean;
    message: string;
    torrent_id?: string;
    stream_url?: string;
}

export interface LibraryItem {
    id: string;
    title: string;
    media_type: string;
    size: number;
    thumbnail_url?: string;
    duration?: number;
    stream_url: string;
}

export interface SearchResult {
    title: string;
    year?: number;
    rating?: number;
    genre?: string;
    media_type: string;
    plot?: string;
    poster_url?: string;
    torrents: TorrentItem[];
}

export interface TorrentItem {
    quality: string;
    size: number;
    seeders: number;
    magnet_link: string;
}

export interface Settings {
    download_limit?: number;
    upload_limit?: number;
    max_connections: number;
    streaming_port: number;
    web_ui_port: number;
    enable_upnp: boolean;
    enable_dht: boolean;
    enable_pex: boolean;
}

// Re-export display formatting functions
export { formatBytes, formatSpeed, formatProgress } from '../display/formatting.js';