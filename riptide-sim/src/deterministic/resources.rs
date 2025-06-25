//! Resource tracking and limits for simulation control.

use std::fmt;

/// Types of resources tracked in simulations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResourceType {
    Memory,
    Connections,
    DiskSpace,
    CpuTime,
}

impl fmt::Display for ResourceType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ResourceType::Memory => write!(f, "Memory"),
            ResourceType::Connections => write!(f, "Connections"),
            ResourceType::DiskSpace => write!(f, "DiskSpace"),
            ResourceType::CpuTime => write!(f, "CpuTime"),
        }
    }
}

/// Resource limits for simulation constraints.
#[derive(Debug, Clone)]
pub struct ResourceLimits {
    /// Maximum memory usage in bytes
    pub max_memory: usize,
    /// Maximum concurrent connections
    pub max_connections: usize,
    /// Maximum disk usage in bytes
    pub max_disk_usage: u64,
    /// Maximum CPU time per operation in microseconds
    pub max_cpu_time_us: u64,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            max_memory: 1024 * 1024 * 1024,          // 1 GB
            max_connections: 1000,                   // 1000 connections
            max_disk_usage: 10 * 1024 * 1024 * 1024, // 10 GB
            max_cpu_time_us: 1_000_000,              // 1 second
        }
    }
}

impl ResourceLimits {
    /// Creates resource limits suitable for constrained testing.
    pub fn constrained() -> Self {
        Self {
            max_memory: 128 * 1024 * 1024,      // 128 MB
            max_connections: 10,                // 10 connections
            max_disk_usage: 1024 * 1024 * 1024, // 1 GB
            max_cpu_time_us: 100_000,           // 100ms
        }
    }

    /// Creates resource limits suitable for stress testing.
    pub fn stress_test() -> Self {
        Self {
            max_memory: 4 * 1024 * 1024 * 1024,       // 4 GB
            max_connections: 5000,                    // 5000 connections
            max_disk_usage: 100 * 1024 * 1024 * 1024, // 100 GB
            max_cpu_time_us: 10_000_000,              // 10 seconds
        }
    }
}

/// Current resource usage in simulation.
#[derive(Debug, Clone, Default)]
pub struct ResourceUsage {
    /// Current memory usage in bytes
    pub memory: usize,
    /// Current number of connections
    pub connections: usize,
    /// Current disk usage in bytes
    pub disk_usage: u64,
    /// Total CPU time used in microseconds
    pub cpu_time_us: u64,
}

impl ResourceUsage {
    /// Creates new resource usage tracker.
    pub fn new() -> Self {
        Default::default()
    }

    /// Checks if usage exceeds any limit.
    pub fn check_limits(&self, limits: &ResourceLimits) -> Option<(ResourceType, u64, u64)> {
        if self.memory > limits.max_memory {
            return Some((
                ResourceType::Memory,
                self.memory as u64,
                limits.max_memory as u64,
            ));
        }

        if self.connections > limits.max_connections {
            return Some((
                ResourceType::Connections,
                self.connections as u64,
                limits.max_connections as u64,
            ));
        }

        if self.disk_usage > limits.max_disk_usage {
            return Some((
                ResourceType::DiskSpace,
                self.disk_usage,
                limits.max_disk_usage,
            ));
        }

        if self.cpu_time_us > limits.max_cpu_time_us {
            return Some((
                ResourceType::CpuTime,
                self.cpu_time_us,
                limits.max_cpu_time_us,
            ));
        }

        None
    }

    /// Updates resource usage based on event type.
    pub fn update_for_event(&mut self, event_type: &crate::EventType) {
        use crate::EventType;

        match event_type {
            EventType::PeerConnect { .. } => {
                self.connections += 1;
                self.memory += 1024 * 64; // 64KB per connection
            }
            EventType::PeerDisconnect { .. } => {
                self.connections = self.connections.saturating_sub(1);
                self.memory = self.memory.saturating_sub(1024 * 64);
            }
            EventType::PieceComplete { .. } => {
                self.disk_usage += 262144; // 256KB piece
                self.cpu_time_us += 1000; // 1ms for hash verification
            }
            EventType::ResourceLimit {
                resource, current, ..
            } => {
                // Update tracked usage from event
                match resource {
                    ResourceType::Memory => self.memory = *current as usize,
                    ResourceType::Connections => self.connections = *current as usize,
                    ResourceType::DiskSpace => self.disk_usage = *current,
                    ResourceType::CpuTime => self.cpu_time_us = *current,
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resource_limits_defaults() {
        let limits = ResourceLimits::default();
        assert_eq!(limits.max_memory, 1024 * 1024 * 1024);
        assert_eq!(limits.max_connections, 1000);
    }

    #[test]
    fn test_resource_usage_limit_checking() {
        let limits = ResourceLimits::constrained();
        let mut usage = ResourceUsage::new();

        // Should be within limits initially
        assert!(usage.check_limits(&limits).is_none());

        // Exceed connection limit
        usage.connections = 11;
        let result = usage.check_limits(&limits);
        assert!(result.is_some());

        let (resource_type, current, limit) = result.unwrap();
        assert_eq!(resource_type, ResourceType::Connections);
        assert_eq!(current, 11);
        assert_eq!(limit, 10);
    }

    #[test]
    fn test_resource_usage_event_updates() {
        let mut usage = ResourceUsage::new();

        // Peer connection increases resources
        usage.update_for_event(&crate::EventType::PeerConnect {
            peer_id: "TEST".to_string(),
        });
        assert_eq!(usage.connections, 1);
        assert_eq!(usage.memory, 65536); // 64KB

        // Peer disconnect decreases resources
        usage.update_for_event(&crate::EventType::PeerDisconnect {
            peer_id: "TEST".to_string(),
        });
        assert_eq!(usage.connections, 0);
        assert_eq!(usage.memory, 0);
    }
}
