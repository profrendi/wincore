use crate::modules::defender::run_ps;
use std::fs;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::Command;

const CREATE_NO_WINDOW: u32 = 0x08000000;

pub fn get_service_status(name: &str) -> String {
    let out = Command::new("sc")
        .args(["query", name])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    if let Ok(o) = out {
        let s = String::from_utf8_lossy(&o.stdout);
        if s.contains("RUNNING") {
            return "RUNNING".into();
        } else if s.contains("STOPPED") {
            return "STOPPED".into();
        }
    }
    "UNKNOWN".into()
}

pub fn set_service_running(name: &str, start: bool) -> Result<(), String> {
    if start {
        let _ = Command::new("sc")
            .args(["config", name, "start=", "auto"])
            .creation_flags(CREATE_NO_WINDOW)
            .status();
        let st = Command::new("sc")
            .args(["start", name])
            .creation_flags(CREATE_NO_WINDOW)
            .status()
            .map_err(|e| e.to_string())?;
        if st.success() { Ok(()) } else { Err(format!("Gagal start {}", name)) }
    } else {
        let _ = Command::new("sc")
            .args(["stop", name])
            .creation_flags(CREATE_NO_WINDOW)
            .status();
        let st = Command::new("sc")
            .args(["config", name, "start=", "disabled"])
            .creation_flags(CREATE_NO_WINDOW)
            .status()
            .map_err(|e| e.to_string())?;
        if st.success() { Ok(()) } else { Err(format!("Gagal stop/disable {}", name)) }
    }
}

pub fn set_autostart_admin(enable: bool) -> Result<(), String> {
    if enable {
        let exe = std::env::current_exe()
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .into_owned();
        let tr = format!("\"{}\"", exe);
        let st = Command::new("schtasks")
            .args(["/Create", "/TN", "WinCoreAutoStart", "/TR", &tr, "/SC", "ONLOGON", "/RL", "HIGHEST", "/F"])
            .creation_flags(CREATE_NO_WINDOW)
            .status()
            .map_err(|e| e.to_string())?;
        if st.success() { Ok(()) } else { Err("Gagal membuat Task Scheduler.".into()) }
    } else {
        let st = Command::new("schtasks")
            .args(["/Delete", "/TN", "WinCoreAutoStart", "/F"])
            .creation_flags(CREATE_NO_WINDOW)
            .status()
            .map_err(|e| e.to_string())?;
        if st.success() { Ok(()) } else { Err("Gagal menghapus Task Scheduler.".into()) }
    }
}

pub fn block_app_firewall(app_name: &str, exe_path: &str, block: bool) -> Result<(), String> {
    let rule_name = format!("WinCore_Block_{}", app_name);
    if block {
        let st = Command::new("netsh")
            .args([
                "advfirewall", "firewall", "add", "rule",
                &format!("name={}", rule_name),
                "dir=out", "action=block",
                &format!("program={}", exe_path),
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .status()
            .map_err(|e| e.to_string())?;
        if st.success() { Ok(()) } else { Err("Gagal blokir firewall.".into()) }
    } else {
        let st = Command::new("netsh")
            .args(["advfirewall", "firewall", "delete", "rule", &format!("name={}", rule_name)])
            .creation_flags(CREATE_NO_WINDOW)
            .status()
            .map_err(|e| e.to_string())?;
        if st.success() { Ok(()) } else { Err("Gagal hapus aturan firewall.".into()) }
    }
}

pub fn toggle_memory_compression(enable: bool) -> Result<(), String> {
    if enable {
        run_ps("Enable-MMAgent -mc")
    } else {
        run_ps("Disable-MMAgent -mc")
    }
}

pub fn disable_8dot3_names() -> Result<(), String> {
    let st = Command::new("fsutil")
        .args(["8dot3name", "set", "1"])
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map_err(|e| e.to_string())?;
    if st.success() { Ok(()) } else { Err("Gagal set 8dot3name.".into()) }
}

pub fn apply_network_latency_tweak() -> Result<(), String> {
    run_ps("Set-ItemProperty -Path 'HKLM:\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Multimedia\\SystemProfile' -Name 'NetworkThrottlingIndex' -Value 0xffffffff -Type DWord -Force")
}

pub fn set_power_plan(mode: &str) -> Result<(), String> {
    let guid = match mode {
        "saver" => "a1841308-3541-4fab-bc81-f71556f20b4a",
        "high" => "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c",
        _ => "381b4222-f694-41f0-9685-ff5bb260df2e", // Balanced
    };
    let st = Command::new("powercfg")
        .args(["/setactive", guid])
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map_err(|e| e.to_string())?;
    if st.success() { Ok(()) } else { Err("Gagal ganti Power Plan.".into()) }
}

pub fn clean_temp_files() -> (usize, u64) {
    let mut files_removed = 0usize;
    let mut bytes_freed = 0u64;
    let mut dirs = vec![String::from("C:\\Windows\\Temp")];
    if let Ok(t) = std::env::var("TEMP") {
        dirs.push(t);
    }

    for d in dirs {
        clean_dir_recursive(Path::new(&d), &mut files_removed, &mut bytes_freed);
    }
    (files_removed, bytes_freed / (1024 * 1024))
}

fn clean_dir_recursive(dir: &Path, count: &mut usize, bytes: &mut u64) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_file() {
                let sz = entry.metadata().map(|m| m.len()).unwrap_or(0);
                if fs::remove_file(&p).is_ok() {
                    *count += 1;
                    *bytes += sz;
                }
            } else if p.is_dir() {
                clean_dir_recursive(&p, count, bytes);
                let _ = fs::remove_dir(&p);
            }
        }
    }
}