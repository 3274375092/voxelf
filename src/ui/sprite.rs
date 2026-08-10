use macroquad::color::Color;

use crate::state::Phase;

pub const SIZE: usize = 32;

/// 二次元美少女 32x32(原创像素设计: 双马尾 + 红蝴蝶结 + 水手服 + 蓝裙 + 白袜)。
/// 字符含义: . 透明 | O 轮廓 | P 头发(樱花粉) | S 皮肤 | E 眼睛 | H 眼神光
///           C 腮红 | M 嘴 | B 水手服 | D 裙 | W 白领/袜 | F 蝴蝶结
const ROWS: [&str; SIZE] = [
    "................................",
    "...OPPO.....OFFOOFFO.....OPPO...",
    "...OPPO.....OFFOOFFO.....OPPO...",
    "...OPPO.....OFFOOFFO.....OPPO...",
    "...OPPO.OPPPPPPPPPPPPPPO.OPPO...",
    "...OPPO.OPPPPPPPPPPPPPPO.OPPO...",
    "...OPPO.OPPPPPPPPPPPPPPO.OPPO...",
    "...OPPO.OPPSSSSSSSSSSPPO.OPPO...",
    "...OPPO.OPPEEESSEEESSPPO.OPPO...",
    "...OPPO.OPPHEESSHEESSPPO.OPPO...",
    "...OPPO.OPPSSSSSSSSSSPPO.OPPO...",
    "...OPPO.OPPSCCSSSSCCSPPO.OPPO...",
    "...OPPO.OPPSSSMMMSSSSPPO.OPPO...",
    "...OPPO.OPPSSSSSSSSSSPPO.OPPO...",
    "...OPPO.OPPSSSSSSSSSSPPO.OPPO...",
    "...OPPO..OPPSSSSSSSSPPO..OPPO...",
    "..........OSSSSSSSSSSO..........",
    "...........OSSSSSSSSO...........",
    "..........OWWWWWWWWWWO..........",
    ".........OBBBBBBBBBBBBO.........",
    ".........OBBBBBBBBBBBBO.........",
    ".........OBBBWWWWWWBBBO.........",
    ".........OBBBWWWWWWBBBO.........",
    ".........OBBBBBBBBBBBBO.........",
    ".........OBBBBBBBBBBBBO.........",
    "........ODDDDDDDDDDDDDDO........",
    ".......ODDDDDDDDDDDDDDDDO.......",
    ".......ODDDDDDDDDDDDDDDDO.......",
    ".......ODDDDDDDDDDDDDDDDO.......",
    "........ODDDDDDDDDDDDDDO........",
    "..........OWWWWWWWWWWO..........",
    "...........OPPPPPPPPO...........",
];

/// 眼睛所在的格子(两颗 3x2 大眼,二次元风格;眨眼时替换为皮肤色)
const EYES: [(usize, usize); 12] = [
    (11, 8), (12, 8), (13, 8), (11, 9), (12, 9), (13, 9),
    (16, 8), (17, 8), (18, 8), (16, 9), (17, 9), (18, 9),
];
/// 眼神光(睁眼时保留,闭眼时消失)
const HIGHLIGHTS: [(usize, usize); 2] = [(11, 9), (16, 9)];
/// 聆听时头顶竖起的猫耳(平时透明,竖起时显示头发色)
const EARS: [(usize, usize); 8] = [
    (9, 2), (10, 2), (9, 3), (10, 3),
    (21, 2), (22, 2), (21, 3), (22, 3),
];
/// 嘴巴所在行与列范围(3 格小嘴)
const MOUTH_ROW: usize = 12;
const MOUTH_COLS: std::ops::Range<usize> = 14..17;

#[derive(Clone, Copy)]
pub struct Palette {
    pub outline: Color,
    pub skin: Color,
    pub hair: Color,
    pub body: Color,
    pub body_dark: Color,
    pub eye: Color,
    pub cheek: Color,
    /// 蝴蝶结(原脚蹼槽位)
    pub feet: Color,
    /// 白色(衣领/袜)
    pub cream: Color,
    /// 眼神光
    pub highlight: Color,
    /// 嘴内
    pub mouth: Color,
}

impl Default for Palette {
    fn default() -> Self {
        Self {
            outline: Color::from_hex(0x5a3a2a),
            skin: Color::from_hex(0xffe8dc),
            hair: Color::from_hex(0xf5a0b8),
            body: Color::from_hex(0x5a8ae0),
            body_dark: Color::from_hex(0x3a5ab8),
            eye: Color::from_hex(0x5a2a4a),
            cheek: Color::from_hex(0xff9a8a),
            feet: Color::from_hex(0xff5a7a),
            cream: Color::from_hex(0xffffff),
            highlight: Color::from_hex(0xffffff),
            mouth: Color::from_hex(0xc05a5a),
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

    /// 眼睛开合: 1.0 全开, 0.0 全闭(眨眼中)。闭眼时替换为皮肤色。
    pub fn eyes(&mut self, open: f32) {
        for &(x, y) in &EYES {
            self.grid[y][x] = if open > 0.75 { 'E' } else { 'S' };
        }
        for &(x, y) in &HIGHLIGHTS {
            self.grid[y][x] = if open > 0.75 { 'H' } else { 'S' };
        }
    }

    /// 嘴巴张开程度: 0.0 闭嘴(皮肤色), 1.0 大张嘴
    pub fn mouth(&mut self, open: f32) {
        let n = (open * 3.0).round() as usize;
        let pad = (3 - n) / 2;
        for (i, x) in MOUTH_COLS.enumerate() {
            self.grid[MOUTH_ROW][x] = if n > 0 && i >= pad && i < pad + n { 'M' } else { 'S' };
        }
    }

    /// 头顶猫耳(聆听时竖起)
    pub fn ears(&mut self, on: bool) {
        for &(x, y) in &EARS {
            self.grid[y][x] = if on { 'P' } else { '.' };
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
            s.ears(true);
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
                'P' => palette.hair,
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
