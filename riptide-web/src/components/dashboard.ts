import { api } from '../api/client';
import type { ActivityItem, ServerStats } from '../../types/api';
import { formatTimestamp } from '../utils/formatting';

/**
 * Dashboard component logic for home page
 */
export class Dashboard {
  private activityListElement: HTMLElement;
  private statsElements: Map<string, HTMLElement>;

  constructor() {
    this.activityListElement = document.querySelector('.activity-list') as HTMLElement;
    this.statsElements = new Map();
    
    // Cache stat value elements
    const statCards = document.querySelectorAll('.stat-card');
    statCards.forEach((card) => {
      const title = card.querySelector('h3')?.textContent?.toLowerCase();
      const valueElement = card.querySelector('.stat-value') as HTMLElement;
      if (title && valueElement) {
        this.statsElements.set(title, valueElement);
      }
    });
  }

  /**
   * Initialize dashboard with data from server
   */
  async initialize(): Promise<void> {
    try {
      await Promise.all([
        this.loadServerStats(),
        this.loadRecentActivity(),
      ]);
    } catch (error) {
      console.error('Failed to initialize dashboard:', error);
      this.showError('Failed to load dashboard data');
    }
  }

  /**
   * Load and display server statistics
   */
  private async loadServerStats(): Promise<void> {
    const stats = await api.getServerStats();
    this.updateStats(stats);
  }

  /**
   * Load and display recent activity
   */
  private async loadRecentActivity(): Promise<void> {
    const activities = await api.getRecentActivity();
    this.renderActivityList(activities);
  }

  /**
   * Update statistics display
   */
  private updateStats(stats: ServerStats): void {
    const updates = [
      { key: 'active torrents', value: stats.total_torrents.toString() },
      { key: 'active streams', value: stats.active_streams.toString() },
      { key: 'download speed', value: `${stats.download_speed} KB/s` },
      { key: 'upload speed', value: `${stats.upload_speed} KB/s` },
    ];

    updates.forEach(({ key, value }) => {
      const element = this.statsElements.get(key);
      if (element) {
        element.textContent = value;
      }
    });
  }

  /**
   * Render activity list
   */
  private renderActivityList(activities: ActivityItem[]): void {
    if (!this.activityListElement) {
      console.warn('Activity list element not found');
      return;
    }

    // Clear existing content
    this.activityListElement.innerHTML = '';

    activities.forEach((activity) => {
      const activityElement = this.createActivityElement(activity);
      this.activityListElement.appendChild(activityElement);
    });
  }

  /**
   * Create activity item element
   */
  private createActivityElement(activity: ActivityItem): HTMLElement {
    const div = document.createElement('div');
    div.className = 'activity-item';
    
    div.innerHTML = `
      <div class="activity-icon ${activity.activity_type}"></div>
      <div class="activity-content">
        <p class="activity-description">${this.escapeHtml(activity.description)}</p>
        <p class="activity-time">${formatTimestamp(activity.timestamp)}</p>
      </div>
    `;
    
    return div;
  }

  /**
   * Show error message to user
   */
  private showError(message: string): void {
    // Create or update error banner
    let errorBanner = document.querySelector('.error-banner') as HTMLElement;
    if (!errorBanner) {
      errorBanner = document.createElement('div');
      errorBanner.className = 'error-banner';
      document.querySelector('.dashboard')?.prepend(errorBanner);
    }
    
    errorBanner.textContent = message;
    errorBanner.style.display = 'block';
    
    // Auto-hide after 5 seconds
    setTimeout(() => {
      errorBanner.style.display = 'none';
    }, 5000);
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
   * Refresh dashboard data
   */
  async refresh(): Promise<void> {
    await this.initialize();
  }
}

/**
 * Initialize dashboard when DOM is ready
 */
export function initializeDashboard(): void {
  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', () => {
      const dashboard = new Dashboard();
      dashboard.initialize();
      
      // Store reference for potential external access
      (window as any).dashboard = dashboard;
    });
  } else {
    const dashboard = new Dashboard();
    dashboard.initialize();
    (window as any).dashboard = dashboard;
  }
}