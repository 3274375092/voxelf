// 二次元美少女 v4(定稿候选): 双马尾 + 红蝴蝶结 + 水手服 + 蓝裙 + 白袜 + 皮鞋
// 字符: . 透明 | O 轮廓 | P 头发 | S 皮肤 | E 眼睛 | H 高光
//       C 腮红 | M 嘴 | B 衣服 | D 裙 | W 白领/袜 | F 蝴蝶结
function row(segments) {
  const cells = new Array(32).fill('.');
  for (const [s, e, c] of segments) {
    for (let x = s; x <= e; x++) cells[x] = c;
  }
  const out = cells.join('');
  if (out.length !== 32) throw new Error(out.length + ' != 32');
  return out;
}
const P = (s, e) => [s, e, 'P'];
const O = (s, e) => [s, e, 'O'];
const F = (s, e) => [s, e, 'F'];
const S = (s, e) => [s, e, 'S'];
const E = (s, e) => [s, e, 'E'];
const H = (s, e) => [s, e, 'H'];
const C = (s, e) => [s, e, 'C'];
const M = (s, e) => [s, e, 'M'];
const B = (s, e) => [s, e, 'B'];
const D = (s, e) => [s, e, 'D'];
const W = (s, e) => [s, e, 'W'];
// 马尾(左右): r1..r15, 轮廓 3-6/25-28, 内部 4-5/26-27
const pony = (r) => row([
  ...(r >= 1 && r <= 15 ? [O(3, 6), P(4, 5), O(25, 28), P(26, 27)] : []),
]);
// 蝴蝶结: 列 13-18, r1-3(整体红)
const bow = (r) => row([
  ...(r >= 1 && r <= 3 ? [O(12, 19), F(13, 18), O(15, 16)] : []),
]);
// 头顶+刘海: r4-6
const top = (r) => row([
  ...(r >= 4 && r <= 6 ? [O(8, 23), P(9, 22)] : []),
]);
// 脸: r7-14, 侧发 2 列(9-10/21-22), 脸 11-20
const face = (r) => row([
  ...(r >= 7 && r <= 14 ? [O(8, 23), P(9, 10), P(21, 22), S(11, 20)] : []),
]);
// 眼睛 r8-9: 左 11-13, 右 16-18(3 列); 高光 r9 错位 1 列
const eyes = (r) => {
  const segs = [];
  if (r === 8) { segs.push([11, 13, 'E'], [16, 18, 'E']); }
  if (r === 9) { segs.push([11, 11, 'H'], [12, 13, 'E'], [16, 16, 'H'], [17, 18, 'E']); }
  return row(segs);
};
// 腮红 r11(12-13/18-19), 嘴 r12(14-16)
const faceExtras = (r) => row([
  ...(r === 11 ? [[12, 13, 'C'], [18, 19, 'C']] : []),
  ...(r === 12 ? [[14, 16, 'M']] : []),
]);
// 下巴 r15-16, 脖子 r17
const chin = (r) => row([
  ...(r === 15 ? [O(9, 22), P(10, 11), P(20, 21), S(12, 19)] : []),
  ...(r === 16 ? [O(10, 21), S(11, 20)] : []),
  ...(r === 17 ? [O(11, 20), S(12, 19)] : []),
]);
// 白领 r18
const collar = (r) => row([
  ...(r === 18 ? [O(10, 21), W(11, 20)] : []),
]);
// 水手服 r19-24, 领巾 r21-22
const body = (r) => row([
  ...(r >= 19 && r <= 24 ? [O(9, 22), B(10, 21)] : []),
  ...(r >= 21 && r <= 22 ? [[13, 18, 'W']] : []),
]);
// 裙 r25-29
const skirt = (r) => row([
  ...(r === 25 ? [O(8, 23), D(9, 22)] : []),
  ...(r >= 26 && r <= 28 ? [O(7, 24), D(8, 23)] : []),
  ...(r === 29 ? [O(8, 23), D(9, 22)] : []),
]);
// 袜 r30, 鞋 r31
const legs = (r) => row([
  ...(r === 30 ? [O(10, 21), W(11, 20)] : []),
  ...(r === 31 ? [O(11, 20), P(12, 19)] : []),
]);

const composed = [];
for (let r = 0; r < 32; r++) {
  const parts = [pony(r), bow(r), top(r), face(r), eyes(r), faceExtras(r), chin(r), collar(r), body(r), skirt(r), legs(r)];
  const cells = new Array(32).fill('.');
  for (const p of parts) {
    for (let x = 0; x < 32; x++) {
      const c = p[x];
      if (c !== '.') cells[x] = c;
    }
  }
  composed.push(cells.join(''));
}

const palette = {
  O: '#5a3a2a', // 轮廓 深棕
  P: '#f5a0b8', // 头发 樱花粉
  S: '#ffe8dc', // 皮肤
  E: '#5a2a4a', // 眼睛 深紫红
  H: '#ffffff', // 高光
  C: '#ff9a8a', // 腮红
  M: '#c05a5a', // 嘴
  B: '#5a8ae0', // 水手服 蓝
  D: '#3a5ab8', // 裙 深蓝
  W: '#ffffff', // 白领/袜
  F: '#ff5a7a', // 蝴蝶结 红
};

module.exports = { rows: composed, palette };
if (require.main === module) {
  composed.forEach((r, i) => console.log(i + ' ' + r));
}
