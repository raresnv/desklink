//! Bounded, decode-acknowledged video with conservative congestion adaptation.
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

pub(super) const MAX_IN_FLIGHT: usize = 6;

pub(super) struct FrameWindow {
    pending: VecDeque<u64>,
    next: u64,
    quality: u8,
    interval: Instant,
    completed: u32,
    pressured: bool,
    healthy: u8,
}
impl Default for FrameWindow {
    fn default() -> Self {
        Self::at(Instant::now())
    }
}
impl FrameWindow {
    fn at(now: Instant) -> Self {
        Self {
            pending: VecDeque::new(),
            next: rand::random::<u64>() | (1 << 63),
            quality: 75,
            interval: now,
            completed: 0,
            pressured: false,
            healthy: 0,
        }
    }
    pub fn quality(&self) -> u8 {
        self.quality
    }
    pub fn has_capacity(&self) -> bool {
        self.pending.len() < MAX_IN_FLIGHT
    }
    pub fn reserve(&mut self) -> Option<u64> {
        if self.pending.len() >= MAX_IN_FLIGHT {
            self.pressured = true;
            return None;
        }
        let ticket = self.next;
        self.next = self.next.wrapping_add(1) | (1 << 63);
        self.pending.push_back(ticket);
        Some(ticket)
    }
    pub fn acknowledge(&mut self, ticket: u64) -> bool {
        self.acknowledge_at(ticket, Instant::now())
    }
    fn acknowledge_at(&mut self, ticket: u64, now: Instant) -> bool {
        let Some(position) = self.pending.iter().position(|t| *t == ticket) else {
            return false;
        };
        self.pending.remove(position);
        self.completed += 1;
        let elapsed = now.saturating_duration_since(self.interval);
        if elapsed >= Duration::from_secs(2) {
            let fps = self.completed as f64 / elapsed.as_secs_f64();
            if self.pressured && fps < 30.0 {
                self.quality = self.quality.saturating_sub(5).max(40);
                self.healthy = 0;
            } else if !self.pressured && fps >= 32.0 {
                self.healthy += 1;
                if self.healthy >= 5 {
                    self.quality = (self.quality + 2).min(75);
                    self.healthy = 0;
                }
            } else {
                self.healthy = 0;
            }
            self.completed = 0;
            self.pressured = false;
            self.interval = now;
        }
        true
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recovered_thirty_five_fps_restores_quality_with_headroom() {
        let start = Instant::now();
        let mut window = FrameWindow::at(start);
        window.quality = 60;
        let mut now = start;
        for _ in 0..5 {
            for _ in 0..70 {
                let ticket = window.reserve().unwrap();
                now += Duration::from_millis(29);
                window.acknowledge_at(ticket, now);
            }
        }
        assert!(window.quality() > 60);
    }
    #[test]
    fn congestion_lowers_quality_but_static_desktops_do_not() {
        let start = std::time::Instant::now();
        let mut idle = FrameWindow::at(start);
        let ticket = idle.reserve().unwrap();
        idle.acknowledge_at(ticket, start + std::time::Duration::from_secs(3));
        assert_eq!(idle.quality(), 75);
        let mut busy = FrameWindow::at(start);
        let mut tickets = std::collections::VecDeque::new();
        for _ in 0..MAX_IN_FLIGHT {
            tickets.push_back(busy.reserve().unwrap());
        }
        assert!(busy.reserve().is_none());
        busy.acknowledge_at(
            tickets.pop_front().unwrap(),
            start + std::time::Duration::from_secs(3),
        );
        assert_eq!(busy.quality(), 70);
    }
}

#[cfg(test)]
mod limits {
    use super::*;
    #[test]
    fn repeated_congestion_never_exceeds_quality_bounds_or_accepts_duplicate_acks() {
        let start = Instant::now();
        let mut window = FrameWindow::at(start);
        let mut tickets = VecDeque::new();
        for step in 1..=20 {
            while let Some(ticket) = window.reserve() {
                tickets.push_back(ticket);
            }
            let ticket = tickets.pop_front().unwrap();
            assert!(window.acknowledge_at(ticket, start + Duration::from_secs(step * 3)));
            let quality = window.quality();
            assert!((40..=75).contains(&quality));
            assert!(!window.acknowledge_at(ticket, start + Duration::from_secs(step * 3 + 1)));
            assert_eq!(quality, window.quality());
        }
        assert_eq!(window.quality(), 40);
    }
}
