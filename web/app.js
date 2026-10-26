// The browser version of hdrshot. Images are read and converted in Web
// Workers (worker.js) by the same Rust code as the desktop app; nothing is
// uploaded anywhere.

import { zip, MAX_ZIP_BYTES } from "./zip.js";

const $ = (id) => document.getElementById(id);

const EXTENSIONS = ["jxr", "wdp", "exr", "hdr", "jxl", "png", "jpg", "jpeg", "heic", "heif", "hif"];
const DEFAULTS = {
  mode: "both",
  hdr_format: 0,
  for_discord: false,
  hdr_peak: 0,
  sdr_jpeg: false,
  sdr_white: 80,
  max_height: 0,
  save: "download",
};
const HDR_KINDS = [
  { name: "HDR PNG", ext: "png", type: "image/png" },
  { name: "HDR AVIF", ext: "avif", type: "image/avif" },
  { name: "Ultra HDR JPEG", ext: "jpg", type: "image/jpeg" },
];
const SDR_KINDS = {
  png: { name: "SDR PNG", ext: "png", type: "image/png" },
  jpeg: { name: "SDR JPEG", ext: "jpg", type: "image/jpeg" },
};
const FORMAT_HELP = [
  "Shown in HDR by Windows Photos, Chrome and Edge. Lossless, so the files are large.",
  "About a twentieth of the size of a PNG. Slow here: about 25 seconds for a 4K image.",
  "A JPEG that is SDR in every viewer and HDR in those that read gain maps.",
];

// Per-visitor conveniences; the page works the same without storage.
const store = {
  get(key, fallback) {
    try {
      const value = localStorage.getItem(`hdrshot.${key}`);
      return value === null ? fallback : JSON.parse(value);
    } catch {
      return fallback;
    }
  },
  set(key, value) {
    try {
      localStorage.setItem(`hdrshot.${key}`, JSON.stringify(value));
    } catch {
      // Storage is blocked or full; the setting lasts until the tab closes.
    }
  },
};

let settings = { ...DEFAULTS, ...store.get("settings", {}) };
const canPickFolder = typeof window.showDirectoryPicker === "function";
if (!canPickFolder) settings.save = "download";
let folder = null;

/** Every image in the list, in order. */
let items = [];
let nextId = 1;
/** The images of the current Convert click, to count "3 of 5". */
let run = [];

// ---------------------------------------------------------------- workers

const MAX_WORKERS = Math.max(
  1,
  Math.min((navigator.hardwareConcurrency || 2) - 1, navigator.deviceMemory && navigator.deviceMemory <= 4 ? 2 : 4),
);
const wasmUrl = new URL("pkg/hdrshot_web_bg.wasm", import.meta.url);
const workerUrl = new URL("worker.js", import.meta.url);
let module = null;
const pool = [];
/** Jobs waiting for a worker: { item, convert, settings }. */
let queue = [];
let broken = null;

function loadModule() {
  module ??= (async () => {
    const response = fetch(wasmUrl);
    try {
      return await WebAssembly.compileStreaming(response);
    } catch {
      // Served without the application/wasm type, or no streaming support.
      return WebAssembly.compile(await (await fetch(wasmUrl)).arrayBuffer());
    }
  })();
  return module;
}

function spawn() {
  const slot = { worker: new Worker(workerUrl, { type: "module" }), job: null, ready: false };
  slot.worker.onmessage = ({ data }) => {
    if (data.ready) {
      slot.ready = true;
      $("version").textContent = `Version ${data.version}`;
      pump();
    } else {
      finish(slot, data);
    }
  };
  slot.worker.onerror = (event) => {
    event.preventDefault();
    fail(slot, "The converter could not start in this browser. Use a current version of Chrome, Edge, Firefox or Safari, or the desktop app.");
  };
  loadModule().then(
    (m) => slot.worker.postMessage({ module: m }),
    () => fail(slot, "The converter could not be loaded. Check the connection and reload the page."),
  );
  pool.push(slot);
  return slot;
}

/** Stops a worker that can't go on and fails what it was doing. */
function fail(slot, message) {
  slot.worker.terminate();
  pool.splice(pool.indexOf(slot), 1);
  if (slot.job) failJob(slot.job, message);
  if (!slot.ready && !pool.some((s) => s.ready)) {
    broken = message;
    for (const job of queue) failJob(job, message);
    queue = [];
  } else if (queue.length && pool.length === 0) {
    spawn();
  }
  pump();
}

