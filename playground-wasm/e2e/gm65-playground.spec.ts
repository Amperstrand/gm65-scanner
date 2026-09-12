import { test, expect } from "@playwright/test";

// End-to-end against the deployed GitHub Pages demo (or a local server via
// PLAYGROUND_URL). Drives the real wasm driver through the full seam:
// init → start_scanning → inject-as-scan mid-read → byte-exact payload.
const PAYLOAD = "cashuAeyEeV1NUTsWALLETplaygroundE2E";

test("init detects GM65 and reaches Ready", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Init" }).click();
  await expect(page.locator("#log")).toContainText("init ✓ model=GM65", {
    timeout: 15_000,
  });
  await expect(page.locator("#stateBadge")).toHaveText(/Ready/);
});

test("scan round-trip delivers injected payload byte-exact", async ({ page }) => {
  await page.goto("/");
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
