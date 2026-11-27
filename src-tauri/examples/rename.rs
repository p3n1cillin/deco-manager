//! Rename one client on the Deco itself (the name the Deco phone app shows).
//!   cargo run --example rename -- <MAC> "<new name>"
//! Uses DECO_HOST / DECO_PASSWORD, or the app's detected host and saved login.
#[path = "../src/deco.rs"] mod deco;
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde_json::{json, Value};

fn find(list: &Value, mac: &str) -> Option<Value> {
    list["client_list"].as_array()?.iter().find(|c| c["mac"].as_str().map(|m| m.eq_ignore_ascii_case(mac)).unwrap_or(false)).cloned()
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let mut args = std::env::args().skip(1);
    let (mac, name) = (args.next().expect("MAC"), args.next().expect("name"));
    let host = std::env::var("DECO_HOST").ok().or_else(deco::default_gateway).expect("set DECO_HOST");
    let pw = std::env::var("DECO_PASSWORD").unwrap_or_else(|_| keyring::Entry::new("deco-manager", "admin").unwrap().get_password().unwrap());
    let mut d = deco::Deco::new(&host, &pw);
    d.login().await.expect("login");
    let read = json!({"operation": "read", "params": {"device_mac": "default"}});

    let before = find(&d.request("admin/client?form=client_list", read.clone()).await.unwrap(), &mac).expect("client not found");
    let name_before = String::from_utf8(B64.decode(before["name"].as_str().unwrap_or("")).unwrap_or_default()).unwrap_or_default();
    println!("before: name={name_before:?} type={} raw={before}", before["client_type"]);

    let payload = json!({"operation": "write", "params": {"client_list": [{
        "mac": before["mac"], "name": B64.encode(&name), "user_set_name_type": true, "client_type": before["client_type"],
    }]}});
    println!("write -> {:?}", d.request("admin/client?form=client_list", payload).await);

    let after = find(&d.request("admin/client?form=client_list", read).await.unwrap(), &mac).expect("client gone");
    let name_after = String::from_utf8(B64.decode(after["name"].as_str().unwrap_or("")).unwrap_or_default()).unwrap_or_default();
    println!("after:  name={name_after:?} type={}", after["client_type"]);
}
