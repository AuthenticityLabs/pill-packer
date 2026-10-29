import {inflateGzip} from "./gzip.ts";
import {span, width, type PILDefinition} from "./layout.ts";
// Version-zero PAC: decoded semantic header, or a small compression wrapper.
export type PACExtension={type:number;required:boolean;version:number;bytes:Uint8Array};
export type PACHeader={digest:string;source?:string;count:number;start:number;end:number;common:Uint8Array;extensions:PACExtension[];headers:Record<string,Uint8Array>;addressBytes:number;recordWidth:number;directory:Uint8Array};
function appendExtended(out:number[],n:number){if(n<65535)out.push(n&255,n>>>8);else out.push(255,255,n&255,(n>>>8)&255,(n>>>16)&255,n>>>24);}

/** Reconstruct one logical PIL record from its PAC common frame and member. */
export function mergeFrames(parts:Uint8Array[],def:PILDefinition,omit=new Set<number>()):Uint8Array{
 const m=Math.ceil(def.fields.length/4),mask=new Uint8Array(m);
 const fields=def.fields.map((f,i)=>{
  if(omit.has(i))return {state:0,bytes:new Uint8Array(),bits:0};
  const found=parts.map(raw=>({raw,s:span(raw,def,i)})).filter(v=>v.s.state!==0);
  if(found.length>1)throw Error('Overlapping PAC common/member fields');
  const v=found[0],state=v?.s.state??0;
  const bytes=v?v.raw.subarray(v.s.start,v.s.end):new Uint8Array();
  if(v&&(v.s.end>v.raw.length||v.s.start>v.s.end))throw Error('Invalid encoded field');
  mask[Math.floor(i/4)]|=state<<(6-2*(i%4));
  const adaptive=!f.values?.length&&['16','32'].includes(def.adaptive_integers??'');
  const bits=state===2&&adaptive?(/^[IU](32|64)$/.test(f.type)?2:def.adaptive_integers==='16'&&/^[IU]16$/.test(f.type)?1:0):0;
  if(state===2&&(bits?(![1,2,4,8].includes(bytes.length)||bytes.length>width(f)):bytes.length!==width(f)))throw Error('Invalid direct field width');
  return {state,bytes,bits};
 });
 const plane=new Uint8Array(Math.ceil(fields.reduce((n,f)=>n+f.bits,0)/8));let bit=0;
 const offsets:number[]=[];let end=0;
 for(const f of fields){
  if(f.bits){const code=Math.log2(f.bytes.length);for(let b=f.bits-1;b>=0;b--){plane[Math.floor(bit/8)]|=((code>>b)&1)<<(7-bit%8);bit++;}}
  if(f.state===3){end+=f.bytes.length;appendExtended(offsets,end);}
 }
 const length=m+plane.length+offsets.length+fields.reduce((n,f)=>n+f.bytes.length,0);
 if(length>64*1024*1024)throw Error('Merged record exceeds limit');
 const out=new Uint8Array(length);out.set(mask);out.set(plane,m);let at=m+plane.length;
 for(const f of fields)if(f.state===2){out.set(f.bytes,at);at+=f.bytes.length;}
 out.set(offsets,at);at+=offsets.length;
 for(const f of fields)if(f.state===3){out.set(f.bytes,at);at+=f.bytes.length;}
 return out;
}
export function pacHeader(bytes:Uint8Array,offset:number,maximum=0xffffffff,supported:ReadonlySet<number>=new Set([1])):PACHeader|undefined {
 if(bytes.length-offset<64)return;
 const view=new DataView(bytes.buffer,bytes.byteOffset,bytes.byteLength);

 const flags=view.getUint32(offset+4,true);
 const number=(at:number)=>{const n=view.getBigUint64(at,true);if(n>BigInt(Number.MAX_SAFE_INTEGER))throw Error('PAC length exceeds addressable range');return Number(n)};
 const size=number(offset+8),header=number(offset+16),count=number(offset+24);
 if(view.getUint32(offset,true)!==size||(flags&~5)!==0||!(flags&4)||size>maximum||header<64||header>size)throw Error('Invalid PAC bounds/features');
 const end=offset+size;if(end>bytes.length)return;
 let at=offset+64;const limit=offset+header;
 const integer=()=>{
  if(limit-at<2)throw Error('Truncated PAC integer');let n=view.getUint16(at,true);at+=2;
  if(n===65535){if(limit-at<4)throw Error('Truncated PAC integer');n=view.getUint32(at,true);at+=4;
   if(n===4294967295){if(limit-at<8)throw Error('Truncated PAC integer');n=number(at);at+=8;if(n<4294967295)throw Error('Nonminimal PAC integer')}
   else if(n<65535)throw Error('Nonminimal PAC integer');
  }return n;
 };
 const n=integer();if(n>Math.floor((limit-at)/10))throw Error('Invalid PAC extension count');
 const extensions:PACExtension[]=[],seen=new Set<number>();let source:string|undefined;
 for(let i=0;i<n;i++){
  if(limit-at<8)throw Error('Truncated PAC extension');
  const type=view.getUint32(at,true),version=view.getUint16(at+4,true),ef=view.getUint16(at+6,true);at+=8;
  const length=integer();if(length>limit-at||ef>1||seen.has(type))throw Error('Invalid PAC extension');
  if((ef===1&&!supported.has(type))||(supported.has(type)&&version!==0))throw Error('Unsupported required PAC extension');
  if(ef===1&&version!==0)throw Error('Unsupported required PAC extension schema');
  seen.add(type);const data=bytes.subarray(at,at+length);at+=length;
  extensions.push({type,version,required:ef===1,bytes:data});
  if(type===1)source=new TextDecoder('utf-8',{fatal:true}).decode(data);
 }
 if(limit-at<5)throw Error('Truncated PAC layout');
 const addressBytes=bytes[at++],recordWidth=view.getUint32(at,true);at+=4;
 if(![0,2,3,4].includes(addressBytes))throw Error('Invalid PAC address width');
 const headers:Record<string,Uint8Array>=Object.create(null),names=integer();let previous:Uint8Array|undefined;
 if(names>Math.floor((limit-at)/4))throw Error('Invalid PAC header count');
 const decoder=new TextDecoder('utf-8',{fatal:true});
 for(let i=0;i<names;i++){const n=integer();if(!n||n>limit-at)throw Error('Invalid PAC header name');const encoded=bytes.subarray(at,at+n),name=decoder.decode(encoded);at+=n;
  if(previous){let i=0;while(i<Math.min(previous.length,encoded.length)&&previous[i]===encoded[i])i++;if(i===encoded.length||i<previous.length&&previous[i]>encoded[i])throw Error('Invalid PAC header order');}
  previous=encoded;
  const length=integer();if(length>limit-at)throw Error('Invalid PAC header value');headers[name]=bytes.subarray(at,at+length);at+=length;
 }
 const commonLength=integer();if(commonLength>limit-at||Boolean(flags&1)!==Boolean(commonLength))throw Error('Invalid PAC common frame');
 const common=bytes.subarray(at,at+commonLength);at+=commonLength;
 if(count>0xffffffff||count>size-header||count*addressBytes!==limit-at)throw Error('Invalid PAC directory count');
 const directory=bytes.subarray(at,limit);
 const result={digest:Array.from(bytes.subarray(offset+32,offset+64),b=>b.toString(16).padStart(2,'0')).join(''),source,count,start:limit,end,common,extensions,headers,addressBytes,recordWidth,directory};
 if(recordWidth&&recordWidth*count!==end-limit)throw Error('Invalid fixed PAC extent');
 if(addressBytes){let previous=-1;for(let i=0;i<count;i++){const value=directoryOffset(directory,addressBytes,i);if(i===0&&value!==0||value<=previous||value>=end-limit||recordWidth&&value!==i*recordWidth)throw Error('Invalid PAC directory');previous=value;}}
 return result;
}
export function validatePACMember(common:Uint8Array,member:Uint8Array,fields:number):void {
 if(!common.length)return;
 for(let i=0;i<fields;i++){
  const shift=6-2*(i%4);
  if(((common[i>>2]>>shift)&3)!==0&&((member[i>>2]>>shift)&3)!==0)throw Error('PAC common field overridden');
 }
}

