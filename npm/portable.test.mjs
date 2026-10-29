import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {decodeObject} from 'pill-packer/object';
import {span,read} from 'pill-packer/layout';
import {pacHeader,expandPAC,indexedPACBounds,mergeFrames} from 'pill-packer/pac';
import {inflateGzip} from 'pill-packer/gzip';
import {unsignedBig,unsignedNumber,shuffleColumns} from 'pill-packer/columns';
import {createCodec} from './index.js';
const bytes=s=>Uint8Array.from(Buffer.from(s,'base64'));
const f=JSON.parse(await readFile(new URL('./fixtures/portable.json',import.meta.url),'utf8'));
const definition=bytes(f.Definition),def=decodeObject(definition);
test('Go PAC compression interoperates with standalone browser and Rust/WASM readers',async()=>{
 const packed=bytes(f.Compressed),expanded=await expandPAC(packed,0,16*1024*1024);
 assert.deepEqual(expanded.bytes,bytes(f.PAC));
 const header=pacHeader(expanded.bytes,0);assert.equal(header.count,3);
 const wasm=await createCodec(await readFile(new URL('./codec.wasm',import.meta.url)));
 const id=wasm.define(definition);wasm.upload(expanded.bytes);
 const indices=wasm.pac(id,0,expanded.bytes.length,3);
 assert.equal(indices.length,6);
 for(let i=0;i<3;i++){
  const member=expanded.bytes.subarray(indices[i*2],indices[i*2+1]);
  const frame=mergeFrames([header.common,member],def);
  assert.deepEqual(frame,bytes(f.Frames[i]));assert.equal(read(frame,def,0),i);assert.match(read(frame,def,1),/^portable codec/);
 }
 wasm.release(id);
});
test('bounded inflation rejects wrong expanded sizes',async()=>{
 const raw=bytes(f.Compressed),view=new DataView(raw.buffer);assert.equal(view.getUint32(4,true),2);
 const n=view.getUint32(8,true);await assert.rejects(inflateGzip(raw.subarray(16),n-1,n));await assert.rejects(inflateGzip(raw.subarray(16),n+1,n));
});
test('column transforms preserve exact large integers and reject unsafe numbers',()=>{
 const raw=new Uint8Array([1,2,3,4,5,6,7,8,9,10,11,12]),original=raw.slice(),spans=[{offset:0,count:3,width:2},{offset:6,count:2,width:3}];
 shuffleColumns(raw,spans);shuffleColumns(raw,spans,true);assert.deepEqual(raw,original);
 assert.throws(()=>shuffleColumns(raw,[{offset:0,count:20,width:8}]));assert.deepEqual(raw,original);
 const large=new Uint8Array(8);new DataView(large.buffer).setBigUint64(0,0xffffffffffffffffn,true);
 assert.equal(unsignedBig(new DataView(large.buffer),0,{width:8,base:0n}),0xffffffffffffffffn);
 assert.throws(()=>unsignedNumber(new DataView(large.buffer),0,{width:8,base:0n}));assert.throws(()=>unsignedBig(new DataView(large.buffer),0,{width:8,base:1n}));
});

test('producer directory indexes members without field validation',async()=>{
 const raw=bytes(f.PAC),header=pacHeader(raw,0),codec=await createCodec(await readFile(new URL('./codec.wasm',import.meta.url))),id=codec.define(definition);
 codec.upload(raw);const expected=codec.pac(id,0,raw.length,header.count);
 codec.upload(raw);assert.deepEqual(codec.pacStructural(id,0,raw.length,header.count),expected);
 const index=new Uint8Array(header.count*16),view=new DataView(index.buffer);
 for(let i=0;i<header.count;i++)view.setUint32(i*16+12,expected[i*2]-header.start,true);
 assert.deepEqual(indexedPACBounds(header,index,16,12),expected);
 view.setUint32(12,1,true);assert.throws(()=>indexedPACBounds(header,index,16,12),/initial/);
 view.setUint32(12,0,true);view.setUint32(28,0,true);assert.throws(()=>indexedPACBounds(header,index,16,12),/offset/);
 assert.throws(()=>indexedPACBounds(header,index.subarray(1),16,12),/shape/);
 codec.release(id);
});
