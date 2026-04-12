function route() {
    const hash = location.hash || "#config";
    document.getElementById("config-page").hidden = hash !== "#config";
    document.getElementById("monitor-page").hidden = hash !== "#monitor";

    document.querySelectorAll("nav a").forEach(a => {
        a.classList.toggle("active", a.getAttribute("href") === hash);
    });
}

window.addEventListener("hashchange", route);
route();

document.getElementById("advanced-toggle").addEventListener("change", (e) => {
    document.querySelectorAll(".advanced").forEach(el => {
        el.hidden = !e.target.checked;
    });
});

async function authFetch(url, opts) {
    const res = await fetch(url, opts);
    if (res.status === 401) {
        window.location = "/login";
        throw new Error("unauthorized");
    }
    return res;
}

document.getElementById("logout-btn").addEventListener("click", async () => {
    try {
        await fetch("/api/logout", { method: "POST" });
    } finally {
        window.location = "/login";
    }
});

const configForm = document.getElementById("config-form");
const saveStatus = document.getElementById("save-status");
let saveTimeout = null;
let debounceTimer = null;

function setFormValue(name, value) {
    const el = configForm.elements[name];
    if (!el) return;
    if (el.tagName === "SELECT") {
        el.value = value;
    } else {
        el.value = value;
    }
}

async function loadConfig() {
    try {
        const res = await authFetch("/api/config");
        const cfg = await res.json();

        setFormValue("adapter_index", cfg.adapter_index);

        setFormValue("br_edr.rpl_threshold", cfg.br_edr.rpl_threshold);
        setFormValue("br_edr.disconnect_action", cfg.br_edr.disconnect_action);
        setFormValue("br_edr.lock_count", cfg.br_edr.lock_count);
        setFormValue("br_edr.unlock_count", cfg.br_edr.unlock_count);
        setFormValue("br_edr.poll_interval_ms", cfg.br_edr.poll_interval_ms);
        setFormValue("br_edr.disconnect_poll_interval_ms", cfg.br_edr.disconnect_poll_interval_ms);
        setFormValue("br_edr.kalman_q", cfg.br_edr.kalman_q);
        setFormValue("br_edr.kalman_r", cfg.br_edr.kalman_r);
        setFormValue("br_edr.kalman_initial", cfg.br_edr.kalman_initial);

        setFormValue("le.rpl_threshold", cfg.le.rpl_threshold);
        setFormValue("le.disconnect_action", cfg.le.disconnect_action);
        setFormValue("le.lock_count", cfg.le.lock_count);
        setFormValue("le.unlock_count", cfg.le.unlock_count);
        setFormValue("le.assumed_tx_power", cfg.le.assumed_tx_power);
        setFormValue("le.poll_interval_ms", cfg.le.poll_interval_ms);
        setFormValue("le.disconnect_poll_interval_ms", cfg.le.disconnect_poll_interval_ms);
        setFormValue("le.kalman_q", cfg.le.kalman_q);
        setFormValue("le.kalman_r", cfg.le.kalman_r);
        setFormValue("le.kalman_initial", cfg.le.kalman_initial);

        setFormValue("wake.duration_secs", cfg.wake.duration_secs);
        setFormValue("wake.mouse_interval_ms", cfg.wake.mouse_interval_ms);
        setFormValue("wake.enter_interval_ms", cfg.wake.enter_interval_ms);

        setFormValue("web.port", cfg.web.port);
    } catch (e) {
        if (e.message !== "unauthorized") {
            showSave("Failed to load config", true);
        }
    }
}

function getFormValue(name) {
    const el = configForm.elements[name];
    if (!el) return undefined;
    if (el.tagName === "SELECT") return el.value;
    if (el.type === "number") {
        const v = el.valueAsNumber;
        return isNaN(v) ? undefined : v;
    }
    return el.value;
}

