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
        setFormValue("bluetooth.address_type", cfg.bluetooth.address_type);
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
            address_type: getFormValue("bluetooth.address_type"),
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

async function fetchStatus() {
    try {
        const res = await authFetch("/api/status");
        const data = await res.json();
        void data;
    } catch (e) {
        if (e.message !== "unauthorized") {
            console.log(e);
        }
    }
}

loadConfig();
fetchStatus();
setInterval(fetchStatus, 1000);
