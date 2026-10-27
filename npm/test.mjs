import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {createCodec} from './index.js';
const wasm = await readFile(new URL('./codec.wasm', import.meta.url));
test('JSON import, standalone PIL scan, explicit definition release', async () => {
  const c = await createCodec(wasm);
  const raw = c.definitionFromJSON('{"pil":0,"definition":"example","version":0,"fields":[{"name":"n","type":"U16"}],"vendor":{"anything":true}}');
  const id = c.define(raw);
  c.upload(new Uint8Array([0x80, 42, 0]));
  assert.equal(c.scan(id, 0, 3), 3);
  assert.equal(c.scan(id, 0, 2), 0);
  c.release(id);
  assert.throws(() => c.scan(id, 0, 3));
  assert.throws(() => c.definitionFromJSON('{"pil":0}'));
});
test('PAC fixture and explicit host extension support', async () => {
  const c = await createCodec(wasm);
  const base = new URL('../rust/tests/fixtures/pac/', import.meta.url);
  const id = c.define(await readFile(new URL('definition.cbor', base)));
  const pac = await readFile(new URL('packed.pac', base));
  c.upload(pac);
  assert.equal(c.pac(id, 0, pac.length, 16).length, 32);
  assert.throws(() => c.pac(id, 0, pac.length, 17), /count mismatch/);
  const bad = Uint8Array.from(pac); bad[32] ^= 1; c.upload(bad);
  assert.throws(() => c.pac(id, 0, bad.length, 16), /definition binding/);
});

test('structural scan is explicit, bounded, and keeps the validating API',async()=>{
 const c=await createCodec(wasm),id=c.define(c.definitionFromJSON('{"pil":0,"definition":"structural","version":0,"fields":[{"name":"text","type":"STRING"}]}'));
 // One offset field with an invalid UTF-8 payload: framing is valid.
 c.upload(Uint8Array.of(0xc0,1,0,255));
 assert.equal(c.scanStructural(id,0,4),4);assert.throws(()=>c.scan(id,0,4));assert.equal(c.scanStructural(id,0,3),0);
 c.release(id);
});
test('CRC32C verification handles corruption and the complete input extent',async()=>{
 const c=await createCodec(wasm),raw=new TextEncoder().encode('123456789');
 c.verifyCRC32C(raw,0xe3069283);c.verifyCRC32C(new Uint8Array(),0);
 raw[3]^=1;assert.throws(()=>c.verifyCRC32C(raw,0xe3069283),/checksum/);
 assert.throws(()=>c.verifyCRC32C(raw,-1),/Invalid CRC/);
});