function collectConfig() {
    return {
        adapter_index: getFormValue("adapter_index"),
        br_edr: {
            rpl_threshold: getFormValue("br_edr.rpl_threshold"),
            disconnect_action: getFormValue("br_edr.disconnect_action"),
            lock_count: getFormValue("br_edr.lock_count"),
            unlock_count: getFormValue("br_edr.unlock_count"),
            poll_interval_ms: getFormValue("br_edr.poll_interval_ms"),
            disconnect_poll_interval_ms: getFormValue("br_edr.disconnect_poll_interval_ms"),
            kalman_q: getFormValue("br_edr.kalman_q"),
            kalman_r: getFormValue("br_edr.kalman_r"),
            kalman_initial: getFormValue("br_edr.kalman_initial"),
        },
        le: {
            rpl_threshold: getFormValue("le.rpl_threshold"),
            disconnect_action: getFormValue("le.disconnect_action"),
            lock_count: getFormValue("le.lock_count"),
            unlock_count: getFormValue("le.unlock_count"),
            assumed_tx_power: getFormValue("le.assumed_tx_power"),
            poll_interval_ms: getFormValue("le.poll_interval_ms"),
            disconnect_poll_interval_ms: getFormValue("le.disconnect_poll_interval_ms"),
            kalman_q: getFormValue("le.kalman_q"),
            kalman_r: getFormValue("le.kalman_r"),
            kalman_initial: getFormValue("le.kalman_initial"),
        },
        wake: {
            duration_secs: getFormValue("wake.duration_secs"),
            mouse_interval_ms: getFormValue("wake.mouse_interval_ms"),
            enter_interval_ms: getFormValue("wake.enter_interval_ms"),
        },
        web: {
            port: getFormValue("web.port"),
        },
    };
}

function showSave(msg, isError) {
    saveStatus.textContent = msg;
    saveStatus.className = isError ? "error" : "ok";
    clearTimeout(saveTimeout);
    saveTimeout = setTimeout(() => {
        saveStatus.textContent = "";
        saveStatus.className = "";
    }, 2000);
}

async function saveConfig() {
    try {
        const res = await authFetch("/api/config", {
            method: "PUT",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify(collectConfig()),
        });
        if (res.ok) {
            showSave("Saved", false);
        } else {
            const data = await res.json().catch(() => ({}));
            showSave(data.error || "Save failed", true);
        }
    } catch (e) {
        if (e.message !== "unauthorized") {
            showSave("Connection error", true);
        }
    }
}

configForm.addEventListener("change", () => {
    clearTimeout(debounceTimer);
    debounceTimer = setTimeout(saveConfig, 500);
});


const CHART_LEN = 60;
const deviceState = new Map(); // Map<mac, { chartData: [[], [], []], chart: uPlot }>
const t0 = Date.now() / 1000;

function getOrCreateDeviceCard(mac) {
    if (deviceState.has(mac)) return deviceState.get(mac);

    const idSuffix = mac.replaceAll(":", "-");
    const card = document.createElement("div");
    card.className = "device-card";
    card.dataset.mac = mac;

    const info = document.createElement("div");
    info.className = "device-info";
    info.innerHTML =
        `<span>Target: <span id="mon-mac-${idSuffix}">-</span></span>` +
        `<span>State: <span id="mon-state-${idSuffix}">-</span></span>` +
        `<span>Connected: <span id="mon-connected-${idSuffix}">-</span></span>` +
        `<span>RPL: <span id="mon-rpl-${idSuffix}">-</span></span>`;
    card.appendChild(info);

    const chartContainer = document.createElement("div");
    card.appendChild(chartContainer);

    document.getElementById("device-cards").appendChild(card);

    const opts = {
        width: chartContainer.clientWidth || 600,
        height: 250,
        series: [
            {},
            { label: "Filtered", stroke: "cyan", width: 2 },
            { label: "Raw", stroke: "gray", width: 1, dash: [4, 4] },
        ],
        axes: [
            { label: "Time (s)" },
            { label: "RPL" },
        ],
    };
    const chartData = [[], [], []];
    const chart = new uPlot(opts, [[], [], []], chartContainer);

    const entry = { chartData, chart };
    deviceState.set(mac, entry);
    return entry;
}

async function fetchStatus() {
    try {
        const res = await authFetch("/api/status");
        const data = await res.json();

        document.getElementById("mon-uptime").textContent = data.uptime_secs + "s";

        const activeMacs = new Set();
        for (const device of data.devices || []) {
            const mac = device.target_mac;
            const { chartData, chart } = getOrCreateDeviceCard(mac);
            const idSuffix = mac.replaceAll(":", "-");

            document.getElementById(`mon-mac-${idSuffix}`).textContent = mac;
            document.getElementById(`mon-state-${idSuffix}`).textContent = device.state || "-";
            document.getElementById(`mon-connected-${idSuffix}`).textContent = device.connected ? "yes" : "no";
            document.getElementById(`mon-rpl-${idSuffix}`).textContent = device.rpl != null ? device.rpl.toFixed(1) : "-";

            chartData[0].push(Date.now() / 1000 - t0);
            chartData[1].push(device.rpl ?? null);
            chartData[2].push(device.raw_rpl ?? null);

            if (chartData[0].length > CHART_LEN) {
                chartData[0].shift();
                chartData[1].shift();
                chartData[2].shift();
            }

            chart.setData(chartData);
            activeMacs.add(mac);
        }

        for (const mac of deviceState.keys()) {
            if (!activeMacs.has(mac)) {
                const entry = deviceState.get(mac);
                entry.chart.destroy();
                document.querySelector(`[data-mac="${CSS.escape(mac)}"]`)?.remove();
                deviceState.delete(mac);
            }
        }
    } catch (e) {
        if (e.message !== "unauthorized") {
            console.log(e);
        }
    }
}

