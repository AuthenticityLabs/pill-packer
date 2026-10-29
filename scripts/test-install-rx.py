#!/usr/bin/env python3
"""Installer orchestration tests; system tools are replaced with temporary stubs."""
import json, os, pathlib, shutil, subprocess, sys, tempfile, unittest
INSTALLER = pathlib.Path(__file__).with_name('install-rx.sh')
STUB = r'''import os,pathlib,sys,json
name=pathlib.Path(sys.argv[0]).name
args=sys.argv[1:]
log=pathlib.Path(os.environ['RX_TEST_LOG'])
with log.open('a') as f:f.write(json.dumps([name,*args])+'\n')
mode=os.environ.get('RX_TEST_MODE','')
if name=='uname':print(os.environ.get('RX_TEST_OS','Linux') if args==['-s'] else 'x86_64')
elif name=='xcode-select':sys.exit(1 if mode=='no-xcode' else 0)
elif name=='rustc':
 if mode=='missing-rust':sys.exit(1)
 elif mode=='old-rust':print('rustc 1.70.0')
 else:print('rustc 1.90.0')
elif name in ('curl','wget'):
 if mode=='download-fail':sys.exit(22)
 out=pathlib.Path(args[args.index('--output')+1] if name=='curl' else next(a.split('=',1)[1] for a in args if a.startswith('--output-document=')))
 out.write_text('exit 0\n') if 'https://sh.rustup.rs' in args else out.write_bytes(b'test archive')
elif name=='tar':
 root=pathlib.Path(args[args.index('-C')+1])/'rust';root.mkdir()
 (root/'Cargo.lock').write_text('test lock')
elif name=='cargo':
 if mode=='build-fail':sys.exit(1)
 assert '--locked' in args and args[args.index('--bin')+1]=='rx'
 assert args[args.index('--features')+1]=='cli'
 root=pathlib.Path(args[args.index('--root')+1]);(root/'bin').mkdir(parents=True,exist_ok=True)
 exe=root/'bin/rx';exe.write_text('#!/bin/sh\nexit 0\n');exe.chmod(0o755)
'''
class InstallerTests(unittest.TestCase):
 def run_installer(self, mode='', system='Linux', wget=False):
  with tempfile.TemporaryDirectory(prefix='rx-installer-test-') as raw:
   root=pathlib.Path(raw);bin=root/'tools';bin.mkdir();temps=root/'temps';temps.mkdir()
   log=root/'calls';log.write_text('');cargo_home=root/'cargo-home';cargo_home.mkdir()
   for name in ['uname','xcode-select','rustc','cargo','rustup','cc','wget' if wget else 'curl','tar']:
    if name=='rustup' and mode=='missing-rust':continue
    file=bin/name;file.write_text('#!'+sys.executable+'\n'+STUB);file.chmod(0o755)
   for name in ['mktemp','mkdir','rm','awk','sh']:(bin/name).symlink_to(shutil.which(name))
   env=os.environ.copy();env.update(PATH=str(bin),CARGO_HOME=str(cargo_home),RX_INSTALL_ROOT=str(root/'installed'),RX_REVISION='abc123',TMPDIR=str(temps),RX_TEST_LOG=str(log),RX_TEST_MODE=mode,RX_TEST_OS=system)
   run=subprocess.run(['/bin/sh',str(INSTALLER)],env=env,capture_output=True,text=True)
   calls=[json.loads(s) for s in log.read_text().splitlines()]
   self.assertEqual(list(temps.iterdir()),[], 'Temporary build files must be cleaned')
   return run,calls,(root/'installed/bin/rx').exists()
 def test_linux_and_macos(self):
  for system in ['Linux','Darwin']:
   with self.subTest(system=system):
    run,calls,installed=self.run_installer(system=system)
    self.assertEqual(run.returncode,0,run.stderr);self.assertTrue(installed)
    self.assertIn('Installed RX:',run.stdout)
 def test_wget_fallback(self):
  run,calls,_=self.run_installer(wget=True)
  self.assertEqual(run.returncode,0,run.stderr);self.assertTrue(any(c[0]=='wget' for c in calls))
 def test_old_toolchain(self):
  run,calls,_=self.run_installer(mode='old-rust')
  self.assertEqual(run.returncode,0,run.stderr)
  self.assertIn(['rustup','toolchain','install','stable','--profile','minimal'],calls)
  self.assertTrue(any(c[:2]==['cargo','+stable'] for c in calls))
 def test_missing_toolchain(self):
  run,calls,_=self.run_installer(mode='missing-rust')
  self.assertEqual(run.returncode,0,run.stderr)
  self.assertTrue(any('https://sh.rustup.rs' in c for c in calls))
 def test_download_or_build_failure(self):
  for mode in ['download-fail','build-fail']:
   with self.subTest(mode=mode):
    run,_,installed=self.run_installer(mode=mode)
    self.assertNotEqual(run.returncode,0);self.assertFalse(installed)
    self.assertNotIn('Installed RX:',run.stdout)
 def test_unsupported_os(self):
  run,calls,_=self.run_installer(system='MINGW64_NT')
  self.assertNotEqual(run.returncode,0);self.assertIn('WSL',run.stderr)
  self.assertFalse(any(c[0]=='cargo' for c in calls))
 def test_missing_apple_tools(self):
  run,calls,_=self.run_installer(mode='no-xcode',system='Darwin')
  self.assertNotEqual(run.returncode,0);self.assertIn('xcode-select --install',run.stderr)
  self.assertFalse(any(c[0]=='curl' for c in calls))
if __name__=='__main__':unittest.main()
