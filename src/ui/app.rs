use macroquad::prelude::*;
use macroquad::text::{load_ttf_bytes_from_bytes, Font};
use macroquad::window::Conf;

use crate::config::Config;
use crate::state::{Phase, SharedState, UiState};
use crate::ui::sprite::{self, Palette};

const FONT_PATH: &str = "assets/fonts/NotoSansSC.ttf";
const CHAR_SCALE: f32 = 6.0;

struct App {
    state: SharedState,
    font: Option<Font>,
    t: f32,
    stars: Vec<(f32, f32, f32)>,
    palette: Palette,
}

impl App {
    fn new(state: SharedState, font: Option<Font>) -> Self {
        let stars = (0..70)
            .map(|i| {
                let x = (i as f32 * 137.5 + 40.0) % screen_width();
                let y = (i as f32 * 89.3 + 20.0) % (screen_height() * 0.55);
                (x, y, 1.0 + (i % 3) as f32)
            })
            .collect();
        Self { state, font, t: 0.0, stars, palette: Palette::default() }
    }

    fn draw(&mut self) {
        self.t += 1.0 / 60.0;
        clear_background(Color::from_hex(0x141526));

        // 背景星星
        for (i, &(x, y, r)) in self.stars.iter().enumerate() {
            let tw = 0.25 + 0.35 * (0.5 + 0.5 * (self.t * 1.7 + i as f32 * 0.7).sin());
            draw_circle(x, y, r, Color::from_rgba(150, 155, 210, (tw * 255.0) as u8));
        }

        let st = match self.state.lock() {
            Ok(s) => s.clone(),
            Err(_) => UiState::default(),
        };

        // 地面
        draw_rectangle(0.0, screen_height() * 0.86, screen_width(), 4.0, Color::from_hex(0x2b2f55));

        // 小人(带轻微上下浮动)
        let bob = match st.phase {
            Phase::Idle => (self.t * 2.0).sin() * 3.0,
            Phase::Listening => (self.t * 2.4).sin() * 4.0,
            Phase::Thinking => (self.t * 3.0).sin() * 2.0,
            Phase::Speaking => (self.t * 6.0).sin().abs() * 5.0,
            Phase::Working => (self.t * 5.0).sin() * 3.0,
            Phase::Error => 0.0,
        };
        let cx = screen_width() * 0.36;
        let cy = screen_height() * 0.72 + bob;
        let sprite = sprite::build_frame(st.phase, self.t, st.mic_level);
        sprite::draw(&sprite, cx - 16.0 * CHAR_SCALE / 2.0, cy - 32.0 * CHAR_SCALE, CHAR_SCALE, &self.palette);

        // 头顶状态装饰
        match st.phase {
            Phase::Thinking => self.draw_think_bubble(cx, cy - 34.0 * CHAR_SCALE),
            Phase::Speaking => self.draw_notes(cx, cy - 34.0 * CHAR_SCALE),
            Phase::Working => self.draw_keyboard(cx, cy),
            _ => {}
        }

        // 对话框
        let accent = phase_color(st.phase);
        let bubble_x = screen_width() * 0.58;
        let bubble_y = 70.0;
        let text = if st.reply_text.is_empty() {
            match st.phase {
                Phase::Idle => "你好呀!我是 Vox,对着麦克风跟我说点什么吧~".to_string(),
                Phase::Listening => {
                    if st.asr_partial.is_empty() { "我在听呢...".to_string() } else { st.asr_partial.clone() }
                }
                Phase::Thinking => "嗯...让我想想~".to_string(),
                Phase::Speaking => "正在说话...".to_string(),
                Phase::Working => "收到,开始干活!".to_string(),
                Phase::Error => st.error.clone().unwrap_or_else(|| "出错了".to_string()),
            }
        } else {
            st.reply_text.clone()
        };
        self.draw_bubble(bubble_x, bubble_y, &text, accent);

        // 底部状态栏
        self.draw_hud(&st);
    }

    fn draw_bubble(&mut self, x: f32, y: f32, text: &str, accent: Color) {
        let font = self.font;
        let lines = wrap(text, 20);
        let font_size = 18;
        let line_h = 26.0;
        let max_w = lines
            .iter()
            .map(|l| measure_text(l, font, font_size, 1.0).width)
            .fold(60.0, f32::max);
        let w = max_w + 28.0;
        let h = lines.len() as f32 * line_h + 20.0;

        draw_rectangle(x, y, w, h, Color::from_hex(0x1f2340));
        draw_rectangle(x, y, w, h, accent); // 边框
        draw_rectangle(x + 2.0, y + 2.0, w - 4.0, h - 4.0, Color::from_hex(0x1f2340));
        // 小尾巴
        draw_rectangle(x + 18.0, y + h, 14.0, 10.0, Color::from_hex(0x1f2340));
        draw_rectangle(x + 20.0, y + h, 10.0, 8.0, accent);

        for (i, line) in lines.iter().enumerate() {
            if let Some(f) = font {
                draw_text_ex(
                    line,
                    x + 14.0,
                    y + 22.0 + i as f32 * line_h,
                    TextParams {
                        font: Some(f),
                        font_size,
                        color: Color::from_hex(0xe8eaf6),
                        ..Default::default()
                    },
                );
            } else {
                draw_text(line, x + 14.0, y + 22.0 + i as f32 * line_h, font_size, Color::from_hex(0xe8eaf6));
            }
        }
    }

