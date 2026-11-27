# Deco Manager

Desktop app (Tauri 2 + Rust) for managing a TP-Link Deco mesh over its **local** API (no TP-Link cloud).

## Run
- Dev: `.\dev.ps1` (or `npx tauri dev` with cargo on PATH)
- Installer: `npx tauri build` → `src-tauri/target/release/bundle/nsis/`

## Layout
- `src-tauri/src/deco.rs`: Deco protocol (RSA login, AES-128-CBC session, signed requests, auto re-login)
- `src-tauri/src/main.rs`: Tauri commands: `connect`, `auto_connect`, `forget`, `dashboard`, `clients`, `set_label`, `set_wifi`, `reboot`, `raw`
- `ui/`: plain HTML/CSS/JS front end (no bundler)
- `src-tauri/examples/probe.rs`: headless login test (`cargo run --example probe`)
- `reference-deco.py`: original Python CLI prototype

## Storage
- Password: Windows Credential Manager (`deco-manager`)
- Host + device labels: `%APPDATA%\com.local.decomanager\`

## Notes
- Deco allows one admin session; opening the web UI/app logs this out, and the client re-logs in automatically.
- `client_list` with `device_mac: "default"` returns all clients; per-node lists (`device_mac: <node mac>`) tell you which Deco each device is on.
- Parental controls, QoS and blocking are cloud/app-only on most firmware; use the API explorer tab to probe for endpoints.
