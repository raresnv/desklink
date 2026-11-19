//! Bounded, decode-acknowledged video with conservative congestion adaptation.
use crate::video::VideoProfile;
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

pub(super) const MAX_IN_FLIGHT: usize = 6;

pub(super) struct FrameWindow {
    pending: VecDeque<u64>,
    next: u64,
    profile: VideoProfile,
    quality: u8,
    interval: Instant,
    completed: u32,
    pressured: bool,
    healthy: u8,
}
impl FrameWindow {
    pub(super) fn for_peer(supports_video_settings: bool) -> Self {
        let profile = if supports_video_settings {
            VideoProfile::Sharp
        } else {
            VideoProfile::Balanced
        };
        Self::at(profile, Instant::now())
    }
    fn at(profile: VideoProfile, now: Instant) -> Self {
        Self {
            pending: VecDeque::new(),
            next: rand::random::<u64>() | (1 << 63),
            profile,
            quality: profile.max_quality(),
            interval: now,
            completed: 0,
            pressured: false,
            healthy: 0,
        }
    }
    pub fn profile(&self) -> VideoProfile {
        self.profile
    }
    pub fn set_profile(&mut self, profile: VideoProfile) {
        if self.profile != profile {
            self.profile = profile;
            self.quality = profile.max_quality();
            self.interval = Instant::now();
            self.completed = 0;
            self.pressured = false;
            self.healthy = 0;
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
                self.quality = self
                    .quality
                    .saturating_sub(5)
                    .max(self.profile.min_quality());
                self.healthy = 0;
            } else if !self.pressured && fps >= 32.0 {
                self.healthy += 1;
                if self.healthy >= 5 {
                    self.quality = (self.quality + 2).min(self.profile.max_quality());
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
    fn negotiated_defaults_and_profile_changes_keep_decode_tickets() {
        use crate::video::VideoProfile::*;
        let legacy = FrameWindow::for_peer(false);
        assert_eq!((legacy.profile(), legacy.quality()), (Balanced, 75));
        let mut window = FrameWindow::for_peer(true);
        assert_eq!((window.profile(), window.quality()), (Sharp, 85));
        let tickets: Vec<_> = (0..MAX_IN_FLIGHT)
            .map(|_| window.reserve().unwrap())
            .collect();
        assert!(window.reserve().is_none());
        window.set_profile(Original);
        assert_eq!((window.quality(), window.has_capacity()), (90, false));
        assert!(!window.pressured);
        for ticket in tickets {
            assert!(window.acknowledge(ticket));
        }
        for profile in [Sharp, Original, Balanced] {
            window.set_profile(profile);
            let mut now = window.interval;
            let mut tickets = VecDeque::new();
            for _ in 0..20 {
                while let Some(ticket) = window.reserve() {
                    tickets.push_back(ticket);
                }
                now += Duration::from_secs(3);
                window.acknowledge_at(tickets.pop_front().unwrap(), now);
            }
            assert_eq!(window.quality(), profile.min_quality());
            window.set_profile(profile);
            assert_eq!(
                window.quality(),
                profile.min_quality(),
                "duplicate settings reset adaptation"
            );
            for ticket in tickets {
                window.acknowledge_at(ticket, now);
            }
            for _ in 0..8000 {
                now += Duration::from_millis(28);
                let ticket = window.reserve().unwrap();
                window.acknowledge_at(ticket, now);
            }
            assert_eq!(window.quality(), profile.max_quality());
        }
    }
    #[test]
    fn recovered_thirty_five_fps_restores_quality_with_headroom() {
        let start = Instant::now();
        let mut window = FrameWindow::at(VideoProfile::Balanced, start);
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
        let mut idle = FrameWindow::at(VideoProfile::Balanced, start);
        let ticket = idle.reserve().unwrap();
        idle.acknowledge_at(ticket, start + std::time::Duration::from_secs(3));
        assert_eq!(idle.quality(), 75);
        let mut busy = FrameWindow::at(VideoProfile::Balanced, start);
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
        let mut window = FrameWindow::at(VideoProfile::Balanced, start);
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
