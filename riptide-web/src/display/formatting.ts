/**
 * Utility functions for formatting data display
 */

/**
 * Format bytes to human readable format
 */
export function formatBytes(bytes: number): string {
  if (bytes >= 1073741824) {
    return (bytes / 1073741824).toFixed(1) + ' GB';
  } else if (bytes >= 1048576) {
    return (bytes / 1048576).toFixed(1) + ' MB';
  } else if (bytes >= 1024) {
    return (bytes / 1024).toFixed(0) + ' KB';
  }
  return bytes + ' B';
}

/**
 * Format timestamp to local date string
 */
export function formatTimestamp(timestamp: string): string {
  try {
    const date = new Date(timestamp);
    return date.toLocaleString();
  } catch (error) {
    console.error('Invalid timestamp:', timestamp);
    return 'Invalid date';
  }
}

/**
 * Format duration in seconds to human readable format
 */
export function formatDuration(seconds: number): string {
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  
  if (hours > 0) {
    return `${hours}h ${minutes}m`;
  }
  return `${minutes}m`;
}

/**
 * Format speed in bytes per second to KB/s or MB/s
 */
export function formatSpeed(bytesPerSecond: number): string {
  if (bytesPerSecond >= 1048576) {
    return (bytesPerSecond / 1048576).toFixed(1) + ' MB/s';
  } else if (bytesPerSecond >= 1024) {
    return (bytesPerSecond / 1024).toFixed(0) + ' KB/s';
  }
  return bytesPerSecond + ' B/s';
}

/**
 * Format progress percentage
 */
export function formatProgress(progress: number): string {
  return `${progress.toFixed(1)}%`;
}

/**
 * Escape HTML to prevent XSS attacks
 */
export function escapeHtml(text: string): string {
  const div = document.createElement('div');
  div.textContent = text;
  return div.innerHTML;
}

/**
 * Debounce function to limit rapid function calls
 */
export function debounce<T extends (...args: any[]) => any>(
  func: T,
  wait: number
): (...args: Parameters<T>) => void {
  let timeout: number | null = null;
  
  return (...args: Parameters<T>) => {
    if (timeout !== null) {
      clearTimeout(timeout);
    }
    
    timeout = window.setTimeout(() => {
      func.apply(null, args);
    }, wait);
  };
}