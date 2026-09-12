// gm65-scanner playground harness.
// The device task in wasm owns the driver; this script only sends commands
// (buttons, canvas taps), feeds scan input (camera/paste), and renders
// device events into the side wing.
// Build the wasm-bindgen glue into this directory (see README).
import init, {
  pg_boot, pg_init, pg_set_on_event, pg_cmd_start, pg_cmd_stop,
  pg_feed_scan_payload, pg_lcd_tap, pg_state, pg_status,
  pg_module_scanning, pg_module_settings, pg_tx_hex, pg_rx_hex,
  pg_ui_tap_targets,
} from "./playground_wasm.js";

const $ = (id) => document.getElementById(id);
const logEl = $("log");
function log(msg) {
  const t = new Date().toTimeString().slice(0, 8);
  logEl.textContent = `[${t}] ${msg}\n` + logEl.textContent;
  logEl.scrollTop = 0;
}

function refresh() {
  const scanning = pg_module_scanning();
  document.body.classList.toggle("scanning", scanning);
  const badge = $("stateBadge");
  badge.textContent = pg_state();
  badge.classList.toggle("on", scanning);
  $("status").textContent = pg_status();
  $("tx").textContent = pg_tx_hex() || "—";
  $("rx").textContent = pg_rx_hex() || "—";
}

function onEvent(raw) {
  const ev = JSON.parse(raw);
  if (ev.type === "log") {
    log(ev.msg);
  } else if (ev.type === "scan") {
    $("payload").textContent = ev.payload;
  }
  refresh();
}

await init();
pg_boot();
pg_set_on_event(onEvent);
// Expose for the e2e harness and console tinkering.
window.__pg = { state: pg_state, targets: pg_ui_tap_targets, settings: () => pg_module_settings() };
log("wasm module loaded");
refresh();

// --- device LCD touch → pixel coords ---------------------------------------
$("lcd").addEventListener("click", (e) => {
  const rect = e.currentTarget.getBoundingClientRect();
  const px = Math.round(((e.clientX - rect.left) / rect.width) * 480);
  const py = Math.round(((e.clientY - rect.top) / rect.height) * 800);
  pg_lcd_tap(px, py);
});

// --- host wing --------------------------------------------------------------
$("btnInit").onclick = async () => {
  try {
    await pg_init();
    log("device task starting");
  } catch (e) {
    log(`init ✗ ${e}`);
  }
};

$("btnStart").onclick = () => {
  pg_cmd_start(Number($("policy").value));
};

$("btnStop").onclick = () => {
  pg_cmd_stop();
};

$("btnFeed").onclick = () => {
  const v = $("paste").value.trim();
  if (!v) return;
  pg_feed_scan_payload(v);
  log(`injected scan payload (${v.length} chars)`);
};

// --- camera (BarcodeDetector, Chromium) — the module's viewfinder ----------
let camStream = null;
let camLoop = false;

$("btnCam").onclick = async () => {
  if (!("BarcodeDetector" in window)) {
    log("camera ✗ BarcodeDetector unavailable in this browser — use paste");
    return;
  }
  try {
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
    log("camera started — tap Start scan on the device, then aim at a QR");
    (async function loop() {
      while (camLoop) {
        try {
          const codes = await detector.detect(video);
          if (codes.length > 0 && codes[0].rawValue) {
            pg_feed_scan_payload(codes[0].rawValue);
          }
        } catch { /* transient */ }
        await new Promise((r) => setTimeout(r, 300));
      }
    })();
  } catch (e) {
    log(`camera ✗ ${e}`);
  }
};

$("btnCamStop").onclick = () => {
  camLoop = false;
  if (camStream) camStream.getTracks().forEach((t) => t.stop());
  camStream = null;
  $("video").srcObject = null;
  document.body.classList.remove("cam-live");
  $("btnCamStop").disabled = true;
  log("camera stopped");
};

setInterval(refresh, 500);
