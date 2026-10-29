import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {readFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
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
  const id = c.define(await readFile(new URL('definition.rx', base)));
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

test('nested CDDL schemas share exact Rust/Go encoding and reject invalid children', async () => {
  const read=name=>new Uint8Array(readFileSync(new URL(`../rust/tests/fixtures/schema/${name}`,import.meta.url)));
  const codec=await createCodec(readFileSync(new URL('./codec.wasm',import.meta.url)));
  const id=codec.define(read('purchase.rx'));
  try {
    const frame=codec.encodeJSON(id,read('record.json'));
    assert.deepEqual(frame,read('record.pil'));
    assert.match(codec.decodeJSON(id,frame),/18446744073709551615/);
    codec.upload(frame);assert.equal(codec.scan(id,0,frame.length),frame.length);
    const invalid=read('invalid-child.pil');codec.upload(invalid);
    assert.throws(()=>codec.scan(id,0,invalid.length),/schema/);
    assert.throws(()=>codec.decodeJSON(id,invalid),/schema/);
    const pac=body=>{
      const raw=new Uint8Array(75+body.length),v=new DataView(raw.buffer);
      v.setUint32(0,raw.length,true);v.setUint32(4,4,true);v.setBigUint64(8,BigInt(raw.length),true);v.setBigUint64(16,75n,true);v.setBigUint64(24,1n,true);
      raw.set(createHash('sha256').update(read('purchase.rx')).digest(),32);raw.set(body,75);return raw;
    };
    const good=pac(frame);codec.upload(good);codec.pac(id,0,good.length,1);
    const bad=pac(invalid);codec.upload(bad);assert.throws(()=>codec.pac(id,0,bad.length,1),/schema/);
    assert.throws(()=>codec.encodeJSON(id,'{"id":1,"customer":{},"items":[],"kind":"purchase"}'));
  } finally {codec.release(id)}
});

test('DOC open maps and string dictionaries share native schema encoding',async()=>{
 const read=name=>new Uint8Array(readFileSync(new URL(`../rust/tests/fixtures/authoring/${name}`,import.meta.url)));
 const codec=await createCodec(wasm),id=codec.define(read('record.rx'));
 try{
  const raw=codec.encodeJSON(id,read('record.json'));
  assert.deepEqual(raw,read('record.pil'));
  assert.deepEqual(JSON.parse(codec.decodeJSON(id,raw)),JSON.parse(new TextDecoder().decode(read('record.json'))));
  const value={state:'unseen',count:99,extra:{nested:['new',false]}};
  assert.deepEqual(JSON.parse(codec.decodeJSON(id,codec.encodeJSON(id,JSON.stringify(value)))),value);
  assert.throws(()=>codec.encodeJSON(id,JSON.stringify({...value,count:256})),/schema|range|encode/i);
 }finally{codec.release(id);}
});