// The compressed body is a complete decoded PAC. No outer-header bytes survive.
export async function expandPAC(bytes:Uint8Array,offset:number,maximum:number,inflate?:(b:Uint8Array,n:number)=>Uint8Array):Promise<{bytes:Uint8Array;end:number}|undefined>{
 if(bytes.length-offset<8)return;
 const v=new DataView(bytes.buffer,bytes.byteOffset+offset,bytes.length-offset),size=v.getUint32(0,true),flags=v.getUint32(4,true);
 const compressed=flags===2,min=compressed?16:64;
 if((compressed?false:(flags&~5)!==0||!(flags&4))||size<min||size>maximum)throw Error('Invalid PAC compression controls');
 if(bytes.length-offset<min)return;
 const decoded=compressed?v.getUint32(8,true):Number(v.getBigUint64(8,true));
 const count=compressed?v.getUint32(12,true):Number(v.getBigUint64(24,true));
 if(decoded<64||decoded>maximum||count>decoded-64)throw Error('Invalid PAC compression bounds');
 if(bytes.length-offset<size)return;
 const raw=bytes.subarray(offset,offset+size);
 const validate=(out:Uint8Array)=>{
  if(out.length!==decoded)throw Error('PAC expansion length mismatch');
  const h=new DataView(out.buffer,out.byteOffset,out.byteLength),head=Number(h.getBigUint64(16,true));
  if(h.getUint32(0,true)!==decoded||((h.getUint32(4,true)&~5)!==0||!(h.getUint32(4,true)&4))||h.getBigUint64(8,true)!==BigInt(decoded)||head<64||head>decoded||h.getBigUint64(24,true)!==BigInt(count))throw Error('PAC decoded header mismatch');
 };
 if(!compressed){validate(raw);return {bytes:raw,end:offset+size};}
 let out:Uint8Array;
 if(inflate)out=inflate(raw.subarray(16),decoded);
 else out=await inflateGzip(raw.subarray(16),decoded,maximum);

 validate(out);
 return {bytes:out,end:offset+size};
}

