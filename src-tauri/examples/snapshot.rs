// Dumps a full UI snapshot as JSON (handy for previewing the UI in a browser).
//   DECO_HOST=... DECO_PASSWORD=... cargo run --example snapshot > snap.json
#[path = "../src/deco.rs"] mod deco;
#[path = "../src/model.rs"] #[allow(dead_code)] mod model;
use serde_json::json;
use std::collections::HashMap;
#[tokio::main(flavor = "current_thread")]
async fn main() {
    let host = std::env::var("DECO_HOST").ok().or_else(deco::default_gateway).expect("set DECO_HOST");
    let pw = std::env::var("DECO_PASSWORD").unwrap_or_else(|_| keyring::Entry::new("deco-manager", "admin").unwrap().get_password().unwrap());
    let mut d = deco::Deco::new(&host, &pw);
    d.login().await.unwrap();
    let r = || json!({"operation":"read"});
    let wan = d.request("admin/network?form=wan_ipv4", r()).await.unwrap();
    let perf = d.request("admin/network?form=performance", r()).await.unwrap_or_default();
    let wlan = d.request("admin/wireless?form=wlan", r()).await.unwrap();
    let mut nodes = model::nodes(&d.request("admin/device?form=device_list", r()).await.unwrap()["device_list"]);
    let all = d.request("admin/client?form=client_list", json!({"operation":"read","params":{"device_mac":"default"}})).await.unwrap();
    let mut map = HashMap::new();
    for n in nodes.iter_mut() {
        if let Ok(v) = d.request("admin/client?form=client_list", json!({"operation":"read","params":{"device_mac": n.mac}})).await {
            let l = v["client_list"].as_array().cloned().unwrap_or_default();
            n.clients = l.len();
            for c in l { map.insert(model::norm_mac(c["mac"].as_str().unwrap_or("")), n.name.clone()); }
        }
    }
    let labels: serde_json::Value = std::fs::read_to_string(std::env::var("APPDATA").unwrap_or_default() + "/com.local.decomanager/labels.json")
        .ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(json!({}));
    let (cpu, mem) = model::perf(&perf);
    let s = model::Snapshot { host: host.clone(), updated: 1, refreshing: false, error: None,
        internet: model::internet(&wan, &nodes), cpu, mem, wifi: model::wifi(&wlan), clients: model::clients(&all["client_list"], &map, &labels), nodes };
    println!("{}", serde_json::to_string(&s).unwrap());
}
