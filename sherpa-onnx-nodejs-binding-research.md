
# sherpa-onnx Official Node.js Binding — Primary-Source Research Report

**Research date:** 2026 (k2-fsa/sherpa-onnx master and npm registry fetched at research time)
**Scope:** official k2-fsa sherpa-onnx Node.js binding, verified ONLY against primary sources:
- GitHub repo `k2-fsa/sherpa-onnx` (master): `nodejs-addon-examples/`, `nodejs-examples/`, `scripts/node-addon-api/`, `.github/workflows/npm-addon*.yaml`, `.github/scripts/node-addon/`, `sherpa-onnx/csrc/*model-config.h`
- Official docs site (Sphinx): https://k2-fsa.github.io/sherpa/onnx/ — note `k2-fsa.github.io/sherpa-onnx/` now 404s; the docs' own "Edit on GitHub" links show the source lives in the **k2-fsa/sherpa** repo at `docs/source/onnx/` (e.g. https://github.com/k2-fsa/sherpa/blob/master/docs/source/onnx/javascript-api/install.rst)
- Official examples: `nodejs-addon-examples/` inside k2-fsa/sherpa-onnx (mirror repo https://github.com/k2-fsa/node-addon-examples)
- npm registry manifests + tarballs (downloaded and inspected): `sherpa-onnx-node`, `sherpa-onnx`, `sherpa-onnx-win-x64`, ...

> **Unverifiable:** `sherpa-onnx.com` — DNS lookup from this environment returns NXDOMAIN ("DNS 名称不存在") and HTTPS/HTTP connections fail (HTTP 000). It is NOT used as a source here; every claim below is cited to the GitHub repo or k2-fsa.github.io. Claims that could not be verified from a primary source are flagged explicitly.

---

## A) npm packaging

### A.1 Packages and versions (verified from npm registry)

| Package | Role | Latest at fetch | Unpacked size (npm `dist.unpackedSize`) | Tarball size |
|---|---|---|---|---|
| `sherpa-onnx-node` | Official **native (node-addon)** JS API package | **1.13.5** | 59,613 B (~58 KB) | 11.5 KB |
| `sherpa-onnx` | Official **WebAssembly** package for Node | **1.13.5** | 15,100,860 B (~14.4 MB) | 4.2 MB |
| `sherpa-onnx-win-x64` (also `-win-ia32`, `-linux-x64`, `-linux-arm64`, `-darwin-x64`, `-darwin-arm64`) | Prebuilt native binary packages (optional deps of `sherpa-onnx-node`) | **1.13.5** | 23,005,246 B (~21.9 MB) for win-x64 | 8.7 MB |

- Both packages are published by k2-fsa (maintainer `csukuangfj` = Fangjun Kuang; npm trusted publisher "GitHub Actions"; repository `git+https://github.com/csukuangfj/sherpa-onnx.git`).
  Sources: https://registry.npmjs.org/sherpa-onnx-node , https://registry.npmjs.org/sherpa-onnx , https://registry.npmjs.org/sherpa-onnx-win-x64
- Install: `npm install sherpa-onnx-node` — documented at https://k2-fsa.github.io/sherpa/onnx/javascript-api/install.html

### A.2 Prebuilt binaries vs node-gyp → PREBUILT, no local compilation

- `sherpa-onnx-node` is a pure-JS wrapper (19 files: `sherpa-onnx.js`, `streaming-asr.js`, `non-streaming-tts.js`, `vad.js`, `addon.js`, `addon-static-import.js`, `types.js`, ...). Its `package.json` has **no install/gyp script**, so nothing compiles at install time; it declares the platform packages as `optionalDependencies`.
- The native binaries ship inside each platform tarball. Example: `sherpa-onnx-win-x64` contains `sherpa-onnx.node` + `onnxruntime.dll` + `onnxruntime_providers_shared.dll` + `sherpa-onnx-c-api.dll` + `sherpa-onnx-cxx-api.dll` + `index.js` (`module.exports = require('./sherpa-onnx.node')`). Its `package.json` sets `"os": ["win32"]`, `"cpu": ["x64"]` so npm installs only the matching platform.
  - Tarballs: https://registry.npmjs.org/sherpa-onnx-node/-/sherpa-onnx-node-1.13.5.tgz , https://registry.npmjs.org/sherpa-onnx-win-x64/-/sherpa-onnx-win-x64-1.13.5.tgz
  - Loader: https://github.com/k2-fsa/sherpa-onnx/blob/master/scripts/node-addon-api/lib/addon.js — search order `../build/Release/sherpa-onnx.node` → `../build/Debug/sherpa-onnx.node` → `./node_modules/sherpa-onnx-<platform>-<arch>/sherpa-onnx.node` → `./sherpa-onnx.node` (the `build/` paths are only a fallback for locally compiled addons).
- Official install page: "You don't need to pre-install anything in order to install `sherpa-onnx-node`. That is, you don't need to install a C/C++ compiler. You don't need to install Python. You don't need to install CMake, etc." — https://k2-fsa.github.io/sherpa/onnx/javascript-api/install.html
- Packaging scripts / where the repo states this:
  - Wrapper build+publish: https://github.com/k2-fsa/sherpa-onnx/blob/master/.github/workflows/npm-addon.yaml (copies `scripts/node-addon-api/lib/*.js` into the `sherpa-onnx-node` package; **Node 24**)
  - Binary build+publish per platform: https://github.com/k2-fsa/sherpa-onnx/blob/master/.github/workflows/npm-addon-win-x64.yaml (cmake build → `cmake-js compile` in `scripts/node-addon-api/` → `.github/scripts/node-addon/run.sh` packs `sherpa-onnx.node` + DLLs as `sherpa-onnx-<platform>-<arch>`; **Node 24**); siblings: `npm-addon-win-x86.yaml`, `npm-addon-linux-x64.yaml`, `npm-addon-linux-aarch64.yaml`, `npm-addon-macos.yaml`
  - Templates/scripts: https://github.com/k2-fsa/sherpa-onnx/blob/master/.github/scripts/node-addon/package.json , package-optional.json , run.sh
  - C++ addon source (cmake-js + `node-addon-api` ^8.3.0; devDeps `@types/node` ^24.10.4): https://github.com/k2-fsa/sherpa-onnx/blob/master/scripts/node-addon-api/package.json and .../CMakeLists.txt
- No file named `build-addon.js` or `build-js` exists in the repo (searched the whole master tarball); the packaging entry points are the `npm-addon*.yaml` workflows + `run.sh`.

### A.3 Node versions / N-API

- **Minimum Node: v16** — official install page ("It requires `Node >= v16`", https://k2-fsa.github.io/sherpa/onnx/javascript-api/install.html) and `nodejs-addon-examples/README.md` ("Note: You need `Node >= 16`", https://github.com/k2-fsa/sherpa-onnx/blob/master/nodejs-addon-examples/README.md).
- **CI regression matrix:** Node `["16","17","18","19","21","22"]` on macOS/Ubuntu/Windows — https://github.com/k2-fsa/sherpa-onnx/blob/master/.github/workflows/test-nodejs-addon-npm.yaml (a second workflow tests `["16","22"]`: test-nodejs-addon-api.yaml).
- **Node 24:** the packaging workflows build the published binaries **with Node 24** (`node-version: '24'`). N-API is ABI-stable, so the published `.node` files load on Node 16–24; Node 24 is not (yet) in the CI test matrix. Typecheck devDep is `@types/node` ^24.10.4.
- **N-API version:** the addon is built on `node-addon-api` (^8.3.0) and the sherpa build does not define `NAPI_VERSION` explicitly (no occurrence in `scripts/node-addon-api`); the binaries must load on Node 16 per the CI matrix (i.e. N-API ≤ 8). The repo does not state the compiled NAPI version verbatim — this is inferred (flagged as inference, not verified by binary inspection).
- **WASM package** `sherpa-onnx`: "You need Node >= 18 for this package", single-threaded (its README inside the npm tarball). The `sherpa-onnx-node` README table: native package "node-addon-api | Support multiple threads: Yes | Minimum required node version: v16"; WASM package "WebAssembly | No | v18".
- **Windows:** "No extra setup is needed. The DLLs are located inside node_modules and are found automatically." (https://k2-fsa.github.io/sherpa/onnx/javascript-api/install.html). macOS/Linux need `DYLD_LIBRARY_PATH` / `LD_LIBRARY_PATH` (exact exports in https://github.com/k2-fsa/sherpa-onnx/blob/master/nodejs-addon-examples/README.md).

## B) Offline TTS API (VITS + Kokoro)

### B.1 Classes / functions (from `scripts/node-addon-api/lib/non-streaming-tts.js` and official API ref)

- `new sherpa_onnx.OfflineTts(config)` — sync constructor
- `sherpa_onnx.OfflineTts.createAsync(config)` — **static** async factory → `Promise<OfflineTts>`
- `tts.generate(obj)` → `{ samples: Float32Array, sampleRate: number }` (GeneratedAudio); `obj = { text, sid?, speed?, generationConfig?, enableExternalBuffer? }`
- `tts.generateAsync(obj)` → `Promise<GeneratedAudio>`; may add `onProgress: ({samples, progress}) => 1|0` (return 0/false to cancel)
- `new sherpa_onnx.GenerationConfig({sid, speed, silenceScale, numSteps, referenceAudio, referenceSampleRate, referenceText, extra})`
- Properties: `tts.config`, `tts.numSpeakers`, `tts.sampleRate`
- WAV helpers: `sherpa_onnx.writeWave(filename, {samples, sampleRate})`, `sherpa_onnx.readWave(filename)`
- Official API reference: https://k2-fsa.github.io/sherpa/onnx/javascript-api/examples/api_offline_tts.html ; source: https://github.com/k2-fsa/sherpa-onnx/blob/master/scripts/node-addon-api/lib/non-streaming-tts.js

### B.2 VITS config (model.onnx + tokens.txt + lexicon.txt + .fst files)

```js
const config = {
  model: {
    vits: {
      model:   './vits-icefall-zh-aishell3/model.onnx',
      tokens:  './vits-icefall-zh-aishell3/tokens.txt',
      lexicon: './vits-icefall-zh-aishell3/lexicon.txt',
    },
    debug: true,
    numThreads: 1,
    provider: 'cpu',
  },
  maxNumSentences: 1,
  // FST rules for Chinese normalization (top-level keys; maps to C rule_fsts / rule_fars):
  ruleFsts: './vits-icefall-zh-aishell3/date.fst,./vits-icefall-zh-aishell3/phone.fst,./vits-icefall-zh-aishell3/number.fst,./vits-icefall-zh-aishell3/new_heteronym.fst',
  ruleFars: './vits-icefall-zh-aishell3/rule.far',
};
const tts = new sherpa_onnx.OfflineTts(config);
const audio = tts.generate({ text, generationConfig });   // {samples: Float32Array, sampleRate: number}
sherpa_onnx.writeWave('out.wav', { samples: audio.samples, sampleRate: audio.sampleRate });
```
Source (verbatim): https://github.com/k2-fsa/sherpa-onnx/blob/master/nodejs-addon-examples/test_tts_non_streaming_vits_zh_aishell3.js

- JS keys for VITS (from `GetOfflineTtsVitsModelConfig` in `scripts/node-addon-api/src/non-streaming-tts.cc`): `model`, `lexicon`, `tokens`, `dataDir`, `noiseScale`, `noiseScaleW`, `lengthScale`.
- The C++ config also has `dict_dir` ("Used for Chinese TTS models using jieba") — https://github.com/k2-fsa/sherpa-onnx/blob/master/sherpa-onnx/csrc/offline-tts-vits-model-config.h — **but the current Node addon parser does NOT map a `dictDir` key from the JS config** (verified in non-streaming-tts.cc). Chinese TTS in Node uses `ruleFsts`/`ruleFars` instead (see example).

### B.3 Kokoro TTS — supported, including Chinese voices

**Config (model type `kokoro`):**
```js
const config = {
  model: {
    kokoro: {
      model:   './kokoro-multi-lang-v1_0/model.onnx',
      voices:  './kokoro-multi-lang-v1_0/voices.bin',
      tokens:  './kokoro-multi-lang-v1_0/tokens.txt',
      dataDir: './kokoro-multi-lang-v1_0/espeak-ng-data',
      // Multiple lexicon files are separated by commas.
      lexicon: './kokoro-multi-lang-v1_0/lexicon-us-en.txt,./kokoro-multi-lang-v1_0/lexicon-zh.txt',
    },
    debug: true,
    numThreads: 1,
    provider: 'cpu',
  },
  maxNumSentences: 1,
};
```
- JS keys (from `GetOfflineTtsKokoroModelConfig`): `model`, `voices`, `tokens`, `dataDir`, `lengthScale`, `lexicon`, `lang`. C++ also defines `dict_dir` (https://github.com/k2-fsa/sherpa-onnx/blob/master/sherpa-onnx/csrc/offline-tts-kokoro-model-config.h) but it is not wired into the Node addon parser.
- **The `dict/` dir and `date-zh.fst`/`number-zh.fst`/`phone-zh.fst` are real files in the model tarball** (`kokoro-multi-lang-v1_0/`: model.onnx 310 MB, voices.bin 26 MB, tokens.txt, lexicon-gb-en.txt, lexicon-us-en.txt, lexicon-zh.txt, espeak-ng-data/, dict/, date-zh.fst, number-zh.fst, phone-zh.fst). In the C++ CLI they are passed via `--tts-rule-fsts=...date-zh.fst,phone-zh.fst,number-zh.fst`; in the Node binding the equivalent top-level config key is **`ruleFsts`** (maps to C `rule_fsts`; see macro SHERPA_ONNX_ASSIGN_ATTR_STR(rule_fsts, ruleFsts) in scripts/node-addon-api/src/non-streaming-tts.cc). Official model docs: https://k2-fsa.github.io/sherpa/onnx/tts/pretrained_models/kokoro.html
- Models: `kokoro-multi-lang-v1_0` (Chinese+English, **53 speakers**; https://github.com/k2-fsa/sherpa-onnx/releases/download/tts-models/kokoro-multi-lang-v1_0.tar.bz2), `kokoro-multi-lang-v1_1` (103 speakers) + int8 `kokoro-int8-multi-lang-v1_1.tar.bz2`, English-only `kokoro-en-v0_19`. No file literally named `model.int8.onnx` appears in the official docs; the int8 variant is the v1_1 tarball (model file inside is `model.onnx`, int8-quantized).
- **Chinese (zh) voices** (official ID→name map from the kokoro docs page):

| ID | Voice | ID | Voice |
|---|---|---|---|
| 45 | `zf_xiaobei` | 49 | `zm_yunjian` |
| 46 | `zf_xiaoni`  | 50 | `zm_yunxi`  |
| 47 | `zf_xiaoxiao` | 51 | `zm_yunxia` |
| 48 | `zf_xiaoyi`  | 52 | `zm_yunyang` |

  (IDs 0–44 are English/other voices: `af_*`, `am_*`, `bf_*`, `bm_*`, `ef_dora`, `em_alex`, `ff_siwis`, `hf_*`, `hm_*`, `if_sara`, `im_nicola`, `jf_*`, `jm_kumo`, `pf_dora`, `pm_alex`, `pm_santa`.)

### B.4 Official Node example — Kokoro Chinese+English (sync), copied verbatim

https://github.com/k2-fsa/sherpa-onnx/blob/master/nodejs-addon-examples/test_tts_non_streaming_kokoro_zh_en.js (also rendered at https://k2-fsa.github.io/sherpa/onnx/javascript-api/examples/tts_kokoro_zh_en.html)

```js
// Copyright (c)  2025  Xiaomi Corporation
const sherpa_onnx = require('sherpa-onnx-node');

// please refer to
// https://k2-fsa.github.io/sherpa/onnx/tts/pretrained_models/kokoro.html
// to download model files
function createOfflineTts() {
  const config = {
    model: {
      kokoro: {
        model: './kokoro-multi-lang-v1_0/model.onnx',
        voices: './kokoro-multi-lang-v1_0/voices.bin',
        tokens: './kokoro-multi-lang-v1_0/tokens.txt',
        dataDir: './kokoro-multi-lang-v1_0/espeak-ng-data',
        lexicon:
            './kokoro-multi-lang-v1_0/lexicon-us-en.txt,./kokoro-multi-lang-v1_0/lexicon-zh.txt',
      },
      debug: true,
      numThreads: 1,
      provider: 'cpu',
    },
    maxNumSentences: 1,
  };
  return new sherpa_onnx.OfflineTts(config);
}

const tts = createOfflineTts();

const text =
    '中英文语音合成测试。This is generated by next generation Kaldi using Kokoro without Misaki. 你觉得中英文说的如何呢？';

const generationConfig = new sherpa_onnx.GenerationConfig({
  sid: 48,
  speed: 1.0,
  silenceScale: 0.2,
});

let start = Date.now();
const audio = tts.generate({text, generationConfig});
let stop = Date.now();
const elapsed_seconds = (stop - start) / 1000;
const duration = audio.samples.length / audio.sampleRate;
const real_time_factor = elapsed_seconds / duration;
console.log('Wave duration', duration.toFixed(3), 'seconds');
console.log('Elapsed', elapsed_seconds.toFixed(3), 'seconds');
console.log(
    `RTF = ${elapsed_seconds.toFixed(3)}/${duration.toFixed(3)} =`,
    real_time_factor.toFixed(3));

const filename = 'test-kokoro-zh-en-48.wav';
sherpa_onnx.writeWave(
    filename, {samples: audio.samples, sampleRate: audio.sampleRate});

console.log(`Saved to ${filename}`);
```

- Async variant: `test_tts_non_streaming_kokoro_zh_en_async.js` — `await sherpa_onnx.OfflineTts.createAsync(config)` + `await tts.generateAsync({text, enableExternalBuffer: true, generationConfig, onProgress})` — https://github.com/k2-fsa/sherpa-onnx/blob/master/nodejs-addon-examples/test_tts_non_streaming_kokoro_zh_en_async.js



## C) Streaming ASR API (zipformer2 transducer)

### C.1 Exact class and method names

- Class: **`sherpa_onnx.OnlineRecognizer`**. There is **no `StreamingRecognizer` class** in sherpa-onnx-node — verified in the current JS API (`scripts/node-addon-api/lib/streaming-asr.js`), the official API reference ("OnlineRecognizer", https://k2-fsa.github.io/sherpa/onnx/javascript-api/examples/api_streaming_asr.html), and even the earliest published npm version 1.0.7 (`class OnlineRecognizer`, `addon.createOnlineRecognizer(config)`). The native C API function is `CreateOnlineRecognizer`.
- Constructor: `new sherpa_onnx.OnlineRecognizer(config)` — `OnlineRecognizerConfig`
- Methods: `recognizer.createStream()` → `OnlineStream`; `recognizer.isReady(stream)` → bool; `recognizer.decode(stream)`; `recognizer.isEndpoint(stream)` → bool; `recognizer.reset(stream)`; `recognizer.getResult(stream)` → `{ text: string, tokens: string[], timestamps: number[], is_final: boolean }`
- `OnlineStream`: `stream.acceptWaveform({samples: Float32Array, sampleRate: number})`, `stream.inputFinished()`
- Helper: `new sherpa_onnx.Display(maxWordPerLine)`, `display.print(idx, text)`
- Official API reference: https://k2-fsa.github.io/sherpa/onnx/javascript-api/examples/api_streaming_asr.html

### C.2 Config fields (transducer = encoder/decoder/joiner + tokens)

```js
const config = {
  'featConfig': { 'sampleRate': 16000, 'featureDim': 80 },
  'modelConfig': {
    'transducer': {
      'encoder': './sherpa-onnx-streaming-zipformer-bilingual-zh-en-2023-02-20/encoder-epoch-99-avg-1.onnx',
      'decoder': './sherpa-onnx-streaming-zipformer-bilingual-zh-en-2023-02-20/decoder-epoch-99-avg-1.onnx',
      'joiner':  './sherpa-onnx-streaming-zipformer-bilingual-zh-en-2023-02-20/joiner-epoch-99-avg-1.onnx',
    },
    'tokens': './sherpa-onnx-streaming-zipformer-bilingual-zh-en-2023-02-20/tokens.txt',
    'numThreads': 2,
    'provider': 'cpu',
    'debug': 1,
  }
};
const recognizer = new sherpa_onnx.OnlineRecognizer(config);
```
Other optional top-level keys (official API ref): `decodingMethod` (e.g. `'greedy_search'`), `maxActivePaths`, `enableEndpoint`, `rule1MinTrailingSilence`, `rule2MinTrailingSilence`, `rule3MinUtteranceLength`, `blankPenalty`, `hotwordsFile`, `hotwordsScore`. Model alternatives under `modelConfig`: `transducer` ({encoder, decoder, joiner}), `paraformer` ({encoder, decoder}), `zipformer2Ctc` ({model}), `nemoCtc` ({model}), plus common `tokens`/`numThreads`/`debug`/`provider` (see `types.js` in the npm package and the API ref).

### C.3 Official example (file-based, bilingual zh-en) — copied verbatim

https://github.com/k2-fsa/sherpa-onnx/blob/master/nodejs-addon-examples/test_asr_streaming_transducer.js
```js
// Copyright (c)  2024  Xiaomi Corporation
const sherpa_onnx = require('sherpa-onnx-node');

// Please download test files from
// https://github.com/k2-fsa/sherpa-onnx/releases/tag/asr-models
const config = {
  'featConfig': {
    'sampleRate': 16000,
    'featureDim': 80,
  },
  'modelConfig': {
    'transducer': {
      'encoder':
          './sherpa-onnx-streaming-zipformer-bilingual-zh-en-2023-02-20/encoder-epoch-99-avg-1.onnx',
      'decoder':
          './sherpa-onnx-streaming-zipformer-bilingual-zh-en-2023-02-20/decoder-epoch-99-avg-1.onnx',
      'joiner':
          './sherpa-onnx-streaming-zipformer-bilingual-zh-en-2023-02-20/joiner-epoch-99-avg-1.onnx',
    },
    'tokens':
        './sherpa-onnx-streaming-zipformer-bilingual-zh-en-2023-02-20/tokens.txt',
    'numThreads': 2,
    'provider': 'cpu',
    'debug': 1,
  }
};

const waveFilename =
    './sherpa-onnx-streaming-zipformer-bilingual-zh-en-2023-02-20/test_wavs/0.wav';

const recognizer = new sherpa_onnx.OnlineRecognizer(config);
console.log('Started');
let start = Date.now();
const stream = recognizer.createStream();
const wave = sherpa_onnx.readWave(waveFilename);
stream.acceptWaveform({sampleRate: wave.sampleRate, samples: wave.samples});

const tailPadding = new Float32Array(wave.sampleRate * 0.4);
stream.acceptWaveform({samples: tailPadding, sampleRate: wave.sampleRate});

while (recognizer.isReady(stream)) {
  recognizer.decode(stream);
}
const result = recognizer.getResult(stream);
let stop = Date.now();
console.log('Done');

const elapsed_seconds = (stop - start) / 1000;
const duration = wave.samples.length / wave.sampleRate;
const real_time_factor = elapsed_seconds / duration;
console.log('Wave duration', duration.toFixed(3), 'seconds');
console.log('Elapsed', elapsed_seconds.toFixed(3), 'seconds');
console.log(
    `RTF = ${elapsed_seconds.toFixed(3)}/${duration.toFixed(3)} =`,
    real_time_factor.toFixed(3));
console.log(waveFilename);
console.log('result
', result);
```

Microphone loop (endpoint + reset), https://github.com/k2-fsa/sherpa-onnx/blob/master/nodejs-addon-examples/test_asr_streaming_transducer_microphone.js:
```js
stream.acceptWaveform({sampleRate: targetSampleRate, samples: resampled});
while (recognizer.isReady(stream)) { recognizer.decode(stream); }
const isEndpoint = recognizer.isEndpoint(stream);
const text = recognizer.getResult(stream).text.toLowerCase();
if (text.length > 0 && lastText != text) { lastText = text; display.print(segmentIndex, lastText); }
if (isEndpoint) {
  if (text.length > 0) { lastText = text; segmentIndex += 1; }
  recognizer.reset(stream);
}
```
(Note: the file example appends 0.4 s of silence as tail padding instead of `inputFinished()`; `stream.inputFinished()` is part of the API and documented at https://k2-fsa.github.io/sherpa/onnx/javascript-api/examples/api_streaming_asr.html. In the example code above, `acceptWaveform` takes an object `{sampleRate, samples}` — i.e. `acceptWaveform`/`acceptSamples` naming: the official API is `acceptWaveform({samples, sampleRate})`.)

### C.4 Chinese streaming models (official docs)

- Bilingual zh–en: `sherpa-onnx-streaming-zipformer-bilingual-zh-en-2023-02-20` (used by the Node example; https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-streaming-zipformer-bilingual-zh-en-2023-02-20.tar.bz2)
- Chinese-only zipformer2 transducer with `encoder.int8.onnx`/`decoder.onnx`/`joiner.int8.onnx`/`tokens.txt` layout: **`sherpa-onnx-streaming-zipformer-zh-int8-2025-06-30`** and **`sherpa-onnx-streaming-zipformer-zh-xlarge-int8-2025-06-30`** — documented at https://k2-fsa.github.io/sherpa/onnx/pretrained_models/online-transducer/zipformer-transducer-models.html (download: https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-streaming-zipformer-zh-int8-2025-06-30.tar.bz2)
- Others on the same page: `sherpa-onnx-streaming-zipformer-multi-zh-hans-2023-12-12`, `icefall-asr-zipformer-streaming-wenetspeech-20230615`; repo README also lists `sherpa-onnx-streaming-zipformer-small-bilingual-zh-en-2023-02-16`, `sherpa-onnx-streaming-zipformer-zh-14M-2023-02-23` (https://github.com/k2-fsa/sherpa-onnx/blob/master/README.md)



## D) VAD API (silero_vad.onnx)

### D.1 Exact API (official API reference + example)

- Constructor: `new sherpa_onnx.Vad(config, bufferSizeInSeconds)` (constructor, not a static `create`)
- Config:
```js
const config = {
  sileroVad: {
    model: './silero_vad.onnx',
    threshold: 0.5,
    minSpeechDuration: 0.25,
    minSilenceDuration: 0.5,
    maxSpeechDuration: 5,     // optional
    windowSize: 512,
  },
  // tenVad: { model: '', threshold: 0.5, minSpeechDuration: 0.25, minSilenceDuration: 0.5, windowSize: 256 },
  sampleRate: 16000,
  debug: true,
  numThreads: 1,
};
const vad = new sherpa_onnx.Vad(config, 60);   // 60 s internal buffer
```
- Methods: `vad.acceptWaveform(samples)` (Float32Array; feed in chunks of `windowSize`), `vad.isDetected()` → bool, `vad.isEmpty()` → bool, `vad.front(enableExternalBuffer=true)` → `{ start: number, samples: Float32Array }`, `vad.pop()`, `vad.flush()`, `vad.clear()`, `vad.reset()`; property `vad.config`
- Official API reference: https://k2-fsa.github.io/sherpa/onnx/javascript-api/examples/api_vad.html ; source: https://github.com/k2-fsa/sherpa-onnx/blob/master/scripts/node-addon-api/lib/vad.js
- Model: `silero_vad.onnx` from https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/silero_vad.onnx (docs: https://k2-fsa.github.io/sherpa/onnx/vad/silero-vad.html). Ten-VAD (`ten-vad.onnx`, windowSize 256) is supported via the `tenVad` config key.

### D.2 Official example — VAD microphone (verbatim, key part)

https://github.com/k2-fsa/sherpa-onnx/blob/master/nodejs-addon-examples/test_vad_microphone.js
```js
const sherpa_onnx = require('sherpa-onnx-node');

function createVad() {
  // please download silero_vad.onnx from
  // https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/silero_vad.onnx
  const config = {
    sileroVad: {
      model: './silero_vad.onnx',
      threshold: 0.5,
      minSpeechDuration: 0.25,
      minSilenceDuration: 0.5,
      windowSize: 512,
    },
    sampleRate: 16000,
    debug: true,
    numThreads: 1,
  };
  const bufferSizeInSeconds = 60;
  return new sherpa_onnx.Vad(config, bufferSizeInSeconds);
}

const vad = createVad();
// ... (mic stream → resample to 16 kHz via sherpa_onnx.LinearResampler) ...
// in the audio callback:
vad.acceptWaveform(samples);                       // windowSize samples at a time
if (vad.isDetected() && !printed) { console.log('Detected speech'); printed = true; }
if (!vad.isDetected()) { printed = false; }
while (!vad.isEmpty()) {
  const segment = vad.front();
  vad.pop();
  sherpa_onnx.writeWave(filename, {samples: segment.samples, sampleRate: vad.config.sampleRate});
}
```



## E) Known issues / notes

1. **Node 24 support status:** No known blocker. The published binaries are **built with Node 24** in the official packaging CI (`npm-addon*.yaml` / `npm.yaml` all use `node-version: '24'`), and Node-API is ABI-stable, so they run on Node 18/20/22/24 (and back to 16). The official *regression* matrix currently covers Node 16–22 (https://github.com/k2-fsa/sherpa-onnx/blob/master/.github/workflows/test-nodejs-addon-npm.yaml); Node 24 is not yet in that matrix but is the build toolchain.
2. **Windows-specific notes:** On Windows **no environment variable is needed** — "The DLLs are located inside node_modules and are found automatically" (https://k2-fsa.github.io/sherpa/onnx/javascript-api/install.html). On macOS/Linux you must set `DYLD_LIBRARY_PATH` / `LD_LIBRARY_PATH` to the installed `node_modules/sherpa-onnx-<platform>-<arch>` dir (https://github.com/k2-fsa/sherpa-onnx/blob/master/nodejs-addon-examples/README.md). Windows x86 (ia32) binaries are still published (workflow `npm-addon-win-x86.yaml`).
3. **CPU/threads:** set `numThreads` in each config. The node-addon binding supports **multiple threads**; the WASM package `sherpa-onnx` does **not** (npm README table; https://k2-fsa.github.io/sherpa/onnx/javascript-api/install.html). The FAQ "the given version 17 is not supported, only version 1 to 10" concerns **ONNX Runtime versions**, not Node (https://k2-fsa.github.io/sherpa/onnx/faqs/index.html).
4. **VAD + streaming ASR in one process:** supported. Official examples run VAD together with ASR in one process (VAD + non-streaming Whisper `test_vad_with_non_streaming_asr_whisper.js`; VAD + non-streaming Moonshine; microphone VAD+ASR `test_vad_asr_non_streaming_*_microphone.js` — https://github.com/k2-fsa/sherpa-onnx/blob/master/nodejs-addon-examples/README.md). A dedicated VAD + *streaming* ASR Node example does not exist in `nodejs-addon-examples/`, but nothing prevents it: streaming ASR is driven manually (`acceptWaveform` → `isReady`/decode → `isEndpoint`/reset) and the VAD produces `{start, samples}` segments you can feed straight into a stream.
5. **No `StreamingRecognizer` class** anywhere in sherpa-onnx-node (checked current 1.13.5 and earliest published 1.0.7). Use `OnlineRecognizer`.
6. **Unverifiable from this environment:** `sherpa-onnx.com` (DNS NXDOMAIN) — treat any claim about it as unverified; the official docs site is https://k2-fsa.github.io/sherpa/onnx/. Also note `k2-fsa/sherpa-onnx` master has **no `docs/` directory**; the docs sources live in the `k2-fsa/sherpa` repo (`docs/source/onnx/...`) per the docs' own "Edit on GitHub" links.

---

## Sources index (primary)

- npm: https://www.npmjs.com/package/sherpa-onnx-node , https://www.npmjs.com/package/sherpa-onnx , https://registry.npmjs.org/sherpa-onnx-win-x64 (+ tarballs listed in A.1)
- Docs install: https://k2-fsa.github.io/sherpa/onnx/javascript-api/install.html
- Docs examples index: https://k2-fsa.github.io/sherpa/onnx/javascript-api/examples/index.html
- Docs API refs: https://k2-fsa.github.io/sherpa/onnx/javascript-api/examples/api_offline_tts.html , .../api_streaming_asr.html , .../api_vad.html
- Docs Kokoro: https://k2-fsa.github.io/sherpa/onnx/tts/pretrained_models/kokoro.html ; Node Kokoro example: .../examples/tts_kokoro_zh_en.html
- Docs streaming zh models: https://k2-fsa.github.io/sherpa/onnx/pretrained_models/online-transducer/zipformer-transducer-models.html
- Docs VAD: https://k2-fsa.github.io/sherpa/onnx/vad/silero-vad.html
- Repo examples: https://github.com/k2-fsa/sherpa-onnx/tree/master/nodejs-addon-examples (README: .../blob/master/nodejs-addon-examples/README.md)
- Repo JS API source: https://github.com/k2-fsa/sherpa-onnx/tree/master/scripts/node-addon-api/lib
- Repo C++ addon: https://github.com/k2-fsa/sherpa-onnx/tree/master/scripts/node-addon-api/src ; C++ configs: .../blob/master/sherpa-onnx/csrc/offline-tts-vits-model-config.h , .../offline-tts-kokoro-model-config.h
- Packaging workflows: https://github.com/k2-fsa/sherpa-onnx/tree/master/.github/workflows (npm-addon.yaml, npm-addon-win-x64.yaml, npm-addon-win-x86.yaml, npm-addon-linux-x64.yaml, npm-addon-linux-aarch64.yaml, npm-addon-macos.yaml, npm.yaml, test-nodejs-addon-npm.yaml, test-nodejs-addon-api.yaml)
- Packaging scripts/templates: https://github.com/k2-fsa/sherpa-onnx/tree/master/.github/scripts/node-addon (package.json, package-optional.json, run.sh)
- WASM examples: https://github.com/k2-fsa/sherpa-onnx/tree/master/nodejs-examples




