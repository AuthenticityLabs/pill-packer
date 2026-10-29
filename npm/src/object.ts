/** Native PIL bootstrap values used by Definitions and open objects. */
import { scalar, extended, type PILDefinition } from "./layout.ts";
const limit=16<<20;
const types=["U8","U64","I64","F64","STRING","BYTES","ARRAY<BYTES>"];
export const valueDefinition:PILDefinition={pil:0,definition:"pil/value",version:0,fields:types.map((type,i)=>({name:["kind","unsigned","negative","float","text","bytes","children"][i],type}))};
function uint(n:number):Uint8Array {if(n<65535)return Uint8Array.of(n&255,n>>>8);const b=new Uint8Array(6),v=new DataView(b.buffer);v.setUint16(0,65535,true);v.setUint32(2,n,true);return b;}
function concat(parts:Uint8Array[]):Uint8Array {const size=parts.reduce((n,b)=>n+b.length,0);if(size>limit)throw Error("PIL value exceeds byte limit");const b=new Uint8Array(size);let at=0;for(const p of parts){b.set(p,at);at+=p.length;}return b;}
const encoder=new TextEncoder();
export function encodeObject(value:unknown):Uint8Array {return encode(value,0);}
function encode(value:unknown,depth:number):Uint8Array {
 if(depth>64)throw Error("PIL value nesting limit");
 let kind=0,active=0;let payload:Uint8Array=new Uint8Array(0);
 if(value===null){kind=0;}else if(typeof value==="boolean"){kind=value?2:1;}
 else if(typeof value==="bigint"||typeof value==="number"){
  if(typeof value==="number"&&!Number.isFinite(value))throw Error("Non-finite JSON number");
  if(typeof value==="bigint"||Number.isInteger(value)){
   if(typeof value==="number"&&!Number.isSafeInteger(value))throw Error("Use bigint for exact 64-bit integers");
   const n=BigInt(value);if(n<-(1n<<63n)||n>=(1n<<64n))throw Error("PIL integer overflow");
   active=n<0n?2:1;kind=n<0n?4:3;payload=new Uint8Array(8);const v=new DataView(payload.buffer);if(n<0n)v.setBigInt64(0,n,true);else v.setBigUint64(0,n,true);
  }else {kind=5;active=3;payload=new Uint8Array(8);new DataView(payload.buffer).setFloat64(0,value as number,true);}
 }else if(typeof value==="string"){kind=6;active=4;payload=encoder.encode(value);if(new TextDecoder("utf-8",{fatal:true}).decode(payload)!==value)throw Error("Unpaired UTF-16 surrogate");}
 else if(value instanceof Uint8Array){kind=7;active=5;payload=value;}
 else if(Array.isArray(value)||value instanceof Map||typeof value==="object"){
  active=6;const children:Uint8Array[]=[];
  if(Array.isArray(value)){kind=8;for(const item of value)children.push(encode(item,depth+1));}
  else {kind=9;const entries=value instanceof Map?[...value.entries()]:Object.entries(value as object).filter(([,v])=>v!==undefined);const pairs=entries.map(([k,v])=>{if(typeof k!=="string"&&typeof k!=="bigint"&&!(typeof k==="number"&&Number.isSafeInteger(k)))throw Error("PIL object key must be text or integer");return [encode(k,depth+1),encode(v,depth+1)] as const;});pairs.sort((a,b)=>compare(a[0],b[0]));for(let i=0;i<pairs.length;i++){if(i&&compare(pairs[i-1][0],pairs[i][0])===0)throw Error("Duplicate PIL object key");children.push(...pairs[i]);}}
  let end=0;const offsets=children.map(b=>uint(end+=b.length));payload=concat([uint(children.length),...offsets,...children]);
 }else throw Error("Unsupported PIL value");
 const mask=new Uint8Array([0x80,0]);
 if(active){const state=active<4?2:3;mask[active>>2]|=state<<(6-2*(active%4));}
 return concat([mask,Uint8Array.of(kind),...(active>=4?[uint(payload.length)]:[]),payload]);
}
function compare(a:Uint8Array,b:Uint8Array):number {for(let i=0;i<Math.min(a.length,b.length);i++)if(a[i]!==b[i])return a[i]-b[i];return a.length-b.length;}
export function decodeObject(bytes:Uint8Array):unknown {if(bytes.length>limit)throw Error("PIL value exceeds byte limit");return decode(bytes,0);}
function decode(bytes:Uint8Array,depth:number):unknown {
 if(depth>64||bytes.length<3)throw Error("Invalid PIL value");
 const kind=bytes[2],active=kind<3?0:kind<8?kind-2:kind<10?6:-1;
 if(active<0)throw Error("Unknown PIL value kind");
 const expected=new Uint8Array([0x80,0]);if(active)expected[active>>2]|=(active<4?2:3)<<(6-2*(active%4));
 if(bytes[0]!==expected[0]||bytes[1]!==expected[1])throw Error("Invalid PIL value fields");
 let at=3,size=active?8:0;
 if(active>=4){[size,at]=extended(new DataView(bytes.buffer,bytes.byteOffset,bytes.length),at);}
 if(at+size!==bytes.length)throw Error("PIL value boundary mismatch");
 if(kind===0)return null;if(kind===1)return false;if(kind===2)return true;
 const payload=bytes.subarray(at);
 if(kind<8){const value=scalar(payload,types[active]);if(kind===4&&BigInt(value as number|bigint)>=0n)throw Error("Invalid negative PIL value");return value;}
 const view=new DataView(payload.buffer,payload.byteOffset,payload.length);
 let [count,cursor]=extended(view,0);
 if(count>Math.floor((payload.length-cursor)/2))throw Error("Array count exceeds boundary");
 const ends:number[]=[];let previous=0;
 for(let i=0;i<count;i++){let end;[end,cursor]=extended(view,cursor);if(end<previous)throw Error("Decreasing PIL offsets");ends.push(end);previous=end;}
 if(cursor+previous!==payload.length)throw Error("PIL array boundary mismatch");
 const children=ends.map((end,i)=>payload.subarray(cursor+(i?ends[i-1]:0),cursor+end));
 if(kind===8)return children.map(b=>decode(b,depth+1));
 if(children.length%2)throw Error("Unpaired PIL object member");
 const map=new Map<unknown,unknown>();let allText=true;
 for(let i=0;i<children.length;i+=2){const key=decode(children[i],depth+1);if(typeof key!=="string"&&typeof key!=="bigint"&&!(typeof key==="number"&&Number.isSafeInteger(key)))throw Error("PIL object key must be text or integer");if(map.has(key))throw Error("Duplicate PIL object key");allText&&=typeof key==="string";map.set(key,decode(children[i+1],depth+1));}
 return allText?Object.fromEntries(map as Map<string,unknown>):map;
}
