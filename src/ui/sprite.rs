use macroquad::color::Color;

use crate::state::Phase;

/// 皮肤画布边长。PNG 皮肤按此尺寸缩放;内置 fallback 也是这个尺寸。
pub const SIZE: usize = 64;

/// 内置 fallback 皮肤(字符矩阵,assets/sprites/vox.png 缺失时使用)。
/// 字符含义: . 透明 | O 轮廓 | P 头发 | p 发高光 | q 发阴影 | S 皮肤 | s 肤阴影
///           E 眼睛 | e 瞳孔 | H 眼神光 | C 腮红 | M 嘴 | m 嘴内
///           B 水手服 | b 衣阴影 | W 白 | w 白阴影 | D 裙 | d 裙阴影 | F 蝴蝶结 | f 蝴蝶结阴影
const ROWS: [&str; SIZE] = [
    "................................................................",
    "................................................................",
    "................OOppqqOO...........OFffFOOffFOOOO...............",
    "................OOppqqOO...........OFffFOOffFOOOO...............",
    "................OOppqqOO...........OFffPPOffFOOOO...............",
    "................OOppqqOO............OFFPPOOFFFOO................",
    "................OOppqqOOOOPPPPPPPPPPPPPPPPPPPPPPPPPPPPOO........",
    "................OOppqqOOOOPppppPPPPPPPPPPPPPPPPPppppPPOO........",
    "................OOppqqOOOOPppppPPPPPPPPPPPPPPPPPppppPPOO........",
    "................OOppqqOOOOPpppPPPPPPPPPPPPPPPPPPPPqqqPOO........",
    "................OOppqqOOOOPpppPPPPPPPPPPPPPPPPPPPPqqqPOO........",
    "................OOppqqOOOOPpppPPPPPPPPPPPPPPPPPPPPqqqPOO........",
    "................OOppqqOOOOPpOPPPSSSSSSSSSSSSSSSSPPPOPPOO........",
    "................OOppqqOO....OPPPsSSSSSSSSSSSSSSsPPPO............",
    "................OOppqqOO....OPPPsSSSSSSSSSSSSSSsPPPO............",
    "................OOppqqOO....OPPPsSSSSSSSSSSSSSSsPPPO............",
    "................OOppqqOO....OPPPsSSSEEEESSSSSEEEEPPO............",
    "................OOppqqOO....OPPPsSSSHEEESSSSSHEEEPPO............",
    "................OOppqqOO....OPPPsSSSEHHeSSSSSEHHePPO............",
    "................OOppqqOO....OPPPsSSSSSSSSSSSSSSsPPPO............",
    "................OOppqqOO....OPPPsSSSSSSSSSSSSSSsPPPO............",
    "................OOppqqOO....OPPPsSSSSSSSSSSSSSSsPPPO............",
    "................OOppqqOO....OPPPsCCCSSSSSSSSSSSCCCPO............",
    "................OOppqqOO....OPPPsSSSSSSSSSSSSSSsPPPO............",
    "................OOppqqOO....OPPPsSSSSSMMMMSSSSSsPPPO............",
    "................OOppqqOO....OPPPsSSSSSSmmSSSSSSsPPPO............",
    "................OOppqqOO....OPPPsSSSSSSSSSSSSSSsPPPO............",
    "................OOppqqOO.....OPPSSSSSSSSSSSSSSSSPPO.............",
    "................OOppqqOO......OSSSSSSSSSSSSSSSSSSO..............",
    "................OOppqqOO........OssSSSSSSSSSSssO................",
    "................OOppqqOO.........OssSSSSSSSSssO.................",
    ".................OppqqO...........OSSSSSSSSSSO..................",
    "..................OPPO..........OwWWWWWWWWWWWWwO................",
    "...............................OwWWWWWWWWWWWWWWwO...............",
    ".............................ObBBBBBBBBBBBBBBBBBBbO.............",
    ".............................ObBBBBBffFFFFffBBBBBbO.............",
    ".............................ObBBBBBffFFFFffBBBBBbO.............",
    ".............................ObBBBBBffFFFFffBBBBBbO.............",
    ".............................ObBBBBBffFFFFffBBBBBbO.............",
    ".............................ObBBBBBBBBBBBBBBBBBBbO.............",
    ".............................ObBBBBBBBBBBBBBBBBBBbO.............",
    ".............................ObBBBBBBBBBBBBBBBBBBbO.............",
    ".............................ObBBBBBBBBBBBBBBBBBBbO.............",
    ".............................ObBBBBBBBBBBBBBBBBBBbO.............",
    ".............................ObbbbbbbbbbbbbbbbbbbbO.............",
    ".............................ObbbbbbbbbbbbbbbbbbbbO.............",
    "..........................ODDDDDddDDDDDDDDDDDDddDDDDDO..........",
    ".........................ODDDDDDddDDDDDDDDDDDDddDDDDDDO.........",
    ".........................ODDDDDDddDDDDDDDDDDDDddDDDDDDO.........",
    ".........................ODDDDDDddDDDDDDDDDDDDddDDDDDDO.........",
    ".........................ODDDDDDddDDDDDddDDDDDddDDDDDDO.........",
    ".........................ODDDDDDddDDDDDddDDDDDddDDDDDDO.........",
    ".........................ODDDDDDddDDDDDddDDDDDddDDDDDDO.........",
    "..........................ODDDDDddDDDDDDDDDDDDddDDDDDO..........",
    "...........................ODDDDDddDDDDDDDDDDddDDDDDO...........",
    "............................ODDDDDDDDDDDDDDDDDDDDDDO............",
    "...............................OSSssSSSSSSSSssSSO...............",
    "................................OsSSSSSSSSSSSSsO................",
    ".................................OSSSSSSSSSSSSO.................",
    "................................OwWWWWWWWWWWWWwO................",
    "................................OwWWWWWWWWWWWWwO................",
    "...............................OqPPPPPPPPPPPPPPqO...............",
    "...............................OqPPPPPPPPPPPPPPqO...............",
    "................................OPPPPPPPPPPPPPPO................",
];

