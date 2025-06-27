/**
 * Shared type definitions for Riptide API
 * These types mirror the Rust structs used in the backend
 */

// Core torrent types
export interface InfoHash {
  hash: string;
}

export interface TorrentMetadata {
  info_hash: InfoHash;
  name: string;
  total_size: number;
  piece_length: number;
  piece_count: number;
  files: TorrentFile[];
}

export interface TorrentFile {
  path: string;
  length: number;
}

// Torrent status and progress
export interface TorrentProgress {
  info_hash: InfoHash;
  name: string;
  status: TorrentStatus;
  progress: number; // 0.0 to 100.0
  download_speed: number; // bytes per second
  upload_speed: number; // bytes per second
  size: number;
  ratio: number;
  peers: number;
  seeds: number;
}

export type TorrentStatus = 
  | 'Downloading'
  | 'Seeding'
  | 'Paused'
  | 'Error'
  | 'Completed';

// Media library types
export interface MediaItem {
  id: string;
  title: string;
  media_type: MediaType;
  size: number;
  duration?: number; // seconds
  thumbnail_url?: string;
  stream_url: string;
  info_hash?: InfoHash;
}

export type MediaType = 'Movie' | 'TvShow' | 'Music' | 'Other';

// Search results
export interface SearchResult {
  title: string;
  year?: number;
  rating?: number;
  genre?: string;
  plot?: string;
  poster_url?: string;
  media_type: MediaType;
  torrents: SearchTorrent[];
}

export interface SearchTorrent {
  quality: string;
  size: number;
  seeders: number;
  magnet_link: string;
}

// Server statistics
export interface ServerStats {
  total_torrents: number;
  active_streams: number;
  download_speed: number; // KB/s
  upload_speed: number; // KB/s
}

// Activity log
export interface ActivityItem {
  timestamp: string; // ISO date
  activity_type: ActivityType;
  description: string;
}

export type ActivityType = 
  | 'download_started'
  | 'download_completed'
  | 'stream_started'
  | 'stream_ended'
  | 'error';

// Server settings
export interface ServerSettings {
  download_limit?: number; // KB/s, undefined = unlimited
  upload_limit?: number; // KB/s, undefined = unlimited
  max_connections: number;
  streaming_port: number;
  web_ui_port: number;
  enable_upnp: boolean;
  enable_dht: boolean;
  enable_pex: boolean;
}

// API response wrappers
export interface ApiResponse<T> {
  success: boolean;
  data?: T;
  error?: string;
}

export interface TorrentAddResult {
  success: boolean;
  message: string;
  stream_url?: string;
  info_hash?: InfoHash;
}

// WebSocket message types
export interface WebSocketMessage {
  type: WebSocketMessageType;
  data: unknown;
}

export type WebSocketMessageType = 
  | 'torrent_progress'
  | 'stream_started'
  | 'stream_ended'
  | 'server_stats';

export interface TorrentProgressMessage {
  type: 'torrent_progress';
  data: TorrentProgress;
}

export interface ServerStatsMessage {
  type: 'server_stats';
  data: ServerStats;
}