//! JavaScript functionality for the Riptide web UI

pub const JS_CONTENT: &str = r#"// Riptide Media Server Web UI JavaScript

document.addEventListener('DOMContentLoaded', function() {
    console.log('Riptide Web UI loaded');
    
    // Initialize components
    initializeNavigation();
    initializeAutoRefresh();
    initializeNotifications();
});

// Navigation handling
function initializeNavigation() {
    const currentPage = document.body.getAttribute('data-page') || 
                       window.location.pathname.split('/')[1] || 'home';
    
    const navLinks = document.querySelectorAll('.nav-link');
    navLinks.forEach(link => {
        const href = link.getAttribute('href');
        if ((href === '/' && currentPage === 'home') || 
            href === `/${currentPage}`) {
            link.classList.add('active');
        }
    });
}

// Auto-refresh for dynamic content
function initializeAutoRefresh() {
    if (window.location.pathname === '/torrents') {
        setInterval(refreshTorrents, 30000); // Refresh every 30 seconds
    }
    
    if (window.location.pathname === '/') {
        setInterval(refreshDashboard, 60000); // Refresh every minute
    }
}

// Refresh torrent list
async function refreshTorrents() {
    try {
        const response = await fetch('/api/torrents');
        const data = await response.json();
        
        if (data.torrents) {
            updateTorrentTable(data.torrents);
        }
    } catch (error) {
        console.error('Failed to refresh torrents:', error);
    }
}

function updateTorrentTable(torrents) {
    const tbody = document.getElementById('torrents-tbody');
    if (!tbody) return;
    
    tbody.innerHTML = '';
    torrents.forEach(torrent => {
        const tr = document.createElement('tr');
        tr.innerHTML = `
            <td class="torrent-name">${escapeHtml(torrent.name)}</td>
            <td><span class="status ${torrent.status.toLowerCase()}">${torrent.status}</span></td>
            <td>
                <div class="progress-bar">
                    <div class="progress-fill" style="width: ${torrent.progress}%"></div>
                    <span class="progress-text">${torrent.progress.toFixed(1)}%</span>
                </div>
            </td>
            <td>${formatSpeed(torrent.download_speed)}</td>
            <td>${formatSpeed(torrent.upload_speed)}</td>
            <td>${formatSize(torrent.size)}</td>
            <td>${torrent.ratio.toFixed(2)}</td>
            <td>${torrent.peers}/${torrent.seeds}</td>
            <td class="torrent-actions">
                <button class="btn btn-sm" onclick="pauseTorrent('${torrent.info_hash}')">Pause</button>
                <button class="btn btn-sm btn-danger" onclick="removeTorrent('${torrent.info_hash}')">Remove</button>
            </td>
        `;
        tbody.appendChild(tr);
    });
}

// Refresh dashboard
async function refreshDashboard() {
    try {
        const response = await fetch('/api/stats');
        const data = await response.json();
        
        updateDashboardStats(data);
    } catch (error) {
        console.error('Failed to refresh dashboard:', error);
    }
}

function updateDashboardStats(stats) {
    const statCards = document.querySelectorAll('.stat-card');
    if (statCards.length >= 4) {
        statCards[0].querySelector('.stat-value').textContent = stats.total_torrents;
        statCards[1].querySelector('.stat-value').textContent = stats.active_streams;
        statCards[2].querySelector('.stat-value').textContent = formatSpeed(stats.download_speed);
        statCards[3].querySelector('.stat-value').textContent = formatSpeed(stats.upload_speed);
    }
}

// Torrent actions
async function pauseTorrent(infoHash) {
    try {
        const response = await fetch(`/api/torrents/${infoHash}/pause`, { method: 'POST' });
        if (response.ok) {
            showNotification('Torrent paused', 'success');
            refreshTorrents();
        } else {
            showNotification('Failed to pause torrent', 'error');
        }
    } catch (error) {
        showNotification('Error: ' + error.message, 'error');
    }
}

async function removeTorrent(infoHash) {
    if (!confirm('Are you sure you want to remove this torrent?')) {
        return;
    }
    
    try {
        const response = await fetch(`/api/torrents/${infoHash}`, { method: 'DELETE' });
        if (response.ok) {
            showNotification('Torrent removed', 'success');
            refreshTorrents();
        } else {
            showNotification('Failed to remove torrent', 'error');
        }
    } catch (error) {
        showNotification('Error: ' + error.message, 'error');
    }
}

// Media details modal
function showMediaDetails(mediaId) {
    // Simple implementation - could be enhanced with modal dialogs
    console.log('Show details for media:', mediaId);
    // For now, just log - could implement modal or navigate to detail page
}

// Notification system
function initializeNotifications() {
    // Create notification container if it doesn't exist
    if (!document.getElementById('notifications')) {
        const container = document.createElement('div');
        container.id = 'notifications';
        container.style.cssText = `
            position: fixed;
            top: 20px;
            right: 20px;
            z-index: 1000;
            max-width: 400px;
        `;
        document.body.appendChild(container);
    }
}

function showNotification(message, type = 'info') {
    const container = document.getElementById('notifications');
    if (!container) return;
    
    const notification = document.createElement('div');
    notification.className = `notification notification-${type}`;
    notification.style.cssText = `
        background: ${type === 'success' ? '#dcfce7' : type === 'error' ? '#fee2e2' : '#dbeafe'};
        color: ${type === 'success' ? '#059669' : type === 'error' ? '#dc2626' : '#2563eb'};
        border: 1px solid ${type === 'success' ? '#bbf7d0' : type === 'error' ? '#fecaca' : '#93c5fd'};
        padding: 1rem;
        border-radius: 0.375rem;
        margin-bottom: 0.5rem;
        box-shadow: 0 4px 6px -1px rgba(0, 0, 0, 0.1);
        transform: translateX(100%);
        transition: transform 0.3s ease-in-out;
    `;
    notification.textContent = message;
    
    container.appendChild(notification);
    
    // Animate in
    setTimeout(() => {
        notification.style.transform = 'translateX(0)';
    }, 10);
    
    // Auto-remove after 5 seconds
    setTimeout(() => {
        notification.style.transform = 'translateX(100%)';
        setTimeout(() => {
            if (notification.parentNode) {
                notification.parentNode.removeChild(notification);
            }
        }, 300);
    }, 5000);
}

// Utility functions
function formatSpeed(bytesPerSecond) {
    if (bytesPerSecond < 1024) {
        return bytesPerSecond + ' B/s';
    } else if (bytesPerSecond < 1024 * 1024) {
        return (bytesPerSecond / 1024).toFixed(1) + ' KB/s';
    } else {
        return (bytesPerSecond / (1024 * 1024)).toFixed(1) + ' MB/s';
    }
}

function formatSize(bytes) {
    if (bytes < 1024) {
        return bytes + ' B';
    } else if (bytes < 1024 * 1024) {
        return (bytes / 1024).toFixed(1) + ' KB';
    } else if (bytes < 1024 * 1024 * 1024) {
        return (bytes / (1024 * 1024)).toFixed(1) + ' MB';
    } else {
        return (bytes / (1024 * 1024 * 1024)).toFixed(1) + ' GB';
    }
}

function escapeHtml(text) {
    const div = document.createElement('div');
    div.textContent = text;
    return div.innerHTML;
}

// Global functions for template usage
window.refreshTorrents = refreshTorrents;
window.pauseTorrent = pauseTorrent;
window.removeTorrent = removeTorrent;
window.showMediaDetails = showMediaDetails;
window.showNotification = showNotification;"#;