/** Use a producer-supplied offset directory without scanning member fields.
 * The caller verifies envelope/directory integrity before invoking this helper. */
export function indexedPACBounds(header:PACHeader,index:Uint8Array,stride:number,offset:number):Uint32Array {
 if(!Number.isSafeInteger(stride)||stride<4||!Number.isSafeInteger(offset)||offset<0||offset+4>stride||index.length!==header.count*stride)throw Error('Invalid PAC index shape');
 const result=new Uint32Array(header.count*2),v=new DataView(index.buffer,index.byteOffset,index.byteLength),size=header.end-header.start;
 let start=header.count?v.getUint32(offset,true):0;
 if(start!==0||(!header.count&&size))throw Error('Invalid PAC initial member offset');
 for(let i=0;i<header.count;i++){
  const end=i+1<header.count?v.getUint32((i+1)*stride+offset,true):size;
  if(end<=start||end>size)throw Error('Invalid PAC member offset');
  result[i*2]=header.start+start;result[i*2+1]=header.start+end;start=end;
 }
 return result;
}

export function directoryOffset(bytes:Uint8Array,width:number,ordinal:number):number {
 let n=0;for(let j=width-1;j>=0;j--)n=n*256+bytes[ordinal*width+j];return n;
}
/** O(1), allocation-free member coordinates from the encoded directory or fixed layout. */
export function memberBounds(header:PACHeader,ordinal:number):[number,number] {
 if(!Number.isSafeInteger(ordinal)||ordinal<0||ordinal>=header.count)throw Error('PAC ordinal outside pack');
 if(header.recordWidth)return [header.start+ordinal*header.recordWidth,header.start+(ordinal+1)*header.recordWidth];
 if(!header.addressBytes)throw Error('PAC requires sequential or external addressing');
 return [header.start+directoryOffset(header.directory,header.addressBytes,ordinal),ordinal+1<header.count?header.start+directoryOffset(header.directory,header.addressBytes,ordinal+1):header.end];
}
export function pacBounds(header:PACHeader):Uint32Array {
 const bounds=new Uint32Array(header.count*2);for(let i=0;i<header.count;i++)bounds.set(memberBounds(header,i),i*2);return bounds;
}
