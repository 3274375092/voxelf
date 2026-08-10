use macroquad::color::Color;

use crate::state::Phase;

pub const SIZE: usize = 32;

/// 奶蛙风格 32x32(原创,AI 变异版: 头小身大+五官失调+狂笑大嘴;每行恰好 32 字符)。
/// 字符含义: . 透明 | O 轮廓 | B 身体(奶黄橙) | D 阴影 | W 奶油肚皮
///           E 眼睛 | H 眼神光 | C 腮红 | M 嘴 | F 脚蹼
const ROWS: [&str; SIZE] = [
    "................................",
    "................................",
    "................................",
    "................................",
    "..............OOOOOO............",
    ".............OOOOOO.............",
    "...........OEEBEEO..............",
    "...........OEEBEEO..............",
    "............OMMMO...............",
    "............OBBBO...............",
    ".............OOOOO..............",
    "..........OOOOOOOOO.............",
    ".........OBBBBBBBBBO............",
    "........OBBBBBBBBBBBO...........",
    "........OBCBBBBBBBBBCBO.........",
    "........OBBBBBBBBBBBBBO.........",
    ".......OBBBBBBBBBBBBBO..........",
    ".......OBBBBBBBBBBBBBBO.........",
    ".......OBBWWWWWWWWWWBBO.........",
    ".......OBBWWWWWWWWWWBBO.........",
    ".......OBBWWWWWWWWWWBBO.........",
    ".......OBBBBBBBBBBBBBBO.........",
    ".......OBBBBBBBBBBBBBBO.........",
    ".......OBBBBBBBBBBBBBBO.........",
    "........OBBBBBBBBBBBBBO.........",
    "........OBBBBBBBBBBBBBO.........",
    ".........OBBBBBBBBBBBO..........",
    "..........OOOOOOOOOOO...........",
    "........OOOOOOOOOOOOO...........",
    ".......OOOOOOOOOOOOOOO..........",
    "......OOOOOOOOOOOOOOOOO.........",
    "................................",
];

/// 眼睛所在的格子(两颗 2x2 大眼,靠上靠中;眨眼时替换为身体色)
const EYES: [(usize, usize); 8] = [
    (12, 6), (13, 6), (12, 7), (13, 7),
    (15, 6), (16, 6), (15, 7), (16, 7),
];
/// 眼神光(睁眼时保留,闭眼时消失)
const HIGHLIGHTS: [(usize, usize); 2] = [(13, 6), (15, 6)];
/// 聆听时头顶竖起的小耳羽(奶油色)
const EARS: [(usize, usize); 8] = [
    (10, 5), (11, 5), (10, 6), (11, 6),
    (19, 5), (20, 5), (19, 6), (20, 6),
];
/// 嘴巴所在行与列范围(小嘴)
const MOUTH_ROW: usize = 8;
const MOUTH_COLS: std::ops::Range<usize> = 13..16;

#[derive(Clone, Copy)]
pub struct Palette {
    pub outline: Color,
    pub skin: Color,
    pub hair: Color,
    pub body: Color,
    pub body_dark: Color,
    pub eye: Color,
    pub cheek: Color,
    pub feet: Color,
    /// 奶油肚皮/耳羽(原 skin 槽位,兼容旧代码)
    pub cream: Color,
    /// 眼神光
    pub highlight: Color,
    /// 嘴内
    pub mouth: Color,
}

impl Default for Palette {
    fn default() -> Self {
        Self {
            outline: Color::from_hex(0x7a4a26),
            skin: Color::from_hex(0xfff0b0),
            hair: Color::from_hex(0xfad44e),
            body: Color::from_hex(0xfad44e),
            body_dark: Color::from_hex(0xe8b93c),
            eye: Color::from_hex(0x5a3a20),
            cheek: Color::from_hex(0xf08c3c),
            feet: Color::from_hex(0xe8b93c),
            cream: Color::from_hex(0xfff0b0),
            highlight: Color::from_hex(0xffffff),
            mouth: Color::from_hex(0x7a4a26),
        }
    }
}

#[derive(Clone)]
pub struct Sprite {
    pub grid: [[char; SIZE]; SIZE],
}

