// CBOR is opened only for definitions and individual requested field values.
// Events themselves remain PIL byte ranges. Integer precision is never rounded.
const decoder = new TextDecoder("utf-8", { fatal: true });
export function cbor(bytes: Uint8Array): unknown {
  let at = 0;
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const take = (n: number) => {
    const start = at;
    at += n;
    if (at > bytes.length) throw Error("Incomplete CBOR");
    return bytes.subarray(start, at);
  };
  const number = (n: bigint) =>
    n >= BigInt(Number.MIN_SAFE_INTEGER) && n <= BigInt(Number.MAX_SAFE_INTEGER)
      ? Number(n)
      : n;
  const argument = (a: number): number | bigint => {
    if (a < 24) return a;
    const start = at;
    if (a === 24) return take(1)[0];
    if (a === 25) {
      take(2);
      return view.getUint16(start);
    }
    if (a === 26) {
      take(4);
      return view.getUint32(start);
    }
    if (a === 27) {
      take(8);
      return number(view.getBigUint64(start));
    }
    throw Error("Invalid CBOR argument");
  };
  const length = (n: number | bigint) => {
    if (typeof n !== "number" || n > bytes.length)
      throw Error("CBOR length exceeds field");
    return n;
  };
  function item(depth: number): unknown {
    if (depth > 32) throw Error("CBOR nesting exceeds 32");
    const head = take(1)[0],
      major = head >> 5,
      info = head & 31;
    if (major === 7) {
      if (info === 20 || info === 21) return info === 21;
      if (info === 22) return null;
      if (info === 23) return undefined;
      const start = at;
      if (info === 25) {
        const h = Number(argument(25));
        const e = (h >> 10) & 31;
        return (
          (h >> 15 ? -1 : 1) *
          (e === 31
            ? h & 1023
              ? NaN
              : Infinity
            : (e ? 1 + (h & 1023) / 1024 : (h & 1023) / 1024) *
              2 ** (e ? e - 15 : -14))
        );
      }
      if (info === 26) {
        take(4);
        return view.getFloat32(start);
      }
      if (info === 27) {
        take(8);
        return view.getFloat64(start);
      }
      throw Error("Unsupported CBOR simple value");
    }
    if (info === 31) {
      if (major < 2 || major > 5) throw Error("Invalid indefinite CBOR");
      const items: unknown[] = [];
      while (bytes[at] !== 255) items.push(item(depth + 1));
      take(1);
      if (major === 2) {
        if (!items.every((x) => x instanceof Uint8Array))
          throw Error("Invalid CBOR byte chunks");
        const parts = items as Uint8Array[];
        const result = new Uint8Array(parts.reduce((n, b) => n + b.length, 0));
        let p = 0;
        for (const b of parts) {
          result.set(b, p);
          p += b.length;
        }
        return result;
      }
      if (major === 3) {
        if (!items.every((x) => typeof x === "string"))
          throw Error("Invalid CBOR text chunks");
        return items.join("");
      }
      if (major === 4) return items;
      if (items.length % 2) throw Error("Invalid CBOR map");
      return map(items);
    }
    const n = argument(info);
    if (major === 0) return n;
    if (major === 1) return number(-1n - BigInt(n));
    if (major === 2) return take(length(n));
    if (major === 3) return decoder.decode(take(length(n)));
    if (major === 4)
      return Array.from({ length: length(n) }, () => item(depth + 1));
    if (major === 5)
      return map(Array.from({ length: length(n) * 2 }, () => item(depth + 1)));
    if (major === 6) return item(depth + 1);
    throw Error("Invalid CBOR");
  }
  function map(items: unknown[]) {
    const result: Record<string, unknown> = Object.create(null);
    for (let i = 0; i < items.length; i += 2) {
      const key = items[i];
      if (typeof key !== "string" && typeof key !== "number")
        throw Error("Unsupported CBOR map key");
      if (Object.hasOwn(result, String(key)))
        throw Error("Duplicate CBOR map key");
      result[String(key)] = items[i + 1];
    }
    return result;
  }
  const result = item(0);
  if (at !== bytes.length) throw Error("Trailing CBOR data");
  return result;
}

// Only used to install the two small, fixed transport control definitions.
/** Encode definition/control documents, including binary fields; never base64. */
export function encodeCBOR(value: unknown): Uint8Array {
  const chunks: Uint8Array[] = []; let size=0;
  const push=(bytes:Uint8Array)=>{chunks.push(bytes);size+=bytes.length;};
  const head=(major:number,n:bigint)=>{
    if(n<0n||n>0xffffffffffffffffn)throw Error('CBOR integer out of range');
    const width=n<24n?0:n<=255n?1:n<=65535n?2:n<=4294967295n?4:8;
    const b=new Uint8Array(1+width);b[0]=major*32+(width===0?Number(n):width===1?24:width===2?25:width===4?26:27);
    for(let i=width;i>0;i--){b[i]=Number(n&255n);n>>=8n;}push(b);
  };
  const write=(v:unknown,depth:number):void=>{
    if(depth>32)throw Error('CBOR nesting exceeds 32');
    if(v===null){push(Uint8Array.of(246));return;}
    if(v===undefined){push(Uint8Array.of(247));return;}
    if(typeof v==='boolean'){push(Uint8Array.of(v?245:244));return;}
    if(typeof v==='bigint'||typeof v==='number'&&Number.isSafeInteger(v)){
      const n=BigInt(v);head(n<0n?1:0,n<0n?-1n-n:n);return;
    }
    if(typeof v==='number'){
      if(!Number.isFinite(v))throw Error('Nonfinite CBOR control number');
      const b=new Uint8Array(9);b[0]=251;new DataView(b.buffer).setFloat64(1,v);push(b);return;
    }
    if(typeof v==='string'){const b=new TextEncoder().encode(v);head(3,BigInt(b.length));push(b);return;}
    if(v instanceof Uint8Array){head(2,BigInt(v.length));push(v);return;}
    if(Array.isArray(v)){head(4,BigInt(v.length));for(const x of v)write(x,depth+1);return;}
    if(v&&typeof v==='object'&&(Object.getPrototypeOf(v)===Object.prototype||Object.getPrototypeOf(v)===null)){
      const entries=Object.entries(v).filter(([,x])=>x!==undefined);head(5,BigInt(entries.length));
      for(const [k,x]of entries){write(k,depth+1);write(x,depth+1);}return;
    }
    throw Error('Unsupported CBOR control value');
  };
  write(value,0);const out=new Uint8Array(size);let at=0;for(const b of chunks){out.set(b,at);at+=b.length;}return out;
}
export const controlCBOR = encodeCBOR;
