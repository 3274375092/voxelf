# Node.js 本地语音识别/合成技术栈选型(Windows / Node.js 24)

> 任务:把 voxelf(Rust + sherpa-onnx)已跑通的本地语音管线搬到 Node.js(TypeScript)侧,做成 DeepSeek Harness 插件。
> 全部结论基于 2026-08-14 对 npm registry(npmjs + npmmirror)、k2-fsa/sherpa-onnx 官方源码/文档的检索,
> 并在本机(Node v24.15.0 / npm 12.0.1 / Windows)用本地模型(D:\code\voxelf\assets\models)做了**实测冒烟验证**。

---

## 0. 结论摘要(推荐栈)

| 用途 | 方案 | 版本 | 安装 |
|---|---|---|---|
| 离线 TTS(VITS / Kokoro 中文) | **sherpa-onnx-node** + 平台包 **sherpa-onnx-win-x64**(预编译,免编译) | 1.13.5 | npm i sherpa-onnx-node |
| 流式 ASR(zipformer2 中文) | 同上(同一包内 OnlineRecognizer) | 1.13.5 | 同上 |
| VAD(silero) | 同上(同一包内 Vad) | 1.13.5 | 同上 |
| 麦克风采集(可选) | node-cpal(自带 win32-x64 预编译) | 0.1.1 | npm i node-cpal |
| 播放(默认) | PowerShell System.Media.SoundPlayer 子进程(零依赖) | 系统自带 | 无 |
| 播放(低延迟/流式,可选) | ffplay(需 ffmpeg) | — | winget install ffmpeg |
| 播放(仅当有 MSVC 工具链) | speaker(官方示例依赖,node-gyp 编译) | 0.5.5 | 不推荐默认 |

**一句话**:用 sherpa-onnx-node@1.13.5(Windows 上自动拉取 sherpa-onnx-win-x64@1.13.5 预编译二进制),
与 voxelf 现有模型**完全兼容**(同源同版本),三个能力(TTS/流式 ASR/VAD)一个包全搞定;
播放先用零依赖的 PowerShell SoundPlayer,需要流式再上 ffplay。

> 注意(本机 npm 12 环境):DSH 注入的 npm_config_allow_scripts 环境变量会让项目内 npm install 报 EALLOWSCRIPTS。
> 安装前 Remove-Item Env:npm_config_allow_scripts(或用 package.json 的 allowScripts 字段)即可;普通用户机器无此变量则无此问题。

---

## 1. sherpa-onnx-node 事实核查

### 1.1 存在性与版本
- npm 包 **sherpa-onnx-node** 存在,latest = **1.13.5**(npmjs 与 npmmirror 同步,2026-08-11 更新,官方维护活跃)。
  - https://www.npmjs.com/package/sherpa-onnx-node
  - https://registry.npmmirror.com/sherpa-onnx-node (npm view 实测)
- 姊妹包 **sherpa-onnx**(WebAssembly 版)同为 1.13.5:https://www.npmjs.com/package/sherpa-onnx
  (WASM 单线程,Node>=18,仅作兜底,见 §4)

### 1.2 win32-x64 预编译(是,免编译)
- sherpa-onnx-node 本体只有 JS 包装(58 KB / 19 文件,**无任何 .node 二进制**);
  原生 addon 通过 optionalDependencies 的**平台包**分发:
  sherpa-onnx-win-x64、sherpa-onnx-win-ia32、sherpa-onnx-darwin-x64、sherpa-onnx-darwin-arm64、
  sherpa-onnx-linux-x64、sherpa-onnx-linux-arm64(均 ^1.13.5,package.json 原文)。
- **sherpa-onnx-win-x64@1.13.5**:解压 **23.0 MB** / 8 文件(tgz 8.5 MB),内容:
  sherpa-onnx.node(671 KB)、onnxruntime.dll(17.4 MB)、sherpa-onnx-c-api.dll(4.6 MB)、
  sherpa-onnx-cxx-api.dll(258 KB)、onnxruntime_providers_shared.dll。os=win32, cpu=x64。
