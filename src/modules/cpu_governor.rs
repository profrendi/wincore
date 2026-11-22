use crate::modules::process_list::{
    list_processes, pin_to_last_two_cores, set_eco_qos, set_io_priority_low, set_priority,
};
use crate::state::LiveState;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

const HEAVY_BG_TARGETS: &[&str] = &[
    "tiworker.exe",
    "msiexec.exe",
    "searchindexer.exe",
    "compattelrunner.exe",
    "mousocoreworker.exe",
    "trustedinstaller.exe",
    "edgeupdate.exe",
    "googleupdate.exe",
];

pub fn spawn(state: Arc<LiveState>) {
    thread::spawn(move || {
        let mut last_fg_pid: u32 = 0;
        let my_pid = std::process::id();

        loop {
            thread::sleep(Duration::from_secs(2));
            let gov_on = state.cpu_governor_on.load(Ordering::Relaxed);
            let fg_on = state.foreground_boost_on.load(Ordering::Relaxed);
            if !gov_on && !fg_on {
                continue;
            }

            let fg_pid = unsafe {
                let hwnd = GetForegroundWindow();
                let mut pid = 0u32;
                GetWindowThreadProcessId(hwnd, Some(&mut pid));
                pid
            };

            // Dynamic Foreground Boost
            if fg_on && fg_pid > 4 && fg_pid != last_fg_pid {
                if last_fg_pid > 4 && last_fg_pid != my_pid {
                    let _ = set_priority(last_fg_pid, "normal");
                }
                if fg_pid != my_pid {
                    let _ = set_priority(fg_pid, "above");
                    let _ = set_eco_qos(fg_pid, false);
                }
                last_fg_pid = fg_pid;
            }

            if !gov_on {
                continue;
            }

            // ProBalance: Throttle proses berat latar belakang (CPU Idle + EcoQoS + I/O Low + Affinity)
            let mut count = 0usize;
            for proc in list_processes() {
                if proc.pid == fg_pid || proc.pid <= 4 || proc.pid == my_pid {
                    continue;
                }
                let lower = proc.name.to_lowercase();
                if HEAVY_BG_TARGETS.contains(&lower.as_str()) {
                    if set_priority(proc.pid, "idle") {
                        let _ = set_eco_qos(proc.pid, true);
                        let _ = set_io_priority_low(proc.pid);
                        let _ = pin_to_last_two_cores(proc.pid);
                        count += 1;
                    }
                }
            }

            if let Ok(mut c) = state.throttled_count.lock() {
                *c = count;
            }
        }
    });
}