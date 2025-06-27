import { ApiClient } from './shared/api-client.js';
import { Settings } from './shared/types.js';

export class SettingsManager {
    private apiClient: ApiClient;

    constructor() {
        this.apiClient = new ApiClient();
    }

    public initialize(): void {
        this.setupEventListeners();
    }

    private setupEventListeners(): void {
        const settingsForm = document.querySelector('.settings-form') as HTMLFormElement;
        const resetButton = document.getElementById('reset-settings') as HTMLButtonElement;

        if (settingsForm) {
            settingsForm.addEventListener('submit', (e) => this.saveSettings(e));
        }

        if (resetButton) {
            resetButton.addEventListener('click', () => this.resetSettings());
        }
    }

    private async saveSettings(event: Event): Promise<void> {
        event.preventDefault();

        const form = event.target as HTMLFormElement;
        const formData = new FormData(form);
        const settings = this.extractSettingsFromForm(formData);

        const resultDiv = document.getElementById('settings-result');
        if (!resultDiv) return;

        try {
            await this.apiClient.updateSettings(settings);

            resultDiv.className = 'result-message success';
            resultDiv.textContent = 'Settings saved successfully!';
            resultDiv.style.display = 'block';

            setTimeout(() => {
                resultDiv.style.display = 'none';
            }, 3000);
        } catch (error) {
            const errorMessage = error instanceof Error ? error.message : 'Unknown error';
            resultDiv.className = 'result-message error';
            resultDiv.textContent = `Failed to save settings: ${errorMessage}`;
            resultDiv.style.display = 'block';

            setTimeout(() => {
                resultDiv.style.display = 'none';
            }, 5000);
        }
    }

    private extractSettingsFromForm(formData: FormData): Settings {
        const getNumber = (key: string): number | undefined => {
            const value = formData.get(key) as string;
            return value && value.trim() ? parseInt(value, 10) : undefined;
        };

        const getBoolean = (key: string): boolean => {
            return formData.has(key);
        };

        return {
            download_limit: getNumber('download_limit'),
            upload_limit: getNumber('upload_limit'),
            max_connections: getNumber('max_connections') || 50,
            streaming_port: getNumber('streaming_port') || 8080,
            web_ui_port: getNumber('web_ui_port') || 3000,
            enable_upnp: getBoolean('enable_upnp'),
            enable_dht: getBoolean('enable_dht'),
            enable_pex: getBoolean('enable_pex'),
        };
    }

    private async resetSettings(): Promise<void> {
        if (!confirm('Reset all settings to defaults?')) {
            return;
        }

        try {
            await this.apiClient.resetSettings();
            location.reload();
        } catch (error) {
            const errorMessage = error instanceof Error ? error.message : 'Unknown error';
            alert(`Failed to reset settings: ${errorMessage}`);
        }
    }

    public async loadCurrentSettings(): Promise<Settings | null> {
        try {
            return await this.apiClient.getSettings();
        } catch (error) {
            console.error('Failed to load current settings:', error);
            return null;
        }
    }

    public populateForm(settings: Settings): void {
        this.setInputValue('download-limit', settings.download_limit?.toString() || '');
        this.setInputValue('upload-limit', settings.upload_limit?.toString() || '');
        this.setInputValue('max-connections', settings.max_connections.toString());
        this.setInputValue('streaming-port', settings.streaming_port.toString());
        this.setInputValue('web-ui-port', settings.web_ui_port.toString());
        this.setCheckboxValue('enable-upnp', settings.enable_upnp);
        this.setCheckboxValue('enable-dht', settings.enable_dht);
        this.setCheckboxValue('enable-pex', settings.enable_pex);
    }

    private setInputValue(id: string, value: string): void {
        const input = document.getElementById(id) as HTMLInputElement;
        if (input) {
            input.value = value;
        }
    }

    private setCheckboxValue(id: string, checked: boolean): void {
        const checkbox = document.getElementById(id) as HTMLInputElement;
        if (checkbox) {
            checkbox.checked = checked;
        }
    }
}

// Initialize when DOM is loaded
document.addEventListener('DOMContentLoaded', async () => {
    const settingsManager = new SettingsManager();
    settingsManager.initialize();

    // Load and populate current settings if not already populated by template
    const currentSettings = await settingsManager.loadCurrentSettings();
    if (currentSettings) {
        settingsManager.populateForm(currentSettings);
    }
});