function failJob(job, message) {
  job.item.status = "error";
  job.item.error = message;
  renderRow(job.item);
}

function enqueue(item, convert, values) {
  if (broken) {
    failJob({ item }, broken);
    return;
  }
  queue = queue.filter((job) => job.item !== item);
  queue.push({ item, convert, settings: values });
  pump();
}

function pump() {
  while (queue.length) {
    let slot = pool.find((s) => s.ready && !s.job);
    if (!slot) {
      // Start another worker if all are busy, unless one is still loading.
      if (pool.length < MAX_WORKERS && pool.every((s) => s.ready)) spawn();
      return;
    }
    // Conversions go first; thumbnails of new images fill the gaps.
    const index = Math.max(0, queue.findIndex((job) => job.convert));
    const job = queue.splice(index, 1)[0];
    slot.job = job;
    if (job.convert) job.item.status = "converting";
    renderRow(job.item);
    slot.worker.postMessage({ id: job.item.id, file: job.item.file, settings: workerSettings(job.settings), convert: job.convert });
  }
  update();
}

async function finish(slot, data) {
  if (data.trapped) {
    fail(slot, "Ran out of memory converting this image. Close other tabs and try again, or use the desktop app.");
    return;
  }
  const job = slot.job;
  slot.job = null;
  pump();
  const item = job.item;
  if (!items.includes(item)) return;
  if (data.error) {
    failJob(job, sentence(data.error));
    update();
    return;
  }
  item.width = data.width;
  item.height = data.height;
  item.hdrInput = data.hdrInput;
  if (!item.thumb) item.thumb = await thumbnail(data.thumb);
  if (job.convert) {
    const hdrKind = HDR_KINDS[job.settings.hdr_format];
    const sdrKind = SDR_KINDS[job.settings.sdr_jpeg ? "jpeg" : "png"];
    const base = item.name.replace(/\.[^.]+$/, "");
    item.outputs = [
      data.hdr && output(data.hdr, hdrKind, `${base}_hdr.${hdrKind.ext}`),
      data.sdr && output(data.sdr, sdrKind, `${base}_sdr.${sdrKind.ext}`),
    ].filter(Boolean);
    item.status = "done";
    if (job.settings.save === "folder" && folder) {
      try {
        for (const file of item.outputs) file.savedAs = await saveToFolder(folder, file);
      } catch (e) {
        item.status = "error";
        item.error = `Could not save to ${folder.name}: ${e.message}`;
      }
    }
  } else if (item.status === "reading") {
    item.status = "ready";
  }
  renderRow(item);
  update();
}

function workerSettings(values) {
  return {
    hdr: values.mode !== "sdr",
    sdr: values.mode !== "hdr",
    hdr_format: values.hdr_format,
    for_discord: values.for_discord && values.hdr_format === 0,
    hdr_peak: values.hdr_peak,
    sdr_jpeg: values.sdr_jpeg,
    sdr_white: values.sdr_white,
    max_height: values.max_height,
  };
}

function output(bytes, kind, name) {
  const blob = new Blob([bytes], { type: kind.type });
  return { kind, name, blob, size: blob.size, url: null };
}

async function thumbnail({ width, height, rgba }) {
  const canvas = document.createElement("canvas");
  canvas.width = width;
  canvas.height = height;
  canvas.getContext("2d").putImageData(new ImageData(new Uint8ClampedArray(rgba.buffer), width, height), 0, 0);
  const blob = await new Promise((resolve) => canvas.toBlob(resolve, "image/jpeg", 0.85));
  return blob ? URL.createObjectURL(blob) : canvas.toDataURL();
}

/** Writes `file` into the chosen folder, next to any file of that name. */
async function saveToFolder(dir, file) {
  const dot = file.name.lastIndexOf(".");
  for (let n = 1; ; n++) {
    const name = n === 1 ? file.name : `${file.name.slice(0, dot)} (${n})${file.name.slice(dot)}`;
    try {
      await dir.getFileHandle(name);
      continue;
    } catch (e) {
      if (e.name !== "NotFoundError") throw e;
    }
    const handle = await dir.getFileHandle(name, { create: true });
    const writable = await handle.createWritable();
    await writable.write(file.blob);
    await writable.close();
    return name;
  }
}

