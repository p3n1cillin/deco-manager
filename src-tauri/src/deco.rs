//! Local API client for TP-Link Deco (same protocol as the Deco web UI).
//! Login: RSA(PKCS#1 v1.5) password + AES-128-CBC session, requests signed with RSA.
use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use md5::{Digest, Md5};
use rand::RngCore;
use rsa::{BigUint, Pkcs1v15Encrypt, RsaPublicKey};
use serde_json::{json, Value};

type Enc = cbc::Encryptor<aes::Aes128>;
type Dec = cbc::Decryptor<aes::Aes128>;

pub struct Deco {
    host: String,
    password: String,
    http: reqwest::Client,
    key: String,
    iv: String,
    pwd_nn: String,
    pwd_ee: String,
    nn: String,
    ee: String,
    seq: u64,
    stok: String,
    sysauth: String,
    logged: bool,
}

fn rand_hex() -> String {
    let mut b = [0u8; 8];
    rand::thread_rng().fill_bytes(&mut b);
    hex::encode(b)
}

fn rsa_encrypt(data: &[u8], nn: &str, ee: &str) -> Result<String> {
    let n = BigUint::parse_bytes(nn.as_bytes(), 16).ok_or(anyhow!("bad RSA n"))?;
    let e = BigUint::parse_bytes(ee.as_bytes(), 16).ok_or(anyhow!("bad RSA e"))?;
    let key = RsaPublicKey::new(n, e)?;
    Ok(hex::encode(key.encrypt(&mut rand::thread_rng(), Pkcs1v15Encrypt, data)?))
}

impl Deco {
    pub fn new(host: &str, password: &str) -> Self {
        let host = if host.starts_with("http") { host.to_string() } else { format!("http://{host}") };
        Deco {
            host: host.trim_end_matches('/').to_string(),
            password: password.to_string(),
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap(),
            key: rand_hex(),
            iv: rand_hex(),
            pwd_nn: String::new(),
            pwd_ee: String::new(),
            nn: String::new(),
            ee: String::new(),
            seq: 0,
            stok: String::new(),
            sysauth: String::new(),
            logged: false,
        }
    }

    fn aes_encrypt(&self, s: &str) -> String {
        let c = Enc::new(self.key.as_bytes().into(), self.iv.as_bytes().into());
        B64.encode(c.encrypt_padded_vec_mut::<Pkcs7>(s.as_bytes()))
    }

    fn aes_decrypt(&self, s: &str) -> Result<Value> {
        let raw = B64.decode(s)?;
        let c = Dec::new(self.key.as_bytes().into(), self.iv.as_bytes().into());
        // Firmware padding isn't always strict PKCS7; strip leniently like the Deco web UI does.
        let mut plain = c
            .decrypt_padded_vec_mut::<aes::cipher::block_padding::NoPadding>(&raw)
            .map_err(|_| anyhow!("session expired"))?;
        if let Some(&p) = plain.last() {
            if (1..=16).contains(&p) && plain.len() >= p as usize {
                plain.truncate(plain.len() - p as usize);
            }
        }
        serde_json::from_str(&String::from_utf8_lossy(&plain)).map_err(|_| anyhow!("session expired"))
    }

    fn prepare(&self, data: &str) -> Result<String> {
        let enc = self.aes_encrypt(data);
        let hash = hex::encode(Md5::digest(format!("admin{}", self.password)));
        let seq = self.seq + enc.len() as u64;
        let s = if self.logged {
            format!("h={hash}&s={seq}")
        } else {
            format!("k={}&i={}&h={hash}&s={seq}", self.key, self.iv)
        };
        let mut sign = String::new();
        for chunk in s.as_bytes().chunks(53) {
            sign += &rsa_encrypt(chunk, &self.nn, &self.ee)?;
        }
        Ok(serde_urlencoded::to_string([("sign", sign), ("data", enc)])?)
    }

    async fn plain_post(&self, path: &str) -> Result<Value> {
        let url = format!("{}/cgi-bin/luci/;stok=/login?form={path}&operation=read", self.host);
        let v: Value = self.http.post(url).send().await?.json().await?;
        Ok(v["result"].clone())
    }

    pub async fn login(&mut self) -> Result<()> {
        self.logged = false;
        let keys = self.plain_post("keys").await.context("cannot reach Deco")?;
        self.pwd_nn = keys["password"][0].as_str().unwrap_or_default().into();
        self.pwd_ee = keys["password"][1].as_str().unwrap_or_default().into();
        let auth = self.plain_post("auth").await?;
        self.seq = auth["seq"].as_u64().or_else(|| auth["seq"].as_str()?.parse().ok()).unwrap_or(0);
        self.nn = auth["key"][0].as_str().unwrap_or_default().into();
        self.ee = auth["key"][1].as_str().unwrap_or_default().into();

        let pwd = rsa_encrypt(self.password.as_bytes(), &self.pwd_nn, &self.pwd_ee)?;
        let body = self.prepare(&json!({"params": {"password": pwd}, "operation": "login"}).to_string())?;
        let resp = self
            .http
            .post(format!("{}/cgi-bin/luci/;stok=/login?form=login", self.host))
            .header("Content-Type", "application/json")
            .body(body)
            .send()
            .await?;
        let cookie = resp
            .headers()
            .get_all("set-cookie")
            .iter()
            .filter_map(|h| h.to_str().ok())
            .find_map(|c| c.split(';').find_map(|p| p.trim().strip_prefix("sysauth=").map(String::from)));
        let v: Value = resp.json().await?;
        let d = self.aes_decrypt(v["data"].as_str().ok_or(anyhow!("login rejected"))?)?;
        if d["error_code"].as_i64() != Some(0) {
            bail!("Login failed (wrong password?) - error {}", d["error_code"]);
        }
        self.stok = d["result"]["stok"].as_str().unwrap_or_default().into();
        self.sysauth = cookie.ok_or(anyhow!("no session cookie"))?;
        self.logged = true;
        Ok(())
    }

    pub async fn request_once(&self, path: &str, payload: &Value) -> Result<Value> {
        let body = self.prepare(&payload.to_string())?;
        let v: Value = self
            .http
            .post(format!("{}/cgi-bin/luci/;stok={}/{path}", self.host, self.stok))
            .header("Content-Type", "application/json")
            .header("Cookie", format!("sysauth={}", self.sysauth))
            .body(body)
            .send()
            .await?
            .json()
            .await?;
        let d = self.aes_decrypt(v["data"].as_str().ok_or(anyhow!("session expired"))?)?;
        if d["error_code"].as_i64() != Some(0) {
            bail!("Deco error {} on {path}", d["error_code"]);
        }
        Ok(d["result"].clone())
    }

    /// Signed request; re-logs in once if the session was evicted (Deco allows one admin session).
    pub async fn request(&mut self, path: &str, payload: Value) -> Result<Value> {
        if !self.logged {
            self.login().await?;
        }
        match self.request_once(path, &payload).await {
            Ok(v) => Ok(v),
            Err(_) => {
                self.login().await?;
                self.request_once(path, &payload).await
            }
        }
    }

    #[allow(dead_code)]
    pub async fn read(&mut self, path: &str) -> Result<Value> {
        self.request(path, json!({"operation": "read"})).await
    }
}

/// The PC's default gateway, which on a Deco network is the main Deco.
pub fn default_gateway() -> Option<String> {
    netdev::get_default_gateway().ok()?.ipv4.first().map(|ip| ip.to_string())
}