/// 眼睛所在的格子(两颗 4x3 大眼;眨眼时替换为皮肤色)
const EYES: [(usize, usize); 24] = [
    (36, 16), (37, 16), (38, 16), (39, 16), (36, 17), (37, 17), (38, 17), (39, 17),
    (36, 18), (37, 18), (38, 18), (39, 18),
    (45, 16), (46, 16), (47, 16), (48, 16), (45, 17), (46, 17), (47, 17), (48, 17),
    (45, 18), (46, 18), (47, 18), (48, 18),
];
/// 眼神光(睁眼时保留,闭眼时消失)
const HIGHLIGHTS: [(usize, usize); 6] = [
    (36, 17), (37, 18), (38, 18),
    (45, 17), (46, 18), (47, 18),
];
/// 聆听时头顶竖起的猫耳(平时透明,竖起时显示头发色)
const EARS: [(usize, usize); 16] = [
    (28, 2), (29, 2), (30, 2), (31, 2), (28, 3), (29, 3), (30, 3), (31, 3),
    (48, 2), (49, 2), (50, 2), (51, 2), (48, 3), (49, 3), (50, 3), (51, 3),
];
/// 嘴巴: 上唇行(4 格)与下唇行(2 格)
const MOUTH_ROW: usize = 24;
const MOUTH_COLS: std::ops::Range<usize> = 38..42;
const MOUTH_LOWER_ROW: usize = 25;
const MOUTH_LOWER_COLS: std::ops::Range<usize> = 39..41;

/// 皮肤颜色。PNG 皮肤只影响基底像素;动画覆盖层(眨眼/嘴/猫耳)用这里的颜色。
#[derive(Clone, Copy)]
pub struct Palette {
    pub outline: Color,
    pub skin: Color,
    pub skin_shade: Color,
    pub hair: Color,
    pub hair_light: Color,
    pub hair_dark: Color,
    pub body: Color,
    pub body_dark: Color,
    pub eye: Color,
    pub pupil: Color,
    pub cheek: Color,
    /// 蝴蝶结
    pub feet: Color,
    /// 蝴蝶结阴影
    pub bow_dark: Color,
    /// 白色(衣领/袜)
    pub cream: Color,
    /// 白阴影
    pub cream_dark: Color,
    /// 眼神光
    pub highlight: Color,
    /// 嘴
    pub mouth: Color,
    /// 嘴内
    pub mouth_dark: Color,
}

