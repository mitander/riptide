/**
 * Main application JavaScript
 * Handles common functionality and loads page-specific modules
 */

class RiptideApp {
    constructor() {
        this.currentPage = this.getCurrentPage();
        this.init();
    }

    init() {
        this.setupGlobalEventListeners();
        this.setupNotifications();
        this.loadPageSpecificScript();
        this.setupKeyboardShortcuts();
    }

    getCurrentPage() {
        const path = window.location.pathname;
        if (path === '/' || path === '/dashboard') return 'dashboard';
        if (path === '/library') return 'library';
        if (path === '/torrents') return 'torrents';
        if (path === '/add-torrent') return 'add-torrent';
        if (path === '/search') return 'search';
        if (path === '/settings') return 'settings';
        return 'unknown';
    }

    setupGlobalEventListeners() {
        // Handle navigation active states
        this.updateActiveNavigation();

        // Handle responsive navigation
        this.setupMobileNavigation();

        // Handle global error reporting
        window.addEventListener('error', (e) => {
            console.error('Global error:', e.error);
            this.showNotification('An unexpected error occurred', 'error');
        });

        // Handle unhandled promise rejections
        window.addEventListener('unhandledrejection', (e) => {
            console.error('Unhandled promise rejection:', e.reason);
            this.showNotification('An unexpected error occurred', 'error');
        });
    }

    updateActiveNavigation() {
        const navLinks = document.querySelectorAll('.nav-link');
        const currentPath = window.location.pathname;
        
        navLinks.forEach(link => {
            const href = link.getAttribute('href');
            const isActive = (currentPath === href) || 
                           (currentPath === '/' && href === '/') ||
                           (currentPath.startsWith(href) && href !== '/');
            
            link.classList.toggle('active', isActive);
        });
    }

    setupMobileNavigation() {
        // Add mobile menu toggle if needed
        const header = document.querySelector('.header');
        if (header && window.innerWidth <= 768) {
            this.createMobileMenuToggle();
        }

        // Handle window resize
        window.addEventListener('resize', () => {
            if (window.innerWidth > 768) {
                this.removeMobileMenuToggle();
            } else if (!document.querySelector('.mobile-menu-toggle')) {
                this.createMobileMenuToggle();
            }
        });
    }

    createMobileMenuToggle() {
        const nav = document.querySelector('.nav');
        if (!nav || document.querySelector('.mobile-menu-toggle')) return;

        const toggle = document.createElement('button');
        toggle.className = 'mobile-menu-toggle';
        toggle.innerHTML = '☰';
        toggle.addEventListener('click', () => {
            nav.classList.toggle('open');
        });

        const container = document.querySelector('.header .container');
        if (container) {
            container.appendChild(toggle);
        }
    }

    removeMobileMenuToggle() {
        const toggle = document.querySelector('.mobile-menu-toggle');
        if (toggle) {
            toggle.remove();
        }

        const nav = document.querySelector('.nav');
        if (nav) {
            nav.classList.remove('open');
        }
    }

    setupNotifications() {
        // Create notification container if it doesn't exist
        if (!document.querySelector('.notification-container')) {
            const container = document.createElement('div');
            container.className = 'notification-container';
            document.body.appendChild(container);
        }
    }

    loadPageSpecificScript() {
        const scripts = {
            'dashboard': '/static/js/dashboard.js',
            'library': '/static/js/library.js',
            'torrents': '/static/js/torrents.js',
            'add-torrent': '/static/js/add-torrent.js',
            'search': '/static/js/search.js',
            'settings': '/static/js/settings.js'
        };

        const scriptUrl = scripts[this.currentPage];
        if (scriptUrl) {
            this.loadScript(scriptUrl);
        }
    }

    loadScript(url) {
        return new Promise((resolve, reject) => {
            const script = document.createElement('script');
            script.src = url;
            script.onload = resolve;
            script.onerror = reject;
            document.head.appendChild(script);
        });
    }

    setupKeyboardShortcuts() {
        document.addEventListener('keydown', (e) => {
            // Global shortcuts (with Ctrl/Cmd)
            if (e.ctrlKey || e.metaKey) {
                switch (e.key) {
                    case 'k':
                        e.preventDefault();
                        this.focusSearch();
                        break;
                    case 'n':
                        e.preventDefault();
                        window.location.href = '/add-torrent';
                        break;
                    case 'h':
                        e.preventDefault();
                        window.location.href = '/';
                        break;
                }
            }

            // Single key shortcuts (when not in input)
            if (!this.isInputFocused()) {
                switch (e.key) {
                    case 'g':
                        this.handleGotoShortcuts(e);
                        break;
                    case '?':
                        e.preventDefault();
                        this.showKeyboardShortcuts();
                        break;
                }
            }
        });
    }

    handleGotoShortcuts(initialEvent) {
        // Show goto mode indicator
        this.showGotoMode();

        const handleSecondKey = (e) => {
            e.preventDefault();
            document.removeEventListener('keydown', handleSecondKey);
            this.hideGotoMode();

            switch (e.key) {
                case 'h':
                    window.location.href = '/';
                    break;
                case 's':
                    window.location.href = '/search';
                    break;
                case 'l':
                    window.location.href = '/library';
                    break;
                case 't':
                    window.location.href = '/torrents';
                    break;
                case 'a':
                    window.location.href = '/add-torrent';
                    break;
                case 'c':
                    window.location.href = '/settings';
                    break;
            }
        };

        document.addEventListener('keydown', handleSecondKey);

        // Auto-cancel after 3 seconds
        setTimeout(() => {
            document.removeEventListener('keydown', handleSecondKey);
            this.hideGotoMode();
        }, 3000);
    }

