/**
 * Add torrent page functionality
 */

class AddTorrent {
    constructor() {
        this.init();
    }

    init() {
        this.setupForm();
        this.setupFileUpload();
    }

    setupForm() {
        const form = document.querySelector('.add-torrent-form');
        if (form) {
            form.addEventListener('submit', (e) => this.handleSubmit(e));
        }

        // Auto-focus the magnet link input
        const magnetInput = document.getElementById('magnet-link');
        if (magnetInput) {
            magnetInput.focus();
        }
    }

    setupFileUpload() {
        // Create file input for .torrent files
        const fileInput = document.createElement('input');
        fileInput.type = 'file';
        fileInput.accept = '.torrent';
        fileInput.style.display = 'none';
        document.body.appendChild(fileInput);

        // Add file upload button
        const uploadBtn = document.createElement('button');
        uploadBtn.type = 'button';
        uploadBtn.className = 'btn btn-secondary';
        uploadBtn.textContent = 'Upload .torrent file';
        uploadBtn.onclick = () => fileInput.click();

        const magnetGroup = document.querySelector('.form-group');
        if (magnetGroup) {
            magnetGroup.appendChild(uploadBtn);
        }

        fileInput.addEventListener('change', (e) => this.handleFileUpload(e));
    }

    async handleSubmit(event) {
        event.preventDefault();
        
        const magnetInput = document.getElementById('magnet-link');
        const startImmediately = document.getElementById('start-immediately');
        const resultDiv = document.getElementById('add-result');
        
        if (!magnetInput || !resultDiv) return;

        const magnetLink = magnetInput.value.trim();
        
        if (!magnetLink) {
            this.showResult('Please enter a magnet link', 'error');
            return;
        }

        if (!this.isValidMagnetLink(magnetLink)) {
            this.showResult('Invalid magnet link format', 'error');
            return;
        }

        this.showLoading(true);
        
        try {
            const params = new URLSearchParams({
                magnet: magnetLink,
                start: startImmediately?.checked ? 'true' : 'false'
            });

            const response = await fetch(`/api/torrents/add?${params}`, {
                method: 'POST'
            });
            
            const data = await response.json();
            
            if (data.result?.success) {
                let message = data.result.message;
                
                if (data.result.stream_url) {
                    message += ` <a href="${data.result.stream_url}" class="btn btn-primary btn-sm">Stream Now</a>`;
                }
                
                this.showResult(message, 'success');
                magnetInput.value = '';
                
                // Redirect to torrents page after 2 seconds
                setTimeout(() => {
                    window.location.href = '/torrents';
                }, 2000);
            } else {
                this.showResult(data.result?.message || 'Failed to add torrent', 'error');
            }
        } catch (error) {
            this.showResult(`Failed to add torrent: ${error.message}`, 'error');
        } finally {
            this.showLoading(false);
        }
    }

    async handleFileUpload(event) {
        const file = event.target.files[0];
        if (!file) return;

        if (!file.name.endsWith('.torrent')) {
            this.showResult('Please select a .torrent file', 'error');
            return;
        }

        this.showLoading(true);

        try {
            const formData = new FormData();
            formData.append('torrent', file);

            const response = await fetch('/api/torrents/upload', {
                method: 'POST',
                body: formData
            });

            const data = await response.json();

            if (data.result?.success) {
                this.showResult(data.result.message, 'success');
                
                // Redirect to torrents page after 2 seconds
                setTimeout(() => {
                    window.location.href = '/torrents';
                }, 2000);
            } else {
                this.showResult(data.result?.message || 'Failed to upload torrent', 'error');
            }
        } catch (error) {
            this.showResult(`Failed to upload torrent: ${error.message}`, 'error');
        } finally {
            this.showLoading(false);
        }
    }

    isValidMagnetLink(link) {
        return link.startsWith('magnet:') && link.includes('xt=urn:btih:');
    }

    showResult(message, type) {
        const resultDiv = document.getElementById('add-result');
        if (!resultDiv) return;

        resultDiv.className = `result-message ${type}`;
        resultDiv.innerHTML = message;
        resultDiv.style.display = 'block';

        // Hide after 5 seconds for error messages
        if (type === 'error') {
            setTimeout(() => {
                resultDiv.style.display = 'none';
            }, 5000);
        }
    }

    showLoading(show) {
        const submitBtn = document.querySelector('button[type="submit"]');
        if (!submitBtn) return;

        if (show) {
            submitBtn.disabled = true;
            submitBtn.textContent = 'Adding...';
        } else {
            submitBtn.disabled = false;
            submitBtn.textContent = 'Add Torrent';
        }
    }
}

// Initialize add torrent functionality when DOM is ready
document.addEventListener('DOMContentLoaded', () => {
    new AddTorrent();
});