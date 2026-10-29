import {decodeObject} from './object.ts';
import {span,width,type PILDefinition} from './layout.ts';
import {pacHeader,memberBounds,mergeFrames,expandPAC,type PACHeader} from './pac.ts';
export interface Codec { define(bytes:Uint8Array):number; upload(bytes:Uint8Array):void; scan(id:number,start:number,length:number):number; scanStructural(id:number,start:number,length:number):number; definitionFromJSON(text:string|Uint8Array):Uint8Array; deflate(bytes:Uint8Array,level:number):Uint8Array; }
export type Definition={bytes:Uint8Array;digest:Uint8Array;layout:PILDefinition;id:number;codec:Codec};
export const absent=Symbol('absent'),nullValue=Symbol('null');
export const rx={write:(codec:Codec,json:string|Uint8Array)=>codec.definitionFromJSON(json),read:(bytes:Uint8Array)=>decodeObject(bytes)};
export async function define(bytes:Uint8Array,codec:Codec):Promise<Definition>{
 const owned=Uint8Array.from(bytes),layout=decodeObject(owned) as PILDefinition;
 if(!Array.isArray(layout.fields))throw Error('PIL Definition has no positional fields');
 return {bytes:owned,digest:new Uint8Array(await crypto.subtle.digest('SHA-256',owned)),layout,id:codec.define(owned),codec};
}
function join(parts:Uint8Array[]):Uint8Array{const out=new Uint8Array(parts.reduce((n,p)=>n+p.length,0));let at=0;for(const p of parts){out.set(p,at);at+=p.length;}return out;}
function ext(n:number):Uint8Array{if(!Number.isSafeInteger(n)||n<0||n>0xfffffffe)throw Error('PIL size overflow');const out=new Uint8Array(n<65535?2:6),v=new DataView(out.buffer);v.setUint16(0,n<65535?n:65535,true);if(n>=65535)v.setUint32(2,n,true);return out;}
function encode(def:PILDefinition,values:({state:number;bytes:Uint8Array})[]):Uint8Array{
 const mask=new Uint8Array(Math.ceil(values.length/4));let count=0;
 const bits=values.map((v,i)=>v.state===2&&!def.fields[i].values?.length&&['16','32'].includes(def.adaptive_integers??'')?(/^[IU](32|64)$/.test(def.fields[i].type)?2:def.adaptive_integers==='16'&&/^[IU]16$/.test(def.fields[i].type)?1:0):0);
 for(let i=0;i<values.length;i++){mask[i>>2]|=values[i].state<<(6-2*(i%4));count+=bits[i];}
 const plane=new Uint8Array(Math.ceil(count/8));let bit=0,end=0;const offsets:Uint8Array[]=[];
 for(let i=0;i<values.length;i++){const v=values[i];if(bits[i]){const code=Math.log2(v.bytes.length);if(!Number.isInteger(code)||code>=(1<<bits[i]))throw Error('Invalid direct width');for(let b=bits[i]-1;b>=0;b--){plane[bit>>3]|=((code>>b)&1)<<(7-bit%8);bit++;}}if(v.state===3){end+=v.bytes.length;offsets.push(ext(end));}}
 return join([mask,plane,...values.filter(v=>v.state===2).map(v=>v.bytes),...offsets,...values.filter(v=>v.state===3).map(v=>v.bytes)]);
}
export function fill(def:Definition,fields:(Uint8Array|typeof absent|typeof nullValue)[]):Uint8Array{
 if(fields.length!==def.layout.fields.length)throw Error('PIL field count mismatch');
 const bytes=encode(def.layout,fields.map((value,i)=>typeof value==='symbol'?{state:value===absent?0:1,bytes:new Uint8Array()}:{state:width(def.layout.fields[i])?2:3,bytes:value}));
 validate(def,bytes);return bytes;
}
function validate(def:Definition,bytes:Uint8Array){def.codec.upload(bytes);if(def.codec.scan(def.id,0,bytes.length)!==bytes.length)throw Error('Trailing PIL data');}
function fields(def:Definition,bytes:Uint8Array){return def.layout.fields.map((_,i)=>{const s=span(bytes,def.layout,i);return {state:s.state,bytes:bytes.subarray(s.start,s.end)}});}
export type PackOptions={layout?:{kind:'variable'}|{kind:'fixed';recordWidth?:number};packing?:{commonValues?:boolean};headers?:Record<string,Uint8Array>;index?:false|{addressBytes:2|3|4;offsets?:Uint8Array}};
export class UPack {
 readonly kind='upack';readonly definition:Definition;readonly bytes:Uint8Array;readonly header:PACHeader;
 constructor(definition:Definition,bytes:Uint8Array,validation:'full'|'structural'='full'){
  this.definition=definition;this.bytes=bytes;const h=pacHeader(bytes,0);
  if(!h||h.end!==bytes.length||h.digest!==Array.from(definition.digest,b=>b.toString(16).padStart(2,'0')).join(''))throw Error('Invalid PAC binding');this.header=h;
  if(validation==='full'){let end=h.start;for(let i=0;i<h.count;i++){const member=this.member(i);end+=member.length;validate(definition,h.common.length?mergeFrames([h.common,member],definition.layout):member);}if(end!==h.end)throw Error('Trailing PAC data');}
 }
 /** Borrowed member. Common values remain separate; get() returns an owned standalone PIL. */
 member(ordinal:number):Uint8Array{
  if(this.header.recordWidth||this.header.addressBytes){const [start,end]=memberBounds(this.header,ordinal);return this.bytes.subarray(start,end);}
  if(!Number.isSafeInteger(ordinal)||ordinal<0||ordinal>=this.header.count)throw Error('PAC ordinal outside pack');
  let at=this.header.start;this.definition.codec.upload(this.bytes);
  for(let i=0;i<=ordinal;i++){const n=this.definition.codec.scanStructural(this.definition.id,at,this.header.end-at);if(!n||n>this.header.end-at)throw Error('Invalid PAC member');if(i===ordinal)return this.bytes.subarray(at,at+n);at+=n;}
  throw Error('PAC member missing');
 }
 get(target:number|{offset:number}):Uint8Array{
  let member:Uint8Array;
  if(typeof target==='number')member=this.member(target);
  else {let ordinal=0,at=0;for(;ordinal<this.header.count;ordinal++){const m=this.member(ordinal);if(at===target.offset)break;at+=m.length;}if(ordinal===this.header.count)throw Error('Offset is not a PAC member boundary');member=this.member(ordinal);}
  return this.header.common.length?mergeFrames([this.header.common,member],this.definition.layout):member.slice();
 }
}
export class CPack {readonly kind='cpack';readonly bytes:Uint8Array;readonly definition:Definition;constructor(definition:Definition,bytes:Uint8Array){this.definition=definition;this.bytes=bytes;}}
export function pack(def:Definition,records:Uint8Array[],options:PackOptions={}):UPack{
 for(const record of records)validate(def,record);
 let members=records;let common:Uint8Array=new Uint8Array();
 if(options.packing?.commonValues&&records.length>1){const all=records.map(r=>fields(def,r)),shared=all[0].map((v,i)=>v.state&&all.every(row=>row[i].state===v.state&&row[i].bytes.length===v.bytes.length&&row[i].bytes.every((b,j)=>b===v.bytes[j]))?v:{state:0,bytes:new Uint8Array()});
  if(shared.some(v=>v.state)){common=encode(def.layout,shared);members=all.map(row=>encode(def.layout,row.map((v,i)=>shared[i].state?{state:0,bytes:new Uint8Array()}:v)));}
 }
 const addressBytes=options.index?options.index.addressBytes:0;
 if(![0,2,3,4].includes(addressBytes))throw Error('Invalid PAC directory width');
 const recordWidth=options.layout?.kind==='fixed'?(options.layout.recordWidth??members[0]?.length):0;
 if(recordWidth===undefined||!Number.isSafeInteger(recordWidth)||recordWidth<0||options.layout?.kind==='fixed'&&(!recordWidth||members.some(m=>m.length!==recordWidth)))throw Error('PAC fixed width mismatch');
 const controls=new Uint8Array(5),cv=new DataView(controls.buffer);controls[0]=addressBytes;cv.setUint32(1,recordWidth,true);
 const encoder=new TextEncoder(),headers=options.headers??{},names=Object.keys(headers).map(name=>({name,bytes:encoder.encode(name)})).sort((a,b)=>{for(let i=0;i<Math.min(a.bytes.length,b.bytes.length);i++)if(a.bytes[i]!==b.bytes[i])return a.bytes[i]-b.bytes[i];return a.bytes.length-b.bytes.length;});
 const extras=[ext(0),controls,ext(names.length)];
 for(const {name,bytes} of names){if(!name||new TextDecoder('utf-8',{fatal:true}).decode(bytes)!==name)throw Error('Invalid header name');extras.push(ext(bytes.length),bytes,ext(headers[name].length),headers[name]);}
 extras.push(ext(common.length),common);
 const directory=new Uint8Array(members.length*addressBytes);let offset=0;
 members.forEach((m,i)=>{if(!m.length)throw Error('Empty PAC member');if(addressBytes&&offset>=2**(8*addressBytes))throw Error('PAC directory overflow');for(let j=0;j<addressBytes;j++)directory[i*addressBytes+j]=Math.floor(offset/2**(8*j))%256;offset+=m.length;});
 if(options.index&&options.index.offsets&&(options.index.offsets.length!==directory.length||directory.some((b,i)=>b!==(options.index as {offsets:Uint8Array}).offsets[i])))throw Error('PAC supplied directory mismatch');
 extras.push(directory);const header=join([new Uint8Array(64),...extras]),size=header.length+offset;
 if(size>0xfffffffe)throw Error('PAC size overflow');const v=new DataView(header.buffer);v.setUint32(0,size,true);v.setUint32(4,4|(common.length?1:0),true);v.setBigUint64(8,BigInt(size),true);v.setBigUint64(16,BigInt(header.length),true);v.setBigUint64(24,BigInt(members.length),true);header.set(def.digest,32);
 return new UPack(def,join([header,...members]),'structural');
}
export function open(def:Definition,bytes:Uint8Array,options:{validation?:'full'|'structural'}={}):UPack|CPack{
 if(bytes.length<8||new DataView(bytes.buffer,bytes.byteOffset,bytes.byteLength).getUint32(0,true)!==bytes.length)throw Error('Invalid PAC length');
 if(new DataView(bytes.buffer,bytes.byteOffset,bytes.byteLength).getUint32(4,true)===2){if(bytes.length<=16)throw Error('Truncated CPack');return new CPack(def,bytes);}
 return new UPack(def,bytes,options.validation);
}
export function compress(upack:UPack,options:{algorithm:'gzip';level?:number;onlyIfSmaller?:boolean}):UPack|CPack{
 if(options.algorithm!=='gzip')throw Error('Unsupported PAC codec');const level=options.level??1;if(!Number.isInteger(level)||level<0||level>9)throw Error('Invalid compression level');
 const body=upack.definition.codec.deflate(upack.bytes,level);if(options.onlyIfSmaller&&body.length+16>=upack.bytes.length)return upack;
 const header=new Uint8Array(16),v=new DataView(header.buffer);v.setUint32(0,16+body.length,true);v.setUint32(4,2,true);v.setUint32(8,upack.bytes.length,true);v.setUint32(12,upack.header.count,true);return new CPack(upack.definition,join([header,body]));
}
export async function decompress(cpack:CPack,options:{validation?:'full'|'structural'}={}):Promise<UPack>{const expanded=await expandPAC(cpack.bytes,0,0xfffffffe);if(!expanded)throw Error('Truncated CPack');return new UPack(cpack.definition,expanded.bytes,options.validation);}
export function unpack(upack:UPack){return {records:Array.from({length:upack.header.count},(_,i)=>upack.get(i)),headers:upack.header.headers};}
export function push(upack:UPack,pil:Uint8Array,options:PackOptions={}):UPack{return pack(upack.definition,[...unpack(upack).records,pil],{...options,headers:options.headers??upack.header.headers});}
export function pop(upack:UPack,options:PackOptions={}):{pack:UPack;record:Uint8Array|undefined}{const records=unpack(upack).records,record=records.pop();return {pack:pack(upack.definition,records,{...options,headers:options.headers??upack.header.headers}),record};}
