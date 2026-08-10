// 二次元美少女 v5: 64x64 高精度版
// 字符: . 透明 | O 轮廓 | P 发色 | p 发高光 | q 发阴影 | S 皮肤 | s 肤阴影
//       E 眼睛 | e 瞳孔 | H 高光 | C 腮红 | M 嘴 | m 嘴内 | B 水手服 | b 衣阴影
//       W 白 | w 白阴影 | D 裙 | d 裙阴影 | F 蝴蝶结 | f 蝴蝶结阴影
function row(segments) {
  const cells = new Array(64).fill('.');
  for (const [s, e, c] of segments) {
    for (let x = s; x <= e; x++) cells[x] = c;
  }
  const out = cells.join('');
  if (out.length !== 64) throw new Error(out.length + ' != 64');
  return out;
}
const seg = (s, e, c) => [s, e, c];

// 部件: 每个返回 [行号, 区间数组]
const parts = [];

// ---- 双马尾(左右): r2..r30, 轮廓 8-15/48-55, 内部 10-13/50-53, 高光 10-11, 阴影 12-13 ----
for (let r = 2; r <= 30; r++) {
  const l = [seg(8, 15, 'O'), seg(10, 13, 'P'), seg(10, 11, 'p'), seg(12, 13, 'q')];
  const rr = [seg(48, 55, 'O'), seg(50, 53, 'P'), seg(50, 51, 'p'), seg(52, 53, 'q')];
  parts.push([r, [...l, ...rr]]);
}
// 马尾尾端收尖
parts.push([31, [seg(9, 14, 'O'), seg(10, 13, 'P'), seg(10, 11, 'p'), seg(12, 13, 'q')]]);
parts.push([32, [seg(10, 13, 'O'), seg(11, 12, 'P')]]);
parts.push([31, [seg(49, 54, 'O'), seg(50, 53, 'P'), seg(50, 51, 'p'), seg(52, 53, 'q')]]);
parts.push([32, [seg(50, 53, 'O'), seg(51, 52, 'P')]]);

// ---- 蝴蝶结: r2..r6, 列 27-40 ----
parts.push([2, [seg(27, 40, 'O'), seg(28, 31, 'F'), seg(33, 36, 'F'), seg(29, 30, 'f'), seg(34, 35, 'f'), seg(32, 33, 'O')]]);
parts.push([3, [seg(27, 40, 'O'), seg(28, 31, 'F'), seg(33, 36, 'F'), seg(29, 30, 'f'), seg(34, 35, 'f'), seg(32, 33, 'O')]]);
parts.push([4, [seg(27, 40, 'O'), seg(28, 31, 'F'), seg(33, 36, 'F'), seg(29, 30, 'f'), seg(34, 35, 'f'), seg(32, 33, 'O')]]);
parts.push([5, [seg(28, 39, 'O'), seg(29, 32, 'F'), seg(34, 37, 'F'), seg(33, 34, 'O')]]);
parts.push([6, [seg(30, 37, 'O'), seg(31, 34, 'F'), seg(32, 33, 'O')]]);

// ---- 头顶+刘海: r6..r12 ----
parts.push([6, [seg(16, 47, 'O'), seg(18, 45, 'P')]]);
for (let r = 7; r <= 8; r++) {
  parts.push([r, [seg(16, 47, 'O'), seg(18, 45, 'P'), seg(19, 22, 'p'), seg(40, 43, 'p')]]);
}
for (let r = 9; r <= 11; r++) {
  parts.push([r, [seg(16, 47, 'O'), seg(18, 45, 'P'), seg(19, 21, 'p'), seg(42, 44, 'q')]]);
}
parts.push([12, [seg(16, 47, 'O'), seg(18, 45, 'P'), seg(19, 20, 'p')]]);

// ---- 脸: r12..r28, 侧发 21-23/40-42, 脸 24-39 ----
for (let r = 12; r <= 26; r++) {
  parts.push([r, [seg(20, 43, 'O'), seg(21, 23, 'P'), seg(40, 42, 'P'), seg(24, 39, 'S')]]);
}
// 脸侧阴影
for (let r = 13; r <= 26; r++) {
  parts.push([r, [seg(24, 24, 's'), seg(39, 39, 's')]]);
}

// ---- 眼睛: r16..r18, 左 28-31, 右 37-40(4x3) ----
parts.push([16, [seg(28, 31, 'E'), seg(37, 40, 'E')]]);
parts.push([17, [seg(28, 31, 'E'), seg(37, 40, 'E'), seg(28, 28, 'H'), seg(37, 37, 'H')]]);
parts.push([18, [seg(28, 31, 'E'), seg(37, 40, 'E'), seg(29, 30, 'H'), seg(38, 39, 'H'), seg(31, 31, 'e'), seg(40, 40, 'e')]]);

// ---- 腮红 r22, 嘴 r24-25 ----
parts.push([22, [seg(25, 27, 'C'), seg(39, 41, 'C')]]);
parts.push([24, [seg(30, 33, 'M')]]);
parts.push([25, [seg(31, 32, 'm')]]);

