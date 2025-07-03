//! Priority queue and request management

use std::collections::VecDeque;
use std::time::Instant;

use crate::torrent::{InfoHash, PieceIndex};

/// Priority levels for piece requests
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Priority {
    Background,
    Normal,
    High,
    Critical,
}

/// Priority-based request queue for piece management
#[derive(Debug)]
pub struct PriorityRequestQueue {
    critical_queue: VecDeque<PendingRequest>,
    high_queue: VecDeque<PendingRequest>,
    normal_queue: VecDeque<PendingRequest>,
    background_queue: VecDeque<PendingRequest>,
}

/// A pending piece request
#[derive(Debug, Clone)]
pub struct PendingRequest {
    pub info_hash: InfoHash,
    pub piece_index: PieceIndex,
    pub requested_at: Instant,
    pub deadline: Option<Instant>,
    pub priority: Priority,
    pub retry_count: u32,
}

impl PriorityRequestQueue {
    /// Create a new priority request queue
    pub fn new() -> Self {
        Self {
            critical_queue: VecDeque::new(),
            high_queue: VecDeque::new(),
            normal_queue: VecDeque::new(),
            background_queue: VecDeque::new(),
        }
    }

    /// Add a request to the appropriate priority queue
    pub fn push(&mut self, request: PendingRequest) {
        match request.priority {
            Priority::Critical => self.critical_queue.push_back(request),
            Priority::High => self.high_queue.push_back(request),
            Priority::Normal => self.normal_queue.push_back(request),
            Priority::Background => self.background_queue.push_back(request),
        }
    }

    /// Get the next request in priority order
    pub fn pop(&mut self) -> Option<PendingRequest> {
        self.critical_queue
            .pop_front()
            .or_else(|| self.high_queue.pop_front())
            .or_else(|| self.normal_queue.pop_front())
            .or_else(|| self.background_queue.pop_front())
    }

    /// Check if queue is empty
    pub fn is_empty(&self) -> bool {
        self.critical_queue.is_empty()
            && self.high_queue.is_empty()
            && self.normal_queue.is_empty()
            && self.background_queue.is_empty()
    }

    /// Get total number of pending requests
    pub fn len(&self) -> usize {
        self.critical_queue.len()
            + self.high_queue.len()
            + self.normal_queue.len()
            + self.background_queue.len()
    }

    /// Remove expired requests based on deadlines
    pub fn remove_expired(&mut self) -> Vec<PendingRequest> {
        let now = Instant::now();
        let mut expired = Vec::new();

        for queue in [
            &mut self.critical_queue,
            &mut self.high_queue,
            &mut self.normal_queue,
            &mut self.background_queue,
        ] {
            let mut i = 0;
            while i < queue.len() {
                if let Some(request) = queue.get(i)
                    && let Some(deadline) = request.deadline
                    && now > deadline
                {
                    expired.push(queue.remove(i).unwrap());
                    continue;
                }
                i += 1;
            }
        }

        expired
    }

    /// Requests queue for priority level
    pub fn priority_queue(&self, priority: Priority) -> &VecDeque<PendingRequest> {
        match priority {
            Priority::Critical => &self.critical_queue,
            Priority::High => &self.high_queue,
            Priority::Normal => &self.normal_queue,
            Priority::Background => &self.background_queue,
        }
    }
}

impl PendingRequest {
    /// Create a new pending request
    pub fn new(
        info_hash: InfoHash,
        piece_index: PieceIndex,
        priority: Priority,
        deadline: Option<Instant>,
    ) -> Self {
        Self {
            info_hash,
            piece_index,
            requested_at: Instant::now(),
            deadline,
            priority,
            retry_count: 0,
        }
    }

    /// Check if request has expired
    pub fn is_expired(&self) -> bool {
        if let Some(deadline) = self.deadline {
            Instant::now() > deadline
        } else {
            false
        }
    }

    /// Increment retry count
    pub fn retry(&mut self) {
        self.retry_count += 1;
    }

    /// Check if maximum retries exceeded
    pub fn max_retries_exceeded(&self) -> bool {
        self.retry_count >= 3
    }
}

impl Default for PriorityRequestQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for Priority {
    fn default() -> Self {
        Self::Normal
    }
}