    fn draw_hud(&mut self, st: &UiState) {
        let y = screen_height() - 40.0;
        // 状态徽章
        let label = st.phase.label();
        let w = 96.0;
        let accent = phase_color(st.phase);
        draw_rectangle(20.0, y - 8.0, w, 28.0, accent);
        draw_rectangle(22.0, y - 6.0, w - 4.0, 24.0, Color::from_hex(0x141526));
        if let Some(f) = self.font {
            draw_text_ex(label, 30.0, y + 10.0, TextParams { font: Some(f), font_size: 16, color: accent, ..Default::default() });
        } else {
            draw_text(label, 30.0, y + 10.0, 16.0, accent);
        }

        // 麦克风电平条
        draw_rectangle(130.0, y, 200.0, 6.0, Color::from_hex(0x2b2f55));
        draw_rectangle(130.0, y, 200.0 * st.mic_level.clamp(0.0, 1.0), 6.0, Color::from_hex(0x5bc0be));

        // ASR 实时文本
        let partial = st.asr_partial.trim();
        if !partial.is_empty() {
            if let Some(f) = self.font {
                draw_text_ex(partial, 130.0, y - 12.0, TextParams { font: Some(f), font_size: 14, color: Color::from_hex(0x8f94b8), ..Default::default() });
            }
        }
        if !st.status.is_empty() {
            if let Some(f) = self.font {
                draw_text_ex(&st.status, 20.0, y + 36.0, TextParams { font: Some(f), font_size: 14, color: Color::from_hex(0xffb86b), ..Default::default() });
            }
        }
    }

    fn draw_think_bubble(&self, cx: f32, top: f32) {
        let bob = (self.t * 2.0).sin() * 2.0;
        for (i, r) in [(0.0, 5.0), (18.0, 8.0), (38.0, 11.0)].iter().enumerate() {
            let yy = top - 22.0 * i as f32 - bob;
            draw_circle(cx + r.0, yy, r.1, Color::from_hex(0xffd166));
        }
    }

    fn draw_notes(&self, cx: f32, top: f32) {
        let bob = (self.t * 4.0).sin() * 3.0;
        draw_circle(cx + 24.0, top - 14.0 + bob, 4.0, Color::from_hex(0x7ad0ff));
        draw_rectangle(cx + 28.0, top - 18.0 + bob, 3.0, 14.0, Color::from_hex(0x7ad0ff));
        draw_circle(cx + 8.0, top - 30.0 - bob, 4.0, Color::from_hex(0x7ad0ff));
        draw_rectangle(cx + 12.0, top - 34.0 - bob, 3.0, 14.0, Color::from_hex(0x7ad0ff));
    }

    fn draw_keyboard(&self, cx: f32, cy: f32) {
        // 工作模式:小人面前放一个小键盘
        let kx = cx - 46.0;
        let ky = cy + 32.0 * CHAR_SCALE * 0.55;
        draw_rectangle(kx, ky, 92.0, 30.0, Color::from_hex(0x3a4a6b));
        for r in 0..2 {
            for c in 0..5 {
                draw_rectangle(kx + 6.0 + c as f32 * 17.0, ky + 6.0 + r as f32 * 12.0, 12.0, 8.0, Color::from_hex(0x5a6a9b));
            }
        }
    }
}

pub fn phase_color(p: Phase) -> Color {
    match p {
        Phase::Idle => Color::from_hex(0x5bc0be),
        Phase::Listening => Color::from_hex(0xffd166),
        Phase::Thinking => Color::from_hex(0xff7e67),
        Phase::Speaking => Color::from_hex(0x7ad0ff),
        Phase::Working => Color::from_hex(0xa78bfa),
        Phase::Error => Color::from_hex(0xff5d5d),
    }
}

fn wrap(text: &str, max_chars: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cur = String::new();
    for c in text.chars() {
        if c == '\n' || cur.chars().count() >= max_chars {
            lines.push(std::mem::take(&mut cur));
            if c == '\n' {
                continue;
            }
        }
        cur.push(c);
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn load_font() -> Option<Font> {
    let bytes = std::fs::read(FONT_PATH).ok()?;
    load_ttf_bytes_from_bytes(&bytes).ok()
}

fn window_conf(cfg: &Config) -> Conf {
    Conf {
        window_title: "voxelf - 语音像素伙伴".to_string(),
        window_width: cfg.ui.window_width,
        window_height: cfg.ui.window_height,
        window_resizable: false,
        high_dpi: false,
        ..Default::default()
    }
}

/// 主界面:窗口循环,直到关闭。
pub async fn run_ui(cfg: Config, state: SharedState) {
    let mut window = Window::new(window_conf(&cfg)).await;
    window.set_cursor_grab(false);
    let font = load_font();
    if font.is_none() {
        tracing::warn!("字体缺失,界面中文将无法显示: 运行 node scripts/download-models.js");
    }
    let mut app = App::new(state, font);
    loop {
        window.next_frame().await;
        if is_key_pressed(KeyCode::Escape) {
            window.quit();
        }
        app.draw();
        if window.is_quit_requested() {
            break;
        }
    }
}

/// 渲染一帧并保存 PNG(冒烟测试)。
pub async fn run_smoke(cfg: Config, out: &str) {
    let mut window = Window::new(window_conf(&cfg)).await;
    window.next_frame().await;

    let state: SharedState = std::sync::Arc::new(std::sync::Mutex::new(UiState {
        phase: Phase::Speaking,
        asr_partial: "你好世界".into(),
        last_user_text: String::new(),
        reply_text: "你好,我是 Vox,你的像素伙伴!".into(),
        mic_level: 0.3,
        status: String::new(),
        error: None,
    }));
    let mut app = App::new(state, load_font());
    app.draw();
    window.next_frame().await;

    let img = get_screen_data();
    let _ = image::save_buffer(
        out,
        &img.bytes,
        img.width as u32,
        img.height as u32,
        image::ExtendedColorType::Rgba8,
    );
    tracing::info!("已保存截图 {out} ({}x{})", img.width, img.height);
    window.quit();
}