- 官方文档原话:"**You don't need to install a C/C++ compiler... no Python, no CMake**";
  Windows 下 "No extra setup is needed. The DLLs are located inside node_modules and are found automatically."
  - https://k2-fsa.github.io/sherpa/onnx/javascript-api/install.html
- **实测**:npm i sherpa-onnx-node(npmmirror,2 秒)后 require('sherpa-onnx-node')
  在本机 **Node v24.15.0** 成功加载:version=1.13.5, onnxruntimeVersion=1.27.1, gitSha1=3dc7c569。

### 1.3 安装体积
- 合计 ≈ **23 MB**(sherpa-onnx-node 58 KB + sherpa-onnx-win-x64 23 MB;实测 node_modules 中 21.9 MB)。
- 对比:onnxruntime-node@1.27.0 解压 **270 MB**(见 §4)。

### 1.4 Node 24 兼容性
- 官方要求 **Node >= 16**(README 与 install 文档一致);基于 node-addon-api 8.x(N-API),ABI 稳定。
- **实测 Node v24.15.0 全部冒烟通过**(TTS 合成、流式 ASR 解码、VAD 检出)。

---

## 2. API 形态与本地模型兼容性(均经实测)

包导出(从发布包 sherpa-onnx.js 源码确认):
OnlineRecognizer / OfflineTts / GenerationConfig / Vad / CircularBuffer / LinearResampler /
readWave / writeWave / version / onnxruntimeVersion ...

### 2.1 离线 TTS(输出 Float32Array + sampleRate,可 writeWave 存 wav)
```js
const sherpa_onnx = require('sherpa-onnx-node');
const tts = new sherpa_onnx.OfflineTts(config);            // 或 await OfflineTts.createAsync(config)
const audio = tts.generate({ text, generationConfig });    // { samples: Float32Array, sampleRate }
sherpa_onnx.writeWave('out.wav', { samples: audio.samples, sampleRate: audio.sampleRate });
```

**(a) VITS —— assets/models/vits-zh 兼容 ✓**(官方示例 nodejs-addon-examples/test_tts_non_streaming_vits_zh_ll.js 同款结构)
```js
model: { vits: { model, tokens, lexicon }, numThreads, provider: 'cpu' },
ruleFsts: 'phone.fst,date.fst,number.fst,new_heteronym.fst'(逗号分隔路径)
```
- 实测:vits-zh 初始化 820 ms;合成 11 字 → 2.91 s 音频耗时 1.01 s(RTF≈0.35);sampleRate=16000,numSpeakers=5。
- 注意:自 sherpa-onnx **v1.12.15** 起 vits/kokoro/matcha 的 dict_dir(jieba dict)**被忽略**(源码
  offline-tts-vits-model-config.cc:82 原文 "you don't need to provide dict_dir"),本地 vits-zh/dict/ 不再需要。
- 文本规范化 FST 用 ruleFsts(源码 non-streaming-tts.cc: SHERPA_ONNX_ASSIGN_TTS_ATTR 确认支持)。

**(b) Kokoro —— assets/models/kokoro 兼容 ✓**(官方示例 test_tts_non_streaming_kokoro_zh_en.js)
```js
model: { kokoro: { model, voices, tokens,
                   dataDir: '.../espeak-ng-data',
                   lexicon: 'lexicon-zh.txt,lexicon-us-en.txt' }, numThreads, provider: 'cpu' }
```
- 实测:本地 kokoro/model.int8.onnx(109 MB)+ voices.bin(51 MB)加载 1.36 s;合成 4.94 s 音频耗时 5.8 s
  (RTF≈1.17,2 线程);sampleRate=**24000**, numSpeakers=**103**。
