#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod deco;
mod model;

use deco::Deco;
use model::{norm_mac, Snapshot};
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    collections::{HashMap, VecDeque},
    fs,
    path::PathBuf,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::{Mutex, Notify};

const KEYRING: &str = "deco-manager";
const POLL_SECS: u64 = 30;
type R<T> = Result<T, String>;

#[derive(Serialize, Clone)]
struct Activity { time: u64, joined: bool, mac: String, name: String, ip: String, known: bool }

#[derive(Default)]
struct Inner {
    deco: Mutex<Option<Deco>>,
    snap: Mutex<Snapshot>,
    node_of: Mutex<HashMap<String, String>>, // client MAC -> node name
    raw_clients: Mutex<Value>,
    activity: Mutex<VecDeque<Activity>>,
    wake: Notify,
}
type St = Arc<Inner>;

fn e<E: std::fmt::Display>(x: E) -> String { x.to_string() }
fn now_ms() -> u64 { SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64 }

fn data_file(app: &AppHandle, name: &str) -> PathBuf {
    let dir = app.path().app_data_dir().expect("app data dir");
    fs::create_dir_all(&dir).ok();
    dir.join(name)
}
fn load_json(p: PathBuf) -> Value {
    fs::read_to_string(p).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(json!({}))
}
fn labels(app: &AppHandle) -> Value {
    let p = data_file(app, "labels.json");
    load_json(p)
}

/// Run several signed reads in parallel on one session (router handles them concurrently).
async fn read_all(d: &Deco, reqs: &[(&str, Value)]) -> Vec<anyhow::Result<Value>> {
    futures::future::join_all(reqs.iter().map(|(p, v)| d.request_once(p, v))).await
}
fn is_session_err(r: &anyhow::Result<Value>) -> bool {
    r.as_ref().is_err_and(|e| e.to_string().contains("session expired"))
}
/// Deco allows one admin session: the Deco app, its web page or another tool signing in
/// evicts ours. Log back in and retry, backing off a little in case the other login is still active.
async fn read_all_retry(d: &mut Deco, reqs: &[(&str, Value)]) -> Vec<anyhow::Result<Value>> {
    let mut res = read_all(d, reqs).await;
    for wait in [0u64, 2, 5] {
        if !res.iter().any(is_session_err) { break; }
        if wait > 0 { tokio::time::sleep(Duration::from_secs(wait)).await; }
        match d.login().await {
            Ok(()) => {
                eprintln!("[deco] session was taken over by another login; signed back in");
                res = read_all(d, reqs).await;
            }
            Err(e) => eprintln!("[deco] re-login failed: {e}"),
        }
    }
    res
}
fn friendly(e: &str) -> String {
    if e.contains("session expired") {
        "Signed out by another Deco login (the Deco app or web page). Reconnecting on the next refresh…".into()
    } else if e.contains("error sending request") || e.contains("cannot reach") || e.contains("timed out") {
        "Can't reach your Deco. Check this PC is on the home network.".into()
    } else { e.to_string() }
}

async fn emit_snap(app: &AppHandle, st: &St) {
    let s = st.snap.lock().await.clone();
    app.emit("snapshot", s).ok();
}

