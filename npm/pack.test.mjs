import {test} from 'node:test';import assert from 'node:assert/strict';import {readFile} from 'node:fs/promises';
import {createCodec} from './index.js';import {define,fill,pack,open,compress,decompress,unpack,push,pop,absent,nullValue} from './portable/pack.js';
const codec=await createCodec(await readFile(new URL('./codec.wasm',import.meta.url)));
const def=await define(codec.definitionFromJSON(JSON.stringify({pil:0,definition:'pack-test',version:0,fields:[{name:'n',type:'U16'},{name:'text',type:'STRING'}]})),codec);
const records=[fill(def,[new Uint8Array([1,0]),new TextEncoder().encode('same')]),fill(def,[new Uint8Array([2,0]),new TextEncoder().encode('same')])];
for(const index of [false,{addressBytes:2},{addressBytes:3},{addressBytes:4}])for(const commonValues of [false,true])test(`PAC byte API ${JSON.stringify(index)} common=${commonValues}`,async()=>{
 const p=pack(def,records,{index,packing:{commonValues},headers:{Producer:new Uint8Array([42]),Empty:new Uint8Array()}});
 assert.deepEqual(unpack(p).records,records);assert.deepEqual(p.get(1),records[1]);assert.throws(()=>p.get(2));assert.deepEqual(open(def,p.bytes).get(0),records[0]);
 codec.upload(p.bytes);const spans=codec.pac(def.id,0,p.bytes.length,2);assert.equal(spans[0],p.header.start);
 codec.upload(p.bytes);assert.deepEqual(codec.pacStructural(def.id,0,p.bytes.length,2),spans);
 const compressed=compress(p,{algorithm:'gzip',level:6});assert.equal(compressed.kind,'cpack');assert.deepEqual((await decompress(compressed)).bytes,p.bytes);
 assert.deepEqual(unpack(pop(push(p,records[0])).pack).records,records);
});
test('fixed PAC avoids directory and validates final width',()=>{
 const p=pack(def,records,{layout:{kind:'fixed'}});assert.equal(p.header.addressBytes,0);assert.equal(p.header.recordWidth,records[0].length);assert.deepEqual(p.get(1),records[1]);
 assert.throws(()=>pack(def,records,{layout:{kind:'fixed',recordWidth:1}}));assert.throws(()=>pack(def,[],{layout:{kind:'fixed'}}));
 assert.equal(pack(def,[],{layout:{kind:'fixed',recordWidth:3}}).header.count,0);
});
test('absent/null and directory corruption remain distinct',()=>{
 const r=fill(def,[absent,nullValue]);assert.notDeepEqual(r,fill(def,[nullValue,absent]));
 const p=pack(def,records,{index:{addressBytes:2}}),bad=p.bytes.slice();bad[p.header.start-4]=1;assert.throws(()=>open(def,bad));
 assert.throws(()=>pack(def,records,{index:{addressBytes:2,offsets:new Uint8Array(4)}}));
 const large=fill(def,[new Uint8Array([0,0]),new Uint8Array(66000)]);assert.throws(()=>pack(def,[large,records[0]],{index:{addressBytes:2}}));
});
