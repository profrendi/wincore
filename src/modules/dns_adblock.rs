use crate::modules::defender::run_ps;
use crate::state::LiveState;
use std::net::UdpSocket;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

const DEFAULT_BLOCKLIST: &[&str] = &[
    "doubleclick.net",
    "googlesyndication.com",
    "googleadservices.com",
    "adservice.google.com",
    "pagead2.googlesyndication.com",
    "ads.youtube.com",
    "taboola.com",
    "outbrain.com",
    "vortex.data.microsoft.com",
    "telemetry.microsoft.com",
    "watson.telemetry.microsoft.com",
    "settings-win.data.microsoft.com",
    "oca.telemetry.microsoft.com",
    "sqm.telemetry.microsoft.com",
    "ads.tiktok.com",
    "analytics.tiktok.com",
    "graph.instagram.com",
    "pixel.facebook.com",
    "adnxs.com",
    "criteo.com",
    "scorecardresearch.com",
];

pub fn set_system_dns_local(enable: bool) -> Result<(), String> {
    if enable {
        run_ps("Get-NetAdapter | Where-Object {$_.Status -eq 'Up'} | Set-DnsClientServerAddress -ServerAddresses 127.0.0.1")
    } else {
        run_ps("Get-NetAdapter | Where-Object {$_.Status -eq 'Up'} | Set-DnsClientServerAddress -ResetServerAddresses")
    }
}

fn parse_qname(pkt: &[u8]) -> Option<String> {
    if pkt.len() < 13 {
        return None;
    }
    let mut i = 12;
    let mut labels = Vec::new();
    while i < pkt.len() {
        let len = pkt[i] as usize;
        if len == 0 {
            break;
        }
        i += 1;
        if i + len > pkt.len() {
            return None;
        }
        labels.push(String::from_utf8_lossy(&pkt[i..i + len]).to_lowercase());
        i += len;
    }
    if labels.is_empty() {
        None
    } else {
        Some(labels.join("."))
    }
}

fn is_blocked(domain: &str, state: &LiveState) -> bool {
    for &b in DEFAULT_BLOCKLIST {
        if domain == b || domain.ends_with(&format!(".{}", b)) {
            return true;
        }
    }
    if let Ok(custom) = state.custom_domains.lock() {
        for b in custom.iter() {
            if domain == b || domain.ends_with(&format!(".{}", b)) {
                return true;
            }
        }
    }
    false
}

pub fn spawn(state: Arc<LiveState>) {
    thread::spawn(move || {
        let mut socket: Option<UdpSocket> = None;
        let mut buf = [0u8; 512];

        loop {
            if !state.dns_adblock_on.load(Ordering::Relaxed) {
                if socket.is_some() {
                    socket = None;
                }
                thread::sleep(Duration::from_millis(500));
                continue;
            }

            if socket.is_none() {
                match UdpSocket::bind("127.0.0.1:53") {
                    Ok(s) => {
                        let _ = s.set_read_timeout(Some(Duration::from_millis(500)));
                        state.add_log("Local DNS Adblock aktif di 127.0.0.1:53".into());
                        socket = Some(s);
                    }
                    Err(e) => {
                        state.add_log(format!("Gagal bind DNS port 53: {}", e));
                        state.dns_adblock_on.store(false, Ordering::Relaxed);
                        thread::sleep(Duration::from_secs(2));
                        continue;
                    }
                }
            }

            if let Some(ref sock) = socket {
                if let Ok((len, src)) = sock.recv_from(&mut buf) {
                    let req = buf[..len].to_vec();
                    let state_c = state.clone();
                    let sock_clone = sock.try_clone();

                    thread::spawn(move || {
                        if let Ok(s) = sock_clone {
                            if let Some(domain) = parse_qname(&req) {
                                if is_blocked(&domain, &state_c) {
                                    state_c.dns_blocked_count.fetch_add(1, Ordering::Relaxed);
                                    // Balas NXDOMAIN (RCODE = 3)
                                    let mut resp = req;
                                    if resp.len() >= 4 {
                                        resp[2] = 0x81; // QR=1, RD=1
                                        resp[3] = 0x83; // RA=1, RCODE=3 (NXDOMAIN)
                                    }
                                    let _ = s.send_to(&resp, src);
                                    return;
                                }
                            }
                            // Forward ke 1.1.1.1:53
                            if let Ok(up) = UdpSocket::bind("0.0.0.0:0") {
                                let _ = up.set_read_timeout(Some(Duration::from_secs(2)));
                                if up.send_to(&req, "1.1.1.1:53").is_ok() {
                                    let mut up_buf = [0u8; 512];
                                    if let Ok((rlen, _)) = up.recv_from(&mut up_buf) {
                                        let _ = s.send_to(&up_buf[..rlen], src);
                                    }
                                }
                            }
                        }
                    });
                }
            }
        }
    });
}