async function addDevice(mac, addressType) {
    try {
        await authFetch("/api/devices", {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({
                target_mac: mac,
                address_type: addressType,
            }),
        });
        loadDevices();
    } catch (e) {
        if (e.message !== "unauthorized") console.log(e);
    }
}

async function removeDevice(mac) {
    try {
        await authFetch("/api/devices", {
            method: "DELETE",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({ target_mac: mac }),
        });
        loadDevices();
    } catch (e) {
        if (e.message !== "unauthorized") console.log(e);
    }
}

let openAccordionMac = null;
let deviceDebounceTimers = new Map();

function toggleAccordion(mac) {
    const prev = document.querySelector(".device-accordion:not([hidden])");
    if (prev) prev.hidden = true;

    if (openAccordionMac === mac) {
        openAccordionMac = null;
        return;
    }

    const el = document.getElementById(`accordion-${mac}`);
    if (el) {
        el.hidden = false;
        openAccordionMac = mac;
    }
}

function createAccordion(dev) {
    const acc = document.createElement("div");
    acc.className = "device-accordion";
    acc.id = `accordion-${dev.mac}`;
    acc.hidden = openAccordionMac !== dev.mac;

    const bt = dev.bluetooth || {};
    const prox = dev.proximity || {};

    // TODO I'm no js expert but I can't imagine this being a good practice
    acc.innerHTML =
        `<form class="device-config-form" data-mac="${dev.mac}">` +
        `<fieldset><legend>Proximity</legend>` +
        `<label>RPL threshold <input type="number" name="proximity.rpl_threshold" min="0.5" max="200" step="0.5" value="${prox.rpl_threshold ?? ""}" placeholder="global"></label>` +
        `<label>Disconnect action <select name="proximity.disconnect_action">` +
        `<option value="" ${prox.disconnect_action == null ? "selected" : ""}>global</option>` +
        `<option value="lock" ${prox.disconnect_action === "lock" ? "selected" : ""}>Lock</option>` +
        `<option value="unlock" ${prox.disconnect_action === "unlock" ? "selected" : ""}>Unlock</option>` +
        `<option value="none" ${prox.disconnect_action === "none" ? "selected" : ""}>None</option>` +
        `</select></label>` +
        `<label>Lock count <input type="number" name="proximity.lock_count" min="1" max="100" step="1" value="${prox.lock_count ?? ""}" placeholder="global"></label>` +
        `<label>Unlock count <input type="number" name="proximity.unlock_count" min="1" max="100" step="1" value="${prox.unlock_count ?? ""}" placeholder="global"></label>` +
        `<label>Kalman Q <input type="number" name="proximity.kalman_q" min="0.01" max="10" step="0.01" value="${prox.kalman_q ?? ""}" placeholder="global"></label>` +
        `<label>Kalman R <input type="number" name="proximity.kalman_r" min="0.1" max="100" step="0.1" value="${prox.kalman_r ?? ""}" placeholder="global"></label>` +
        `<label>Kalman initial <input type="number" name="proximity.kalman_initial" min="0.1" max="200" step="0.1" value="${prox.kalman_initial ?? ""}" placeholder="global"></label>` +
        `<label>Assumed TX power (dBm) <input type="number" name="proximity.assumed_tx_power" min="-127" max="126" step="1" value="${prox.assumed_tx_power ?? ""}" placeholder="global"></label>` +
        `</fieldset>` +
        `<fieldset><legend>Bluetooth</legend>` +
        `<label>Adapter index <input type="number" name="bluetooth.adapter_index" min="0" max="65535" step="1" value="${bt.adapter_index ?? ""}" placeholder="global"></label>` +
        `<label>Poll interval (ms) <input type="number" name="bluetooth.poll_interval_ms" min="100" max="60000" step="100" value="${bt.poll_interval_ms ?? ""}" placeholder="global"></label>` +
        `<label>Disconnect poll interval (ms) <input type="number" name="bluetooth.disconnect_poll_interval_ms" min="100" max="60000" step="100" value="${bt.disconnect_poll_interval_ms ?? ""}" placeholder="global"></label>` +
        `</fieldset>` +
        `</form>`;

    acc.querySelector("form").addEventListener("change", () => {
        const mac = dev.mac;
        clearTimeout(deviceDebounceTimers.get(mac));
        deviceDebounceTimers.set(mac, setTimeout(() => saveDeviceConfig(mac), 500));
    });

    return acc;
}