impl Sprite {
    pub fn base() -> Self {
        let mut grid = [['.'; SIZE]; SIZE];
        for (y, row) in ROWS.iter().enumerate() {
            debug_assert_eq!(row.chars().count(), SIZE, "sprite row {y} 长度错误");
            for (x, c) in row.chars().enumerate() {
                grid[y][x] = c;
            }
        }
        Self { grid }
    }

    /// 眼睛开合: 1.0 全开, 0.0 全闭(眨眼中)。闭眼时替换为身体色。
    pub fn eyes(&mut self, open: f32) {
        for &(x, y) in &EYES {
            self.grid[y][x] = if open > 0.75 { 'E' } else { 'B' };
        }
        for &(x, y) in &HIGHLIGHTS {
            self.grid[y][x] = if open > 0.75 { 'H' } else { 'B' };
        }
    }

    /// 嘴巴张开程度: 0.0 闭嘴(身体色), 1.0 大张嘴
    pub fn mouth(&mut self, open: f32) {
        let n = (open * 3.0).round() as usize;
        let pad = (3 - n) / 2;
        for (i, x) in MOUTH_COLS.enumerate() {
            self.grid[MOUTH_ROW][x] = if n > 0 && i >= pad && i < pad + n { 'M' } else { 'B' };
        }
    }

    /// 头顶小耳羽(聆听时竖起)
    pub fn ears(&mut self, on: bool) {
        for &(x, y) in &EARS {
            self.grid[y][x] = if on { 'W' } else { 'B' };
        }
    }
}

/// 按状态生成一帧小人。
/// t: 秒, level: 麦克风电平 0..1
pub fn build_frame(phase: Phase, t: f32, level: f32) -> Sprite {
    let mut s = Sprite::base();
    match phase {
        Phase::Idle => {
            let blink = (t * 0.5).sin().abs() < 0.06; // 偶尔眨眼
            s.eyes(if blink { 0.0 } else { 1.0 });
            s.mouth(0.0);
            s.ears(false);
        }
        Phase::Listening => {
            let blink = (t * 1.5).sin().abs() < 0.04;
            s.eyes(if blink { 0.0 } else { 1.0 });
            // 嘴巴随电平轻微起伏,表示在听
            s.mouth(0.0);
            s.ears(true);
        }
        Phase::Thinking => {
            // 半闭眼 + 小嘴,头顶灯泡由 UI 画
            s.eyes(0.5 + (t * 2.0).sin() * 0.2);
            s.mouth(0.1 + (t * 3.0).sin() * 0.05);
            s.ears(false);
        }
        Phase::Speaking => {
            s.eyes(1.0);
            s.mouth(0.4 + 0.6 * (t * 8.0).sin().abs().powf(0.8));
            s.ears(false);
        }
        Phase::Working => {
            let blink = (t * 3.0).sin().abs() < 0.08;
            s.eyes(if blink { 0.0 } else { 0.75 });
            s.mouth(0.0);
            s.ears(false);
        }
        Phase::Error => {
            // 低落: 全闭眼 + 闭嘴
            s.eyes(0.0);
            s.mouth(0.0);
            s.ears(false);
        }
    }
    let _ = level;
    s
}

/// 把小人画到屏幕。x/y 为左上角, scale 为每像素边长。
pub fn draw(sprite: &Sprite, x: f32, y: f32, scale: f32, palette: &Palette) {
    for (row, line) in sprite.grid.iter().enumerate() {
        for (col, &c) in line.iter().enumerate() {
            let color = match c {
                'O' => palette.outline,
                'B' => palette.body,
                'D' => palette.body_dark,
                'W' => palette.cream,
                'E' => palette.eye,
                'H' => palette.highlight,
                'C' => palette.cheek,
                'M' => palette.mouth,
                'F' => palette.feet,
                // 旧皮肤字符,兼容映射
                'S' => palette.skin,
                _ => continue,
            };
            macroquad::shapes::draw_rectangle(
                x + col as f32 * scale,
                y + row as f32 * scale,
                scale + 0.02,
                scale + 0.02,
                color,
            );
        }
    }
}
