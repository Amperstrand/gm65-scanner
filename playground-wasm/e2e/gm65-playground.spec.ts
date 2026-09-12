import { test, expect } from "@playwright/test";

// End-to-end against the deployed GitHub Pages demo (or a local server via
// PLAYGROUND_URL). Drives the device through its real surfaces: LCD touch
// rows, host wing commands, scan injection, and register-level settings.
const PAYLOAD = "cashuAeyEeV1NUTsWALLETplaygroundE2E";

const DEMO_URL =
  process.env.PLAYGROUND_URL ?? "https://amperstrand.github.io/gm65-scanner/";

// Fresh Pages sites 404 briefly after the deploy step reports success, and
// buttons exist in the static HTML before main.js wires handlers — poll for
// the wasm-ready log line instead. (Never goto("/") here: under a subpath
// baseURL a leading slash resolves to the origin root, not the site.)
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

async function boot(page: import("@playwright/test").Page) {
  await open(page);
  await page.getByRole("button", { name: "Init" }).click();
  await expect(page.locator("#log")).toContainText("init ✓ model=GM65", {
    timeout: 15_000,
  });
}

// Tap a touch row by id, computing the click point from the layout the
// device itself reports (pg_ui_tap_targets) — the e2e cannot drift from
// the rendered rows.
async function tapRow(page: import("@playwright/test").Page, id: string) {
  const targets = JSON.parse(await page.evaluate(() => window.__pg.targets()));
  const row = targets.rows.find((r: { id: string }) => r.id === id);
  expect(row, `row ${id} on screen ${targets.screen}`).toBeTruthy();
  const box = await page.locator("#lcd").boundingBox();
  const cx = box.x + ((24 + 456 / 2) / 480) * box.width;
  const cy = box.y + ((row.y + row.h / 2) / 800) * box.height;
  await page.mouse.click(cx, cy);
}

test("init reaches the home screen with a painted LCD", async ({ page }) => {
  await boot(page);
  await expect(page.locator("#stateBadge")).toHaveText("home");
  // Home title is drawn in theme cyan — proves the LCD painted.
  const cyan = await page.evaluate(() => {
    const c = document.getElementById("lcd") as HTMLCanvasElement;
    const d = c.getContext("2d")!.getImageData(0, 0, c.width, c.height).data;
    let n = 0;
    for (let i = 0; i < d.length; i += 4) {
      if (d[i] === 0 && d[i + 1] === 255 && d[i + 2] === 255) n++;
    }
    return n;
  });
  expect(cyan).toBeGreaterThan(50);
});

test("a scan lands on the display by itself — standing read wiring", async ({
  page,
}) => {
  await boot(page);

  // Start via the device's own touch row (no host Read anywhere).
  await tapRow(page, "start");
  await expect(page.locator("#log")).toContainText("scanning started");
  await expect(page.locator("#stateBadge")).toHaveText("scanning");

  await page.locator("#paste").fill(PAYLOAD);
  await page.getByRole("button", { name: "Inject as scan" }).click();

  // The device task's standing read must deliver the decode to the LCD and
  // the wing payload panel without any host interaction.
  await expect(page.locator("#payload")).toHaveText(PAYLOAD, {
    timeout: 20_000,
  });
  await expect(page.locator("#stateBadge")).toHaveText("result");

  await expect(page.locator("#tx")).toContainText("7E 00 08");
  await expect(page.locator("#rx")).toContainText("33 31");
  const rx = await page.locator("#rx").textContent();
  for (const byte of PAYLOAD.slice(0, 8)) {
    expect(rx).toContain(
      byte.charCodeAt(0).toString(16).toUpperCase().padStart(2, "0"),
    );
  }

  // Result screen paints: cyan type title + QR mirror modules.
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

test("dry settings: LCD taps flip real SETTINGS register bits", async ({
  page,
}) => {
  await boot(page);
  await expect(page.locator("#stateBadge")).toHaveText("home");

  await tapRow(page, "settings");
  await expect(page.locator("#stateBadge")).toHaveText("settings");

  const before = await page.evaluate(() => window.__pg.settings());
  await tapRow(page, "buzzer");
  // Buzzer is SETTINGS bit 6; the write is a genuine protocol frame.
  await expect
    .poll(() => page.evaluate(() => window.__pg.settings()))
    .toBe(before ^ 0x40);
  await expect(page.locator("#tx")).toContainText("7E 00 08 01 00 00");
  await expect(page.locator("#log")).toContainText("settings →");

  await tapRow(page, "back");
  await expect(page.locator("#stateBadge")).toHaveText("home");
});
