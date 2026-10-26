import {execFileSync} from 'node:child_process';
import {copyFileSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
const root = new URL('../', import.meta.url);
execFileSync('cargo', ['build', '--release', '--locked', '--target', 'wasm32-unknown-unknown', '--manifest-path', 'rust/wasm/Cargo.toml'], {cwd:fileURLToPath(root), stdio:'inherit'});
copyFileSync(new URL('rust/wasm/target/wasm32-unknown-unknown/release/pill_packer_wasm.wasm', root), new URL('npm/codec.wasm', root));
copyFileSync(new URL('LICENSE', root), new URL('npm/LICENSE', root));
copyFileSync(new URL('README.md', root), new URL('npm/README.md', root));
