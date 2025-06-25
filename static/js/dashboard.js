/**
 * Dashboard page functionality
 */

class Dashboard {
    constructor() {
        this.activities = [];
        this.init();
    }

    init() {
        this.loadActivities();
        this.setupAutoRefresh();
    }

    loadActivities() {
        // This will be populated by the template with server data
        if (window.recentActivity) {
            this.activities = window.recentActivity;
            this.renderActivities();
        }
    }

    renderActivities() {
        const activityList = document.querySelector('.activity-list');
        if (!activityList) return;

        activityList.innerHTML = '';
        
        this.activities.forEach(activity => {
            const div = document.createElement('div');
            div.className = 'activity-item';
            div.innerHTML = `
                <div class="activity-icon ${activity.activity_type}"></div>
                <div class="activity-content">
                    <p class="activity-description">${this.escapeHtml(activity.description)}</p>
                    <p class="activity-time">${new Date(activity.timestamp).toLocaleString()}</p>
                </div>
            `;
            activityList.appendChild(div);
        });
    }

    setupAutoRefresh() {
        // Refresh dashboard stats every 30 seconds
        setInterval(() => {
            this.refreshStats();
        }, 30000);
    }

    async refreshStats() {
        try {
            const response = await fetch('/api/stats');
            const stats = await response.json();
            this.updateStatsDisplay(stats);
        } catch (error) {
            console.error('Failed to refresh stats:', error);
        }
    }

    updateStatsDisplay(stats) {
        const elements = {
            totalTorrents: document.querySelector('[data-stat="total_torrents"]'),
            activeStreams: document.querySelector('[data-stat="active_streams"]'),
            downloadSpeed: document.querySelector('[data-stat="download_speed"]'),
            uploadSpeed: document.querySelector('[data-stat="upload_speed"]')
        };

        if (elements.totalTorrents) elements.totalTorrents.textContent = stats.total_torrents;
        if (elements.activeStreams) elements.activeStreams.textContent = stats.active_streams;
        if (elements.downloadSpeed) elements.downloadSpeed.textContent = `${stats.download_speed} KB/s`;
        if (elements.uploadSpeed) elements.uploadSpeed.textContent = `${stats.upload_speed} KB/s`;
    }

    escapeHtml(text) {
        const div = document.createElement('div');
        div.textContent = text;
        return div.innerHTML;
    }
}

// Initialize dashboard when DOM is ready
document.addEventListener('DOMContentLoaded', () => {
    new Dashboard();
});