/// One refresh cycle: fast data first (~3-5s), then per-node client mapping (~5s more).
async fn refresh(app: &AppHandle, st: &St) {
    let mut guard = st.deco.lock().await;
    let Some(d) = guard.as_mut() else { return };
    { st.snap.lock().await.refreshing = true; }
    emit_snap(app, st).await;

    let read = || json!({"operation": "read"});
    let all = json!({"operation": "read", "params": {"device_mac": "default"}});
    let r = read_all_retry(d, &[
        ("admin/network?form=wan_ipv4", read()),
        ("admin/network?form=performance", read()),
        ("admin/wireless?form=wlan", read()),
        ("admin/device?form=device_list", read()),
        ("admin/client?form=client_list", all),
    ]).await;
    let ok = |i: usize| r[i].as_ref().ok().cloned().unwrap_or(Value::Null);
    let err = r.iter().filter_map(|x| x.as_ref().err()).next().map(|e| e.to_string());
    if let Some(e) = &err { eprintln!("[deco] refresh error: {e}"); }
    let raw_nodes = ok(3)["device_list"].clone();
    let raw_clients = ok(4)["client_list"].clone();
    let lbl = labels(app);

    let mut nodes = model::nodes(&raw_nodes);
    let old_online: HashMap<String, model::Client>;
    {
        let mut s = st.snap.lock().await;
        old_online = s.clients.iter().filter(|c| c.online).map(|c| (c.mac.clone(), c.clone())).collect();
        // Only replace what actually came back; a failed read keeps the last good data on screen.
        let any_ok = r.iter().any(|x| x.is_ok());
        {
            if r[0].is_ok() { s.internet = model::internet(&ok(0), if r[3].is_ok() { &nodes } else { &s.nodes }); }
            if r[1].is_ok() { (s.cpu, s.mem) = model::perf(&ok(1)); }
            if r[2].is_ok() { s.wifi = model::wifi(&ok(2)); }
            if r[4].is_ok() {
                s.clients = model::clients(&raw_clients, &*st.node_of.lock().await, &lbl);
                *st.raw_clients.lock().await = raw_clients.clone();
            }
            if r[3].is_ok() {
                // keep previous client counts until phase 2 recomputes them
                for n in nodes.iter_mut() { n.clients = s.nodes.iter().find(|o| o.mac == n.mac).map_or(0, |o| o.clients); }
                s.nodes = nodes.clone();
            }
        }
        s.error = err.as_deref().map(friendly);
        if any_ok { s.updated = now_ms(); }
    }
    emit_snap(app, st).await;

    // Phase 2: which Deco is each client on?
    if !nodes.is_empty() {
        let reqs: Vec<(&str, Value)> = nodes.iter()
            .map(|n| ("admin/client?form=client_list", json!({"operation": "read", "params": {"device_mac": n.mac}})))
            .collect();
        let per = read_all(d, &reqs).await;
        let mut map = HashMap::new();
        for (n, res) in nodes.iter_mut().zip(per) {
            if let Ok(v) = res {
                let list = v["client_list"].as_array().cloned().unwrap_or_default();
                n.clients = list.iter().filter(|c| c["online"].as_bool().unwrap_or(true)).count();
                for c in list { map.insert(norm_mac(c["mac"].as_str().unwrap_or("")), n.name.clone()); }
            }
        }
        *st.node_of.lock().await = map.clone();
        let mut s = st.snap.lock().await;
        s.nodes = nodes;
        s.clients = model::clients(&*st.raw_clients.lock().await, &map, &lbl);
    }
    drop(guard);

    // Join / leave detection
    let new_events: Vec<Activity> = {
        let mut s = st.snap.lock().await;
        s.refreshing = false;
        let mut ev = vec![];
        if !old_online.is_empty() {
            let now_online: HashMap<_, _> = s.clients.iter().filter(|c| c.online).map(|c| (c.mac.clone(), c)).collect();
            let t = now_ms();
            let mk = |c: &model::Client, joined| Activity {
                time: t, joined, mac: c.mac.clone(), ip: c.ip.clone(), known: c.label.is_some(),
                name: c.label.clone().unwrap_or_else(|| c.name.clone()),
            };
            for (m, c) in &now_online { if !old_online.contains_key(m) { ev.push(mk(c, true)); } }
            for (m, c) in &old_online { if !now_online.contains_key(m) { ev.push(mk(c, false)); } }
        }
        ev
    };
    if !new_events.is_empty() {
        let mut a = st.activity.lock().await;
        for ev in &new_events { a.push_front(ev.clone()); }
        a.truncate(500);
        app.emit("activity", &new_events).ok();
    }
    emit_snap(app, st).await;
}

async fn poller(app: AppHandle, st: St) {
    loop {
        refresh(&app, &st).await;
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(POLL_SECS)) => {},
            _ = st.wake.notified() => {},
        }
    }
}

async fn start_session(app: &AppHandle, st: &St, host: &str, d: Deco) {
    *st.deco.lock().await = Some(d);
    *st.snap.lock().await = Snapshot { host: host.into(), ..Default::default() };
    st.wake.notify_one();
    let _ = app;
}

#[tauri::command]
async fn connect(app: AppHandle, st: State<'_, St>, host: String, password: String, remember: bool) -> R<()> {
    let mut d = Deco::new(&host, &password);
    d.login().await.map_err(e)?;
    if remember {
        keyring::Entry::new(KEYRING, "admin").and_then(|k| k.set_password(&password)).map_err(e)?;
        fs::write(data_file(&app, "config.json"), json!({"host": host}).to_string()).map_err(e)?;
    }
    start_session(&app, &st, &host, d).await;
    Ok(())
}

#[tauri::command]
async fn auto_connect(app: AppHandle, st: State<'_, St>) -> R<Option<String>> {
    let Some(host) = load_json(data_file(&app, "config.json"))["host"].as_str().map(String::from) else { return Ok(None) };
    let Ok(pw) = keyring::Entry::new(KEYRING, "admin").and_then(|k| k.get_password()) else { return Ok(None) };
    let mut d = Deco::new(&host, &pw);
    d.login().await.map_err(e)?;
    start_session(&app, &st, &host, d).await;
    Ok(Some(host))
}

#[tauri::command]
async fn forget(app: AppHandle, st: State<'_, St>) -> R<()> {
    keyring::Entry::new(KEYRING, "admin").and_then(|k| k.delete_credential()).ok();
    fs::remove_file(data_file(&app, "config.json")).ok();
    *st.deco.lock().await = None;
    *st.snap.lock().await = Snapshot::default();
    Ok(())
}

