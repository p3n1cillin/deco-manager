//! Turns raw Deco JSON into clean, typed data for the UI (names decoded, secrets dropped).
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;

#[derive(Serialize, Clone, Default)]
pub struct Internet {
    pub online: bool,
    pub wan_ip: String,
    pub gateway: String,
    pub dns: Vec<String>,
    pub conn_type: String,
    pub lan_ip: String,
    pub lan_mask: String,
}

#[derive(Serialize, Clone)]
pub struct Wifi {
    pub band: String,     // band2_4 | band5_1 | band6 (API key)
    pub band_label: String,
    pub network: String,  // host | guest
    pub ssid: String,
    pub enabled: bool,
    pub channel: String,
    pub width: String,
}

#[derive(Serialize, Clone)]
pub struct Node {
    pub mac: String,
    pub name: String,
    pub is_main: bool,
    pub model: String,
    pub hardware: String,
    pub firmware: String,
    pub ip: String,
    pub connected: bool,
    pub internet: bool,
    pub backhaul: String, // "Wired" | "Wireless" | "—"
    pub signal: Option<u8>, // 0-3 bars for wireless backhaul
    pub parent: Option<String>,
    pub clients: usize,
}

#[derive(Serialize, Clone)]
pub struct Client {
    pub mac: String,
    pub name: String,     // name reported by the device / Deco
    pub label: Option<String>,
    pub ip: String,
    pub online: bool,
    pub kind: String,     // phone, pc, iot_device...
    pub wired: bool,
    pub band: String,     // "2.4 GHz" / "5 GHz" / "" for wired
    pub network: String,  // main | guest | iot
    pub node: Option<String>,
    pub down_kbs: u64,
    pub up_kbs: u64,
    pub private_mac: bool,
}

#[derive(Serialize, Clone, Default)]
pub struct Snapshot {
    pub host: String,
    pub updated: u64,
    pub refreshing: bool,
    pub error: Option<String>,
    pub internet: Internet,
    pub cpu: Option<f64>,
    pub mem: Option<f64>,
    pub wifi: Vec<Wifi>,
    pub nodes: Vec<Node>,
    pub clients: Vec<Client>,
}

pub fn norm_mac(m: &str) -> String { m.trim().to_uppercase().replace(':', "-") }

fn s(v: &Value) -> String {
    match v {
        Value::String(x) => x.clone(),
        Value::Null => String::new(),
        x => x.to_string(),
    }
}

/// Deco base64-encodes user-entered names, but not preset ones; only accept a decode that
/// is strict base64 AND valid UTF-8, otherwise keep the raw text.
pub fn decode_name(raw: &str) -> String {
    if raw.len() % 4 == 0 && !raw.is_empty() {
        if let Ok(bytes) = B64.decode(raw) {
            if let Ok(t) = String::from_utf8(bytes) {
                if !t.chars().any(|c| c.is_control()) {
                    return t;
                }
            }
        }
    }
    raw.to_string()
}