// ---------------------------------------------------------------- adding

function extension(name) {
  const dot = name.lastIndexOf(".");
  return dot < 0 ? "" : name.slice(dot + 1).toLowerCase();
}

/**
 * Whether a JPEG found in a folder looks like a phone HDR photo: it has an
 * MPF index or Ultra HDR XMP, and wasn't written by hdrshot. The same test
 * as the desktop app's folder scan.
 */
async function hasGainMap(file) {
  const head = new Uint8Array(await file.slice(0, 256 * 1024).arrayBuffer());
  const text = new TextDecoder("latin1").decode(head);
  return (text.includes("MPF\0") || text.includes("hdrgm:")) && !text.includes('x:xmptk="hdrshot"');
}

/** Adds `found`, each { file, inFolder }, and says what was left out. */
async function addFiles(found) {
  const known = new Set(items.map((i) => `${i.name}|${i.size}|${i.file.lastModified}`));
  let unsupported = 0;
  let folderPngs = 0;
  const added = [];
  for (const { file, inFolder } of found) {
    const ext = extension(file.name);
    if (!EXTENSIONS.includes(ext)) {
      unsupported++;
      continue;
    }
    // A folder of captures often holds the PNGs made from them, so PNGs
    // are only taken when added by name, as in the desktop app. JPEGs are
    // taken from folders only if they are phone HDR photos.
    if (ext === "png" && inFolder) {
      folderPngs++;
      continue;
    }
    if ((ext === "jpg" || ext === "jpeg") && inFolder && !(await hasGainMap(file))) {
      unsupported++;
      continue;
    }
    const key = `${file.name}|${file.size}|${file.lastModified}`;
    if (known.has(key)) continue;
    known.add(key);
    added.push(file);
  }
  added.sort((a, b) => a.name.localeCompare(b.name, undefined, { numeric: true }));
  const notes = [];
  if (unsupported) notes.push(`Left out ${plural(unsupported, "file")} that hdrshot can't convert.`);
  if (folderPngs) notes.push(`Left out ${plural(folderPngs, "PNG")} found in folders; add HDR PNGs by name.`);
  showMessage(notes.join(" "));
  for (const file of added) {
    const item = { id: nextId++, file, name: file.name, size: file.size, status: "reading", outputs: [] };
    items.push(item);
    $("rows").append(createRow(item));
    enqueue(item, false, settings);
  }
  update();
}

async function filesFromDrop(dataTransfer) {
  // Entries must be taken while the drop event runs.
  const entries = [...dataTransfer.items].map((i) => i.webkitGetAsEntry?.()).filter(Boolean);
  if (!entries.length) return [...dataTransfer.files].map((file) => ({ file, inFolder: false }));
  const found = [];
  const walk = async (entry, inFolder) => {
    if (entry.isFile) {
      found.push({ file: await new Promise((resolve, reject) => entry.file(resolve, reject)), inFolder });
    } else if (entry.isDirectory) {
      const reader = entry.createReader();
      for (;;) {
        const batch = await new Promise((resolve, reject) => reader.readEntries(resolve, reject));
        if (!batch.length) break;
        for (const child of batch) await walk(child, true);
      }
    }
  };
  for (const entry of entries) await walk(entry, false);
  return found;
}

// ---------------------------------------------------------------- rows

function createRow(item) {
  const row = document.createElement("div");
  row.className = "row";
  row.innerHTML = `<button class="thumb" disabled><div class="ph"></div></button>
    <div class="info"><div class="name"></div><div class="meta num"></div></div>
    <div class="end"></div>`;
  row.querySelector(".name").textContent = item.name;
  row.querySelector(".name").title = item.name;
  row.querySelector(".thumb").addEventListener("click", () => showViewer(item));
  row.querySelector(".end").addEventListener("click", (event) => {
    const button = event.target.closest("button");
    if (!button) return;
    if (button.dataset.download !== undefined) download(item.outputs[Number(button.dataset.download)]);
    if (button.classList.contains("remove")) removeItem(item);
  });
  item.row = row;
  renderRow(item);
  return row;
}

