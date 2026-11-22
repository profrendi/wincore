use std::os::windows::process::CommandExt;
use std::process::Command;

const CREATE_NO_WINDOW: u32 = 0x08000000;

pub fn apply_tuning(cpu_limit: u8, low_priority: bool) -> Result<(), String> {
    let low_str = if low_priority { "$true" } else { "$false" };
    let script = format!(
        "Set-MpPreference -ScanAvgCPULoadFactor {} -EnableLowCpuPriority {}",
        cpu_limit.clamp(5, 50),
        low_str
    );
    run_ps(&script)
}

pub fn add_exclusion(path: &str) -> Result<(), String> {
    let clean = path.trim().replace('\'', "''");
    if clean.is_empty() {
        return Err("Path kosong.".into());
    }
    run_ps(&format!("Add-MpPreference -ExclusionPath '{}'", clean))
}

pub fn run_ps(script: &str) -> Result<(), String> {
    let status = Command::new("powershell")
        .args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", script])
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err("PowerShell mengembalikan error.".into())
    }
}