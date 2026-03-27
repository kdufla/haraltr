async function fetchStatus() {
    try {
        const res = await fetch("/api/status");
        if (res.status === 401) {
            window.location = "/login";
            return;
        }
        const data = await res.json();
        document.getElementById("status").innerHTML =
            "<pre>" + JSON.stringify(data, null, 2) + "</pre>";
    } catch {
        document.getElementById("status").textContent = "Connection error";
    }
}

async function fetchConfig() {
    try {
        const res = await fetch("/api/config");
        if (res.status === 401) return;
        const data = await res.json();
        document.getElementById("config").innerHTML =
            "<pre>" + JSON.stringify(data, null, 2) + "</pre>";
    } catch {
        document.getElementById("config").textContent = "Connection error";
    }
}

fetchStatus();
fetchConfig();
setInterval(fetchStatus, 1000);