impl Default for Palette {
    fn default() -> Self {
        Self {
            outline: Color::from_hex(0x5a3a2a),
            skin: Color::from_hex(0xffe8dc),
            skin_shade: Color::from_hex(0xe8c8b8),
            hair: Color::from_hex(0xffd23e), // 金发
            hair_light: Color::from_hex(0xffe88a),
            hair_dark: Color::from_hex(0xd8a020),
            body: Color::from_hex(0x5a8ae0),
            body_dark: Color::from_hex(0x3a5ab8),
            eye: Color::from_hex(0xc03048), // 红瞳
            pupil: Color::from_hex(0x7a1020),
            cheek: Color::from_hex(0xff9a8a),
            feet: Color::from_hex(0xe84050), // 红蝴蝶结/领结
            bow_dark: Color::from_hex(0xa82838),
            cream: Color::from_hex(0xffffff),
            cream_dark: Color::from_hex(0xc8c8d8),
            highlight: Color::from_hex(0xffffff),
            mouth: Color::from_hex(0xc05a5a),
            mouth_dark: Color::from_hex(0x8a3a3a),
        }
    }
}

/// 一帧皮肤位图(RGBA,row-major)。
#[derive(Clone)]
pub struct Sprite {
    pub size: usize,
    pub pixels: Vec<[u8; 4]>,
}

type Rgba = [u8; 4];

impl Sprite {
    /// 加载皮肤 PNG(assets/sprites/vox.png);缺失或解码失败时用内置字符矩阵。
    pub fn base() -> Self {
        if let Ok(img) = image::open("assets/sprites/vox.png") {
            let rgba = img.to_rgba8();
            if rgba.width() as usize == SIZE && rgba.height() as usize == SIZE {
                let mut pixels = Vec::with_capacity(SIZE * SIZE);
                for p in rgba.pixels() {
                    pixels.push(p.0);
                }
                return Self { size: SIZE, pixels };
            }
            tracing::warn!(
                "皮肤 PNG 尺寸 {}x{} 与 SIZE={SIZE} 不符,回退内置",
                rgba.width(),
                rgba.height()
            );
        } else {
            tracing::debug!("未找到 assets/sprites/vox.png,使用内置皮肤");
        }
        Self::builtin()
    }

    /// 内置字符矩阵皮肤(默认 Palette 上色)。
    fn builtin() -> Self {
        let pal = Palette::default();
        let color_of = |c: char| -> Option<Rgba> {
            let col = match c {
                'O' => pal.outline,
                'B' | 'b' => pal.body,
                'D' | 'd' => pal.body_dark,
                'W' => pal.cream,
                'w' => pal.cream_dark,
                'E' => pal.eye,
                'e' => pal.pupil,
                'H' => pal.highlight,
                'C' => pal.cheek,
                'M' => pal.mouth,
                'm' => pal.mouth_dark,
                'F' => pal.feet,
                'f' => pal.bow_dark,
                'P' => pal.hair,
                'p' => pal.hair_light,
                'q' => pal.hair_dark,
                'S' => pal.skin,
                's' => pal.skin_shade,
                _ => return None,
            };
            Some([col.r as u8, col.g as u8, col.b as u8, 255])
        };
        let mut pixels = vec![[0u8; 4]; SIZE * SIZE];
        for (y, row) in ROWS.iter().enumerate() {
            debug_assert_eq!(row.chars().count(), SIZE, "sprite row {y} 长度错误");
            for (x, c) in row.chars().enumerate() {
                if let Some(rgba) = color_of(c) {
                    pixels[y * SIZE + x] = rgba;
                }
            }
        }
        Self { size: SIZE, pixels }
    }

    fn paint(&mut self, x: usize, y: usize, c: Color) {
        if x < self.size && y < self.size {
            self.pixels[y * self.size + x] = [c.r as u8, c.g as u8, c.b as u8, c.a as u8];
        }
    }

