//! Bandwidth management and allocation for peer connections

use std::collections::HashMap;
use std::time::Instant;

use super::Priority;

/// Global bandwidth manager for all torrent connections
#[derive(Debug)]
pub struct GlobalBandwidthManager {
    upload_allocator: BandwidthAllocator,
    download_allocator: BandwidthAllocator,
    last_update: Instant,
}

/// Bandwidth allocator with priority-based scheduling
#[derive(Debug)]
pub struct BandwidthAllocator {
    total_bandwidth: u64,
    allocated_bandwidth: HashMap<Priority, u64>,
    _reserved_bandwidth: HashMap<Priority, u64>,
}

impl GlobalBandwidthManager {
    /// Create a new global bandwidth manager
    pub fn new() -> Self {
        Self {
            upload_allocator: BandwidthAllocator::new(1_000_000), // 1 MB/s default
            download_allocator: BandwidthAllocator::new(10_000_000), // 10 MB/s default
            last_update: Instant::now(),
        }
    }

    /// Allocate bandwidth for a priority level
    pub fn allocate_bandwidth(&mut self, priority: Priority, bytes: u64, is_upload: bool) -> u64 {
        let allocator = if is_upload {
            &mut self.upload_allocator
        } else {
            &mut self.download_allocator
        };

        allocator.allocate(priority, bytes)
    }

    /// Update bandwidth statistics
    pub fn update_stats(&mut self) {
        self.last_update = Instant::now();
        // Reset allocations for next period
        self.upload_allocator.reset_allocations();
        self.download_allocator.reset_allocations();
    }
}

impl BandwidthAllocator {
    /// Create a new bandwidth allocator
    pub fn new(total_bandwidth: u64) -> Self {
        Self {
            total_bandwidth,
            allocated_bandwidth: HashMap::new(),
            _reserved_bandwidth: HashMap::new(),
        }
    }

    /// Allocate bandwidth for a priority level
    pub fn allocate(&mut self, priority: Priority, requested: u64) -> u64 {
        let current_allocation = self.allocated_bandwidth.get(&priority).unwrap_or(&0);
        let available = self
            .total_bandwidth
            .saturating_sub(self.allocated_bandwidth.values().sum::<u64>());

        let granted = requested.min(available);
        self.allocated_bandwidth
            .insert(priority, current_allocation + granted);
        granted
    }

    /// Reset allocations for a new time period
    pub fn reset_allocations(&mut self) {
        self.allocated_bandwidth.clear();
    }

    /// Get total allocated bandwidth
    pub fn total_allocated(&self) -> u64 {
        self.allocated_bandwidth.values().sum()
    }

    /// Get remaining bandwidth
    pub fn remaining(&self) -> u64 {
        self.total_bandwidth.saturating_sub(self.total_allocated())
    }
}

impl Default for GlobalBandwidthManager {
    fn default() -> Self {
        Self::new()
    }
}
