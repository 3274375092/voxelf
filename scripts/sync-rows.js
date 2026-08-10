// 把 girl-v5.js 生成的 64 行替换进 sprite.rs 的 ROWS 数组
const fs = require('fs');
const m = require('./girl-v5.js');
const rows = m.rows;
if (rows.length !== 64 || rows.some(r => r.length !== 64)) {
  console.error('行数或长度错误');
  process.exit(1);
}
let src = fs.readFileSync('../src/ui/sprite.rs', 'utf8');
const start = src.indexOf('const ROWS: [&str; SIZE] = [');
const end = src.indexOf('];', start) + 2;
if (start < 0 || end < 2) { console.error('未找到 ROWS 数组'); process.exit(1); }
const newRows = 'const ROWS: [&str; SIZE] = [\n' +
  rows.map(r => `    "${r}",`).join('\n') + '\n];';
src = src.slice(0, start) + newRows + src.slice(end);
fs.writeFileSync('../src/ui/sprite.rs', src, 'utf8');
console.log('已替换 ROWS(64 行)');
