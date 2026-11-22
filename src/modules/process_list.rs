use std::ffi::{c_void, OsString};
use std::os::windows::ffi::OsStringExt;
use windows::core::{w, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, LUID};
use windows::Win32::Security::{
    AdjustTokenPrivileges, LookupPrivilegeValueW, SE_DEBUG_NAME, SE_PRIVILEGE_ENABLED,
    TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::ProcessStatus::{
    EmptyWorkingSet, GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
};
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, ProcessPowerThrottling,
    QueryFullProcessImageNameW, SetPriorityClass, SetProcessAffinityMask, SetProcessInformation,
    TerminateProcess, ABOVE_NORMAL_PRIORITY_CLASS, BELOW_NORMAL_PRIORITY_CLASS,
    IDLE_PRIORITY_CLASS, NORMAL_PRIORITY_CLASS, PROCESS_CREATION_FLAGS, PROCESS_NAME_WIN32,
    PROCESS_POWER_THROTTLING_CURRENT_VERSION, PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
    PROCESS_POWER_THROTTLING_STATE, PROCESS_QUERY_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_SET_INFORMATION, PROCESS_SET_QUOTA, PROCESS_SUSPEND_RESUME, PROCESS_TERMINATE,
};

#[link(name = "ntdll")]
extern "system" {
    fn NtSuspendProcess(process_handle: *mut c_void) -> i32;
    fn NtResumeProcess(process_handle: *mut c_void) -> i32;
    fn NtSetInformationProcess(
        process_handle: *mut c_void,
        process_information_class: u32,
        process_information: *const c_void,
        process_information_length: u32,
    ) -> i32;
    fn NtSetTimerResolution(
        desired_resolution: u32,
        set_resolution: u8,
        current_resolution: *mut u32,
    ) -> i32;
}

#[derive(Clone)]
pub struct ProcInfo {
    pub pid: u32,
    pub name: String,
    pub ram_mb: f64,
}

pub fn enable_privileges() {
    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY,
            &mut token,
        )
        .is_ok()
        {
            for priv_name in [SE_DEBUG_NAME, w!("SeProfileSingleProcessPrivilege")] {
                let mut luid = LUID::default();
                if LookupPrivilegeValueW(None, priv_name, &mut luid).is_ok() {
                    let mut tp = TOKEN_PRIVILEGES {
                        PrivilegeCount: 1,
                        ..Default::default()
                    };
                    tp.Privileges[0].Luid = luid;
                    tp.Privileges[0].Attributes = SE_PRIVILEGE_ENABLED;
                    let _ = AdjustTokenPrivileges(token, false, Some(&tp), 0, None, None);
                }
            }
            let _ = CloseHandle(token);
        }
    }
}

pub fn get_system_ram_mb() -> (u64, u64) {
    unsafe {
        let mut mem = MEMORYSTATUSEX {
            dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
            ..Default::default()
        };
        if GlobalMemoryStatusEx(&mut mem).is_ok() {
            let total = mem.ullTotalPhys / (1024 * 1024);
            let avail = mem.ullAvailPhys / (1024 * 1024);
            return (total.saturating_sub(avail), total);
        }
    }
    (0, 0)
}

pub fn list_processes() -> Vec<ProcInfo> {
    let mut result = Vec::new();
    unsafe {
        if let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
            let mut entry = PROCESSENTRY32W {
                dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
                ..Default::default()
            };
            if Process32FirstW(snap, &mut entry).is_ok() {
                loop {
                    let len = entry
                        .szExeFile
                        .iter()
                        .position(|&c| c == 0)
                        .unwrap_or(entry.szExeFile.len());
                    let name = OsString::from_wide(&entry.szExeFile[..len])
                        .to_string_lossy()
                        .into_owned();

                    let ram_mb = get_proc_ram_mb(entry.th32ProcessID);
                    result.push(ProcInfo {
                        pid: entry.th32ProcessID,
                        name,
                        ram_mb,
                    });

                    if Process32NextW(snap, &mut entry).is_err() {
                        break;
                    }
                }
            }
            let _ = CloseHandle(snap);
        }
    }
    result.sort_by(|a, b| {
        b.ram_mb
            .partial_cmp(&a.ram_mb)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    result
}

fn get_proc_ram_mb(pid: u32) -> f64 {
    if pid == 0 || pid == 4 {
        return 0.0;
    }
    unsafe {
        if let Ok(h) = OpenProcess(PROCESS_QUERY_INFORMATION, false, pid) {
            let mut pmc = PROCESS_MEMORY_COUNTERS::default();
            let ok = GetProcessMemoryInfo(
                h,
                &mut pmc,
                std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
            )
            .is_ok();
            let _ = CloseHandle(h);
            if ok {
                return pmc.WorkingSetSize as f64 / (1024.0 * 1024.0);
            }
        }
    }
    0.0
}

pub fn get_process_path(pid: u32) -> Option<String> {
    unsafe {
        if let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            let mut buf = [0u16; 1024];
            let mut size = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(
                h,
                PROCESS_NAME_WIN32,
                PWSTR(buf.as_mut_ptr()),
                &mut size,
            )
            .is_ok();
            let _ = CloseHandle(h);
            if ok && size > 0 {
                return Some(String::from_utf16_lossy(&buf[..size as usize]));
            }
        }
    }
    None
}