function collectDeviceConfig(mac) {
    const form = document.querySelector(`.device-config-form[data-mac="${CSS.escape(mac)}"]`);
    if (!form) return null;

    function optNum(name) {
        const el = form.elements[name];
        if (!el || el.value === "") return null;
        const v = el.valueAsNumber;
        return isNaN(v) ? null : v;
    }
    function optStr(name) {
        const el = form.elements[name];
        if (!el || el.value === "") return null;
        return el.value;
    }

    return {
        target_mac: mac,
        bluetooth: {
            adapter_index: optNum("bluetooth.adapter_index"),
            poll_interval_ms: optNum("bluetooth.poll_interval_ms"),
            disconnect_poll_interval_ms: optNum("bluetooth.disconnect_poll_interval_ms"),
        },
        proximity: {
            rpl_threshold: optNum("proximity.rpl_threshold"),
            lock_count: optNum("proximity.lock_count"),
            unlock_count: optNum("proximity.unlock_count"),
            kalman_q: optNum("proximity.kalman_q"),
            kalman_r: optNum("proximity.kalman_r"),
            kalman_initial: optNum("proximity.kalman_initial"),
            disconnect_action: optStr("proximity.disconnect_action"),
            assumed_tx_power: optNum("proximity.assumed_tx_power"),
        },
    };
}

async function saveDeviceConfig(mac) {
    const body = collectDeviceConfig(mac);
    if (!body) return;
    try {
        const res = await authFetch("/api/devices", {
            method: "PATCH",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify(body),
        });
        if (res.ok) {
            showSave("Saved", false);
        } else {
            const data = await res.json().catch(() => ({}));
            showSave(data.error || "Save failed", true);
        }
    } catch (e) {
        if (e.message !== "unauthorized") {
            showSave("Connection error", true);
        }
    }
}

function renderDeviceItem(dev, monitored) {
    const li = document.createElement("li");
    li.className = "device-item";

    const row = document.createElement("div");
    row.className = "device-row";

    const name = document.createElement("span");
    name.textContent = dev.name || dev.mac;
    row.appendChild(name);

    const btn = document.createElement("button");
    if (monitored) {
        btn.className = "btn-unmonitor";
        btn.textContent = "−";
        btn.addEventListener("click", (e) => { e.stopPropagation(); removeDevice(dev.mac); });
        row.addEventListener("click", () => toggleAccordion(dev.mac));
        row.style.cursor = "pointer";
    } else {
        btn.className = "btn-monitor";
        btn.textContent = "+";
        btn.addEventListener("click", () => addDevice(dev.mac, dev.address_type));
    }
    row.appendChild(btn);
    li.appendChild(row);

    if (monitored) {
        li.appendChild(createAccordion(dev));
    }

    return li;
}

async function loadDevices() {
    try {
        const res = await authFetch("/api/devices");
        const data = await res.json();

        const connectedList = document.getElementById("connected-devices");
        const availableList = document.getElementById("available-devices");
        connectedList.innerHTML = "";
        availableList.innerHTML = "";

        for (const dev of data.devices || []) {
            const li = renderDeviceItem(dev, dev.monitored);
            (dev.connected ? connectedList : availableList).appendChild(li);
        }
    } catch (e) {
        if (e.message !== "unauthorized") console.log(e);
    }
}

document.getElementById("refresh-devices-btn").addEventListener("click", loadDevices);

// --- Init ---

loadConfig();
loadDevices();

let fetchPending = false;
async function scheduledFetch() {
    if (fetchPending) return;
    fetchPending = true;
    try {
        await fetchStatus();
    } finally {
        fetchPending = false;
    }
}
scheduledFetch();
setInterval(scheduledFetch, 1000);
