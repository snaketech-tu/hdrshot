// The .zip writer of "Download all": run with `node --test web/tests/*.test.mjs`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { zip } from "../zip.js";

/** The entries of a stored (uncompressed) .zip, read from its central directory. */
function readZip(bytes) {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const endAt = bytes.length - 22;
  assert.equal(view.getUint32(endAt, true), 0x06054b50, "end of central directory");
  const count = view.getUint16(endAt + 10, true);
  let at = view.getUint32(endAt + 16, true);
  const decoder = new TextDecoder();
  const entries = [];
  for (let i = 0; i < count; i++) {
    assert.equal(view.getUint32(at, true), 0x02014b50, "central directory entry");
    const flags = view.getUint16(at + 8, true);
    const method = view.getUint16(at + 10, true);
    const time = view.getUint16(at + 12, true);
    const date = view.getUint16(at + 14, true);
    const crc = view.getUint32(at + 16, true);
    const size = view.getUint32(at + 20, true);
    const nameLength = view.getUint16(at + 28, true);
    const local = view.getUint32(at + 42, true);
    const name = decoder.decode(bytes.subarray(at + 46, at + 46 + nameLength));
    assert.equal(view.getUint32(local, true), 0x04034b50, "local header");
    const localNameLength = view.getUint16(local + 26, true);
    const dataAt = local + 30 + localNameLength + view.getUint16(local + 28, true);
    entries.push({
      name,
      flags,
      method,
      time,
      date,
      crc,
      data: bytes.subarray(dataAt, dataAt + size),
    });
    at += 46 + nameLength + view.getUint16(at + 30, true) + view.getUint16(at + 32, true);
  }
  return entries;
}

test("stores each file with its name, CRC and date", async () => {
  const encoder = new TextEncoder();
  const modified = new Date(2026, 9, 21, 22, 12, 34);
  const blob = zip([
    { name: "shot_hdr.png", bytes: encoder.encode("hello"), modified },
    { name: "Größe é.png", bytes: new Uint8Array([0, 1, 2, 255]), modified: new Date(2026, 0, 1) },
  ]);
  assert.equal(blob.type, "application/zip");
  const entries = readZip(new Uint8Array(await blob.arrayBuffer()));
  assert.deepEqual(
    entries.map((e) => e.name),
    ["shot_hdr.png", "Größe é.png"],
  );
  const [first, second] = entries;
  assert.equal(first.method, 0, "stored, not compressed");
  assert.equal(first.flags & 0x0800, 0x0800, "UTF-8 names");
  assert.equal(first.crc, 0x3610a686, "CRC-32 of 'hello'");
  assert.equal(new TextDecoder().decode(first.data), "hello");
  assert.deepEqual([...second.data], [0, 1, 2, 255]);
  // MS-DOS date and time, to two seconds.
  assert.equal(first.date, ((2026 - 1980) << 9) | (10 << 5) | 21);
  assert.equal(first.time, (22 << 11) | (12 << 5) | 17);
});

test("an empty list is a valid empty .zip", async () => {
  const bytes = new Uint8Array(await zip([]).arrayBuffer());
  assert.equal(bytes.length, 22);
  assert.deepEqual(readZip(bytes), []);
});

test("dates before 1980 are stored as 1980", async () => {
  const blob = zip([{ name: "old.png", bytes: new Uint8Array(1), modified: new Date(1970, 0, 1) }]);
  const [entry] = readZip(new Uint8Array(await blob.arrayBuffer()));
  assert.equal(entry.date >> 9, 0);
});