function renderRow(item) {
  const row = item.row;
  if (!row) return;
  const thumb = row.querySelector(".thumb");
  if (item.thumb && !thumb.querySelector("img")) thumb.innerHTML = `<img src="${item.thumb}" alt="">`;
  thumb.disabled = item.status !== "done";
  thumb.setAttribute("aria-label", `Show ${item.name} larger`);

  const meta = row.querySelector(".meta");
  meta.classList.toggle("error", item.status === "error");
  if (item.status === "error") {
    meta.textContent = item.error;
  } else if (item.status === "done") {
    const parts = item.outputs.map((o) => `${o.kind.name} ${megabytes(o.size)}`);
    if (!item.hdrInput) parts.push("the original is SDR");
    meta.textContent = parts.join(" · ");
  } else {
    meta.textContent = [item.width && `${item.width} × ${item.height}`, extension(item.name).toUpperCase(), megabytes(item.size)]
      .filter(Boolean)
      .join(" · ");
  }

  const end = row.querySelector(".end");
  const remove = `<button class="remove" aria-label="Remove ${escapeHtml(item.name)}" title="Remove"><svg width="14" height="14" aria-hidden="true"><use href="#i-x"/></svg></button>`;
  const status = (icon, spin = false) =>
    `<svg class="status${spin ? " spin" : ""}" width="22" height="22" aria-hidden="true"><use href="#${icon}"/></svg>`;
  if (item.status === "done") {
    const buttons = item.outputs
      .map(
        (o, i) =>
          `<button class="dl" data-download="${i}" title="Download ${escapeHtml(o.name)}"><svg width="13" height="13" aria-hidden="true"><use href="#i-down"/></svg>${o.kind.name}</button>`,
      )
      .join("");
    end.innerHTML = buttons + status("i-check");
  } else if (item.status === "converting") {
    end.innerHTML = status("i-spin", true);
  } else if (item.status === "queued") {
    end.innerHTML = status("i-wait");
  } else if (item.status === "error") {
    end.innerHTML = remove + status("i-error");
  } else {
    end.innerHTML = remove + status("i-wait");
  }
}

function removeItem(item) {
  items = items.filter((i) => i !== item);
  queue = queue.filter((job) => job.item !== item);
  release(item);
  item.row.remove();
  update();
}

function release(item) {
  if (item.thumb?.startsWith("blob:")) URL.revokeObjectURL(item.thumb);
  for (const o of item.outputs) if (o.url) URL.revokeObjectURL(o.url);
}

function clearAll() {
  for (const item of items) release(item);
  items = [];
  run = [];
  queue = [];
  $("rows").replaceChildren();
  showMessage("");
  update();
}

// ---------------------------------------------------------------- saving

function download(file) {
  file.url ??= URL.createObjectURL(file.blob);
  const a = document.createElement("a");
  a.href = file.url;
  a.download = file.name;
  document.body.append(a);
  a.click();
  a.remove();
}

async function downloadAll() {
  const done = items.filter((i) => i.status === "done");
  const files = done.flatMap((item) => item.outputs.map((o) => ({ ...o, modified: new Date(item.file.lastModified) })));
  if (files.length === 1) {
    download(files[0]);
    return;
  }
  const total = files.reduce((sum, f) => sum + f.size, 0);
  if (total > MAX_ZIP_BYTES - files.length * 200) {
    showMessage("These files are too large for one .zip. Download them one by one with the buttons on each row.");
    return;
  }
  const button = $("zip");
  button.disabled = true;
  $("zip-label").textContent = "Making the .zip";
  try {
    const names = new Set();
    const entries = [];
    for (const f of files) {
      let name = f.name;
      for (let n = 2; names.has(name.toLowerCase()); n++) name = f.name.replace(/(\.[^.]+)$/, ` (${n})$1`);
      names.add(name.toLowerCase());
      entries.push({ name, bytes: new Uint8Array(await f.blob.arrayBuffer()), modified: f.modified });
    }
    const blob = zip(entries);
    const file = { name: "hdrshot.zip", blob };
    download(file);
    setTimeout(() => URL.revokeObjectURL(file.url), 60_000);
  } catch (e) {
    showMessage(`Could not make the .zip: ${e.message}`);
  } finally {
    button.disabled = false;
    $("zip-label").textContent = "Download all as .zip";
  }
}

async function pickFolder() {
  try {
    folder = await window.showDirectoryPicker({ id: "hdrshot", mode: "readwrite", startIn: "pictures" });
    return true;
  } catch {
    return false;
  }
}

// ---------------------------------------------------------------- converting

