use macroquad::color::Color;

use crate::state::Phase;

pub const SIZE: usize = 32;

/// 基础小人 32x32(代码内像素画,每行恰好 32 字符)。
/// 字符含义: . 透明 | O 轮廓 | S 皮肤 | H 头发 | B 身体 | E 眼睛 | C 腮红 | F 鞋子
const ROWS: [&str; SIZE] = [
    "................................",
    "................................",
    "................................",
    "................................",
    "................................",
    ".............OOOOOO.............",
    "...........OOOOOOOOOO...........",
    "...........OOHHHHHHOO...........",
    "...........OHHHHHHHHO...........",
    "..........OHHSSSSSHHOO..........",
    ".........OHHSSSSSSSHOO..........",
    ".........OHHSSSSSSSSHO..........",
    ".........OHSSSSSSSSSSHO.........",
    "......OHSSEESSSSSSEESSSSHO......",
    "........OHSSSSSSSSSSSSSHO.......",
    ".....OHSSCCSSSSSSCCSSSSSSHO.....",
    "......OHSSSMMMMMMMSSSSSHO.......",
    ".......OHHSSSSSSSSSSHHHO........",
    "..........OHHHHHHHHHHO..........",
    ".........OOOHHHHHHHHOOO.........",
    "..........OOOOOOOOOOO...........",
    "...........OBBBBBBBBO...........",
    "...........OBBBBBBBBO...........",
    "...........OBBBBBBBBO...........",
    "..........OOBBBBBBBBOO..........",
    "..........OOBBBBBBBBOO..........",
    "...........OBBBBBBBBO...........",
    "...........OBBBBBBBBO...........",
    "...........OBBBBBBBO............",
    "...........OBBBBBBBO............",
    "..........OFFFFFFFFFO...........",
    "..........OFFFFFFFFFO...........",
];

/// 眼睛在基础网格中的位置(两枚 2x2)
const EYES: [(usize, usize); 4] = [(10, 13), (11, 13), (18, 13), (19, 13)];
/// 聆听时竖起的耳朵(紧贴头部两侧)
const EARS: [(usize, usize); 8] = [
    (6, 10), (7, 10), (6, 11), (7, 11),
    (24, 10), (25, 10), (24, 11), (25, 11),
];
/// 嘴巴所在行与列范围
const MOUTH_ROW: usize = 16;
const MOUTH_COLS: std::ops::Range<usize> = 11..18;

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
}

impl Default for Palette {
    fn default() -> Self {
        Self {
            outline: Color::from_hex(0x241c3c),
            skin: Color::from_hex(0xffd9a0),
            hair: Color::from_hex(0x7a4a2b),
            body: Color::from_hex(0xff7e67),
            body_dark: Color::from_hex(0xe05f50),
            eye: Color::from_hex(0x241c3c),
            cheek: Color::from_hex(0xffb3a7),
            feet: Color::from_hex(0x4a5a7a),
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

    /// 眼睛开合: 1.0 全开, 0.0 全闭(眨眼中), 中间值半闭
    pub fn eyes(&mut self, open: f32) {
        for &(x, y) in &EYES {
            self.grid[y][x] = if open > 0.75 {
                'E'
            } else if open > 0.25 {
                'S'
            } else {
                'S'
            };
        }
    }

    /// 嘴巴张开程度: 0.0 闭嘴, 1.0 大张嘴
    pub fn mouth(&mut self, open: f32) {
        let n = (open * 7.0).round() as usize;
        let pad = (7 - n) / 2;
        for (i, x) in MOUTH_COLS.enumerate() {
            self.grid[MOUTH_ROW][x] = if n > 0 && i >= pad && i < pad + n { 'M' } else { 'S' };
        }
    }

    /// 竖起耳朵(聆听)
    pub fn ears(&mut self, on: bool) {
        for &(x, y) in &EARS {
            self.grid[y][x] = if on { 'S' } else { '.' };
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
            // 低落: 全闭眼 + 嘴角下撇(用 0 宽度嘴)
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
                'S' => palette.skin,
                'H' => palette.hair,
                'B' => palette.body,
                'D' => palette.body_dark,
                'E' => palette.eye,
                'C' => palette.cheek,
                'F' => palette.feet,
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
