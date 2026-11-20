//! Schedule the clock from the Windows wall clock, including after sleep or clock changes.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

const TICKS_PER_SECOND: u64 = 10_000_000;
const TICKS_PER_MINUTE: u64 = 60 * TICKS_PER_SECOND;
const RECHECK_AFTER: Duration = Duration::from_secs(5);

fn next_tick(ticks: u64, interval: u64) -> u64 {
    ticks + (interval - ticks % interval)
}

fn relative_wait(interval: u64) {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let ticks = (now.as_nanos() / 100) as u64;
    let remaining = Duration::from_nanos((next_tick(ticks, interval) - ticks) * 100);
    std::thread::sleep(remaining.min(RECHECK_AFTER));
}

pub struct ClockTimer {
    #[cfg(windows)]
    handle: windows_sys::Win32::Foundation::HANDLE,
}

#[cfg(windows)]
fn settings_event() -> windows_sys::Win32::Foundation::HANDLE {
    use std::{ptr::null, sync::OnceLock};
    use windows_sys::Win32::System::Threading::CreateEventW;

    // An unnamed, auto-reset event shared by the settings command and the clock worker.
    static EVENT: OnceLock<usize> = OnceLock::new();
    *EVENT.get_or_init(|| unsafe { CreateEventW(null(), 0, 0, null()) as usize }) as _
}

impl ClockTimer {
    pub fn settings_changed() {
        #[cfg(windows)]
        {
            let event = settings_event();
            if !event.is_null() {
                unsafe { windows_sys::Win32::System::Threading::SetEvent(event) };
            }
        }
    }

    pub fn new() -> Self {
        #[cfg(windows)]
        {
            use std::ptr::null;
            use windows_sys::Win32::System::Threading::{
                CreateWaitableTimerExW, CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,
                SYNCHRONIZATION_SYNCHRONIZE, TIMER_MODIFY_STATE,
            };

            let access = TIMER_MODIFY_STATE | SYNCHRONIZATION_SYNCHRONIZE;
            // Each instance owns an unnamed timer. Older systems can use the same
            // absolute deadline with a standard waitable timer.
            let mut handle = unsafe {
                CreateWaitableTimerExW(
                    null(),
                    null(),
                    CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,
                    access,
                )
            };
            if handle.is_null() {
                handle = unsafe { CreateWaitableTimerExW(null(), null(), 0, access) };
            }
            Self { handle }
        }
        #[cfg(not(windows))]
        Self {}
    }

    pub fn wait(&self, seconds: bool) {
        let interval = if seconds {
            TICKS_PER_SECOND
        } else {
            TICKS_PER_MINUTE
        };
        #[cfg(windows)]
        if self.wait_on_windows(interval) {
            return;
        }
        relative_wait(interval);
    }

    #[cfg(windows)]
    fn wait_on_windows(&self, interval: u64) -> bool {
        use std::ptr::null;
        use windows_sys::Win32::{
            Foundation::{FILETIME, WAIT_OBJECT_0, WAIT_TIMEOUT},
            System::{
                SystemInformation::GetSystemTimePreciseAsFileTime,
                Threading::{SetWaitableTimer, WaitForMultipleObjects},
            },
        };

        if self.handle.is_null() {
            return false;
        }
        let mut filetime = FILETIME::default();
        unsafe { GetSystemTimePreciseAsFileTime(&mut filetime) };
        let ticks = (u64::from(filetime.dwHighDateTime) << 32) | u64::from(filetime.dwLowDateTime);
        // FILETIME's UTC epoch starts on a minute boundary. The positive value
        // configures an absolute wall-clock deadline, with no periodic drift.
        let due = next_tick(ticks, interval) as i64;
        if unsafe { SetWaitableTimer(self.handle, &due, 0, None, null(), 0) } == 0 {
            return false;
        }
        let handles = [self.handle, settings_event()];
        let count = if handles[1].is_null() { 1 } else { 2 };
        let status = unsafe {
            WaitForMultipleObjects(count, handles.as_ptr(), 0, RECHECK_AFTER.as_millis() as u32)
        };
        status == WAIT_TIMEOUT || (WAIT_OBJECT_0..WAIT_OBJECT_0 + count).contains(&status)
    }
}

#[cfg(windows)]
impl Drop for ClockTimer {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            unsafe { windows_sys::Win32::Foundation::CloseHandle(self.handle) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_the_exact_minute_without_accumulating_drift() {
        let minute = 83 * TICKS_PER_MINUTE;
        assert_eq!(next_tick(minute - 1, TICKS_PER_MINUTE), minute);
        assert_eq!(
            next_tick(minute, TICKS_PER_MINUTE),
            minute + TICKS_PER_MINUTE
        );
        assert_eq!(
            next_tick(minute + 31_714, TICKS_PER_MINUTE),
            minute + TICKS_PER_MINUTE
        );
    }

    #[test]
    fn recalculates_the_boundary_after_a_wall_clock_jump() {
        let first = next_tick(
            73 * TICKS_PER_MINUTE + 59 * TICKS_PER_SECOND,
            TICKS_PER_MINUTE,
        );
        let after_sleep = next_tick(
            782 * TICKS_PER_MINUTE + 27 * TICKS_PER_SECOND,
            TICKS_PER_MINUTE,
        );
        assert_eq!(first, 74 * TICKS_PER_MINUTE);
        assert_eq!(after_sleep, 783 * TICKS_PER_MINUTE);
    }

    #[test]
    fn seconds_follow_the_system_boundaries_across_minute_rollover() {
        let minute = 83 * TICKS_PER_MINUTE;
        assert_eq!(next_tick(minute - 1, TICKS_PER_SECOND), minute);
        assert_eq!(
            next_tick(minute + 82_813, TICKS_PER_SECOND),
            minute + TICKS_PER_SECOND
        );
    }
}
