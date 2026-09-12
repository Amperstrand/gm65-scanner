import { test, expect } from "@playwright/test";

// End-to-end against the deployed GitHub Pages demo (or a local server via
// PLAYGROUND_URL). Drives the real wasm driver through the full seam:
// init → start_scanning → inject-as-scan mid-read → byte-exact payload.
const PAYLOAD = "cashuAeyEeV1NUTsWALLETplaygroundE2E";

const DEMO_URL =
  process.env.PLAYGROUND_URL ?? "https://amperstrand.github.io/gm65-scanner/";

// First deployments of a fresh Pages site 404 for a while after the deploy
// step reports success — poll until the page is actually interactive.
// Interactive means the "wasm module loaded" log line: buttons exist in the
// static HTML before main.js wires their handlers, so a click that lands
// earlier is silently lost. (Also: never goto("/") here — with a subpath
// baseURL the leading slash resolves to the origin root, not the site.)
async function open(page: import("@playwright/test").Page) {
  for (let i = 0; i < 10; i++) {
    await page
      .goto(DEMO_URL, { waitUntil: "domcontentloaded" })
      .catch(() => undefined);
    const log = await page
      .locator("#log")
      .textContent({ timeout: 2_000 })
      .catch(() => null);
    if (log && log.includes("wasm module loaded")) {
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

  // The device LCD (mirrored firmware UI) must actually be painted —
  // the result screen draws a cyan type title, white payload text + QR
  // field, and black QR modules. Class counts tolerate sparse text.
  const lcd = await page.evaluate(() => {
    const c = document.getElementById("lcd") as HTMLCanvasElement;
    const d = c.getContext("2d")!.getImageData(0, 0, c.width, c.height).data;
    let cyan = 0;
    let black = 0;
    for (let i = 0; i < d.length; i += 4) {
      const [r, g, b] = [d[i], d[i + 1], d[i + 2]];
      if (r === 0 && g === 255 && b === 255) cyan++;
      else if (r < 8 && g < 8 && b < 8) black++;
    }
    return { cyan, black };
  });
  expect(lcd.cyan).toBeGreaterThan(100);
  expect(lcd.black).toBeGreaterThan(500);
});
