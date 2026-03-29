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

        setFormValue("bluetooth.adapter_index", cfg.bluetooth.adapter_index);
        setFormValue("bluetooth.poll_interval_ms", cfg.bluetooth.poll_interval_ms);
        setFormValue("bluetooth.disconnect_poll_interval_ms", cfg.bluetooth.disconnect_poll_interval_ms);

        setFormValue("proximity.rpl_threshold", cfg.proximity.rpl_threshold);
        setFormValue("proximity.lock_count", cfg.proximity.lock_count);
        setFormValue("proximity.unlock_count", cfg.proximity.unlock_count);
        setFormValue("proximity.kalman_q", cfg.proximity.kalman_q);
        setFormValue("proximity.kalman_r", cfg.proximity.kalman_r);
        setFormValue("proximity.kalman_initial", cfg.proximity.kalman_initial);
        setFormValue("proximity.disconnect_action", cfg.proximity.disconnect_action);

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
        bluetooth: {
            adapter_index: getFormValue("bluetooth.adapter_index"),
            poll_interval_ms: getFormValue("bluetooth.poll_interval_ms"),
            disconnect_poll_interval_ms: getFormValue("bluetooth.disconnect_poll_interval_ms"),
        },
        proximity: {
            rpl_threshold: getFormValue("proximity.rpl_threshold"),
            lock_count: getFormValue("proximity.lock_count"),
            unlock_count: getFormValue("proximity.unlock_count"),
            kalman_q: getFormValue("proximity.kalman_q"),
            kalman_r: getFormValue("proximity.kalman_r"),
            kalman_initial: getFormValue("proximity.kalman_initial"),
            disconnect_action: getFormValue("proximity.disconnect_action"),
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
const chartData = [[], [], []]; // [timestamps, filtered RPL, raw RPL]
let chart = null;
const t0 = Date.now() / 1000;

function initChart() {
    const el = document.getElementById("rpl-chart");
    const opts = {
        width: el.clientWidth || 600,
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
    chart = new uPlot(opts, [[], [], []], el);
}

async function fetchStatus() {
    try {
        const res = await authFetch("/api/status");
        const data = await res.json();

        // monitor text
        document.getElementById("mon-mac").textContent = data.target_mac || "—";
        document.getElementById("mon-state").textContent = data.state || "—";
        document.getElementById("mon-connected").textContent = data.connected ? "yes" : "no";
        document.getElementById("mon-rpl").textContent = data.rpl != null ? data.rpl.toFixed(1) : "—";

        // chart data
        chartData[0].push(Date.now() / 1000 - t0);
        chartData[1].push(data.rpl ?? null);
        chartData[2].push(data.raw_rpl ?? null);

        if (chartData[0].length > CHART_LEN) {
            chartData[0].shift();
            chartData[1].shift();
            chartData[2].shift();
        }

        if (chart) {
            chart.setData(chartData);
        }
    } catch (e) {
        if (e.message !== "unauthorized") {
            console.log(e);
        }
    }
}

async function loadDevices() {
    try {
        const res = await authFetch("/api/bt-devices");
        const data = await res.json();
        const sel = document.getElementById("device-select");
        sel.length = 1;
        for (const dev of data.devices || []) {
            const opt = document.createElement("option");
            opt.value = dev.mac;
            opt.dataset.addressType = dev.address_type;
            opt.textContent = dev.name + (dev.connected ? " [connected]" : "");
            sel.appendChild(opt);
        }
    } catch (e) {
        if (e.message !== "unauthorized") console.log(e);
    }
}

async function loadCurrentTarget() {
    try {
        const res = await authFetch("/api/devices");
        const data = await res.json();
        document.getElementById("current-target").textContent =
            data.target_mac ? "Current: " + data.target_mac : "No device selected";
    } catch (e) {
        if (e.message !== "unauthorized") console.log(e);
    }
}

document.getElementById("device-select").addEventListener("change", async (e) => {
    if (!e.target.value) return;
    const selected = e.target.options[e.target.selectedIndex];
    try {
        await authFetch("/api/devices", {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({
                target_mac: e.target.value,
                address_type: selected.dataset.addressType,
            }),
        });
        loadCurrentTarget();
    } catch (e) {
        if (e.message !== "unauthorized") console.log(e);
    }
});

document.getElementById("clear-device-btn").addEventListener("click", async () => {
    try {
        await authFetch("/api/devices", { method: "DELETE" });
        loadCurrentTarget();
        document.getElementById("device-select").value = "";
    } catch (e) {
        if (e.message !== "unauthorized") console.log(e);
    }
});

document.getElementById("refresh-devices-btn").addEventListener("click", loadDevices);

// --- Init ---

loadConfig();
loadDevices();
loadCurrentTarget();
initChart();
fetchStatus();
setInterval(fetchStatus, 1000);
