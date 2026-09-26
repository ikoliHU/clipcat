// Synthetic FFmpeg test: kills only its own child; captures no desktop or microphone.
import { spawn, spawnSync } from 'node:child_process';
import { mkdtemp, rm } from 'node:fs/promises';
import { readFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { tmpdir } from 'node:os';
const ffmpeg = resolve(process.argv[2] || 'src-tauri/obs/ffmpeg/ffmpeg.exe');
const engine = readFileSync(new URL('../src-tauri/src/engine/mod.rs', import.meta.url), 'utf8');
const muxerSettings = engine.slice(engine.indexOf('pub fn start_recording')).match(/str\(\s*"muxer_settings",\s*"([^"]+)"/)?.[1];
if (!muxerSettings) throw new Error('Could not read the actual recording muxer options');
const dir = await mkdtemp(join(tmpdir(), 'clipcat-crash-test-'));
try {
  for (const fragmented of [false, true]) {
    const path = join(dir, fragmented ? 'fragmented.mp4' : 'plain.mp4');
    const flags = fragmented ? muxerSettings.split(' ').flatMap(option => {
      const [key, value] = option.split('='); return [`-${key}`, value];
    }) : [];
    const child = spawn(ffmpeg, ['-hide_banner', '-loglevel', 'error', '-re', '-f', 'lavfi', '-i', 'testsrc2=size=160x90:rate=10', '-c:v', 'libx264', '-g', '10', '-preset', 'ultrafast', ...flags, path], { windowsHide: true, stdio: 'ignore' });
    const done = new Promise((resolve, reject) => { child.on('error', reject); child.on('exit', (code, signal) => resolve({code, signal})); });
    await new Promise(resolve => setTimeout(resolve, 3500));
    if (child.exitCode !== null) throw new Error('FFmpeg exited before simulated crash');
    child.kill('SIGKILL');
    await done;
    const probe = spawnSync(ffmpeg, ['-hide_banner', '-loglevel', 'error', '-i', path, '-frames:v', '1', '-f', 'null', '-'], { windowsHide: true, encoding: 'utf8', timeout: 10000 });
    if (fragmented ? probe.status !== 0 : probe.status === 0) throw new Error(`Unexpected decode result: ${probe.stderr}`);
    console.log(fragmented ? 'PASS: interrupted fragmented MP4 remains decodable' : 'REPRODUCED: interrupted plain MP4 cannot be decoded (missing moov)');
  }
} finally {
  if (resolve(dir).startsWith(resolve(tmpdir()) + '\\') || resolve(dir).startsWith(resolve(tmpdir()) + '/')) await rm(dir, { recursive: true, force: true });
}
