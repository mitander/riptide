/**
 * Settings page functionality
 */

class Settings {
    constructor() {
        this.originalSettings = {};
        this.init();
    }

    init() {
        this.loadSettings();
        this.setupForm();
        this.setupValidation();
    }

    loadSettings() {
        // Store original settings for reset functionality
        const form = document.querySelector('.settings-form');
        if (form) {
            const formData = new FormData(form);
            for (let [key, value] of formData.entries()) {
                this.originalSettings[key] = value;
            }
        }
    }

    setupForm() {
        const form = document.querySelector('.settings-form');
        if (form) {
            form.addEventListener('submit', (e) => this.handleSubmit(e));
        }

        const resetBtn = document.querySelector('.btn-secondary');
        if (resetBtn) {
            resetBtn.addEventListener('click', () => this.resetSettings());
        }
    }

    setupValidation() {
        // Port validation
        const portInputs = document.querySelectorAll('input[type="number"][name*="port"]');
        portInputs.forEach(input => {
            input.addEventListener('blur', () => this.validatePort(input));
        });

        // Bandwidth validation
        const bandwidthInputs = document.querySelectorAll('input[name*="limit"]');
        bandwidthInputs.forEach(input => {
            input.addEventListener('blur', () => this.validateBandwidth(input));
        });
    }

    async handleSubmit(event) {
        event.preventDefault();
        
        const form = event.target;
        const formData = new FormData(form);
        const settings = {};
        
        // Convert FormData to object
        for (let [key, value] of formData.entries()) {
            if (value === '') {
                settings[key] = null; // Empty values become null
            } else {
                settings[key] = value;
            }
        }

        // Add checkbox values (they're not included in FormData if unchecked)
        const checkboxes = form.querySelectorAll('input[type="checkbox"]');
        checkboxes.forEach(checkbox => {
            settings[checkbox.name] = checkbox.checked;
        });

        // Validate settings
        const validation = this.validateSettings(settings);
        if (!validation.valid) {
            this.showResult(validation.error, 'error');
            return;
        }

        this.showLoading(true);

        try {
            const response = await fetch('/api/settings', {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json',
                },
                body: JSON.stringify(settings)
            });

            if (response.ok) {
                const data = await response.json();
                this.showResult(data.message || 'Settings saved successfully!', 'success');
                
                // Update original settings
                this.originalSettings = { ...settings };
                
                // Show restart notice if needed
                if (data.restart_required) {
                    this.showRestartNotice();
                }
            } else {
                const error = await response.text();
                this.showResult(`Failed to save settings: ${error}`, 'error');
            }
        } catch (error) {
            this.showResult(`Failed to save settings: ${error.message}`, 'error');
        } finally {
            this.showLoading(false);
        }
    }

    validateSettings(settings) {
        // Port validation
        const ports = ['streaming_port', 'web_ui_port'];
        for (const portName of ports) {
            const port = parseInt(settings[portName]);
            if (isNaN(port) || port < 1024 || port > 65535) {
                return {
                    valid: false,
                    error: `${portName.replace('_', ' ')} must be between 1024 and 65535`
                };
            }
        }

        // Check for port conflicts
        if (settings.streaming_port === settings.web_ui_port) {
            return {
                valid: false,
                error: 'Streaming port and Web UI port cannot be the same'
            };
        }

        // Bandwidth validation
        const bandwidthFields = ['download_limit', 'upload_limit'];
        for (const field of bandwidthFields) {
            if (settings[field] !== null) {
                const value = parseInt(settings[field]);
                if (isNaN(value) || value < 0) {
                    return {
                        valid: false,
                        error: `${field.replace('_', ' ')} must be a positive number`
                    };
                }
            }
        }

        // Connection validation
        const maxConnections = parseInt(settings.max_connections);
        if (isNaN(maxConnections) || maxConnections < 10 || maxConnections > 1000) {
            return {
                valid: false,
                error: 'Max connections must be between 10 and 1000'
            };
        }

        return { valid: true };
    }

    validatePort(input) {
        const port = parseInt(input.value);
        const isValid = !isNaN(port) && port >= 1024 && port <= 65535;
        
        input.classList.toggle('invalid', !isValid);
        
        if (!isValid) {
            this.showFieldError(input, 'Port must be between 1024 and 65535');
        } else {
            this.clearFieldError(input);
        }
    }

    validateBandwidth(input) {
        if (input.value === '') {
            this.clearFieldError(input);
            return;
        }

        const value = parseInt(input.value);
        const isValid = !isNaN(value) && value >= 0;
        
        input.classList.toggle('invalid', !isValid);
        
        if (!isValid) {
            this.showFieldError(input, 'Must be a positive number');
        } else {
            this.clearFieldError(input);
        }
    }

    showFieldError(input, message) {
        this.clearFieldError(input);
        
        const error = document.createElement('span');
        error.className = 'field-error';
        error.textContent = message;
        
        input.parentNode.appendChild(error);
    }

    clearFieldError(input) {
        const existingError = input.parentNode.querySelector('.field-error');
        if (existingError) {
            existingError.remove();
        }
    }

    resetSettings() {
        if (!confirm('Reset all settings to their current saved values?')) {
            return;
        }

        const form = document.querySelector('.settings-form');
        if (!form) return;

        // Reset form fields to original values
        Object.entries(this.originalSettings).forEach(([key, value]) => {
            const input = form.querySelector(`[name="${key}"]`);
            if (input) {
                if (input.type === 'checkbox') {
                    input.checked = value === 'true';
                } else {
                    input.value = value;
                }
            }
        });

        // Clear any validation errors
        const errors = form.querySelectorAll('.field-error');
        errors.forEach(error => error.remove());

        const invalidInputs = form.querySelectorAll('.invalid');
        invalidInputs.forEach(input => input.classList.remove('invalid'));

        this.showResult('Settings reset to saved values', 'info');
    }

    showRestartNotice() {
        const notice = document.createElement('div');
        notice.className = 'restart-notice';
        notice.innerHTML = `
            <strong>Restart Required</strong>
            <p>Some settings require a server restart to take effect.</p>
            <button onclick="this.parentNode.remove()">Dismiss</button>
        `;
        
        document.body.appendChild(notice);
    }

    showResult(message, type) {
        const resultDiv = document.getElementById('settings-result');
        if (!resultDiv) return;

        resultDiv.className = `result-message ${type}`;
        resultDiv.textContent = message;
        resultDiv.style.display = 'block';

        // Auto-hide after 5 seconds
        setTimeout(() => {
            resultDiv.style.display = 'none';
        }, 5000);
    }

    showLoading(show) {
        const submitBtn = document.querySelector('button[type="submit"]');
        if (!submitBtn) return;

        if (show) {
            submitBtn.disabled = true;
            submitBtn.textContent = 'Saving...';
        } else {
            submitBtn.disabled = false;
            submitBtn.textContent = 'Save Settings';
        }
    }
}

// Initialize settings functionality when DOM is ready
document.addEventListener('DOMContentLoaded', () => {
    new Settings();
});