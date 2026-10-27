export type UnsignedEncoding={width:number;base:bigint};
function compactSmall(v: DataView, at: number, width: number) {
  switch (width) {
    case 1: return v.getUint8(at);
    case 2: return v.getUint16(at, true);
    case 3: return v.getUint16(at, true) + v.getUint8(at + 2) * 65536;
    case 4: return v.getUint32(at, true);
  }
  let n = 0;
  for (let i = width - 1; i >= 0; i--) n = n * 256 + v.getUint8(at + i);
  return n;
}
export function unsignedBig(v: DataView, at: number, e: UnsignedEncoding) {
  if(!Number.isInteger(e.width)||e.width<1||e.width>8||e.base<0n||e.base>0xffffffffffffffffn)throw Error("Invalid unsigned encoding");
  let n: bigint;
  if (e.width <= 6) n = BigInt(compactSmall(v, at, e.width));
  else if (e.width === 8) n = v.getBigUint64(at, true);
  else n = BigInt(v.getUint32(at, true)) + BigInt(v.getUint16(at + 4, true)) * 4294967296n
    + BigInt(v.getUint8(at + 6)) * 281474976710656n;
  n += e.base;
  if (n > 0xffffffffffffffffn) throw Error('Unsigned base overflow');
  return n;
}
export function unsignedNumber(v: DataView, at: number, e: UnsignedEncoding, exact = true) {
  if(!Number.isInteger(e.width)||e.width<1||e.width>8||e.base<0n||e.base>0xffffffffffffffffn)throw Error("Invalid unsigned encoding");
  // Common columns avoid BigInt arithmetic on each row.
  const n = e.width <= 6 && e.base <= BigInt(Number.MAX_SAFE_INTEGER)
    ? compactSmall(v, at, e.width) + Number(e.base)
    : Number(unsignedBig(v, at, e));
  if (exact && !Number.isSafeInteger(n)) throw Error('Unsigned integer exceeds safe range');
  return n;
}

export type ColumnSpan={offset:number;count:number;width:number};
export function shuffleColumns(raw:Uint8Array,spans:readonly ColumnSpan[],reverse=false):void {
 let end=0;
 for(const c of spans){if(!Number.isSafeInteger(c.offset)||!Number.isSafeInteger(c.count)||!Number.isInteger(c.width)||c.offset<end||c.count<0||c.width<1||c.width>8||c.offset>raw.length||c.count>Math.floor((raw.length-c.offset)/c.width))throw Error('Invalid column span');end=c.offset+c.count*c.width;}
 let scratch=new Uint8Array(0);
 for(const c of spans){if(c.count<2||c.width<2)continue;const length=c.count*c.width;if(scratch.length<length)scratch=new Uint8Array(length);scratch.set(raw.subarray(c.offset,c.offset+length));for(let b=0;b<c.width;b++)for(let i=0;i<c.count;i++)if(reverse)raw[c.offset+i*c.width+b]=scratch[b*c.count+i];else raw[c.offset+b*c.count+i]=scratch[i*c.width+b];}
}
