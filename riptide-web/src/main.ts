/**
 * Main entry point for Riptide web UI TypeScript
 * This file coordinates all components and provides global functionality
 */

import { initializeDashboard } from './components/dashboard';
import { initializeTorrentManager } from './components/torrents';
import { api } from './api/client';
import { debounce } from './display/formatting';

/**
 * Application state and global functionality
 */
class RiptideApp {
  private currentPage: string;

  constructor() {
    this.currentPage = this.detectCurrentPage();
    this.initialize();
  }

  /**
   * Detect current page from URL or body class
   */
  private detectCurrentPage(): string {
    const path = window.location.pathname;
    
    if (path === '/' || path === '/home') return 'home';
    if (path.includes('/library')) return 'library';
    if (path.includes('/torrents')) return 'torrents';
    if (path.includes('/add-torrent')) return 'add-torrent';
    if (path.includes('/search')) return 'search';
    if (path.includes('/settings')) return 'settings';
    
    return 'unknown';
  }

  /**
   * Initialize the application
   */
  private initialize(): void {
    console.log(`Initializing Riptide UI for page: ${this.currentPage}`);
    
    // Initialize page-specific components
    switch (this.currentPage) {
      case 'home':
        initializeDashboard();
        break;
      case 'torrents':
        initializeTorrentManager();
        break;
      case 'library':
        this.initializeLibrary();
        break;
      case 'search':
        this.initializeSearch();
        break;
      case 'add-torrent':
        this.initializeAddTorrent();
        break;
      case 'settings':
        this.initializeSettings();
        break;
    }
    
    // Initialize global functionality
    this.initializeGlobalFeatures();
  }

  /**
   * Initialize library page
   */
  private initializeLibrary(): void {
    const searchInput = document.getElementById('search') as HTMLInputElement;
    const typeFilter = document.getElementById('type-filter') as HTMLSelectElement;
    const mediaGrid = document.getElementById('media-grid') as HTMLElement;
    
    if (!searchInput || !typeFilter || !mediaGrid) {
      console.error('Library elements not found');
      return;
    }

    // Load library items
    this.loadLibraryItems();
    
    // Set up search functionality
    const debouncedSearch = debounce(() => {
      this.filterLibraryItems();
    }, 300);
    
    searchInput.addEventListener('input', debouncedSearch);
    typeFilter.addEventListener('change', () => this.filterLibraryItems());
  }

  /**
   * Initialize search page
   */
  private initializeSearch(): void {
    const searchForm = document.querySelector('.search-input-form') as HTMLFormElement;
    const searchQuery = document.getElementById('search-query') as HTMLInputElement;
    
    if (!searchForm || !searchQuery) {
      console.error('Search elements not found');
      return;
    }

    // Auto-focus search input
    searchQuery.focus();
    
    // Set up search form submission
    searchForm.addEventListener('submit', (event) => {
      this.handleSearchSubmit(event);
    });
  }

  /**
   * Initialize add torrent page
   */
  private initializeAddTorrent(): void {
    const form = document.querySelector('.add-torrent-form') as HTMLFormElement;
    
    if (!form) {
      console.error('Add torrent form not found');
      return;
    }

    form.addEventListener('submit', (event) => {
      this.handleAddTorrent(event);
    });
  }

  /**
   * Initialize settings page
   */
  private initializeSettings(): void {
    const form = document.querySelector('.settings-form') as HTMLFormElement;
    
    if (!form) {
      console.error('Settings form not found');
      return;
    }

    // Load current settings
    this.loadSettings();
    
    form.addEventListener('submit', (event) => {
      this.handleSaveSettings(event);
    });
  }

  /**
   * Initialize global features available on all pages
   */
  private initializeGlobalFeatures(): void {
    // Add any global event listeners or functionality here
    // For example: keyboard shortcuts, notifications, etc.
  }

