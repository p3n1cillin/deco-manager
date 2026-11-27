//! Headless login + read test against your Deco.
//!   DECO_HOST=192.168.68.1 DECO_PASSWORD=... cargo run --example probe
//! Without DECO_PASSWORD it uses the password saved by the app.
#[path = "../src/deco.rs"] mod deco;
#[path = "../src/model.rs"] #[allow(dead_code)] mod model;
use std::time::Instant;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let host = std::env::var("DECO_HOST").unwrap_or_else(|_| "192.168.0.1".into());
    let pw = std::env::var("DECO_PASSWORD").unwrap_or_else(|_| {
        keyring::Entry::new("deco-manager", "admin").and_then(|k| k.get_password())
            .expect("set DECO_PASSWORD or sign in with the app first")
    });
    let mut d = deco::Deco::new(&host, &pw);
    let t = Instant::now();
    d.login().await.expect("login failed");
    println!("logged in to {host} in {:?}", t.elapsed());
    let v = d.read("admin/device?form=device_list").await.expect("device_list failed");
    for n in model::nodes(&v["device_list"]) {
        println!("{:<14} {:<5} {:<15} main={} backhaul={} fw={}", n.name, n.model, n.ip, n.is_main, n.backhaul, n.firmware);
    }
}
