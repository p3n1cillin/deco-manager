#[path = "../src/deco.rs"] mod deco;
#[path = "../src/model.rs"] #[allow(dead_code)] mod model;
use serde_json::json;
#[tokio::main(flavor = "current_thread")]
async fn main() {
    let pw = keyring::Entry::new("deco-manager", "admin").unwrap().get_password().unwrap();
    let mut d = deco::Deco::new("192.168.0.1", &pw);
    d.login().await.unwrap();
    let v = d.request("admin/client?form=client_list", json!({"operation":"read","params":{"device_mac":"default"}})).await.unwrap();
    for c in model::clients(&v["client_list"], &Default::default(), &json!({})) {
        println!("{}\t{}\t{}\t{}\t{}\t{}", c.ip, c.mac, c.name, c.kind, if c.online {"on"} else {"off"}, if c.wired {"wired"} else {"wifi"});
    }
}