- 注意:本地模型实为 **kokoro-int8-multi-lang-v1_1(103 音色)**,不是 v1_0(53 音色)。
  官方 v1_1 音色表(https://k2-fsa.github.io/sherpa/onnx/tts/all/Chinese-English/kokoro-multi-lang-v1_1.html):
  中文音色为 **zf_001(sid=3)…zf_099(sid=57)**、zm_*(58+);sid 0=af_maple、1=af_sol、2=bf_vale(非中文)。
  注意:voxelf Rust 侧把 xiaobei→0 / xiaoni→1 / xiaoxiao→2 / xiaoyi→3,与本地 v1_1 官方表**不一致**
  (这些是 v1_0 的命名,且 v1_0 里对应 45–48)。迁移到 Node 时建议直接用 sid(如 3=zf_001),并回头核对 Rust 侧。

### 2.2 流式 ASR —— assets/models/asr-zh 兼容 ✓(官方示例 test_asr_streaming_transducer.js)
```js
new sherpa_onnx.OnlineRecognizer({
  featConfig: { sampleRate: 16000, featureDim: 80 },
  modelConfig: { transducer: { encoder, decoder, joiner },
                 tokens, numThreads, provider: 'cpu', modelType: 'zipformer2' },
  decodingMethod: 'greedy_search', enableEndpoint: false,
});
const stream = recognizer.createStream();
stream.acceptWaveform({ samples, sampleRate });   // Float32Array
while (recognizer.isReady(stream)) recognizer.decode(stream);
stream.inputFinished();
while (recognizer.isReady(stream)) recognizer.decode(stream);
const r = recognizer.getResult(stream);           // { text, tokens, timestamps }
```
- 实测:本地 asr-zh 的 encoder.int8.onnx / decoder.onnx / joiner.int8.onnx + tokens.txt,
  modelType: 'zipformer2'(voxelf 默认),test.wav 全量喂入 decode 仅 **418 ms**,输出合理中文文本。
- 接口形态与 Rust 侧一一对应(createStream / acceptWaveform / is_ready+decode / input_finished / get_result)。

### 2.3 VAD —— assets/models/vad/silero_vad.onnx ✓(官方示例 test_vad_microphone.js)
```js
const vad = new sherpa_onnx.Vad({
  sileroVad: { model, threshold: 0.5, minSpeechDuration: 0.25, minSilenceDuration: 0.5, windowSize: 512 },
  sampleRate: 16000, numThreads: 1, debug: false,
}, 30 /* bufferSizeInSeconds */);
vad.acceptWaveform(samples);          // Float32Array(无采样率参数,在 config 里)
vad.isDetected(); vad.isEmpty();
const seg = vad.front();              // { start, samples }
vad.pop(); vad.reset(); vad.flush();
```
- 实测:本地 silero_vad.onnx 正常检出语音。
- 注意:官方示例按 windowSize(512)分块 + CircularBuffer 喂入;一次性喂整段后立即 front() 可能为空
  (段在检测到后续静音后才弹出),收尾需补喂静音或调 flush()。

---

## 3. Windows 播放 PCM/wav 的方案对比

| 方案 | 版本 | win32 预编译 | 维护状态 | 结论 |
|---|---|---|---|---|
| speaker(tootallnate/node-speaker) | 0.5.5 | 无(install=node-gyp rebuild,内含 mpg123 源码要编译) | 陈旧(2024-05 仅元数据更新) | 官方文档推荐,但**需 VS Build Tools + Python**,Node 24 风险中高;仅当机器有工具链才用 |
| naudiodon(PortAudio 封装) | 2.3.6 | 无(install=node-gyp rebuild) | 2022-05 后停止更新 | 不推荐 |
| node-speaker | — | npm 上**不存在**(npmjs/npmmirror 均 E404) | — | 无此包 |
| node-cpal(官方麦克风依赖) | 0.1.1 | 自带 bin/win32-x64/index.node(558 KB) | 2025-03 更新 | 推荐用于**采集**;输出侧有 beep 示例但主职是 mic |
| PowerShell System.Media.SoundPlayer(子进程) | 系统自带 | 零依赖 | Windows 内置 | **推荐默认**:wav 播放,本机 pwsh 7.6.4 实测 Load 成功;启动开销 ~200–400 ms |
| ffplay(ffmpeg) | — | 外部程序 | 活跃 | 需安装 ffmpeg(本机未装);支持**流式** raw PCM(ffplay -f f32le -ar 24000 -ac 1 -)与低延迟 |

**推荐**:DSH 插件默认 **PowerShell SoundPlayer 子进程**(零依赖、免编译、稳定)——TTS 是"整句合成后播放"的场景完全够用;
需要流式/低延迟播放时再引入 **ffplay**;麦克风采集用 **node-cpal**(有 win32 预编译)。
不要在插件默认路径上依赖 node-gyp。

---

## 4. 备选方案代价对比

1. **onnxruntime-node@1.27.0 + 自实现**:解压 270 MB;VITS 需自实现文本→音素(FST/jieba)、对齐、声码器;
   Kokoro 需自实现 espeak-ng 音素化(JS 无现成库);zipformer2 流式解码需自实现贪心/beam search。
   **代价:数周~数月,且 Kokoro 基本不可行。不推荐。**
2. **Rust 侧车(voxelf 现有 sherpa-onnx Rust binding)**:Node 通过 stdio JSON-RPC / HTTP 调用 voxelf 子进程。
   代价:协议层 + 进程生命周期管理;收益:零重写、零编译风险、复用已调好的 VAD/尾音补偿逻辑。
   适合"Node 只做壳"的场景;缺点是多一个常驻进程。
3. **sherpa-onnx(WASM)@1.13.5**:零原生依赖(15 MB / wasm 14.8 MB),但**单线程**、RTF 明显差,Node>=18;
   只作无原生权限环境的兜底。
4. **sherpa-onnx-node(推荐)**:与 Rust 侧同源同版本,模型零改动,实测三能力全通。代价仅是 23 MB 运行时。

---

## 5. 最小代码草图(基于本机实测通过的 smoke 脚本精简)

### 5.1 TTS 合成(kokoro 中文 + vits 二选一)
```js
const sherpa_onnx = require('sherpa-onnx-node');
const M = 'D:/code/voxelf/assets/models';

function makeTts(kind) {
  const model = kind === 'kokoro'
    ? { kokoro: { model: M + '/kokoro/model.int8.onnx', voices: M + '/kokoro/voices.bin',
                  tokens: M + '/kokoro/tokens.txt', dataDir: M + '/kokoro/espeak-ng-data',
                  lexicon: M + '/kokoro/lexicon-zh.txt,' + M + '/kokoro/lexicon-us-en.txt' } }
    : { vits: { model: M + '/vits-zh/model.onnx', tokens: M + '/vits-zh/tokens.txt',
                lexicon: M + '/vits-zh/lexicon.txt' } };
  const cfg = { model: { ...model, numThreads: 2, provider: 'cpu' }, maxNumSentences: 1 };
  if (kind === 'vits') cfg.ruleFsts = [ 'phone','date','number','new_heteronym' ]
    .map(f => M + '/vits-zh/' + f + '.fst').join(',');
  return new sherpa_onnx.OfflineTts(cfg);
}

const tts = makeTts('kokoro');                    // 24000 Hz;中文音色 zf_001 用 sid=3
const audio = tts.generate({
  text: '你好，世界，这是本地语音合成测试。',
  generationConfig: new sherpa_onnx.GenerationConfig({ sid: 3, speed: 1.0, silenceScale: 0.2 }),
});
sherpa_onnx.writeWave('out.wav', { samples: audio.samples, sampleRate: audio.sampleRate });
console.log(tts.sampleRate, audio.samples.length / audio.sampleRate);
```

### 5.2 流式 ASR + VAD(silero)
```js
const sherpa_onnx = require('sherpa-onnx-node');
const M = 'D:/code/voxelf/assets/models';

const recognizer = new sherpa_onnx.OnlineRecognizer({
  featConfig: { sampleRate: 16000, featureDim: 80 },
  modelConfig: { transducer: { encoder: M + '/asr-zh/encoder.int8.onnx',
                               decoder: M + '/asr-zh/decoder.onnx',
                               joiner: M + '/asr-zh/joiner.int8.onnx' },
                 tokens: M + '/asr-zh/tokens.txt',
                 numThreads: 2, provider: 'cpu', modelType: 'zipformer2' },
  decodingMethod: 'greedy_search', enableEndpoint: false,
});
const vad = new sherpa_onnx.Vad({
  sileroVad: { model: M + '/vad/silero_vad.onnx', threshold: 0.5,
               minSpeechDuration: 0.25, minSilenceDuration: 0.5, windowSize: 512 },
  sampleRate: 16000, numThreads: 1,
}, 30);

function feed(samples) {                       // samples: Float32Array@16k
  vad.acceptWaveform(samples);
  while (!vad.isEmpty()) {
    const seg = vad.front(); vad.pop();         // 一段完整语音
    const stream = recognizer.createStream();
    for (let i = 0; i < seg.samples.length; i += 1600) {
      stream.acceptWaveform({ sampleRate: 16000, samples: seg.samples.subarray(i, i + 1600) });
      while (recognizer.isReady(stream)) recognizer.decode(stream);
    }
    stream.inputFinished();
    while (recognizer.isReady(stream)) recognizer.decode(stream);
    console.log('FINAL:', recognizer.getResult(stream).text);
  }
}
```

### 5.3 播放(wav → SoundPlayer;ffplay 备选)
```js
const { execFile } = require('child_process');

async function playWavSoundPlayer(wavPath) {    // 零依赖,整段播放
  const cmd = "(New-Object System.Media.SoundPlayer('" + wavPath.replace(/'/g, "''") + "')).PlaySync()";
  await new Promise((res, rej) => execFile('powershell', [ '-NoProfile', '-Command', cmd ],
    (e) => (e ? rej(e) : res())));
}

async function playPcmFfplay(f32, sampleRate) { // 流式播放备选:需 ffmpeg
  const { spawn } = require('child_process');
  const ffplay = spawn('ffplay', [ '-nodisp', '-autoexit', '-loglevel', 'quiet',
    '-f', 'f32le', '-ar', String(sampleRate), '-ac', '1', '-' ]);
  return new Promise((res, rej) => {
    ffplay.on('error', rej).on('close', res);
    ffplay.stdin.on('error', () => {});         // EPIPE = 播放结束
    ffplay.stdin.write(Buffer.from(f32.buffer, f32.byteOffset, f32.byteLength));
    ffplay.stdin.end();
  });
}
// 用法:先把 TTS 的 {samples, sampleRate} 用 sherpa_onnx.writeWave 落盘,再 playWavSoundPlayer('out.wav')
```

---

## 6. 风险清单

1. **npm 12 安装策略(本机)**:DSH 注入 npm_config_allow_scripts 会让项目内 install 报 EALLOWSCRIPTS;
   需先 Remove-Item Env:npm_config_allow_scripts 或写 package.json allowScripts。普通用户 npm<12 无此问题。
2. **Kokoro 音色映射**:本地 kokoro 是 v1_1(103 音色,zf_001 起),voxelf Rust 侧 xiaobei→0..3 的映射与
   官方 v1_1 表不符(sid 0=af_maple 非中文);Node 侧直接用 sid,并回头核对 Rust 侧音色配置。
3. **TTS 同步阻塞**:generate() 是同步调用(kokoro RTF≈1.17),插件中务必放 worker_thread 或用
   OfflineTts.createAsync + generateAsync(支持 onProgress 流式回调、可取消)。
4. **播放无音频设备**:headless/服务场景 SoundPlayer 可能静默失败或抛错,需降级(丢弃音频/记日志)。
5. **VAD 弹出节奏**:段在检测到静音后才 front() 可见;收尾要补静音或 flush(),别在整段喂完后立即取。
6. **模型体积**:kokoro 109 MB + voices 51 MB、asr ~100 MB 在插件首次加载时会有明显耗时/内存峰值;
   建议懒加载、单例、按需创建。
7. **版本锁定**:npm 包与 sherpa-onnx Rust crate(1.13.4)同源;升级 npm 包时注意 modelType/API 兼容,
   官方要求 "always use the latest version"。
8. **杀软/Defender**:首次加载 17 MB onnxruntime.dll 可能有延迟或误报,插件需容忍加载抖动。
9. **node-cpal**:有 win32-x64 预编译,但 0.1.1 较新;采集链路需单独验收(采样率重采样用包内 LinearResampler)。

---

## 7. 来源(全部一手)

- npm 包:sherpa-onnx-node / sherpa-onnx-win-x64 / sherpa-onnx / speaker / naudiodon / node-cpal / onnxruntime-node
  https://www.npmjs.com/package/sherpa-onnx-node · https://registry.npmmirror.com(实测 npm view / tgz 解包)
- 官方安装文档:https://k2-fsa.github.io/sherpa/onnx/javascript-api/install.html ("Node >= v16"、"无需编译器"、Windows DLL 自动加载)
- 官方示例(仓库内 nodejs-addon-examples/):
  https://github.com/k2-fsa/sherpa-onnx/tree/master/nodejs-addon-examples
  (test_tts_non_streaming_kokoro_zh_en.js / test_tts_non_streaming_vits_zh_ll.js /
   test_asr_streaming_transducer.js / test_vad_microphone.js)
- 原生 addon 源码(JS 字段→C 结构映射):
  https://github.com/k2-fsa/sherpa-onnx/tree/master/scripts/node-addon-api/src
  (non-streaming-tts.cc / streaming-asr.cc / vad.cc / c-api.h)
- C++ 侧 dict_dir 忽略说明:https://github.com/k2-fsa/sherpa-onnx/blob/master/sherpa-onnx/csrc/offline-tts-vits-model-config.cc
- Kokoro 音色表:v1_1(103 音色):https://k2-fsa.github.io/sherpa/onnx/tts/all/Chinese-English/kokoro-multi-lang-v1_1.html
  v1_0(53 音色):https://k2-fsa.github.io/sherpa/onnx/tts/pretrained_models/kokoro.html
- 模型发布:https://github.com/k2-fsa/sherpa-onnx/releases/tag/tts-models(kokoro-int8-multi-lang-v1_1.tar.bz2)

## 附:本机实测记录(2026-08-14)
- Node v24.15.0 / npm 12.0.1 / Windows 10.0.26200
- npm i sherpa-onnx-node(npmmirror)→ sherpa-onnx-node(0.1 MB)+ sherpa-onnx-win-x64(21.9 MB)
- require 成功:version 1.13.5 / onnxruntime 1.27.1 / gitSha 3dc7c569
- kokoro TTS:init 1.36 s, 24 kHz, 103 音色, 合成 4.94 s 音频耗时 5.8 s
- vits TTS:init 0.82 s, 16 kHz, 5 音色, 合成 2.91 s 音频耗时 1.01 s
- 流式 ASR(asr-zh, zipformer2):decode 418 ms,输出合理中文文本
- VAD(silero_vad.onnx):正常检出
- 冒烟脚本位于 D:\code\voxelf\.research-tmp\smoke\(tts-test.js / asr-test.js / 生成的 wav)

## 8. 交叉核对(独立子代理报告,2026-08-14)

独立研究子代理(逐条核对 npm 注册表 tarball + k2-fsa/sherpa-onnx master 源码 + 官方文档)产出
**D:\code\voxelf\sherpa-onnx-nodejs-binding-research.md**(407 行,每条附一手 URL),与本文件结论**完全一致**,
补充/修正如下:

1. **Node 24 官方状态**:发布二进制**就是用 Node 24 构建的**(.github/workflows/npm-addon*.yaml 均为 node-version: '24'),
   N-API 稳定故可加载于 Node 16–24;官方 CI 回归矩阵目前为 Node 16–22(Node 24 尚未进测试矩阵)。
2. **文档 URL 修正**:官方文档在 **k2-fsa.github.io/sherpa/onnx/**(源码在 k2-fsa/sherpa 仓库 docs/source/onnx/);
   **sherpa-onnx.com 无法解析(DNS NXDOMAIN)**,不要引用。
3. **dictDir 未接入 Node addon**:C++ 配置里有 dict_dir(jieba),但 Node addon 解析器**未映射** dictDir 键
   (与本文档"v1.12.15 起忽略"结论一致:传了也不生效,无需传)。
4. **kokoro 的 dict/ 与 date-zh.fst / number-zh.fst / phone-zh.fst 是官方 tarball 里的真实文件**,
   中文数字/日期规范化可经顶层 **ruleFsts** 传入(与 vits 同机制);官方 Node 示例未传、本文档实测不传也可用,属可选增强。
5. **无 StreamingRecognizer**:该类名在 sherpa-onnx-node 中不存在(回溯到最早的 1.0.7 版本也没有),
   流式 ASR 统一叫 **OnlineRecognizer**(与本文档一致)。
