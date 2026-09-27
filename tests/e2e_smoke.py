import os
from pathlib import Path

from playwright.sync_api import sync_playwright


BASE_URL = os.environ.get("SONDE_E2E_URL", "http://127.0.0.1:8091").rstrip("/")
SETUP_TOKEN = os.environ.get("SONDE_E2E_SETUP_TOKEN", "")
ADMIN_EMAIL = "owner@sonde.test"
ADMIN_USERNAME = "Sonde Admin"
ADMIN_PASSWORD = "Orbit-lantern-27-river"

if not SETUP_TOKEN:
    raise RuntimeError("SONDE_E2E_SETUP_TOKEN is required")

OUTPUT = Path("target/e2e-artifacts")
OUTPUT.mkdir(parents=True, exist_ok=True)


with sync_playwright() as playwright:
    browser = playwright.chromium.launch(headless=True)
    context = browser.new_context(viewport={"width": 1440, "height": 1000}, locale="en-US")
    page = context.new_page()
    console_errors: list[str] = []
    server_errors: list[str] = []

    page.on(
        "console",
        lambda message: console_errors.append(message.text) if message.type == "error" else None,
    )
    page.on(
        "response",
        lambda response: server_errors.append(f"{response.status} {response.url}")
        if response.status >= 500
        else None,
    )

    page.goto(BASE_URL)
    page.wait_for_load_state("networkidle")
    page.get_by_role("heading", name="Welcome to Sonde Setup", level=1).wait_for()
    page.screenshot(path=OUTPUT / "01-setup.png", full_page=True)

    page.get_by_label("Setup Token").fill(SETUP_TOKEN)
    page.get_by_label("Administrator Email").fill(ADMIN_EMAIL)
    page.get_by_label("Administrator Username").fill(ADMIN_USERNAME)
    page.get_by_label("Administrator Password").fill(ADMIN_PASSWORD)
    page.get_by_role("button", name="Complete Initialization & Launch").click()

    page.get_by_role("heading", name="Sign In to Sonde", level=1).wait_for()
    page.screenshot(path=OUTPUT / "02-login.png", full_page=True)
    page.get_by_label("Username or Email").fill(ADMIN_EMAIL)
    page.get_by_label("Password").fill(ADMIN_PASSWORD)
    page.get_by_role("button", name="Sign In", exact=True).click()

    page.get_by_role("heading", name="Telemetry Overview", level=1).wait_for()
    page.wait_for_load_state("networkidle")
    page.screenshot(path=OUTPUT / "03-overview.png", full_page=True)

    assert not console_errors, f"browser console errors: {console_errors}"
    assert not server_errors, f"unexpected server errors: {server_errors}"

    browser.close()

print("Sonde production browser smoke test passed")
