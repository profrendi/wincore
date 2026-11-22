use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::Mutex;

pub struct LiveState {
    pub cpu_governor_on: AtomicBool,
    pub foreground_boost_on: AtomicBool,
    pub ram_trimmer_on: AtomicBool,
    pub dns_adblock_on: AtomicBool,
    pub clipboard_on: AtomicBool,
    pub force_exit: AtomicBool,

    pub throttled_count: Mutex<usize>,
    pub total_trim_runs: AtomicU64,
    pub dns_blocked_count: AtomicU64,
    pub frozen_pids: Mutex<HashSet<u32>>,
    pub clipboard_history: Mutex<Vec<String>>,
    pub custom_domains: Mutex<HashSet<String>>,
    pub logs: Mutex<Vec<String>>,
}

impl LiveState {
    pub fn new(
        cpu_on: bool,
        fg_on: bool,
        ram_on: bool,
        dns_on: bool,
        clip_on: bool,
        domains: &[String],
    ) -> Self {
        let mut set = HashSet::new();
        for d in domains {
            let clean = d.trim().to_lowercase();
            if !clean.is_empty() {
                set.insert(clean);
            }
        }
        Self {
            cpu_governor_on: AtomicBool::new(cpu_on),
            foreground_boost_on: AtomicBool::new(fg_on),
            ram_trimmer_on: AtomicBool::new(ram_on),
            dns_adblock_on: AtomicBool::new(dns_on),
            clipboard_on: AtomicBool::new(clip_on),
            force_exit: AtomicBool::new(false),
            throttled_count: Mutex::new(0),
            total_trim_runs: AtomicU64::new(0),
            dns_blocked_count: AtomicU64::new(0),
            frozen_pids: Mutex::new(HashSet::new()),
            clipboard_history: Mutex::new(Vec::new()),
            custom_domains: Mutex::new(set),
            logs: Mutex::new(vec!["WinCore v0.2 aktif dengan hak Administrator.".into()]),
        }
    }

    pub fn add_log(&self, msg: String) {
        if let Ok(mut logs) = self.logs.lock() {
            logs.push(msg);
            if logs.len() > 250 {
                logs.remove(0);
            }
        }
    }
}