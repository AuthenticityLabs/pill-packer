export class Codec {
  constructor(instance: WebAssembly.Instance, supportedExtensions?: number[]);
  upload(bytes: Uint8Array): void;
  define(bytes: Uint8Array): number;
  definitionFromJSON(text: string | Uint8Array): Uint8Array;
  scan(id: number, offset: number, length: number): number;
  pac(id: number, offset: number, length: number, count: number, additionalExtension?: number): Uint32Array;
  inflate(bytes: Uint8Array, length: number): Uint8Array;
  release(id: number): void;
}
export function createCodec(source: BufferSource | WebAssembly.Module, supportedExtensions?: number[]): Promise<Codec>;
