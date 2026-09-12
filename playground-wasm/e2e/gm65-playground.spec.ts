import { test, expect } from "@playwright/test";

// End-to-end against the deployed GitHub Pages demo (or a local server via
// PLAYGROUND_URL). Drives the real wasm driver through the full seam:
// init → start_scanning → inject-as-scan mid-read → byte-exact payload.
const PAYLOAD = "cashuAeyEeV1NUTsWALLETplaygroundE2E";

const DEMO_URL =
  process.env.PLAYGROUND_URL ?? "https://amperstrand.github.io/gm65-scanner/";

// First deployments of a fresh Pages site 404 for a while after the deploy
// step reports success — poll until the page is actually interactive.
// (Also: never goto("/") here — with a subpath baseURL the leading slash
// resolves to the origin root, not the site.)
async function open(page: import("@playwright/test").Page) {
  for (let i = 0; i < 10; i++) {
    await page
      .goto(DEMO_URL, { waitUntil: "domcontentloaded" })
      .catch(() => undefined);
    if (
      await page
        .locator("#btnInit")
        .isVisible({ timeout: 2_000 })
        .catch(() => false)
    ) {
      return;
    }
    await page.waitForTimeout(3_000);
  }
  throw new Error("demo page did not become interactive");
}

test("init detects GM65 and reaches Ready", async ({ page }) => {
  await open(page);
  await page.getByRole("button", { name: "Init" }).click();
  await expect(page.locator("#log")).toContainText("init ✓ model=GM65", {
    timeout: 15_000,
  });
  await expect(page.locator("#stateBadge")).toHaveText(/Ready/);
});

test("scan round-trip delivers injected payload byte-exact", async ({ page }) => {
  await open(page);
  await page.getByRole("button", { name: "Init" }).click();
  await expect(page.locator("#log")).toContainText("init ✓ model=GM65", {
    timeout: 15_000,
  });

  await page.getByRole("button", { name: "Start scanning" }).click();
  await expect(page.locator("#log")).toContainText("scanning started");

  // Auto-loop keeps a read pending; the inject then lands mid-read and
  // must wake it (same-second delivery, not a timeout cycle later).
  await page.locator("#autoRead").check();
  await page.locator("#paste").fill(PAYLOAD);
  await page.getByRole("button", { name: "Inject as scan" }).click();
  await expect(page.locator("#payload")).toHaveText(PAYLOAD, {
    timeout: 20_000,
  });
  await expect(page.locator("#stateBadge")).toHaveText(/ScanComplete/);

  // UART monitors must show genuine protocol bytes.
  await expect(page.locator("#tx")).toContainText("7E 00 08");
  await expect(page.locator("#rx")).toContainText("33 31");
  const rx = await page.locator("#rx").textContent();
  for (const byte of PAYLOAD.slice(0, 8)) {
    // first 8 payload chars appear as hex in the RX stream
    expect(rx).toContain(byte.charCodeAt(0).toString(16).toUpperCase().padStart(2, "0"));
  }
});