// ---- 下巴 r27-28, 脖子 r29-31 ----
parts.push([27, [seg(21, 42, 'O'), seg(22, 23, 'P'), seg(40, 41, 'P'), seg(24, 39, 'S')]]);
parts.push([28, [seg(22, 41, 'O'), seg(23, 40, 'S')]]);
parts.push([29, [seg(24, 39, 'O'), seg(25, 38, 'S'), seg(25, 26, 's'), seg(37, 38, 's')]]);
parts.push([30, [seg(25, 38, 'O'), seg(26, 37, 'S'), seg(26, 27, 's'), seg(36, 37, 's')]]);
parts.push([31, [seg(26, 37, 'O'), seg(27, 36, 'S')]]);

// ---- 白领 r32-33, 水手服 r33-45 ----
parts.push([32, [seg(24, 39, 'O'), seg(25, 38, 'W'), seg(25, 25, 'w'), seg(38, 38, 'w')]]);
parts.push([33, [seg(23, 40, 'O'), seg(24, 39, 'W'), seg(24, 24, 'w'), seg(39, 39, 'w')]]);
for (let r = 34; r <= 45; r++) {
  parts.push([r, [seg(21, 42, 'O'), seg(22, 41, 'B'), seg(22, 22, 'b'), seg(41, 41, 'b')]]);
}
// 领巾 r35-38
for (let r = 35; r <= 38; r++) {
  parts.push([r, [seg(28, 35, 'W'), seg(28, 28, 'w'), seg(35, 35, 'w')]]);
}
// 衣摆阴影 r44-45
parts.push([44, [seg(23, 40, 'b')]]);
parts.push([45, [seg(22, 41, 'b')]]);

// ---- 裙: r46..r55 ----
parts.push([46, [seg(18, 45, 'O'), seg(19, 44, 'D'), seg(24, 25, 'd'), seg(38, 39, 'd')]]);
for (let r = 47; r <= 49; r++) {
  parts.push([r, [seg(17, 46, 'O'), seg(18, 45, 'D'), seg(24, 25, 'd'), seg(38, 39, 'd')]]);
}
for (let r = 50; r <= 52; r++) {
  parts.push([r, [seg(17, 46, 'O'), seg(18, 45, 'D'), seg(24, 25, 'd'), seg(38, 39, 'd'), seg(31, 32, 'd')]]);
}
parts.push([53, [seg(18, 45, 'O'), seg(19, 44, 'D'), seg(24, 25, 'd'), seg(38, 39, 'd')]]);
parts.push([54, [seg(19, 44, 'O'), seg(20, 43, 'D'), seg(25, 26, 'd'), seg(37, 38, 'd')]]);
parts.push([55, [seg(20, 43, 'O'), seg(21, 42, 'D')]]);

// ---- 腿 r56-58, 袜 r59-60, 鞋 r61-63 ----
parts.push([56, [seg(23, 40, 'O'), seg(24, 39, 'S'), seg(26, 27, 's'), seg(36, 37, 's')]]);
parts.push([57, [seg(24, 39, 'O'), seg(25, 38, 'S'), seg(25, 25, 's'), seg(38, 38, 's')]]);
parts.push([58, [seg(25, 38, 'O'), seg(26, 37, 'S')]]);
parts.push([59, [seg(24, 39, 'O'), seg(25, 38, 'W'), seg(25, 25, 'w'), seg(38, 38, 'w')]]);
parts.push([60, [seg(24, 39, 'O'), seg(25, 38, 'W'), seg(25, 25, 'w'), seg(38, 38, 'w')]]);
parts.push([61, [seg(23, 40, 'O'), seg(24, 39, 'P'), seg(24, 24, 'q'), seg(39, 39, 'q')]]);
parts.push([62, [seg(23, 40, 'O'), seg(24, 39, 'P'), seg(24, 24, 'q'), seg(39, 39, 'q')]]);
parts.push([63, [seg(24, 39, 'O'), seg(25, 38, 'P')]]);

// 合成
const composed = [];
for (let r = 0; r < 64; r++) {
  const cells = new Array(64).fill('.');
  for (const [yr, segs] of parts) {
    if (yr !== r) continue;
    for (const [s, e, c] of segs) {
      for (let x = s; x <= e; x++) cells[x] = c;
    }
  }
  composed.push(cells.join(''));
}

const palette = {
  O: '#5a3a2a',
  P: '#f5a0b8', p: '#ffd8e0', q: '#d07898',
  S: '#ffe8dc', s: '#e8c8b8',
  E: '#5a2a4a', e: '#2a1030', H: '#ffffff',
  C: '#ff9a8a', M: '#c05a5a', m: '#8a3a3a',
  B: '#5a8ae0', b: '#3a5ab8',
  W: '#ffffff', w: '#c8c8d8',
  D: '#3a5ab8', d: '#2a3a8a',
  F: '#ff5a7a', f: '#c83a5a',
};

module.exports = { rows: composed, palette };
if (require.main === module) {
  composed.forEach((r, i) => console.log(i + ' ' + r));
}