/// Best guess at the main Deco's address for the sign-in screen.
#[tauri::command]
fn detect_host() -> Option<String> { deco::default_gateway() }

#[tauri::command]
async fn snapshot(st: State<'_, St>) -> R<Snapshot> { Ok(st.snap.lock().await.clone()) }

#[tauri::command]
async fn refresh_now(st: State<'_, St>) -> R<()> { st.wake.notify_one(); Ok(()) }

#[tauri::command]
async fn activity(st: State<'_, St>) -> R<Vec<Activity>> { Ok(st.activity.lock().await.iter().cloned().collect()) }

#[tauri::command]
async fn set_label(app: AppHandle, st: State<'_, St>, mac: String, label: String) -> R<()> {
    let p = data_file(&app, "labels.json");
    let mut l = labels(&app);
    let m = norm_mac(&mac);
    let label = label.trim().to_string();
    if label.is_empty() { l.as_object_mut().unwrap().remove(&m); } else { l[&m] = json!(label); }
    fs::write(p, serde_json::to_string_pretty(&l).unwrap()).map_err(e)?;
    {
        let mut s = st.snap.lock().await;
        if let Some(c) = s.clients.iter_mut().find(|c| c.mac == m) {
            c.label = if label.is_empty() { None } else { Some(label) };
        }
    }
    emit_snap(&app, &st).await;
    Ok(())
}

async fn with_deco<T>(st: &St, f: impl AsyncFnOnce(&mut Deco) -> anyhow::Result<T>) -> R<T> {
    let mut g = st.deco.lock().await;
    let d = g.as_mut().ok_or("Not connected")?;
    f(d).await.map_err(e)
}

#[tauri::command]
async fn set_wifi(app: AppHandle, st: State<'_, St>, band: String, network: String, enable: bool) -> R<()> {
    let mut params = json!({});
    params[&band] = json!({ network.clone(): {"enable": enable} });
    with_deco(&st, async |d| d.request("admin/wireless?form=wlan", json!({"operation": "write", "params": params})).await).await?;
    {
        let mut s = st.snap.lock().await;
        if let Some(w) = s.wifi.iter_mut().find(|w| w.band == band && w.network == network) { w.enabled = enable; }
    }
    emit_snap(&app, &st).await;
    Ok(())
}

#[tauri::command]
async fn reboot(st: State<'_, St>, mac: Option<String>) -> R<()> {
    let macs: Vec<Value> = match mac {
        Some(m) => vec![json!({"mac": m})],
        None => st.snap.lock().await.nodes.iter().map(|n| json!({"mac": n.mac})).collect(),
    };
    with_deco(&st, async |d| d.request("admin/device?form=system", json!({"operation": "reboot", "params": {"mac_list": macs}})).await).await?;
    Ok(())
}

#[tauri::command]
async fn raw(st: State<'_, St>, path: String, payload: Value) -> R<Value> {
    with_deco(&st, async |d| d.request(&path, payload).await).await
}

/// Tauri only sets the window's small icon, so the taskbar (which uses the large one) shows
/// Windows' generic app icon. Hand the window both sizes of the icon embedded in the .exe.
#[cfg(windows)]
fn set_taskbar_icon(window: &tauri::WebviewWindow) {
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, LoadImageW, SendMessageW, ICON_BIG, ICON_SMALL, IMAGE_ICON, LR_SHARED,
        SM_CXICON, SM_CXSMICON, SM_CYICON, SM_CYSMICON, WM_SETICON,
    };
    const APP_ICON_ID: usize = 32512; // tauri-build embeds the app icon under this resource id
    let Ok(hwnd) = window.hwnd() else { return };
    unsafe {
        let module = GetModuleHandleW(std::ptr::null());
        let load = |cx, cy| LoadImageW(module, APP_ICON_ID as *const u16, IMAGE_ICON, GetSystemMetrics(cx), GetSystemMetrics(cy), LR_SHARED);
        let (big, small) = (load(SM_CXICON, SM_CYICON), load(SM_CXSMICON, SM_CYSMICON));
        let hwnd = hwnd.0 as _;
        if !big.is_null() { SendMessageW(hwnd, WM_SETICON, ICON_BIG as usize, big as isize); }
        if !small.is_null() { SendMessageW(hwnd, WM_SETICON, ICON_SMALL as usize, small as isize); }
    }
}

fn main() {
    let st: St = Arc::new(Inner::default());
    tauri::Builder::default()
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .manage(st.clone())
        .setup(move |app| {
            #[cfg(windows)]
            if let Some(w) = app.get_webview_window("main") { set_taskbar_icon(&w); }
            tauri::async_runtime::spawn(poller(app.handle().clone(), st.clone()));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            connect, auto_connect, detect_host, forget, snapshot, refresh_now, activity, set_label, set_wifi, reboot, raw
        ])
        .run(tauri::generate_context!())
        .expect("error running app");
}
