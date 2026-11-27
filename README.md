# Deco Manager

A fast Windows desktop app for managing a **TP-Link Deco** mesh network. It talks to your main Deco directly over your home network using the same local API as the Deco web page. There's no TP-Link cloud account and no phone app in the loop.

Built with [Tauri 2](https://tauri.app) (Rust backend, plain HTML/JS front end).

## Features

- **Dashboard**: internet status, public IP, DNS, connection type, router CPU and memory, total live traffic, busiest devices.
- **Wi-Fi control**: turn the main and guest networks on or off per band.
- **Mesh view**: a topology diagram showing which units are wired or wireless (with signal quality), plus per-unit status, model, firmware and device count. You can reboot a single unit or the whole mesh.
- **Devices**: every connected device with IP, MAC, type, band and which Deco it's on. Search, filter (online, unlabelled, wired, Wi-Fi, per Deco) and sort. Give devices your own names; they're stored locally and keyed by MAC address.
- **Activity**: a live log of devices joining and leaving, with a pop-up when an unlabelled device appears.
- **API explorer**: send any request to the Deco's local API, which is handy for discovering more settings.

The app refreshes in the background every 30 seconds, so switching pages is instant.

## Compatibility

Tested on a mixed **Deco S4 (v4, firmware 1.1.2)** + **Deco M4R (v2, firmware 1.8.2)** mesh. Most Deco models use the same local API, so others (X20, X50, X55, M5, XE75…) should work too. If yours doesn't, please open an issue with your model and firmware version.

Parental controls, QoS, HomeShield and device blocking are handled by TP-Link's cloud on most firmware, so they aren't available through the local API.

## Download

Get the latest Windows installer (`DecoManager_x.y.z_x64-setup.exe`) from the [Releases page](https://github.com/p3n1cillin/deco-manager/releases/latest). It installs for your user account only (no admin needed) and adds Deco Manager to the Start menu.

The installer isn't code-signed, so Windows SmartScreen may say "Windows protected your PC". Click **More info → Run anyway**, or build it yourself from source (below).

## Building from source

### Requirements

- Windows 10/11 (WebView2 is built in)
- [Rust](https://rustup.rs) (stable, MSVC toolchain) and the Visual Studio C++ Build Tools
- [Node.js](https://nodejs.org) 18+ (only used for the Tauri CLI)

### Run it

```powershell
git clone https://github.com/p3n1cillin/deco-manager.git
cd deco-manager
npm install
npx tauri dev      # or .\dev.ps1
```

Sign in with your **Deco owner password**, the one you use in the Deco app. The main Deco's address is filled in automatically from your PC's default gateway; change it if your Deco runs in access-point mode behind another router.

### Build an installer

```powershell
npx tauri build
```

The installer is written to `src-tauri/target/release/bundle/nsis/`.

## Privacy and security

- Your password is stored only in **Windows Credential Manager** (entry `deco-manager`) if you tick "Remember". Use *Sign out & forget* to remove it.
- Device names you set are stored in `%APPDATA%\com.local.decomanager\labels.json`.
- Nothing is sent anywhere except your own Deco. Wi-Fi passwords are never passed to the UI.

## Things to know

- A Deco allows **one admin session at a time**. Opening the Deco web page or app logs Deco Manager out, and it logs straight back in on its next refresh.
- Some endpoints are slow on older Decos (the device and client lists can take around 3 seconds each). The background refresh hides this.

## Project layout

| Path | What it is |
|---|---|
| `src-tauri/src/deco.rs` | Deco protocol: RSA login, AES-128-CBC session, signed requests, automatic re-login |
| `src-tauri/src/model.rs` | Turns raw Deco JSON into clean typed data (decodes names, drops secrets) |
| `src-tauri/src/main.rs` | Background poller, change detection and Tauri commands |
| `ui/` | Front end (no bundler) |
| `src-tauri/examples/probe.rs` | Headless login test: `DECO_PASSWORD=… cargo run --example probe` (set `DECO_HOST` to override the detected address) |
| `src-tauri/examples/snapshot.rs` | Dumps the data the UI sees as JSON |
| `reference-deco.py` | Original Python CLI prototype (uses [`tplinkrouterc6u`](https://github.com/AlexandrErohin/TP-Link-Archer-C6U)) |

## Credits

The Deco login and encryption scheme was worked out by the [`tplinkrouterc6u`](https://github.com/AlexandrErohin/TP-Link-Archer-C6U) project. This app reimplements it in Rust.

## Licence

[MIT](LICENSE). Not affiliated with or endorsed by TP-Link. "Deco" is a trademark of TP-Link.
