from pathlib import Path

from playwright.sync_api import sync_playwright


OUTPUT = Path("target/e2e-artifacts")
OUTPUT.mkdir(parents=True, exist_ok=True)


def choose_option(page, label: str, option: str) -> None:
    page.get_by_role("combobox", name=label).click()
    page.get_by_role("option", name=option, exact=True).click()

with sync_playwright() as playwright:
    browser = playwright.chromium.launch(headless=True)
    context = browser.new_context(viewport={"width": 1440, "height": 1000}, locale="en-US")
    page = context.new_page()
    console_errors: list[str] = []
    http_errors: list[str] = []
    page.on("console", lambda message: console_errors.append(message.text) if message.type == "error" else None)
    page.on("response", lambda response: http_errors.append(f"{response.status} {response.url}") if response.status >= 400 else None)

    page.goto("http://127.0.0.1:8091")
    page.wait_for_load_state("networkidle")
    page.screenshot(path=OUTPUT / "00-reconnaissance.png", full_page=True)
    print("Rendered body:", page.locator("body").inner_text())
    print("Console errors:", console_errors)
    print("HTTP errors:", http_errors)
    page.get_by_role("heading", name="Bring your telemetry analytics platform online", level=1).wait_for()
    assert page.get_by_role("combobox", name="Theme").inner_text() == "System"
    page.screenshot(path=OUTPUT / "01-setup.png", full_page=True)
    choose_option(page, "Theme", "Dark")
    assert page.locator("html").get_attribute("data-theme") == "dark"
    page.wait_for_timeout(200)
    page.screenshot(path=OUTPUT / "01-setup-dark.png", full_page=True)
    choose_option(page, "Theme", "System")
    choose_option(page, "Language", "简体中文")
    page.get_by_role("heading", name="启动你的遥测分析平台", level=1).wait_for()
    choose_option(page, "语言", "English")

    page.get_by_label("Super Admin email").fill("owner@sonde.test")
    page.get_by_label("Super Admin name").fill("Sonde Admin")
    page.get_by_label("Super Admin password").fill("Orbit-lantern-27-river")
    page.get_by_role("button", name="Initialize Sonde").click()

    page.get_by_role("heading", name="Operator sign in").wait_for()
    page.get_by_label("Email").fill("owner@sonde.test")
    page.get_by_label("Password").fill("incorrect password attempt")
    page.get_by_role("button", name="Enter Sonde").click()
    page.get_by_text("Verification required").wait_for()
    page.screenshot(path=OUTPUT / "02-login-challenge.png", full_page=True)
    challenge = page.get_by_role("status", name="Verification code").inner_text()
    console_errors.clear()
    http_errors.clear()
    page.get_by_label("Password").fill("Orbit-lantern-27-river")
    page.get_by_label("Enter verification code").fill(challenge)
    page.get_by_role("button", name="Enter Sonde").click()
    page.get_by_role("heading", name="System overview").wait_for()
    page.screenshot(path=OUTPUT / "03-overview.png", full_page=True)

    page.get_by_role("link", name="Applications").click()
    page.get_by_role("button", name="New application").click()
    page.get_by_label("Application name").fill("BMCBL")
    page.get_by_label("URL slug").fill("bmcbl")
    page.get_by_role("button", name="Create", exact=True).click()
    page.get_by_text("Copy this key now").wait_for()
    page.screenshot(path=OUTPUT / "04-application-key.png", full_page=True)

    page.get_by_role("link", name="Migration").click()
    page.get_by_role("heading", name="D1 migration center").wait_for()
    page.get_by_label("Choose a Wrangler D1 SQL export").set_input_files("tests/fixtures/bmcbl_export.sql")
    page.get_by_role("button", name="Validate and preview").click()
    page.get_by_text("Source rows").wait_for()
    page.get_by_role("button", name="Confirm import").click()
    page.get_by_text("Import completed").wait_for()
    page.screenshot(path=OUTPUT / "05-migration.png", full_page=True)

    page.get_by_role("link", name="Explorer").click()
    page.get_by_role("heading", name="Telemetry explorer").wait_for()
    page.get_by_text("migration.application_start").first.wait_for()
    page.screenshot(path=OUTPUT / "06-explorer.png", full_page=True)

    assert not console_errors, f"browser console errors: {console_errors}"
    assert not http_errors, f"unexpected HTTP errors: {http_errors}"
    browser.close()

print("Sonde e2e smoke test passed")