  /**
   * Load and display library items
   */
  private async loadLibraryItems(): Promise<void> {
    try {
      const items = await api.getLibraryItems();
      // Store items for filtering
      (window as any).libraryItems = items;
      this.renderLibraryItems(items);
    } catch (error) {
      console.error('Failed to load library items:', error);
    }
  }

  /**
   * Render library items in grid
   */
  private renderLibraryItems(_items: any[]): void {
    const mediaGrid = document.getElementById('media-grid');
    if (!mediaGrid) return;
    
    mediaGrid.innerHTML = '';
    // TODO: Implementation will be similar to the original JS but with types
  }

  /**
   * Filter library items based on search and type
   */
  private filterLibraryItems(): void {
    // TODO: Implement filtering logic
    console.log('Filtering library items...');
  }

  /**
   * Handle search form submission
   */
  private async handleSearchSubmit(event: Event): Promise<void> {
    event.preventDefault();
    
    const form = event.target as HTMLFormElement;
    const formData = new FormData(form);
    const query = formData.get('query') as string;
    const category = formData.get('category') as string;
    
    if (!query.trim()) return;
    
    this.showSearchLoading(true);
    
    try {
      const results = await api.searchMedia(query, category as any);
      this.renderSearchResults(results);
    } catch (error) {
      console.error('Search failed:', error);
      this.showSearchError('Search failed. Please try again.');
    } finally {
      this.showSearchLoading(false);
    }
  }

  /**
   * Handle add torrent form submission
   */
  private async handleAddTorrent(event: Event): Promise<void> {
    event.preventDefault();
    
    const form = event.target as HTMLFormElement;
    const formData = new FormData(form);
    const magnetLink = formData.get('magnet') as string;
    
    try {
      const result = await api.addTorrent(magnetLink);
      this.showAddTorrentResult(result);
      
      if (result.success) {
        // Clear form
        form.reset();
      }
    } catch (error) {
      console.error('Failed to add torrent:', error);
      this.showAddTorrentResult({
        success: false,
        message: 'Failed to add torrent: ' + (error as Error).message,
      });
    }
  }

  /**
   * Handle settings form submission
   */
  private async handleSaveSettings(event: Event): Promise<void> {
    event.preventDefault();
    
    const form = event.target as HTMLFormElement;
    const formData = new FormData(form);
    
    // Convert form data to settings object
    const settings = {
      download_limit: formData.get('download_limit') ? Number(formData.get('download_limit')) : undefined,
      upload_limit: formData.get('upload_limit') ? Number(formData.get('upload_limit')) : undefined,
      max_connections: Number(formData.get('max_connections')),
      streaming_port: Number(formData.get('streaming_port')),
      web_ui_port: Number(formData.get('web_ui_port')),
      enable_upnp: formData.has('enable_upnp'),
      enable_dht: formData.has('enable_dht'),
      enable_pex: formData.has('enable_pex'),
    };
    
    try {
      await api.saveSettings(settings);
      this.showSettingsResult('Settings saved successfully!', 'success');
    } catch (error) {
      console.error('Failed to save settings:', error);
      this.showSettingsResult('Failed to save settings', 'error');
    }
  }

  /**
   * Load current settings
   */
  private async loadSettings(): Promise<void> {
    try {
      const settings = await api.getSettings();
      this.populateSettingsForm(settings);
    } catch (error) {
      console.error('Failed to load settings:', error);
    }
  }

  // Placeholder methods for UI updates
  private showSearchLoading(_show: boolean): void { /* TODO: Implementation */ }
  private renderSearchResults(_results: any[]): void { /* TODO: Implementation */ }
  private showSearchError(_message: string): void { /* TODO: Implementation */ }
  private showAddTorrentResult(_result: any): void { /* TODO: Implementation */ }
  private showSettingsResult(_message: string, _type: string): void { /* TODO: Implementation */ }
  private populateSettingsForm(_settings: any): void { /* TODO: Implementation */ }
}

// Initialize the application
const app = new RiptideApp();

// Make API available globally for console debugging
(window as any).riptideApi = api;
(window as any).riptideApp = app;

export default app;