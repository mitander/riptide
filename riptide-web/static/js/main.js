// Utility function for API calls
async function apiCall(endpoint) {
    try {
        const response = await fetch(endpoint);
        if (!response.ok) throw new Error(`HTTP ${response.status}`);
        return await response.json();
    } catch (error) {
        console.error('API call failed:', error);
        throw error;
    }
}

// Auto-refresh stats every 30 seconds
setInterval(async () => {
    try {
        await apiCall('/api/stats');
    } catch (error) {
        console.error('Stats refresh failed:', error);
    }
}, 30000);