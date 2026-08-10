// 像素小人 PNG 渲染器(预览用,不依赖第三方库)
// 用法: node scripts/sprite-render.js <输入js> <输出png>
// 输入 js 导出: rows(32 字符串数组), palette({char: '#rrggbb'})
const fs = require('fs');
const zlib = require('zlib');

const [, , inFile, outFile] = process.argv;
if (!inFile || !outFile) { console.error('用法: node sprite-render.js in.js out.png'); process.exit(1); }
const mod = require('./' + inFile.replace(/\.js$/, ''));
const rows = mod.rows;
const palette = mod.palette;
const scale = 10;

if (rows.length !== 32 || rows.some(r => r.length !== 32)) {
  console.error('rows 必须为 32 行 x 32 字符');
  process.exit(1);
}

// 构建 RGBA
const W = 32 * scale, H = 32 * scale;
const px = Buffer.alloc(W * H * 4);
for (let y = 0; y < 32; y++) {
  for (let x = 0; x < 32; x++) {
    const c = rows[y][x];
    const col = palette[c];
    for (let dy = 0; dy < scale; dy++) {
      for (let dx = 0; dx < scale; dx++) {
        const ox = x * scale + dx, oy = y * scale + dy;
        const i = (oy * W + ox) * 4;
        if (col) {
          px[i] = parseInt(col.slice(1, 3), 16);
          px[i + 1] = parseInt(col.slice(3, 5), 16);
          px[i + 2] = parseInt(col.slice(5, 7), 16);
          px[i + 3] = 255;
        }
      }
    }
  }
}

// PNG 编码
function chunk(type, data) {
  const len = Buffer.alloc(4); len.writeUInt32BE(data.length);
  const t = Buffer.from(type, 'ascii');
  const crc = Buffer.alloc(4);
  const crcTable = (() => {
    const t = [];
    for (let n = 0; n < 256; n++) {
      let c = n;
      for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
      t[n] = c >>> 0;
    }
    return t;
  })();
  let c = 0xffffffff;
  for (const b of Buffer.concat([t, data])) c = crcTable[(c ^ b) & 0xff] ^ (c >>> 8);
  crc.writeUInt32BE((c ^ 0xffffffff) >>> 0);
  return Buffer.concat([len, t, data, crc]);
}
const ihdr = Buffer.alloc(13);
ihdr.writeUInt32BE(W, 0); ihdr.writeUInt32BE(H, 4);
ihdr[8] = 8; ihdr[9] = 6; // 8bit RGBA
// 每行前加 filter byte 0
const raw = Buffer.alloc(H * (W * 4 + 1));
for (let y = 0; y < H; y++) {
  px.copy(raw, y * (W * 4 + 1) + 1, y * W * 4, (y + 1) * W * 4);
}
const png = Buffer.concat([
  Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
  chunk('IHDR', ihdr),
  chunk('IDAT', zlib.deflateSync(raw)),
  chunk('IEND', Buffer.alloc(0)),
]);
fs.writeFileSync(outFile, png);
console.log(`已渲染 ${outFile} (${W}x${H})`);
