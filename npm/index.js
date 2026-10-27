/** Synchronous instance-local buffers. Use a separate instance per worker. */
export class Codec {
  constructor(instance, supportedExtensions = [1]) {
    this.api = instance.exports;
    const bytes = new Uint8Array(supportedExtensions.length * 4);
    const view = new DataView(bytes.buffer);
    supportedExtensions.forEach((n, i) => {
      if (!Number.isInteger(n) || n < 0 || n > 0xffffffff) throw new RangeError('Invalid PAC extension');
      view.setUint32(i * 4, n, true);
    });
    this.upload(bytes);
    this.check(this.api.extensions());
  }
  error() {
    return Error(new TextDecoder().decode(new Uint8Array(this.api.memory.buffer, this.api.error_ptr(), this.api.error_len())));
  }
  check(n) { n >>>= 0; if (n === 0xffffffff) throw this.error(); return n; }
  upload(bytes) {
    const ptr = this.api.input(bytes.length);
    if (!ptr) throw this.error();
    new Uint8Array(this.api.memory.buffer, ptr, bytes.length).set(bytes);
  }
  define(bytes) { this.upload(bytes); return this.check(this.api.definition()); }
  /** Supply raw JSON text, not a JS object: JSON.parse would round large U64s. */
  definitionFromJSON(text) {
    this.upload(typeof text === 'string' ? new TextEncoder().encode(text) : text);
    const n = this.check(this.api.json_definition());
    return new Uint8Array(this.api.memory.buffer, this.api.document_ptr(), n).slice();
  }
  scan(id, offset, length) { return this.check(this.api.scan(id, offset, length)); }
  scanStructural(id, offset, length) { return this.check(this.api.scan_structural(id, offset, length)); }
  verifyCRC32C(bytes, expected) {
    if (!Number.isInteger(expected) || expected < 0 || expected > 0xffffffff) throw Error('Invalid CRC32C');
    this.upload(bytes); this.check(this.api.verify_crc32c(0, bytes.length, expected));
  }
  pacStructural(id, offset, length, count, additionalExtension = 0) { return this._pac(id,offset,length,count,additionalExtension,true); }
  pac(id, offset, length, count, additionalExtension = 0) { return this._pac(id,offset,length,count,additionalExtension,false); }
  _pac(id, offset, length, count, additionalExtension, structural) {
    if (!Number.isInteger(additionalExtension) || additionalExtension < 0 || additionalExtension > 0xffffffff) throw new RangeError("Invalid PAC extension");
    const n = this.check((structural ? this.api.scan_pac_structural : this.api.scan_pac)(id, offset, length, additionalExtension));
    if (n !== length) throw Error('Truncated PAC');
    // Verify count before exposing a view into linear memory.
    if (!Number.isInteger(count) || count < 0 || count !== this.api.pac_count()) throw Error('PAC count mismatch');
    return new Uint32Array(this.api.memory.buffer, this.api.pac_members(), count * 2).slice();
  }
  inflate(bytes, length) {
    this.upload(bytes);
    const n = this.check(this.api.inflate(length));
    return new Uint8Array(this.api.memory.buffer, this.api.inflated_ptr(), n).slice();
  }
  release(id) { this.api.release(id); }
}

/** bytes/module works in browsers and Node; callers own fetching and policy. */
export async function createCodec(source, supportedExtensions = [1]) {
  const compiled = await WebAssembly.instantiate(source);
  return new Codec(compiled instanceof WebAssembly.Instance ? compiled : compiled.instance, supportedExtensions);
}
