"""deco - manage a TP-Link Deco mesh from the command line (local API, no cloud)."""
import base64
import json
import time
from json import dumps
from pathlib import Path

import keyring
import typer
from rich.console import Console
from rich.table import Table
from tplinkrouterc6u import TPLinkDecoClient, Connection

app = typer.Typer(help="Manage your TP-Link Deco mesh network.", no_args_is_help=True)
con = Console()
CFG = Path.home() / ".deco"
CFG.mkdir(exist_ok=True)
CONF_FILE, LABELS_FILE = CFG / "config.json", CFG / "labels.json"
SERVICE = "deco-cli"


def _load(p, default):
    return json.loads(p.read_text()) if p.exists() else default


def _save(p, data):
    p.write_text(json.dumps(data, indent=2))


def client() -> TPLinkDecoClient:
    host = _load(CONF_FILE, {}).get("host")
    pw = keyring.get_password(SERVICE, "admin")
    if not host or not pw:
        con.print("[red]Not logged in. Run:[/] deco login")
        raise typer.Exit(1)
    c = TPLinkDecoClient(f"http://{host}", pw)
    c.authorize()
    return c


def _name(raw: str) -> str:
    try:
        return base64.b64decode(raw).decode()
    except Exception:
        return raw or ""


def _random_mac(mac: str) -> bool:
    # Locally-administered bit => phone/PC "private Wi-Fi address"
    return bool(int(mac.replace("-", ":").split(":")[0], 16) & 0x02)


def _raw_clients(c) -> list[dict]:
    return c.request("admin/client?form=client_list",
                     dumps({"operation": "read", "params": {"device_mac": "default"}})).get("client_list", [])


@app.command()
def login(host: str = typer.Option("192.168.0.1", help="Main Deco IP")):
    """Save Deco host + owner password (password goes to Windows Credential Manager)."""
    pw = typer.prompt("Deco owner password", hide_input=True)
    c = TPLinkDecoClient(f"http://{host}", pw)
    c.authorize()
    c.logout()
    keyring.set_password(SERVICE, "admin", pw)
    _save(CONF_FILE, {"host": host})
    con.print(f"[green]Logged in to {host}.[/]")


@app.command()
def status():
    """Internet / WAN / Wi-Fi / client summary."""
    c = client()
    s, fw = c.get_status(), c.get_firmware()
    t = Table(show_header=False)
    for k, v in [("Model", f"{fw.model} (hw {fw.hardware_version})"), ("Firmware", fw.firmware_version),
                 ("WAN IP", s.wan_ipv4_addr), ("Gateway", s.wan_ipv4_gateway), ("Connection", s.conn_type),
                 ("LAN IP", s.lan_ipv4_addr), ("CPU / Mem", f"{s.cpu_usage} / {s.mem_usage}"),
                 ("Wi-Fi 2.4/5/6", f"{s.wifi_2g_enable} / {s.wifi_5g_enable} / {s.wifi_6g_enable}"),
                 ("Guest 2.4/5", f"{s.guest_2g_enable} / {s.guest_5g_enable}"),
                 ("Clients", f"{s.clients_total} (wired {s.wired_total}, wifi {s.wifi_clients_total}, "
                             f"guest {s.guest_clients_total})")]:
        t.add_row(k, str(v))
    con.print(t)
    c.logout()


@app.command()
def nodes():
    """List every Deco unit in the mesh."""
    c = client()
    t = Table("Name", "Role", "Model", "IP", "MAC", "Status", "Internet", "Sig 2.4/5", "Firmware")
    for n in c.get_mesh_nodes():
        t.add_row(_name(n.name), n.role, n.model, str(n.ipaddr or ""), str(n.macaddr), str(n.status),
                  str(n.internet_status), f"{n.signal_2g}/{n.signal_5g}", n.firmware_version or "")
    con.print(t)
    c.logout()


