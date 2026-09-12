import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: ".",
  timeout: 60_000,
  retries: 1,
  reporter: "list",
  use: {
    baseURL: process.env.PLAYGROUND_URL ?? "https://amperstrand.github.io/gm65-scanner/",
  },
});