async function convert() {
  const todo = items.filter((i) => i.status === "reading" || i.status === "ready");
  if (!todo.length) return;
  $("settings").hidden = true;
  $("c-settings").setAttribute("aria-expanded", "false");
  if (settings.save === "folder" && !folder) {
    if (!(await pickFolder())) return;
    renderSettings();
  }
  const values = { ...settings };
  run = run.filter((i) => items.includes(i) && (i.status === "queued" || i.status === "converting"));
  for (const item of todo) {
    item.status = "queued";
    run.push(item);
    renderRow(item);
    enqueue(item, true, values);
  }
  update();
}

// ---------------------------------------------------------------- page

function update() {
  const count = (status) => items.filter((i) => i.status === status).length;
  const done = count("done");
  const failed = count("error");
  const waiting = count("reading") + count("ready");
  const busy = count("queued") + count("converting");
  const finished = items.length > 0 && done > 0 && waiting === 0 && busy === 0;

  $("drop").hidden = items.length > 0;
  $("rows").hidden = items.length === 0;
  $("list-actions").hidden = items.length === 0;
  $("clear").disabled = busy > 0;
  $("count").textContent = items.length === 0 ? "" : done ? `${plural(items.length, "image")}, ${done} converted` : plural(items.length, "image");

  $("result").hidden = !finished;
  $("controls").hidden = finished;
  $("convert-row").hidden = finished;
  if (finished) {
    const files = items.flatMap((i) => i.outputs);
    const total = files.reduce((sum, f) => sum + f.size, 0);
    $("result-title").textContent =
      failed ? `Converted ${done} of ${plural(done + failed, "image")}` : `Converted ${plural(done, "image")}`;
    const saved = files.length && files.every((f) => f.savedAs) ? `, saved to ${folder.name}` : "";
    $("result-detail").textContent = `${megabytes(total)} in ${plural(files.length, "file")}${saved}`;
    $("zip").hidden = Boolean(saved);
    $("zip-label").textContent = files.length === 1 ? `Download ${files[0].kind.name}` : "Download all as .zip";
    $("discord-note").hidden = !files.some((f) => f.kind === HDR_KINDS[0]);
  }

  const button = $("convert");
  if (busy) {
    const position = run.filter((i) => i.status === "done" || i.status === "error").length + 1;
    button.textContent = `Converting ${Math.min(position, run.length)} of ${run.length}`;
    button.disabled = true;
  } else {
    button.textContent = waiting ? `Convert ${plural(waiting, "image")}` : "Convert";
    button.disabled = waiting === 0;
  }
  $("hint").textContent = busy
    ? "Keep this tab open until all images are done."
    : items.length === 0
      ? "Add images to start."
      : settings.hdr_format === 1 && settings.mode !== "sdr"
        ? "AVIF is slow in the browser: about 25 seconds for a 4K image."
        : "";
}

function showMessage(text) {
  $("message").textContent = text;
  $("message").hidden = !text;
}

function showViewer(item) {
  const hdr = item.outputs.find((o) => o.kind !== SDR_KINDS.png && o.kind !== SDR_KINDS.jpeg);
  const file = hdr ?? item.outputs[0];
  if (!file) return;
  file.url ??= URL.createObjectURL(file.blob);
  $("viewer-title").textContent = file.name;
  $("viewer-image").src = file.url;
  $("viewer-note").hidden = !hdr;
  $("viewer").showModal();
}

// ---------------------------------------------------------------- settings

function renderSettings() {
  const hdr = HDR_KINDS[settings.hdr_format];
  const hdrName = settings.hdr_format === 0 && settings.for_discord ? "HDR PNG for Discord" : hdr.name;
  const sdrName = SDR_KINDS[settings.sdr_jpeg ? "jpeg" : "png"].name;
  const output = $("c-output");
  output.innerHTML = "";
  for (const [value, label] of [["both", `${hdrName} + ${sdrName}`], ["hdr", hdrName], ["sdr", sdrName]]) {
    output.add(new Option(label, value, false, settings.mode === value));
  }
  $("c-size").value = String(settings.max_height);
  const save = $("c-save");
  save.querySelector('[value="folder"]').textContent = folder ? `Save to ${folder.name}` : "Save to a folder";
  save.value = settings.save;
  $("s-format").value = String(settings.hdr_format);
  $("s-format-help").textContent = FORMAT_HELP[settings.hdr_format];
  $("s-sdr").value = settings.sdr_jpeg ? "jpeg" : "png";
  $("s-white").value = settings.sdr_white;
  $("s-peak").value = settings.hdr_peak || "";
  $("s-discord").checked = settings.for_discord;
  $("s-discord").disabled = settings.hdr_format !== 0;
  update();
}

