#[path = "../src/deco.rs"] mod deco;
#[path = "../src/model.rs"] #[allow(dead_code)] mod model;
use serde_json::json;
use std::{collections::HashMap, time::Instant};
#[tokio::main(flavor = "current_thread")]
async fn main() {
    let pw = keyring::Entry::new_with_target("deco-cli", "deco-cli", "admin").unwrap().get_password().unwrap();
    let mut d = deco::Deco::new("192.168.0.1", &pw);
    let t = Instant::now(); d.login().await.unwrap(); println!("login {:?}", t.elapsed());
    let r = || json!({"operation":"read"});
    let t = Instant::now();
    let reqs = [("admin/network?form=wan_ipv4", r()), ("admin/network?form=performance", r()), ("admin/wireless?form=wlan", r()),
                ("admin/device?form=device_list", r()), ("admin/client?form=client_list", json!({"operation":"read","params":{"device_mac":"default"}}))];
    let res = futures::future::join_all(reqs.iter().map(|(p, v)| d.request_once(p, v))).await;
    println!("phase1 {:?} ok={:?}", t.elapsed(), res.iter().map(|x| x.is_ok()).collect::<Vec<_>>());
    let nodes = model::nodes(&res[3].as_ref().unwrap()["device_list"]);
    let t = Instant::now();
    let per = futures::future::join_all(nodes.iter().map(|n| async { (n.name.clone(), d.request_once("admin/client?form=client_list", &json!({"operation":"read","params":{"device_mac": n.mac}})).await) })).await;
    println!("phase2 {:?}", t.elapsed());
    let mut map = HashMap::new();
    for (name, v) in per { for c in v.map(|v| v["client_list"].as_array().cloned().unwrap_or_default()).unwrap_or_default() { map.insert(model::norm_mac(c["mac"].as_str().unwrap()), name.clone()); } }
    for n in &nodes { println!("NODE {} | {} {} | {} sig={:?}", n.name, n.model, n.ip, n.backhaul, n.signal); }
    for w in model::wifi(&res[2].as_ref().unwrap()) { println!("WIFI {} {} {} {}", w.ssid, w.band_label, w.network, w.enabled); }
    for c in model::clients(&res[4].as_ref().unwrap()["client_list"], &map, &json!({})) { println!("CLIENT {:22} {:15} {:12} {:?}", c.name, c.ip, c.kind, c.node); }
}
