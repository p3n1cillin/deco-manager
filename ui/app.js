const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const $ = (s, el = document) => el.querySelector(s);
const $$ = (s, el = document) => [...el.querySelectorAll(s)];
const esc = (s) => String(s ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
const speed = (kb) => (kb >= 1024 ? (kb / 1024).toFixed(1) + " MB/s" : kb + " KB/s");
const ipNum = (ip) => (ip || "0.0.0.0").split(".").reduce((a, b) => a * 256 + (+b || 0), 0);
const KIND = { phone: "Phone", pc: "Computer", laptop: "Laptop", tablet: "Tablet", iot_device: "Smart device", entertainment: "TV / media", camera: "Camera", printer: "Printer", other: "Other", game_console: "Console" };
const kindRaw = (k) => KIND[k] || (k ? k[0].toUpperCase() + k.slice(1).replaceAll("_", " ") : "–");
// The Deco's own guess is often wrong (Fire TV = "phone"), so refine it from names and MAC vendor.
function category(c) {
  const n = `${c.label || ""} ${c.name || ""}`.toLowerCase();
  if (/\b(tv|fire ?tv|fire ?stick|firestick|roku|chromecast|apple tv|shield)\b/.test(n) || c.kind === "entertainment") return ["tv", "TV / media"];
  if (/proxmox|\bpve\d*\b/.test(n)) return ["server", "Server"];
  if (c.mac.startsWith("BC-24-11")) return ["server", "Virtual machine"];
  if (/\b(pc|desktop|laptop|macbook|server)\b/.test(n) || ["pc", "laptop"].includes(c.kind)) return ["pc", kindRaw(c.kind === "laptop" ? "laptop" : "pc")];
  if (/phone|galaxy|pixel|android|\bs\d{2}\b/.test(n) || ["phone", "tablet"].includes(c.kind)) return ["phone", kindRaw(c.kind === "tablet" ? "tablet" : "phone")];
  if (/alexa|echo|light|bulb|plug|camera|sensor|hub/.test(n) || ["iot_device", "camera"].includes(c.kind)) return ["iot", "Smart device"];
  return ["other", kindRaw(c.kind)];
}
const kind = (c) => category(c)[1];
const ICONS = {
  phone: '<rect x="7" y="2.5" width="10" height="19" rx="2.5"/><path d="M11 18.5h2"/>',
  pc: '<rect x="2.5" y="4" width="19" height="12" rx="2"/><path d="M8 20h8M12 16v4"/>',
  tv: '<rect x="2.5" y="5" width="19" height="12.5" rx="2"/><path d="M8 21h8M9 2.5l3 2.5 3-2.5"/>',
  iot: '<path d="M9 18h6M10 21h4M12 3a6 6 0 0 0-3.5 10.9c.6.5 1 1.2 1 2.1h5c0-.9.4-1.6 1-2.1A6 6 0 0 0 12 3z"/>',
  server: '<rect x="3" y="3.5" width="18" height="7" rx="1.5"/><rect x="3" y="13.5" width="18" height="7" rx="1.5"/><path d="M7 7h.01M7 17h.01"/>',
  other: '<circle cx="12" cy="12" r="8.5"/><path d="M12 8v4.5M12 16h.01"/>',
  edit: '<path d="M4 20h4L19 9l-4-4L4 16v4zM13.5 6.5l4 4"/>',
};
const svg = (k) => `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">${ICONS[k]}</svg>`;
const displayName = (c) => c.label || c.name || "Unknown device";

const ui = { snap: null, view: "dashboard", filter: "online", node: "", sort: { key: "ip", dir: 1 }, unread: 0, editing: false };

function toast(msg, bad = false) {
  const t = $("#toast");
  t.textContent = msg; t.className = "toast" + (bad ? " bad" : "");
  clearTimeout(toast.h); toast.h = setTimeout(() => t.classList.add("hidden"), 4000);
}
async function call(cmd, args) {
  try { return await invoke(cmd, args); } catch (e) { toast(String(e), true); throw e; }
}

/* ---------- start / login ---------- */
async function start() {
  listen("snapshot", (e) => { ui.snap = e.payload; render(); });
  listen("activity", (e) => e.payload.forEach(addActivity));
  try {
    const host = await invoke("auto_connect");
    if (host) return enterApp();
  } catch (e) { $("#login-error").textContent = "Saved login failed: " + e; }
  $("#login").classList.remove("hidden");
  const gw = await invoke("detect_host").catch(() => null);
  if (gw && !$("#host").value) { $("#host").value = gw; $("#host-hint").textContent = "Detected from your network (your router's address)."; }
  $("#password").focus();
}
$("#login-form").addEventListener("submit", async (ev) => {
  ev.preventDefault();
  const btn = $("#login-form button"); btn.disabled = true; btn.textContent = "Connecting…"; $("#login-error").textContent = "";
  try {
    await invoke("connect", { host: $("#host").value.trim(), password: $("#password").value, remember: $("#remember").checked });
    $("#password").value = "";
    enterApp();
  } catch (e) { $("#login-error").textContent = String(e); }
  btn.disabled = false; btn.textContent = "Connect";
});
$("#logout").addEventListener("click", async () => {
  if (!confirm("Sign out and remove the saved password from this PC?")) return;
  await call("forget"); location.reload();
});
async function enterApp() {
  $("#login").classList.add("hidden");
  $("#app").classList.remove("hidden");
  ui.snap = await invoke("snapshot");
  (await invoke("activity")).reverse().forEach((a) => addActivity(a, true));
  show("dashboard");
  setInterval(renderStatus, 1000);
}
$("#refresh").addEventListener("click", () => invoke("refresh_now"));

/* ---------- navigation ---------- */
$$("nav a[data-view]").forEach((a) => a.addEventListener("click", () => show(a.dataset.view)));
function show(view) {
  ui.view = view;
  $$("nav a[data-view]").forEach((a) => a.classList.toggle("active", a.dataset.view === view));
  $$(".view").forEach((v) => v.classList.toggle("hidden", v.id !== "view-" + view));
  if (view === "activity") { ui.unread = 0; renderBadge(); }
  render();
}

function render() {
  if (!ui.snap) return;
  renderStatus();
  const s = ui.snap;
  $("#nav-mesh").textContent = s.nodes.length || "";
  $("#nav-devices").textContent = s.clients.filter((c) => c.online).length || "";
  $("#error-bar").classList.toggle("hidden", !s.error);
  $("#error-bar").textContent = s.error || "";
  if (ui.view === "dashboard") renderDashboard();
  if (ui.view === "mesh") renderMesh();
  if (ui.view === "devices" && !ui.editing) renderDevices();
}
function renderStatus() {
  const s = ui.snap; if (!s) return;
  $("#conn").innerHTML = `<span class="dot ${s.internet.online ? "ok" : s.updated ? "bad" : ""}"></span> ${esc(s.host)}`;
  const ago = s.updated ? Math.round((Date.now() - s.updated) / 1000) : null;
  $("#updated").innerHTML = s.refreshing ? `<span class="spin"></span>Refreshing…` : ago == null ? "Waiting for data…" : `Updated ${ago < 5 ? "just now" : ago + "s ago"}`;
  $("#tb-dot").className = "dot " + (s.internet.online ? "ok" : s.updated ? "bad" : "");
  $("#tb-sub").textContent = s.host ? `· ${s.host}${s.nodes.length ? ` · ${s.nodes.length} units · ${s.clients.filter((c) => c.online).length} devices online` : ""}` : "";
}
const loading = () => `<div class="empty"><span class="spin"></span>Loading from your Deco…</div>`;

/* ---------- dashboard ---------- */
function renderDashboard() {
  const s = ui.snap, i = s.internet;
  if (!s.updated) { $("#stats").innerHTML = loading(); $("#wifi").innerHTML = ""; $("#top").innerHTML = ""; return; }
  const online = s.clients.filter((c) => c.online);
  const wired = online.filter((c) => c.wired).length;
  const up = s.nodes.filter((n) => n.connected).length;
  const down = online.reduce((a, c) => a + c.down_kbs, 0), upl = online.reduce((a, c) => a + c.up_kbs, 0);
  const meter = (v) => (v == null ? "" : `<div class="meter"><i style="width:${Math.min(100, v)}%"></i></div>`);
  const stat = (k, v, sub = "", extra = "") => `<div class="card stat"><div class="k">${k}</div><div class="v">${v}</div><div class="s">${sub}</div>${extra}</div>`;
  $("#stats").innerHTML =
    stat("Internet", `<span class="dot ${i.online ? "ok" : "bad"}"></span>${i.online ? "Online" : "Offline"}`, esc(i.conn_type)) +
    stat("Public IP", esc(i.wan_ip || "–"), "DNS " + esc(i.dns.join(", ") || "–")) +
    stat("Mesh", `${up} / ${s.nodes.length}`, esc(modelSummary(s.nodes)) || "units online") +
    stat("Devices online", online.length, `${wired} wired · ${online.length - wired} Wi-Fi · ${s.clients.filter((c) => !c.label).length} unlabelled`) +
    stat("Live traffic", `↓ ${speed(down)}`, `↑ ${speed(upl)}`) +
    stat("Router CPU", s.cpu == null ? "–" : Math.round(s.cpu) + "%", "Main Deco", meter(s.cpu)) +
    stat("Router memory", s.mem == null ? "–" : Math.round(s.mem) + "%", "Main Deco", meter(s.mem)) +
    stat("LAN", esc(i.lan_ip || "–"), esc(i.lan_mask));

  $("#wifi").innerHTML = s.wifi.map((w) => `
    <div class="row"><div><div class="name">${esc(w.ssid || "(hidden)")} <span class="pill ${w.network === "guest" ? "warn" : ""}">${w.network === "guest" ? "Guest" : "Main"}</span></div>
      <div class="sub">${esc(w.band_label)}${w.channel ? " · channel " + esc(w.channel) : ""}${w.width ? " · " + esc(w.width) : ""}</div></div>
      <label class="switch" title="${w.enabled ? "On" : "Off"}"><input type="checkbox" data-band="${w.band}" data-net="${w.network}" ${w.enabled ? "checked" : ""}><span></span></label></div>`).join("") || `<div class="empty">Wi-Fi settings not available.</div>`;
  $$("#wifi input").forEach((el) => el.addEventListener("change", async () => {
    const { band, net } = el.dataset;
    if (net === "host" && !el.checked && !confirm("Turn off your main Wi-Fi on this band? Devices on it will disconnect, possibly including this PC.")) { el.checked = true; return; }
    el.disabled = true;
    try { await call("set_wifi", { band, network: net, enable: el.checked }); toast(`Wi-Fi turned ${el.checked ? "on" : "off"}`); }
    catch { el.checked = !el.checked; }
    el.disabled = false;
  }));

  const top = [...online].sort((a, b) => b.down_kbs + b.up_kbs - (a.down_kbs + a.up_kbs)).slice(0, 6).filter((c) => c.down_kbs + c.up_kbs > 0);
  $("#top").innerHTML = top.map((c) => `
    <div class="row"><div><div class="name">${esc(displayName(c))}</div><div class="sub">${esc(c.ip)} · ${esc(c.node || "")}</div></div>
      <div class="small">↓ ${speed(c.down_kbs)}<br>↑ ${speed(c.up_kbs)}</div></div>`).join("") || `<div class="empty">Nothing using much bandwidth right now.</div>`;
}
function modelSummary(nodes) {
  const m = {};
  nodes.forEach((n) => (m[n.model] = (m[n.model] || 0) + 1));
  return Object.entries(m).map(([k, v]) => `${v}× ${k}`).join(" · ");
}

/* ---------- mesh ---------- */
function bars(n) {
  if (n == null) return "";
  return `<span class="bars">${[1, 2, 3].map((i) => `<i class="${n >= i ? "on" : ""}"></i>`).join("")}</span>`;
}
function renderMesh() {
  const s = ui.snap;
  const byModel = {};
  s.nodes.forEach((n) => (byModel[n.model] ??= []).push(n));
  $("#models").innerHTML = Object.entries(byModel).map(([m, ns]) =>
    `<div class="chip"><b>${ns.length}×</b> ${esc(m)} <span class="muted">hw ${esc([...new Set(ns.map((n) => n.hardware))].join("/"))} · fw ${esc([...new Set(ns.map((n) => n.firmware.split(" ")[0]))].join("/"))}</span></div>`).join("");
  if (!s.nodes.length) { $("#nodes").innerHTML = loading(); $("#topo").classList.add("hidden"); return; }
  renderTopology(s);
  $("#nodes").innerHTML = s.nodes.map((n) => `
    <div class="card node ${n.connected ? "" : "down"}">
      <div class="head"><h4>${esc(n.name)}</h4>
        <span class="pill ${n.is_main ? "main" : ""}">${n.is_main ? "Main" : "Satellite"}</span></div>
      <div class="muted small" title="MAC ${esc(n.mac)}">${esc(n.model)} v${esc(n.hardware)} · ${esc(n.ip)}</div>
      <div class="pills">
        <span class="pill ${n.connected ? "ok" : "bad"}">${n.connected ? "Online" : "Offline"}</span>
        <span class="pill ${n.internet ? "ok" : "warn"}">${n.internet ? "Internet" : "No internet"}</span>
      </div>
      <dl>
        <dt>Devices</dt><dd>${n.clients}</dd>
        <dt>Link</dt><dd>${n.is_main ? "Internet" + (s.internet.conn_type ? " (" + esc(s.internet.conn_type) + ")" : "") : n.backhaul === "Wireless" && n.signal != null ? `${bars(n.signal)}Wi-Fi, ${["none", "weak", "fair", "good"][n.signal]}` : esc(n.backhaul)}</dd>
        <dt>Firmware</dt><dd title="${esc(n.firmware)}">${esc(n.firmware.split(" ")[0])} <span class="muted">(${esc((n.firmware.match(/Build (\d{4})(\d{2})/) || []).slice(1).reverse().join("/") || "?")})</span></dd>
      </dl>
      <button class="ghost sm" data-reboot="${esc(n.mac)}" data-name="${esc(n.name)}">Reboot</button>
    </div>`).join("");
  $$("[data-reboot]").forEach((b) => b.addEventListener("click", async () => {
    if (!confirm(`Reboot ${b.dataset.name}? Devices connected through it drop for about 2 minutes.`)) return;
    await call("reboot", { mac: b.dataset.reboot }); toast(`Reboot sent to ${b.dataset.name}`);
  }));
}
function renderTopology(s) {
  const main = s.nodes.find((n) => n.is_main) || s.nodes[0];
  const kids = s.nodes.filter((n) => n !== main);
  const box = (n, cls = "") => `<div class="tnode ${cls} ${n.connected ? "" : "down"}" title="${esc(n.model)} · ${esc(n.ip)}">
      ${cls.includes("main") ? "" : `<em>${n.backhaul === "Wired" ? "wired" : n.signal != null ? ["", "weak", "fair", "good"][n.signal] || "" : ""}</em>`}
      <b>${esc(n.name)}</b><span>${esc(n.model)} · ${n.clients} device${n.clients === 1 ? "" : "s"}</span></div>`;
  $("#topo").classList.remove("hidden");
  $("#topo").innerHTML = `
    <div class="tnode inet">${s.internet.online ? "Internet" : "Internet (offline)"}</div><div class="tline"></div>
    ${box(main, "main")}
    ${kids.length ? `<div class="tline"></div><div class="tkids">${kids.map((n) =>
      box(n, n.backhaul === "Wired" ? "" : "wifi" + (n.signal != null && n.signal < 2 ? " weak" : ""))).join("")}</div>` : ""}
    <div class="topo-legend"><span><i></i>Wired</span><span><i class="d"></i>Wireless</span><span><i class="w"></i>Weak wireless</span></div>`;
}
$("#reboot-all").addEventListener("click", async () => {
  if (!confirm("Reboot ALL Deco units? Your whole network and internet drop for about 2 minutes.")) return;
  await call("reboot", { mac: null }); toast("Reboot sent to every unit");
});

/* ---------- devices ---------- */
function sortVal(c, k) {
  switch (k) {
    case "ip": return ipNum(c.ip);
    case "name": return displayName(c).toLowerCase();
    case "conn": return (c.wired ? "0" : "1") + c.band;
    case "node": return (c.node || "~") + (c.wired ? "0" : "1");
    case "kind": return kind(c);
    case "speed": return -(c.down_kbs + c.up_kbs);
    default: return String(c[k] ?? "").toLowerCase();
  }
}
function renderDevices() {
  const s = ui.snap, q = $("#search").value.toLowerCase().trim();
  const sel = $("#node-filter"), cur = sel.value;
  sel.innerHTML = `<option value="">All Decos</option>` + s.nodes.map((n) => `<option ${n.name === cur ? "selected" : ""}>${esc(n.name)}</option>`).join("");
  if (!s.updated) { $("#devices tbody").innerHTML = `<tr><td colspan="6">${loading()}</td></tr>`; return; }
  const rows = s.clients.filter((c) => {
    if (ui.filter === "online" && !c.online) return false;
    if (ui.filter === "unlabelled" && c.label) return false;
    if (ui.filter === "wired" && !c.wired) return false;
    if (ui.filter === "wireless" && c.wired) return false;
    if (ui.node && c.node !== ui.node) return false;
    return !q || [c.label, c.name, c.ip, c.mac, kind(c), c.node].join(" ").toLowerCase().includes(q);
  }).sort((a, b) => { const x = sortVal(a, ui.sort.key), y = sortVal(b, ui.sort.key); return (x > y ? 1 : x < y ? -1 : 0) * ui.sort.dir; });
  $$("#devices th").forEach((th) => { th.classList.toggle("sorted", th.dataset.sort === ui.sort.key); th.classList.toggle("asc", ui.sort.dir < 0); });
  const on = s.clients.filter((c) => c.online).length;
  $("#device-count").textContent = `Showing ${rows.length} · ${on} online · ${s.clients.length - on} offline · ${s.clients.filter((c) => !c.label).length} unlabelled`;
  $("#devices tbody").innerHTML = rows.map((c) => {
    const [cat, type] = category(c), title = displayName(c);
    const sub = c.label && c.name && c.label !== c.name ? esc(c.name) : !c.label ? '<span class="pill warn">unlabelled</span>' : "";
    return `
    <tr class="${c.online ? "" : "offline"}">
      <td><div class="dev"><div class="ico ${cat}">${svg(cat)}</div>
        <div><div class="name" title="${esc(title)}">${esc(title)}</div><div class="sub">${sub}</div></div></div></td>
      <td class="addr"><span class="mono">${esc(c.ip)}</span><span class="mono" title="${c.private_mac ? "Private (randomised) Wi-Fi address" : ""}">${esc(c.mac)}${c.private_mac ? " · private" : ""}</span></td>
      <td title="${esc(type)}">${esc(type)}</td>
      <td class="conn"><div>${c.wired ? "Wired" : esc(c.band)}${c.network && c.network !== "main" ? ` <span class="pill warn">${esc(c.network)}</span>` : ""}</div>
        <div class="sub">${esc(c.node || (s.refreshing ? "…" : "–"))}</div></td>
      <td><div class="traffic">${!c.online ? '<span class="muted">offline</span>' : c.down_kbs + c.up_kbs ? `↓ ${speed(c.down_kbs)}<br><span class="muted">↑ ${speed(c.up_kbs)}</span>` : '<span class="muted">idle</span>'}</div></td>
      <td><button class="iconbtn" data-label="${esc(c.mac)}" title="${c.label ? "Rename" : "Add a name"}">${svg("edit")}</button></td>
    </tr>`;
  }).join("") || `<tr><td colspan="6" class="muted">No devices match.</td></tr>`;
  $$("#devices td .name").forEach((n) => n.addEventListener("dblclick", () => editLabel($("[data-label]", n.closest("tr")))));
  $$("[data-label]").forEach((b) => b.addEventListener("click", () => editLabel(b)));
}
function editLabel(btn) {
  const mac = btn.dataset.label, c = ui.snap.clients.find((x) => x.mac === mac);
  const cell = $(".dev > div:last-child", btn.closest("tr"));
  ui.editing = true;
  cell.innerHTML = `<input class="label-edit" placeholder="e.g. Living room TV (blank clears)" value="${esc(c.label || "")}">`;
  const input = $("input", cell); input.focus(); input.select();
  const done = async (save) => {
    input.onblur = null; ui.editing = false;
    if (save) await call("set_label", { mac, label: input.value }).catch(() => {});
    renderDevices();
  };
  input.onblur = () => done(true);
  input.onkeydown = (e) => { if (e.key === "Enter") done(true); if (e.key === "Escape") done(false); };
}
$("#search").addEventListener("input", renderDevices);
$("#node-filter").addEventListener("change", (e) => { ui.node = e.target.value; renderDevices(); });
$$("#filters button").forEach((b) => b.addEventListener("click", () => {
  ui.filter = b.dataset.f; $$("#filters button").forEach((x) => x.classList.toggle("on", x === b)); renderDevices();
}));
$$("#devices th[data-sort]").forEach((th) => th.addEventListener("click", () => {
  const k = th.dataset.sort;
  ui.sort = { key: k, dir: ui.sort.key === k ? -ui.sort.dir : 1 };
  renderDevices();
}));

/* ---------- activity ---------- */
function addActivity(a, initial = false) {
  const log = $("#activity-log");
  if (log.firstElementChild?.classList.contains("empty")) log.innerHTML = "";
  const li = document.createElement("li");
  li.innerHTML = `<span class="t small">${new Date(a.time).toLocaleString([], { weekday: "short", hour: "2-digit", minute: "2-digit", second: "2-digit" })}</span>
    <span class="pill ${a.joined ? "ok" : "bad"}">${a.joined ? "joined" : "left"}</span>
    <span class="name">${esc(a.name || "Unknown")}</span>
    ${!a.known && a.joined ? '<span class="pill warn">unlabelled</span>' : ""}
    <span class="muted mono">${esc(a.ip)} · ${esc(a.mac)}</span>`;
  log.prepend(li);
  if (initial) return;
  if (ui.view !== "activity") { ui.unread++; renderBadge(); }
  if (!a.known && a.joined) toast(`Unlabelled device joined: ${a.name || a.mac} (${a.ip})`);
}
function renderBadge() {
  const b = $("#activity-badge"); b.textContent = ui.unread; b.classList.toggle("hidden", !ui.unread);
}
$("#activity-log").innerHTML = `<li class="empty">No changes yet. Joins and leaves appear here.</li>`;

/* ---------- explorer ---------- */
$("#raw-send").addEventListener("click", async () => {
  let payload;
  try { payload = JSON.parse($("#raw-payload").value); } catch { return toast("Payload is not valid JSON", true); }
  if (payload.operation && payload.operation !== "read" && !confirm(`Send a "${payload.operation}" request? This may change router settings.`)) return;
  $("#raw-out").textContent = "…";
  try { $("#raw-out").textContent = JSON.stringify(await invoke("raw", { path: $("#raw-path").value.trim(), payload }), null, 2); }
  catch (e) { $("#raw-out").textContent = String(e); }
});

/* ---------- desktop window behaviour ---------- */
const win = window.__TAURI__.window?.getCurrentWindow?.();
if (win) {
  $("#tb-min").addEventListener("click", () => win.minimize());
  $("#tb-max").addEventListener("click", () => win.toggleMaximize());
  $("#tb-close").addEventListener("click", () => win.close());
  const maxIcon = async () => {
    const m = await win.isMaximized();
    $("#tb-max").title = m ? "Restore" : "Maximise";
    $("#tb-max svg").innerHTML = m
      ? '<rect x="0.5" y="2.5" width="7" height="7"/><path d="M2.5 2.5V0.5h7v7h-2"/>'
      : '<rect x="0.5" y="0.5" width="9" height="9"/>';
  };
  win.onResized?.(maxIcon); maxIcon();
}
// No browser-style context menu, reload, print, find or zoom.
document.addEventListener("contextmenu", (e) => { if (!e.target.closest("input, textarea, pre")) e.preventDefault(); });
document.addEventListener("keydown", (e) => {
  const k = e.key.toLowerCase();
  if (e.key === "F5" || (e.ctrlKey && ["r", "p", "f", "g", "j", "u", "s", "+", "-", "=", "0"].includes(k))) e.preventDefault();
});
document.addEventListener("wheel", (e) => { if (e.ctrlKey) e.preventDefault(); }, { passive: false });
document.addEventListener("dragstart", (e) => e.preventDefault());

start();
