#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    pub fn width(self) -> i32 {
        self.right - self.left
    }
    pub fn height(self) -> i32 {
        self.bottom - self.top
    }

    pub fn covers(self, other: Rect) -> bool {
        self.left <= other.left
            && self.right >= other.right
            && self.top <= other.top
            && self.bottom >= other.bottom
    }

    pub fn fills_display(self, display: Rect) -> bool {
        // Rounding and invisible frame margins differ slightly across DPI modes.
        const TOLERANCE: i32 = 3;
        self.covers(Rect {
            left: display.left + TOLERANCE,
            top: display.top + TOLERANCE,
            right: display.right - TOLERANCE,
            bottom: display.bottom - TOLERANCE,
        })
    }

    pub fn intersects(self, other: Rect) -> bool {
        self.left < other.right
            && self.right > other.left
            && self.top < other.bottom
            && self.bottom > other.top
    }

    fn is_control_on(self, bar: Rect) -> bool {
        let mid_y = self.top + self.height() / 2;
        self.height() <= bar.height() && mid_y >= bar.top && mid_y < bar.bottom
    }
}

#[derive(Clone, Copy)]
pub enum PlacementSample<T> {
    Active(T),
    Unavailable,
    Hidden,
}

pub struct StablePlacement<T> {
    current: Option<T>,
    misses: u8,
}

#[derive(Default)]
pub struct MissingSurface {
    since: Option<std::time::Instant>,
}

impl MissingSurface {
    pub fn fallback_due(&mut self, missing: bool, now: std::time::Instant) -> bool {
        if !missing {
            self.since = None;
            return false;
        }
        now.duration_since(*self.since.get_or_insert(now)) >= std::time::Duration::from_secs(8)
    }
}

impl<T: Copy> StablePlacement<T> {
    pub fn new() -> Self {
        Self {
            current: None,
            misses: 0,
        }
    }

    pub fn observe(&mut self, sample: PlacementSample<T>) -> Option<T> {
        match sample {
            PlacementSample::Active(position) => {
                self.current = Some(position);
                self.misses = 0;
            }
            PlacementSample::Hidden => {
                self.current = None;
                self.misses = 0;
            }
            PlacementSample::Unavailable => {
                self.misses = self.misses.saturating_add(1);
                if self.misses >= 3 {
                    self.current = None;
                }
            }
        }
        self.current
    }
}