/// Preset room names come as "living_room" -> "Living Room".
fn pretty_room(n: &str) -> String {
    if n.contains(' ') || n.chars().any(|c| c.is_uppercase()) {
        return n.to_string();
    }
    n.split('_')
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn num(v: &Value) -> Option<f64> {
    v.as_f64().or_else(|| v.as_str()?.parse().ok())
}

pub fn internet(wan: &Value, nodes: &[Node]) -> Internet {
    let w = &wan["wan"]["ip_info"];
    let l = &wan["lan"]["ip_info"];
    Internet {
        online: nodes.iter().any(|n| n.is_main && n.internet) || (nodes.is_empty() && !s(&w["ip"]).is_empty()),
        wan_ip: s(&w["ip"]),
        gateway: s(&w["gateway"]),
        dns: [s(&w["dns1"]), s(&w["dns2"])].into_iter().filter(|x| !x.is_empty() && x != "0.0.0.0").collect(),
        conn_type: s(&wan["wan"]["dial_type"]).to_uppercase(),
        lan_ip: s(&l["ip"]),
        lan_mask: s(&l["mask"]),
    }
}

pub fn perf(p: &Value) -> (Option<f64>, Option<f64>) {
    let pct = |v: &Value| num(v).map(|x| if x <= 1.0 { x * 100.0 } else { x });
    (pct(&p["cpu_usage"]), pct(&p["mem_usage"]))
}

pub fn wifi(w: &Value) -> Vec<Wifi> {
    let mut out = vec![];
    for (band, label) in [("band2_4", "2.4 GHz"), ("band5_1", "5 GHz"), ("band5_2", "5 GHz-2"), ("band6", "6 GHz")] {
        for net in ["host", "guest"] {
            let x = &w[band][net];
            if x.is_null() { continue; }
            out.push(Wifi {
                band: band.into(),
                band_label: label.into(),
                network: net.into(),
                ssid: decode_name(&s(&x["ssid"])),
                enabled: x["enable"].as_bool().unwrap_or(false),
                channel: s(&x["channel"]),
                width: s(&x["channel_width"]),
            });
        }
    }
    out
}

pub fn nodes(list: &Value) -> Vec<Node> {
    let mut v: Vec<Node> = list
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|n| {
            let is_main = n["role"] == "master";
            let name = match n["custom_nickname"].as_str() {
                Some(c) if !c.is_empty() => decode_name(c),
                _ => pretty_room(&decode_name(&s(&n["nickname"]))),
            };
            let conn: Vec<String> = n["connection_type"].as_array().cloned().unwrap_or_default().iter().map(s).collect();
            let backhaul = if is_main { "—".into() }
                else if conn.iter().any(|c| c == "wired") { "Wired".into() }
                else if conn.is_empty() { "Unknown".into() }
                else { "Wireless".into() };
            let sig = &n["signal_level"];
            let signal = if backhaul == "Wireless" {
                [num(&sig["band5"]), num(&sig["band2_4"])].into_iter().flatten().fold(None, |a: Option<f64>, b| Some(a.map_or(b, |a| a.max(b)))).map(|x| x as u8)
            } else { None };
            let parent = n["previous"].as_str().map(norm_mac).filter(|p| !is_main && !p.is_empty());
            Node {
                mac: norm_mac(&s(&n["mac"])),
                name,
                is_main,
                model: s(&n["device_model"]),
                hardware: s(&n["hardware_ver"]),
                firmware: s(&n["software_ver"]),
                ip: s(&n["device_ip"]),
                connected: n["group_status"] == "connected",
                internet: n["inet_status"] == "online",
                backhaul,
                signal,
                parent,
                clients: 0,
            }
        })
        .collect();
    v.sort_by_key(|n| (!n.is_main, n.name.clone()));
    v
}

pub fn clients(list: &Value, node_of: &HashMap<String, String>, labels: &Value) -> Vec<Client> {
    let mut out: Vec<Client> = list
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|c| {
            let mac = norm_mac(&s(&c["mac"]));
            let wired = c["wire_type"] == "wired";
            let band = match c["connection_type"].as_str() {
                _ if wired => "",
                Some("band2_4") => "2.4 GHz",
                Some("band5") => "5 GHz",
                Some("band6") => "6 GHz",
                _ => "Wi-Fi",
            };
            let first = u8::from_str_radix(mac.get(0..2).unwrap_or("00"), 16).unwrap_or(0);
            Client {
                label: labels[&mac].as_str().map(String::from),
                node: node_of.get(&mac).cloned(),
                name: decode_name(&s(&c["name"])),
                ip: s(&c["ip"]),
                online: c["online"].as_bool().unwrap_or(false),
                kind: s(&c["client_type"]),
                wired,
                band: band.into(),
                network: s(&c["interface"]),
                down_kbs: num(&c["down_speed"]).unwrap_or(0.0) as u64,
                up_kbs: num(&c["up_speed"]).unwrap_or(0.0) as u64,
                private_mac: first & 0x02 != 0,
                mac,
            }
        })
        .collect();
    out.sort_by_key(|c| c.ip.split('.').map(|p| p.parse::<u32>().unwrap_or(0)).fold(0u64, |a, b| a * 256 + b as u64));
    out
}
