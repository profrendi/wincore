use crate::modules::process_list::{get_system_ram_mb, list_processes, trim_process};
use crate::state::LiveState;
use std::ffi::c_void;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

#[link(name = "ntdll")]
extern "system" {
    fn NtSetSystemInformation(
        system_information_class: u32,
        system_information: *const c_void,
        system_information_length: u32,
    ) -> i32;
}

pub fn purge_standby_list() -> bool {
    const SYSTEM_MEMORY_LIST_INFORMATION: u32 = 80;
    let command: u32 = 4; // MemoryPurgeStandbyList
    unsafe {
        let status = NtSetSystemInformation(
            SYSTEM_MEMORY_LIST_INFORMATION,
            &command as *const u32 as *const c_void,
            std::mem::size_of::<u32>() as u32,
        );
        status == 0
    }
}

pub fn trim_all_now(state: &LiveState) {
    let mut trimmed = 0usize;
    for p in list_processes() {
        if p.pid > 4 && p.ram_mb > 20.0 && trim_process(p.pid) {
            trimmed += 1;
        }
    }
    let standby_ok = purge_standby_list();
    state.total_trim_runs.fetch_add(1, Ordering::Relaxed);
    state.add_log(format!(
        "RAM Trim selesai: {} proses di-trim, Standby Purge: {}",
        trimmed,
        if standby_ok { "OK" } else { "Dilewati" }
    ));
}

pub fn spawn(state: Arc<LiveState>, interval_sec: u64, threshold_mb: u64) {
    thread::spawn(move || loop {
        thread::sleep(Duration::from_secs(interval_sec.max(30)));
        if !state.ram_trimmer_on.load(Ordering::Relaxed) {
            continue;
        }
        let (used, total) = get_system_ram_mb();
        let avail = total.saturating_sub(used);
        if avail < threshold_mb {
            trim_all_now(&state);
        }
    });
}