pub fn place(bar: Rect, tray: Rect, screen: Rect, dpi: u32, occupied: &[Rect]) -> Option<Rect> {
    let px = |dip| (dip * dpi.clamp(96, 480) as i32 + 48) / 96;
    let visible = Rect {
        left: bar.left.max(screen.left),
        right: bar.right.min(screen.right),
        top: bar.top.max(screen.top),
        bottom: bar.bottom.min(screen.bottom),
    };
    if bar.height() > bar.width()
        || !bar.intersects(tray)
        || visible.height() < px(32)
        || visible.height() + px(2) < bar.height()
    {
        return None;
    }
    let height = visible.height().min(px(44));
    let top = visible.top + (visible.height() - height) / 2;
    let bottom = top + height;
    let gap = px(6);
    let tray_gap = px(12);
    let tray_on_right = tray.right - bar.left >= bar.right - tray.left;
    let (mut left, mut right) = if tray_on_right {
        (visible.left + gap, tray.left - tray_gap)
    } else {
        (tray.right + tray_gap, visible.right - gap)
    };
    let available = Rect {
        left,
        top,
        right,
        bottom,
    };
    for control in occupied
        .iter()
        .filter(|control| control.is_control_on(bar) && control.intersects(available))
    {
        if tray_on_right {
            left = left.max(control.right + gap);
        } else {
            right = right.min(control.left - gap);
        }
    }

    if right - left < px(40) {
        return None;
    }
    if tray_on_right {
        left = left.max(right - px(246));
    } else {
        right = right.min(left + px(246));
    }
    Some(Rect {
        left,
        top,
        right,
        bottom,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Rect = Rect {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1080,
    };
    const BAR: Rect = Rect {
        left: 0,
        top: 1020,
        right: 1920,
        bottom: 1080,
    };
    const TRAY: Rect = Rect {
        left: 1460,
        top: 1020,
        right: 1920,
        bottom: 1080,
    };

    #[test]
    fn fits_beside_the_tray_with_space_between_task_icons() {
        let task = Rect {
            left: 0,
            top: 1030,
            right: 995,
            bottom: 1070,
        };
        let rect = place(BAR, TRAY, SCREEN, 96, &[task]).unwrap();
        assert_eq!((rect.left, rect.right, rect.height()), (1202, 1448, 44));
        assert!(!rect.intersects(task));
    }

    #[test]
    fn narrows_and_then_hides_when_task_icons_fill_the_available_space() {
        let task = Rect {
            left: 1320,
            top: 1030,
            right: 1380,
            bottom: 1070,
        };
        assert_eq!(place(BAR, TRAY, SCREEN, 96, &[task]).unwrap().width(), 62);
        let task = Rect {
            right: 1430,
            ..task
        };
        assert_eq!(place(BAR, TRAY, SCREEN, 96, &[task]), None);
    }

    #[test]
    fn hides_with_an_auto_hidden_bar() {
        let hidden_bar = Rect {
            top: 1078,
            bottom: 1138,
            ..BAR
        };
        let hidden_tray = Rect {
            top: hidden_bar.top,
            bottom: hidden_bar.bottom,
            ..TRAY
        };
        assert_eq!(place(hidden_bar, hidden_tray, SCREEN, 96, &[]), None);
    }

    #[test]
    fn handles_a_mirrored_tray_at_150_percent() {
        let tray = Rect {
            left: 0,
            right: 460,
            ..TRAY
        };
        let rect = place(BAR, tray, SCREEN, 144, &[]).unwrap();
        assert_eq!((rect.left, rect.width(), rect.height()), (478, 369, 60));
    }

    #[test]
    fn a_taskbar_preview_crossing_its_edge_is_not_a_taskbar_button() {
        let preview = Rect {
            left: 1010,
            right: 1430,
            top: 957,
            bottom: 1042,
        };
        let preview_button = Rect {
            top: 1003,
            bottom: 1030,
            ..preview
        };
        assert_eq!(
            place(BAR, TRAY, SCREEN, 96, &[preview, preview_button]),
            place(BAR, TRAY, SCREEN, 96, &[])
        );
    }

    #[test]
    fn temporary_explorer_results_preserve_the_surface_but_hidden_bars_do_not() {
        use PlacementSample::*;
        let mut placement = StablePlacement::new();
        assert_eq!(placement.observe(Active(42)), Some(42));
        assert_eq!(placement.observe(Unavailable), Some(42));
        assert_eq!(placement.observe(Unavailable), Some(42));
        assert_eq!(placement.observe(Active(43)), Some(43));
        assert_eq!(placement.observe(Unavailable), Some(43));
        assert_eq!(placement.observe(Unavailable), Some(43));
        assert_eq!(placement.observe(Unavailable), None);
        assert_eq!(placement.observe(Active(42)), Some(42));
        assert_eq!(placement.observe(Hidden), None);
    }

    #[test]
    fn maximized_window_with_visible_taskbar_is_not_fullscreen() {
        let inset = Rect {
            left: 1,
            top: 2,
            right: 1918,
            bottom: 1079,
        };
        assert!(SCREEN.fills_display(SCREEN));
        assert!(inset.fills_display(SCREEN));
        let maximized = Rect {
            bottom: BAR.top,
            ..SCREEN
        };
        assert!(!maximized.fills_display(SCREEN));
    }

    #[test]
    fn fallback_requires_sustained_unavailability() {
        use std::time::{Duration, Instant};
        let t = Instant::now();
        let mut missing = MissingSurface::default();
        assert!(!missing.fallback_due(true, t));
        assert!(!missing.fallback_due(true, t + Duration::from_secs(7)));
        assert!(missing.fallback_due(true, t + Duration::from_secs(8)));
        assert!(!missing.fallback_due(false, t + Duration::from_secs(9)));
        assert!(!missing.fallback_due(true, t + Duration::from_secs(10)));
    }
}
