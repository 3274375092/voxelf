// Download model files + CJK font into assets/.
// Sources: hf-mirror.com (HuggingFace mirror) and jsdelivr (GitHub CDN),
// because github.com direct download is blocked on this network.
//
// Usage: node scripts/download-models.js
const fs = require('node:fs');
const path = require('node:path');
const { execSync } = require('node:child_process');

const MIRROR = 'https://hf-mirror.com';
const JSDELIVR = 'https://cdn.jsdelivr.net';

// Repos and files. Everything under `filter` is included (recursive),
// everything under `exclude` is skipped.
const JOBS = [
  {
    name: 'sherpa-onnx Windows 预编译静态库 (v1.13.4)',
    base: 'https://api.github.com/repos/k2-fsa/sherpa-onnx/releases/assets/469211798',
    files: ['sherpa-onnx-v1.13.4-win-x64-static-MT-Release-lib.tar.bz2'],
    dest: 'target/sherpa-onnx-prebuilt',
    headers: ['-H', 'Accept: application/octet-stream'],
    resume: true,
  },
  {
    name: 'ASR zipformer zh (int8, 2025-06-30)',
    base: `${MIRROR}/csukuangfj/sherpa-onnx-streaming-zipformer-zh-int8-2025-06-30/resolve/main/`,
    files: [
      'encoder.int8.onnx',
      'decoder.onnx',
      'joiner.int8.onnx',
      'tokens.txt',
      'test_wavs/0.wav',
    ],
    dest: 'assets/models/asr-zh',
  },
  {
    name: 'VAD silero',
    base: `${MIRROR}/csukuangfj/vad/resolve/main/`,
    files: ['silero_vad.onnx'],
    dest: 'assets/models/vad',
  },
  {
    name: 'Kokoro TTS int8 multi-lang v1.1',
    base: `${MIRROR}/csukuangfj/kokoro-int8-multi-lang-v1_1/resolve/main/`,
    files: [...collectHfFileList('csukuangfj/kokoro-int8-multi-lang-v1_1')],
    dest: 'assets/models/kokoro',
  },
  {
    name: 'Noto Sans SC font',
    base: `${JSDELIVR}/gh/google/fonts@main/ofl/notosanssc/`,
    files: ['NotoSansSC%5Bwght%5D.ttf'],
    dest: 'assets/fonts',
    rename: { 'NotoSansSC%5Bwght%5D.ttf': 'NotoSansSC.ttf' },
  },
];

function collectHfFileList(repo) {
  const api = `${MIRROR}/api/models/${repo}`;
  const out = execSync(`curl -sL --max-time 30 "${api}"`, { encoding: 'utf8' });
  const meta = JSON.parse(out);
  const skip = new Set(['.gitattributes', 'LICENSE', 'README.md', 'dict/generate_user_dict.py', 'dict/README.md']);
  return (meta.siblings || [])
    .map((s) => s.rfilename)
    .filter((f) => !skip.has(f));
}

const CONCURRENCY = 12;
let active = 0;
let done = 0;
const total = JOBS.reduce((n, j) => n + j.files.length, 0);
const failed = [];

function download(job, destPath, retries = 3) {
  return new Promise((resolve) => {
    fs.mkdirSync(path.dirname(destPath), { recursive: true });
    if (fs.existsSync(destPath) && fs.statSync(destPath).size > 0) {
      done++;
      console.log(`[skip] ${destPath}`);
      return resolve(true);
    }
    const tmp = destPath + '.part';
    const headers = (job.headers || []).join(' ');
    const resume = job.resume ? '-C -' : '';
    const cmd = `curl -sL ${resume} ${headers} --retry 5 --retry-delay 3 --max-time 900 -o "${tmp}" "${job.base}"`;
    try {
      execSync(cmd, { stdio: 'ignore' });
      if (fs.existsSync(tmp) && fs.statSync(tmp).size > 0) {
        fs.renameSync(tmp, destPath);
        done++;
        console.log(`[ok]   ${destPath} (${(fs.statSync(destPath).size / 1048576).toFixed(1)} MB)`);
        resolve(true);
      } else {
        throw new Error('empty file');
      }
    } catch (e) {
      try { fs.unlinkSync(tmp); } catch {}
      if (retries > 0) {
        console.log(`[retry ${3 - retries + 1}] ${destPath}`);
        download(job, destPath, retries - 1).then(resolve);
      } else {
        failed.push(destPath);
        console.log(`[FAIL] ${destPath}: ${e.message.slice(0, 80)}`);
        done++;
        resolve(false);
      }
    }
  });
}

function pump(queue) {
  while (active < CONCURRENCY && queue.length > 0) {
    const { job, destPath } = queue.shift();
    active++;
    download(job, destPath).then(() => {
      active--;
      if (queue.length > 0) pump(queue);
      else if (active === 0) {
        console.log(`\n=== done: ${done}/${total}, failed: ${failed.length} ===`);
        if (failed.length) console.log(failed.join('\n'));
        process.exit(failed.length ? 1 : 0);
      }
    });
  }
}

const queue = [];
for (const job of JOBS) {
  console.log(`\n--- ${job.name}`);
  for (const f of job.files) {
    const destPath = path.join(job.dest, job.rename && job.rename[f] ? job.rename[f] : f);
    queue.push({ job, destPath });
  }
}
pump(queue);
