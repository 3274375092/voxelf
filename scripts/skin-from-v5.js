// 从 girl-v5 生成 PNG 皮肤(64x64 位图) + 放大预览
const m = require('./girl-v5.js');
const zlib = require('zlib'), fs = require('fs');
function pngFrom(rows, palette, scale) {
  const N = rows.length, W = N * scale, H = N * scale;
  const px = Buffer.alloc(W * H * 4);
  for (let y = 0; y < N; y++) for (let x = 0; x < N; x++) {
    const col = palette[rows[y][x]];
    if (!col) continue;
    const r = parseInt(col.slice(1, 3), 16), g = parseInt(col.slice(3, 5), 16), b = parseInt(col.slice(5, 7), 16);
    for (let dy = 0; dy < scale; dy++) for (let dx = 0; dx < scale; dx++) {
      const i = ((y * scale + dy) * W + (x * scale + dx)) * 4;
      px[i] = r; px[i + 1] = g; px[i + 2] = b; px[i + 3] = 255;
    }
  }
  const raw = Buffer.alloc(H * (W * 4 + 1));
  for (let y = 0; y < H; y++) px.copy(raw, y * (W * 4 + 1) + 1, y * W * 4, (y + 1) * W * 4);
  function chunk(t, d) {
    const l = Buffer.alloc(4); l.writeUInt32BE(d.length);
    const tb = Buffer.from(t, 'ascii');
    let c = 0xffffffff;
    for (const b of Buffer.concat([tb, d])) { c ^= b; for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1; }
    const crc = Buffer.alloc(4); crc.writeUInt32BE((c ^ 0xffffffff) >>> 0);
    return Buffer.concat([l, tb, d, crc]);
  }
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(W, 0); ihdr.writeUInt32BE(H, 4); ihdr[8] = 8; ihdr[9] = 6;
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk('IHDR', ihdr), chunk('IDAT', zlib.deflateSync(raw)), chunk('IEND', Buffer.alloc(0)),
  ]);
}
fs.mkdirSync('../assets/sprites', { recursive: true });
fs.writeFileSync('../assets/sprites/vox.png', pngFrom(m.rows, m.palette, 1));
fs.writeFileSync('girl-v5-preview.png', pngFrom(m.rows, m.palette, 8));
console.log('已生成 assets/sprites/vox.png (64x64) 和 girl-v5-preview.png (512x512)');
