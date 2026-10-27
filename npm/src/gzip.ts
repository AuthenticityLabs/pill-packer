// Bounded client-side gzip decoding. The caller owns packet framing and limits.
export async function inflateGzip(raw:Uint8Array,expected:number,maximum:number):Promise<Uint8Array>{
 if(!Number.isSafeInteger(expected)||expected<0||expected>maximum)throw Error('Decoded length exceeds limit');
 const reader=new Blob([raw.slice().buffer]).stream().pipeThrough(new DecompressionStream('gzip')).getReader();
 const out=new Uint8Array(expected);let at=0;
 try{for(;;){const {done,value}=await reader.read();if(done)break;if(value.length>expected-at)throw Error('Expanded data exceeds bound');out.set(value,at);at+=value.length;}if(at!==expected)throw Error('Expanded length mismatch');}
 catch(e){await reader.cancel().catch(()=>{});throw e;}finally{reader.releaseLock();}
 return out;
}