pub fn kill_process(pid: u32) -> bool {
    unsafe {
        if let Ok(h) = OpenProcess(PROCESS_TERMINATE, false, pid) {
            let ok = TerminateProcess(h, 1).is_ok();
            let _ = CloseHandle(h);
            return ok;
        }
    }
    false
}

pub fn suspend_process(pid: u32) -> bool {
    unsafe {
        if let Ok(h) = OpenProcess(PROCESS_SUSPEND_RESUME, false, pid) {
            let ok = NtSuspendProcess(h.0) == 0;
            let _ = CloseHandle(h);
            return ok;
        }
    }
    false
}

pub fn resume_process(pid: u32) -> bool {
    unsafe {
        if let Ok(h) = OpenProcess(PROCESS_SUSPEND_RESUME, false, pid) {
            let ok = NtResumeProcess(h.0) == 0;
            let _ = CloseHandle(h);
            return ok;
        }
    }
    false
}

pub fn trim_process(pid: u32) -> bool {
    unsafe {
        if let Ok(h) = OpenProcess(PROCESS_SET_QUOTA | PROCESS_QUERY_INFORMATION, false, pid) {
            let ok = EmptyWorkingSet(h).is_ok();
            let _ = CloseHandle(h);
            return ok;
        }
    }
    false
}

pub fn set_priority(pid: u32, mode: &str) -> bool {
    let flag: PROCESS_CREATION_FLAGS = match mode {
        "idle" => IDLE_PRIORITY_CLASS,
        "below" => BELOW_NORMAL_PRIORITY_CLASS,
        "above" => ABOVE_NORMAL_PRIORITY_CLASS,
        _ => NORMAL_PRIORITY_CLASS,
    };
    unsafe {
        if let Ok(h) = OpenProcess(PROCESS_SET_INFORMATION, false, pid) {
            let ok = SetPriorityClass(h, flag).is_ok();
            let _ = CloseHandle(h);
            return ok;
        }
    }
    false
}

pub fn set_eco_qos(pid: u32, enable: bool) -> bool {
    unsafe {
        if let Ok(h) = OpenProcess(PROCESS_SET_INFORMATION, false, pid) {
            let state = PROCESS_POWER_THROTTLING_STATE {
                Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
                ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
                StateMask: if enable {
                    PROCESS_POWER_THROTTLING_EXECUTION_SPEED
                } else {
                    0
                },
            };
            let ok = SetProcessInformation(
                h,
                ProcessPowerThrottling,
                &state as *const _ as *const c_void,
                std::mem::size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
            )
            .is_ok();
            let _ = CloseHandle(h);
            return ok;
        }
    }
    false
}

pub fn set_io_priority_low(pid: u32) -> bool {
    const PROCESS_IO_PRIORITY: u32 = 33;
    let io_prio_very_low: u32 = 0;
    unsafe {
        if let Ok(h) = OpenProcess(PROCESS_SET_INFORMATION, false, pid) {
            let ok = NtSetInformationProcess(
                h.0,
                PROCESS_IO_PRIORITY,
                &io_prio_very_low as *const u32 as *const c_void,
                std::mem::size_of::<u32>() as u32,
            ) == 0;
            let _ = CloseHandle(h);
            return ok;
        }
    }
    false
}

pub fn pin_to_last_two_cores(pid: u32) -> bool {
    unsafe {
        if let Ok(h) = OpenProcess(PROCESS_SET_INFORMATION, false, pid) {
            let ok = SetProcessAffinityMask(h, 0b1100).is_ok();
            let _ = CloseHandle(h);
            return ok;
        }
    }
    false
}

pub fn set_timer_resolution_high(enable: bool) -> bool {
    let desired: u32 = if enable { 5000 } else { 156250 }; // 0.5ms vs 15.625ms
    let mut current: u32 = 0;
    unsafe { NtSetTimerResolution(desired, if enable { 1 } else { 0 }, &mut current) == 0 }
}