@app.command()
def clients(all: bool = typer.Option(False, "--all", help="Include offline devices"),
            json_out: bool = typer.Option(False, "--json")):
    """List devices on the network, with your own labels."""
    c = client()
    labels = _load(LABELS_FILE, {})
    nodes_by_mac = {str(n.macaddr): _name(n.name) for n in c.get_mesh_nodes()}
    rows = [r for r in _raw_clients(c) if all or r.get("online")]
    c.logout()
    if json_out:
        print(json.dumps(rows, indent=2)); return
    t = Table("Label / Name", "IP", "MAC", "Type", "Conn", "On node", "Down/Up KB/s", "Notes")
    for r in sorted(rows, key=lambda r: tuple(int(x) for x in r.get("ip", "0.0.0.0").split(".") if x.isdigit())):
        mac = r.get("mac", "").upper().replace(":", "-")
        notes = []
        if _random_mac(mac): notes.append("private MAC")
        if not r.get("online"): notes.append("offline")
        if mac not in labels: notes.append("[yellow]unlabelled[/]")
        conn = r.get("wire_type", "") + (f" {r.get('connection_type', '')}" if r.get("wire_type") == "wireless" else "")
        t.add_row(labels.get(mac) or _name(r.get("name", "")), r.get("ip", ""), mac, r.get("client_type", ""),
                  conn, nodes_by_mac.get(r.get("access_host", "").upper().replace(":", "-"), r.get("access_host", "")),
                  f"{r.get('down_speed', 0)}/{r.get('up_speed', 0)}", ", ".join(notes))
    con.print(t)
    con.print(f"{len(rows)} device(s). Tag one with: deco label <MAC> \"Living room TV\"")


@app.command()
def label(mac: str, name: str = typer.Argument(None, help="Omit to remove the label")):
    """Give a device a friendly name (stored locally in ~/.deco/labels.json)."""
    labels = _load(LABELS_FILE, {})
    mac = mac.upper().replace(":", "-")
    if name: labels[mac] = name
    else: labels.pop(mac, None)
    _save(LABELS_FILE, labels)
    con.print(f"[green]{mac} -> {name or '(removed)'}[/]")


@app.command()
def watch(interval: int = 15):
    """Live-watch for devices joining or leaving the network."""
    labels, seen = _load(LABELS_FILE, {}), None
    con.print(f"Watching every {interval}s, Ctrl+C to stop.")
    while True:
        c = client()
        now = {r["mac"].upper().replace(":", "-"): r for r in _raw_clients(c) if r.get("online")}
        c.logout()
        if seen is not None:
            for m in now.keys() - seen.keys():
                r = now[m]; tag = labels.get(m) or f"[yellow]UNKNOWN[/] {_name(r.get('name', ''))}"
                con.print(f"[green]+ {time.strftime('%H:%M:%S')} joined[/] {tag} {r.get('ip')} {m}")
            for m in seen.keys() - now.keys():
                con.print(f"[red]- {time.strftime('%H:%M:%S')} left[/] {labels.get(m) or _name(seen[m].get('name', ''))} {m}")
        seen = now
        time.sleep(interval)


@app.command()
def wifi(band: str = typer.Argument(..., help="guest-2g, guest-5g, host-2g, host-5g, host-6g"),
         state: str = typer.Argument(..., help="on / off")):
    """Turn a Wi-Fi band (or guest network) on or off."""
    m = {"host-2g": Connection.HOST_2G, "host-5g": Connection.HOST_5G, "host-6g": Connection.HOST_6G,
         "guest-2g": Connection.GUEST_2G, "guest-5g": Connection.GUEST_5G}
    if band not in m or state not in ("on", "off"):
        con.print("[red]Bad band or state.[/]"); raise typer.Exit(1)
    c = client(); c.set_wifi(m[band], state == "on"); c.logout()
    con.print(f"[green]{band} {state}[/]")


@app.command()
def reboot(yes: bool = typer.Option(False, "--yes", help="Skip confirmation")):
    """Reboot the whole mesh."""
    if yes or typer.confirm("Reboot all Deco units? Internet drops for ~2 min"):
        c = client(); c.reboot()
        con.print("[green]Reboot sent.[/]")


@app.command()
def raw(path: str = typer.Argument(..., help="e.g. 'admin/network?form=wan_ipv4'"),
        operation: str = "read"):
    """Explore: call any local API endpoint and print the JSON."""
    c = client()
    print(json.dumps(c.request(path, dumps({"operation": operation})), indent=2))
    c.logout()


if __name__ == "__main__":
    app()
