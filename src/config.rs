use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone)]
pub struct AppConfig {
    pub cpu_governor_enabled: bool,
    pub foreground_boost_enabled: bool,
    pub ram_trimmer_enabled: bool,
    pub ram_trim_interval_sec: u64,
    pub standby_purge_threshold_mb: u64,
    pub high_res_timer_enabled: bool,
    pub dns_adblock_enabled: bool,
    pub clipboard_enabled: bool,
    pub defender_cpu_limit: u8,
    pub defender_low_priority: bool,
    pub defender_exclusions: Vec<String>,
    pub custom_blocked_domains: Vec<String>,
    pub extra_index_dir: String,
    pub minimize_to_tray_on_close: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            cpu_governor_enabled: true,
            foreground_boost_enabled: true,
            ram_trimmer_enabled: true,
            ram_trim_interval_sec: 300,
            standby_purge_threshold_mb: 1500,
            high_res_timer_enabled: false,
            dns_adblock_enabled: false,
            clipboard_enabled: true,
            defender_cpu_limit: 20,
            defender_low_priority: true,
            defender_exclusions: vec![],
            custom_blocked_domains: vec![],
            extra_index_dir: String::new(),
            minimize_to_tray_on_close: true,
        }
    }
}

impl AppConfig {
    fn path() -> PathBuf {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("wincore_config.toml")))
            .unwrap_or_else(|| PathBuf::from("wincore_config.toml"))
    }

    pub fn load() -> Self {
        fs::read_to_string(Self::path())
            .ok()
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        if let Ok(s) = toml::to_string_pretty(self) {
            let _ = fs::write(Self::path(), s);
        }
    }
}