    /// 眼睛开合: 1.0 全开, 0.0 全闭(眨眼中)。闭眼时替换为皮肤色。
    pub fn eyes(&mut self, open: f32, pal: &Palette) {
        for &(x, y) in &EYES {
            self.paint(x, y, if open > 0.75 { pal.eye } else { pal.skin });
        }
        for &(x, y) in &HIGHLIGHTS {
            self.paint(x, y, if open > 0.75 { pal.highlight } else { pal.skin });
        }
    }

    /// 嘴巴张开程度: 0.0 闭嘴(皮肤色), 1.0 大张嘴(上唇 + 下唇)
    pub fn mouth(&mut self, open: f32, pal: &Palette) {
        let n = (open * 4.0).round() as usize;
        let pad = (4 - n) / 2;
        for (i, x) in MOUTH_COLS.enumerate() {
            let c = if n > 0 && i >= pad && i < pad + n { pal.mouth } else { pal.skin };
            self.paint(x, MOUTH_ROW, c);
        }
        let n2 = (open * 2.0).round() as usize;
        let pad2 = (2 - n2) / 2;
        for (i, x) in MOUTH_LOWER_COLS.enumerate() {
            let c = if n2 > 0 && i >= pad2 && i < pad2 + n2 { pal.mouth_dark } else { pal.skin };
            self.paint(x, MOUTH_LOWER_ROW, c);
        }
    }

    /// 头顶猫耳(聆听时竖起)
    pub fn ears(&mut self, on: bool, pal: &Palette) {
        for &(x, y) in &EARS {
            if on {
                self.paint(x, y, pal.hair);
            } else {
                self.pixels[y * self.size + x] = [0, 0, 0, 0];
            }
        }
    }
}

/// 按状态生成一帧小人。
/// t: 秒, level: 麦克风电平 0..1, pal: 动画覆盖层颜色
pub fn build_frame(phase: Phase, t: f32, level: f32, pal: &Palette) -> Sprite {
    let mut s = Sprite::base();
    match phase {
        Phase::Idle => {
            let blink = (t * 0.5).sin().abs() < 0.06; // 偶尔眨眼
            s.eyes(if blink { 0.0 } else { 1.0 }, pal);
            s.mouth(0.0, pal);
            s.ears(false, pal);
        }
        Phase::Listening => {
            let blink = (t * 1.5).sin().abs() < 0.04;
            s.eyes(if blink { 0.0 } else { 1.0 }, pal);
            s.mouth(0.0, pal);
            s.ears(true, pal);
        }
        Phase::Thinking => {
            // 半闭眼 + 小嘴,头顶灯泡由 UI 画
            s.eyes(0.5 + (t * 2.0).sin() * 0.2, pal);
            s.mouth(0.1 + (t * 3.0).sin() * 0.05, pal);
            s.ears(false, pal);
        }
        Phase::Speaking => {
            s.eyes(1.0, pal);
            s.mouth(0.4 + 0.6 * (t * 8.0).sin().abs().powf(0.8), pal);
            s.ears(false, pal);
        }
        Phase::Working => {
            let blink = (t * 3.0).sin().abs() < 0.08;
            s.eyes(if blink { 0.0 } else { 0.75 }, pal);
            s.mouth(0.0, pal);
            s.ears(true, pal);
        }
        Phase::Error => {
            // 低落: 全闭眼 + 闭嘴
            s.eyes(0.0, pal);
            s.mouth(0.0, pal);
            s.ears(false, pal);
        }
    }
    let _ = level;
    s
}

/// 把小人画到屏幕。x/y 为左上角, scale 为每像素边长。
pub fn draw(sprite: &Sprite, x: f32, y: f32, scale: f32, _palette: &Palette) {
    for (i, &[r, g, b, a]) in sprite.pixels.iter().enumerate() {
        if a == 0 {
            continue;
        }
        let row = i / sprite.size;
        let col = i % sprite.size;
        macroquad::shapes::draw_rectangle(
            x + col as f32 * scale,
            y + row as f32 * scale,
            scale + 0.02,
            scale + 0.02,
            Color::from_rgba(r, g, b, a),
        );
    }
}