    showGotoMode() {
        const indicator = document.createElement('div');
        indicator.className = 'goto-mode-indicator';
        indicator.textContent = 'Go to: [h]ome [s]earch [l]ibrary [t]orrents [a]dd [c]onfig';
        document.body.appendChild(indicator);
    }

    hideGotoMode() {
        const indicator = document.querySelector('.goto-mode-indicator');
        if (indicator) {
            indicator.remove();
        }
    }

    showKeyboardShortcuts() {
        const modal = document.createElement('div');
        modal.className = 'shortcuts-modal';
        modal.innerHTML = `
            <div class="shortcuts-content">
                <h3>Keyboard Shortcuts</h3>
                <div class="shortcuts-grid">
                    <div class="shortcuts-section">
                        <h4>Navigation</h4>
                        <div class="shortcut"><kbd>g</kbd> + <kbd>h</kbd> - Home</div>
                        <div class="shortcut"><kbd>g</kbd> + <kbd>s</kbd> - Search</div>
                        <div class="shortcut"><kbd>g</kbd> + <kbd>l</kbd> - Library</div>
                        <div class="shortcut"><kbd>g</kbd> + <kbd>t</kbd> - Torrents</div>
                        <div class="shortcut"><kbd>g</kbd> + <kbd>a</kbd> - Add Torrent</div>
                        <div class="shortcut"><kbd>g</kbd> + <kbd>c</kbd> - Settings</div>
                    </div>
                    <div class="shortcuts-section">
                        <h4>Actions</h4>
                        <div class="shortcut"><kbd>Ctrl/Cmd</kbd> + <kbd>k</kbd> - Focus Search</div>
                        <div class="shortcut"><kbd>Ctrl/Cmd</kbd> + <kbd>n</kbd> - Add Torrent</div>
                        <div class="shortcut"><kbd>/</kbd> - Focus Search (search page)</div>
                        <div class="shortcut"><kbd>?</kbd> - Show Shortcuts</div>
                    </div>
                </div>
                <button onclick="this.parentNode.parentNode.remove()">Close</button>
            </div>
        `;

        modal.addEventListener('click', (e) => {
            if (e.target === modal) {
                modal.remove();
            }
        });

        document.body.appendChild(modal);
    }

    focusSearch() {
        // Try to focus search input on current page
        const searchInput = document.querySelector('#search-query, #search, .search-input');
        if (searchInput) {
            searchInput.focus();
        } else {
            // Navigate to search page
            window.location.href = '/search';
        }
    }

    isInputFocused() {
        const activeElement = document.activeElement;
        return activeElement && (
            activeElement.tagName === 'INPUT' || 
            activeElement.tagName === 'TEXTAREA' ||
            activeElement.tagName === 'SELECT' ||
            activeElement.isContentEditable
        );
    }

    showNotification(message, type = 'info', duration = 5000) {
        const container = document.querySelector('.notification-container');
        if (!container) return;

        const notification = document.createElement('div');
        notification.className = `notification ${type}`;
        notification.innerHTML = `
            <span class="notification-message">${this.escapeHtml(message)}</span>
            <button class="notification-close" onclick="this.parentNode.remove()">×</button>
        `;

        container.appendChild(notification);

        // Auto-remove after duration
        setTimeout(() => {
            if (notification.parentNode) {
                notification.remove();
            }
        }, duration);
    }

    escapeHtml(text) {
        const div = document.createElement('div');
        div.textContent = text;
        return div.innerHTML;
    }

    // Utility methods for API calls
    async apiRequest(url, options = {}) {
        try {
            const response = await fetch(url, {
                ...options,
                headers: {
                    'Content-Type': 'application/json',
                    ...options.headers
                }
            });

            if (!response.ok) {
                throw new Error(`HTTP ${response.status}: ${response.statusText}`);
            }

            return await response.json();
        } catch (error) {
            console.error('API request failed:', error);
            throw error;
        }
    }

    // Format utilities
    formatFileSize(bytes) {
        if (!bytes) return 'Unknown';
        
        const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
        const i = Math.floor(Math.log(bytes) / Math.log(1024));
        
        if (i === 0) return `${bytes} ${sizes[i]}`;
        return `${(bytes / Math.pow(1024, i)).toFixed(1)} ${sizes[i]}`;
    }

    formatDuration(seconds) {
        const hours = Math.floor(seconds / 3600);
        const minutes = Math.floor((seconds % 3600) / 60);
        
        if (hours > 0) {
            return `${hours}h ${minutes}m`;
        }
        return `${minutes}m`;
    }

    formatSpeed(bytesPerSecond) {
        if (bytesPerSecond < 1024) return `${bytesPerSecond} B/s`;
        if (bytesPerSecond < 1048576) return `${(bytesPerSecond / 1024).toFixed(0)} KB/s`;
        return `${(bytesPerSecond / 1048576).toFixed(1)} MB/s`;
    }
}

// Global app instance
window.riptide = new RiptideApp();

// Export utilities for other modules
window.RiptideUtils = {
    formatFileSize: window.riptide.formatFileSize.bind(window.riptide),
    formatDuration: window.riptide.formatDuration.bind(window.riptide),
    formatSpeed: window.riptide.formatSpeed.bind(window.riptide),
    showNotification: window.riptide.showNotification.bind(window.riptide),
    apiRequest: window.riptide.apiRequest.bind(window.riptide),
    escapeHtml: window.riptide.escapeHtml.bind(window.riptide)
};