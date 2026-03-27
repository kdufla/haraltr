document.getElementById("login-form").addEventListener("submit", async (e) => {
    e.preventDefault();
    const errorDiv = document.getElementById("error");
    errorDiv.hidden = true;

    const password = document.getElementById("password").value;

    try {
        const res = await fetch("/api/login", {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({ password }),
        });

        if (res.ok) {
            window.location = "/";
        } else if (res.status === 401) {
            errorDiv.textContent = "Incorrect password";
            errorDiv.hidden = false;
        } else {
            errorDiv.textContent = "Unexpected error";
            errorDiv.hidden = false;
        }
    } catch {
        errorDiv.textContent = "Network error";
        errorDiv.hidden = false;
    }
});
