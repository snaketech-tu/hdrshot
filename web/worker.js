// Converts images in the background with the WebAssembly build of
// hdrshot-core (crates/hdrshot-web). The page sends the compiled module
// once, then one job at a time: { id, file, settings, convert }.

import init, { Settings, process, version } from "./pkg/hdrshot_web.js";

const THUMB_WIDTH = 200;
const THUMB_HEIGHT = 112;

let ready;

self.onmessage = async ({ data }) => {
  if (data.module) {
    ready = init({ module_or_path: data.module }).then(() => self.postMessage({ ready: true, version: version() }));
    return;
  }
  await ready;
  const { id, file, settings: values, convert } = data;
  let settings;
  try {
    const bytes = new Uint8Array(await file.arrayBuffer());
    settings = new Settings();
    for (const [key, value] of Object.entries(values)) settings[key] = value;
    const result = process(bytes, settings, THUMB_WIDTH, THUMB_HEIGHT, convert);
    const reply = {
      id,
      width: result.width,
      height: result.height,
      format: result.format,
      hdrInput: result.hdr_input,
      thumb: { width: result.thumb_width, height: result.thumb_height, rgba: result.take_thumb() },
      hdr: result.take_hdr(),
      sdr: result.take_sdr(),
    };
    result.free();
    const transfer = [reply.thumb.rgba.buffer, reply.hdr?.buffer, reply.sdr?.buffer].filter(Boolean);
    self.postMessage(reply, transfer);
  } catch (e) {
    // A RuntimeError is a trap, usually running out of memory; the module
    // can't be trusted after one, so the page replaces this worker.
    self.postMessage({ id, error: e instanceof Error ? e.message : String(e), trapped: e instanceof WebAssembly.RuntimeError });
  } finally {
    try { settings?.free(); } catch { /* freed with a trapped module */ }
  }
};