function changeSettings(changes) {
  settings = { ...settings, ...changes };
  store.set("settings", settings);
  renderSettings();
}

function number(input, fallback, min, max) {
  const value = Number(input.value);
  return input.value !== "" && Number.isFinite(value) && value >= min && value <= max ? value : fallback;
}

// ---------------------------------------------------------------- helpers

function plural(n, word) {
  return `${n} ${word}${n === 1 ? "" : "s"}`;
}

function megabytes(bytes) {
  return `${(bytes / 1e6).toFixed(1)} MB`;
}

function sentence(text) {
  return text.charAt(0).toUpperCase() + text.slice(1);
}

function escapeHtml(text) {
  return text.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);
}

// ---------------------------------------------------------------- wiring

if (!canPickFolder) $("c-save").querySelector('[value="folder"]').remove();
if (store.get("noticeHidden", false)) $("notice").hidden = true;
$("notice-close").addEventListener("click", () => {
  $("notice").hidden = true;
  store.set("noticeHidden", true);
});

$("choose").addEventListener("click", () => $("picker").click());
$("add").addEventListener("click", () => $("picker").click());
$("picker").addEventListener("change", (event) => {
  addFiles([...event.target.files].map((file) => ({ file, inFolder: false })));
  event.target.value = "";
});
$("clear").addEventListener("click", clearAll);
$("more").addEventListener("click", clearAll);
$("zip").addEventListener("click", downloadAll);
$("convert").addEventListener("click", convert);

let dragDepth = 0;
const dragsFiles = (event) => event.dataTransfer?.types.includes("Files");
window.addEventListener("dragenter", (event) => {
  if (!dragsFiles(event)) return;
  dragDepth++;
  document.body.classList.add("dragging");
});
window.addEventListener("dragleave", () => {
  dragDepth = Math.max(0, dragDepth - 1);
  if (dragDepth === 0) document.body.classList.remove("dragging");
});
window.addEventListener("dragover", (event) => {
  if (!dragsFiles(event)) return;
  event.preventDefault();
  event.dataTransfer.dropEffect = "copy";
});
window.addEventListener("drop", async (event) => {
  if (!dragsFiles(event)) return;
  event.preventDefault();
  dragDepth = 0;
  document.body.classList.remove("dragging");
  addFiles(await filesFromDrop(event.dataTransfer));
});

$("c-output").addEventListener("change", (e) => changeSettings({ mode: e.target.value }));
$("c-size").addEventListener("change", (e) => changeSettings({ max_height: Number(e.target.value) }));
$("c-save").addEventListener("change", async (e) => {
  if (e.target.value === "folder" && !(await pickFolder())) {
    changeSettings({ save: folder ? "folder" : "download" });
    return;
  }
  changeSettings({ save: e.target.value });
});
$("c-settings").addEventListener("click", (e) => {
  const panel = $("settings");
  panel.hidden = !panel.hidden;
  e.currentTarget.setAttribute("aria-expanded", String(!panel.hidden));
});
$("s-format").addEventListener("change", (e) => changeSettings({ hdr_format: Number(e.target.value) }));
$("s-sdr").addEventListener("change", (e) => changeSettings({ sdr_jpeg: e.target.value === "jpeg" }));
$("s-white").addEventListener("change", (e) => changeSettings({ sdr_white: number(e.target, DEFAULTS.sdr_white, 40, 1000) }));
$("s-peak").addEventListener("change", (e) => changeSettings({ hdr_peak: number(e.target, 0, 100, 10000) }));
$("s-discord").addEventListener("change", (e) => changeSettings({ for_discord: e.target.checked }));
$("s-reset").addEventListener("click", () => changeSettings({ ...DEFAULTS, save: settings.save }));

$("viewer-close").addEventListener("click", () => $("viewer").close());
$("viewer").addEventListener("click", (event) => {
  if (event.target === event.currentTarget) event.currentTarget.close();
});

renderSettings();
// Load the converter right away, so the first image starts without a wait.
spawn();
