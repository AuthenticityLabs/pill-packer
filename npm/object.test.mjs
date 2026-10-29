import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {encodeObject,decodeObject} from './portable/object.js';
import {valueDefinition} from './portable/object.js';
import {createCodec} from './index.js';
test('native objects match Go and Rust bytes including exact integers and integer keys',()=>{
 const value={max:18446744073709551615n,min:-9223372036854775808n,text:'é\0 ',bytes:Uint8Array.of(0,255),empty:[],nested:new Map([[7,[null,false,true,1.5]]])};
 const expected=new Uint8Array(Buffer.from(readFileSync(new URL('./fixtures/object.hex',import.meta.url),'utf8').trim(),'hex'));
 assert.deepEqual(encodeObject(value),expected);
 assert.deepEqual(decodeObject(expected),value);
 for(let n=0;n<expected.length;n++)assert.throws(()=>decodeObject(expected.subarray(0,n)));
});
test('native values reject duplicate keys, excessive depth, unsafe integers and malformed strings',()=>{
 assert.throws(()=>encodeObject(new Map([[1,true],[1n,false]])),/Duplicate/);
 assert.throws(()=>encodeObject(Number.MAX_SAFE_INTEGER+1),/bigint/);
 assert.throws(()=>encodeObject('\ud800'),/surrogate/);
 let value=null;for(let i=0;i<66;i++)value=[value];assert.throws(()=>encodeObject(value),/nesting/);
});
test('published .rx bootstrap matches the browser layout and describes its own PIL encoding',async()=>{
 const raw=new Uint8Array(readFileSync(new URL('../schemas/pil.rx',import.meta.url)));
 assert.deepEqual(decodeObject(raw),valueDefinition);
 assert.deepEqual(encodeObject(valueDefinition),raw);
 const codec=await createCodec(readFileSync(new URL('./codec.wasm',import.meta.url)));
 const handle=codec.define(raw);codec.upload(raw);assert.equal(codec.scan(handle,0,raw.length),raw.length);codec.release(handle);
});

test('optional object properties are absent while array elements must have a PIL value',()=>{
 assert.deepEqual(decodeObject(encodeObject({present:null,absent:undefined})),{present:null});
 assert.throws(()=>encodeObject([undefined]),/Unsupported/);
});
