use crate::state::LiveState;
use std::ffi::c_void;
use std::fs::{self, File};
use std::io::BufWriter;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits,
    ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, HGDIOBJ, SRCCOPY,
};
use windows::Win32::System::DataExchange::GetClipboardSequenceNumber;
use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};

const CREATE_NO_WINDOW: u32 = 0x08000000;

#[derive(Clone)]
pub struct IndexedItem {
    pub name: String,
    pub path: String,
}

pub fn build_app_index(extra_dir: &str) -> Vec<IndexedItem> {
    let mut items = Vec::new();
    let mut roots = vec![PathBuf::from(
        "C:\\ProgramData\\Microsoft\\Windows\\Start Menu\\Programs",
    )];
    if let Ok(appdata) = std::env::var("APPDATA") {
        roots.push(
            PathBuf::from(appdata)
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs"),
        );
    }
    if !extra_dir.trim().is_empty() {
        roots.push(PathBuf::from(extra_dir.trim()));
    }

    for r in roots {
        walk_index(&r, &mut items, 0, 4);
    }
    items
}

fn walk_index(dir: &Path, items: &mut Vec<IndexedItem>, depth: usize, max_depth: usize) {
    if depth > max_depth || items.len() > 8000 {
        return;
    }
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                walk_index(&p, items, depth + 1, max_depth);
            } else if let Some(fname) = p.file_name().and_then(|n| n.to_str()) {
                let lower = fname.to_lowercase();
                if lower.ends_with(".lnk")
                    || lower.ends_with(".exe")
                    || lower.ends_with(".bat")
                    || lower.ends_with(".cmd")
                    || depth > 0
                {
                    items.push(IndexedItem {
                        name: fname.to_string(),
                        path: p.to_string_lossy().into_owned(),
                    });
                }
            }
        }
    }
}

pub fn launch_path(path: &str) {
    let _ = Command::new("cmd")
        .args(["/C", "start", "", path])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
}

pub fn try_quick_calc(expr: &str) -> Option<f64> {
    let s = expr.replace(' ', "");
    for op in ['+', '-', '*', '/'] {
        if let Some(idx) = s.find(op) {
            if idx == 0 {
                continue;
            }
            let left: f64 = s[..idx].parse().ok()?;
            let right: f64 = s[idx + 1..].parse().ok()?;
            return match op {
                '+' => Some(left + right),
                '-' => Some(left - right),
                '*' => Some(left * right),
                '/' => if right != 0.0 { Some(left / right) } else { None },
                _ => None,
            };
        }
    }
    None
}

pub fn spawn_clipboard_watcher(state: Arc<LiveState>) {
    thread::spawn(move || {
        let mut last_seq: u32 = unsafe { GetClipboardSequenceNumber() };
        loop {
            thread::sleep(Duration::from_millis(600));
            if !state.clipboard_on.load(Ordering::Relaxed) {
                continue;
            }
            let seq = unsafe { GetClipboardSequenceNumber() };
            if seq != last_seq {
                last_seq = seq;
                if let Ok(mut cb) = arboard::Clipboard::new() {
                    if let Ok(text) = cb.get_text() {
                        let trimmed = text.trim().to_string();
                        if !trimmed.is_empty() {
                            if let Ok(mut hist) = state.clipboard_history.lock() {
                                if hist.first().map(|s| s != &trimmed).unwrap_or(true) {
                                    hist.retain(|x| x != &trimmed);
                                    hist.insert(0, trimmed);
                                    if hist.len() > 50 {
                                        hist.truncate(50);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    });
}

pub fn copy_to_clipboard(text: &str) -> bool {
    if let Ok(mut cb) = arboard::Clipboard::new() {
        return cb.set_text(text.to_string()).is_ok();
    }
    false
}

pub fn capture_screenshot_png() -> Result<String, String> {
    unsafe {
        let w = GetSystemMetrics(SM_CXSCREEN);
        let h = GetSystemMetrics(SM_CYSCREEN);
        if w <= 0 || h <= 0 {
            return Err("Resolusi layar tidak valid.".into());
        }

        let hdc_screen = GetDC(None);
        let hdc_mem = CreateCompatibleDC(hdc_screen);
        let hbmp = CreateCompatibleBitmap(hdc_screen, w, h);
        let old = SelectObject(hdc_mem, HGDIOBJ(hbmp.0));

        let _ = BitBlt(hdc_mem, 0, 0, w, h, hdc_screen, 0, 0, SRCCOPY);

        let mut bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h, // top-down DIB
                biPlanes: 1,
                biBitCount: 32,
                biCompression: 0, // BI_RGB
                ..Default::default()
            },
            ..Default::default()
        };

        let mut bgra = vec![0u8; (w * h * 4) as usize];
        GetDIBits(
            hdc_mem,
            hbmp,
            0,
            h as u32,
            Some(bgra.as_mut_ptr() as *mut c_void),
            &mut bmi,
            DIB_RGB_COLORS,
        );

        SelectObject(hdc_mem, old);
        let _ = DeleteObject(HGDIOBJ(hbmp.0));
        let _ = DeleteDC(hdc_mem);
        ReleaseDC(None, hdc_screen);

        // Konversi BGRA -> RGBA
        for px in bgra.chunks_exact_mut(4) {
            px.swap(0, 2);
            px[3] = 255;
        }

        let dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("screenshots")))
            .unwrap_or_else(|| PathBuf::from("screenshots"));
        let _ = fs::create_dir_all(&dir);

        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let file_path = dir.join(format!("screenshot_{}.png", ts));

        let file = File::create(&file_path).map_err(|e| e.to_string())?;
        let mut encoder = png::Encoder::new(BufWriter::new(file), w as u32, h as u32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
        writer.write_image_data(&bgra).map_err(|e| e.to_string())?;

        Ok(file_path.to_string_lossy().into_owned())
    }
}