// gm65-scanner playground harness.
// wasm-bindgen --target web output must be built into this directory:
//   cargo build -p playground-wasm --target wasm32-unknown-unknown --release
//   wasm-bindgen --target web \
//     ../target/wasm32-unknown-unknown/release/playground_wasm.wasm --out-dir .
//   python3 -m http.server 8901
import init, {
  pg_boot, pg_init, pg_start_scanning, pg_read_scan, pg_stop_scan,
  pg_state, pg_status, pg_module_scanning, pg_feed_scan_payload,
  pg_tx_hex, pg_rx_hex,
} from "./playground_wasm.js";

const $ = (id) => document.getElementById(id);
const logEl = $("log");
function log(msg) {
  const t = new Date().toTimeString().slice(0, 8);
  logEl.textContent = `[${t}] ${msg}\n` + logEl.textContent;
  logEl.scrollTop = 0;
}

let busy = false;
async function guarded(name, fn) {
  if (busy) return;
  busy = true;
  // Only driver ops are mutually exclusive. Scan input (Inject, camera)
  // stays enabled: decodes must land mid-read like a camera would.
  const ops = document.querySelectorAll("button[data-op]");
  ops.forEach((b) => (b.disabled = true));
  try {
    await fn();
  } catch (e) {
    log(`${name} ✗ ${e}`);
  } finally {
    busy = false;
    ops.forEach((b) => (b.disabled = false));
    refresh();
  }
}

let autoLoop = false;
function refresh() {
  const scanning = pg_module_scanning();
  document.body.classList.toggle("scanning", scanning);
  const badge = $("stateBadge");
  badge.textContent = pg_state();
  badge.classList.toggle("on", scanning);
  $("status").textContent = pg_status();
  const tx = pg_tx_hex();
  const rx = pg_rx_hex();
  $("tx").textContent = tx || "—";
  $("rx").textContent = rx || "—";
}

async function readOnce() {
  const got = await pg_read_scan(5000);
  if (got === null || got === undefined) {
    log("read_scan: timeout (5s)");
  } else {
    $("payload").textContent = got;
    log(`read_scan ✓ ${got.length} chars`);
  }
  refresh();
}

await init();
pg_boot();
log("wasm module loaded");
refresh();

$("btnInit").onclick = () =>
  guarded("init", async () => {
    const model = await pg_init();
    log(`init ✓ model=${model}`);
  });

$("btnStart").onclick = () =>
  guarded("start_scanning", async () => {
    const p = Number($("policy").value);
    log(`start_scanning(policy=${p}) → ${await pg_start_scanning(p)}`);
  });

$("btnStop").onclick = () =>
  guarded("stop_scan", async () => {
    log(`stop_scan → ${await pg_stop_scan()}`);
  });

$("btnRead").onclick = () => guarded("read_scan", readOnce);

$("autoRead").onchange = async (e) => {
  autoLoop = e.target.checked;
  while (autoLoop) {
    await guarded("read_scan", readOnce);
    if (autoLoop) await new Promise((r) => setTimeout(r, 200));
  }
};

$("btnFeed").onclick = () => {
  const v = $("paste").value.trim();
  if (!v) return;
  pg_feed_scan_payload(v);
  log(`injected scan payload (${v.length} chars)`);
  refresh();
};

// --- camera (BarcodeDetector, Chromium) ------------------------------------
let camStream = null;
let camLoop = false;

$("btnCam").onclick = () =>
  guarded("camera", async () => {
    if (!("BarcodeDetector" in window)) {
      log("camera ✗ BarcodeDetector unavailable in this browser — use paste");
      return;
    }
    camStream = await navigator.mediaDevices.getUserMedia({
      video: { facingMode: "environment" },
      audio: false,
    });
    const video = $("video");
    video.srcObject = camStream;
    await video.play();
    document.body.classList.add("cam-live");
    $("btnCamStop").disabled = false;
    const detector = new BarcodeDetector({ formats: ["qr_code"] });
    camLoop = true;
    log("camera started — scanning for QR codes");
    (async function loop() {
      while (camLoop) {
        try {
          const codes = await detector.detect(video);
          if (codes.length > 0 && codes[0].rawValue) {
            pg_feed_scan_payload(codes[0].rawValue);
            log(`camera decoded QR (${codes[0].rawValue.length} chars)`);
          }
        } catch { /* transient */ }
        await new Promise((r) => setTimeout(r, 300));
      }
    })();
  });

$("btnCamStop").onclick = () => {
  camLoop = false;
  if (camStream) camStream.getTracks().forEach((t) => t.stop());
  camStream = null;
  $("video").srcObject = null;
  document.body.classList.remove("cam-live");
  $("btnCamStop").disabled = true;
  log("camera stopped");
};

setInterval(() => {
  if (!busy) refresh();
}, 500);
