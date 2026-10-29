export class Codec {
  constructor(instance: WebAssembly.Instance, supportedExtensions?: number[]);
  upload(bytes: Uint8Array): void;
  define(bytes: Uint8Array): number;
  definitionFromJSON(text: string | Uint8Array): Uint8Array;
  encodeJSON(id:number,text:string|Uint8Array):Uint8Array;
  decodeJSON(id:number,bytes:Uint8Array):string;
  scan(id: number, offset: number, length: number): number;
  pac(id: number, offset: number, length: number, count: number, additionalExtension?: number): Uint32Array;
  scanStructural(id:number,offset:number,length:number):number;
  pacStructural(id:number,offset:number,length:number,count:number,additionalExtension?:number):Uint32Array;
  verifyCRC32C(bytes:Uint8Array,expected:number):void;
  inflate(bytes: Uint8Array, length: number): Uint8Array;
  deflate(bytes:Uint8Array,level?:number):Uint8Array;
  release(id: number): void;
}
export function createCodec(source: BufferSource | WebAssembly.Module, supportedExtensions?: number[]): Promise<Codec>;
