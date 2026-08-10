//! eframe(egui)桌宠 UI: 32x32 像素网格电视(每格 10px = 320x320 窗口),
//! 透明无边框 + 置顶 + 可拖动,颜文字在屏幕里发光显示。
//! 状态总线与 ASR/大脑/TTS 管道完全解耦。

use std::sync::Arc;

use eframe::egui;

use crate::state::{Phase, SharedState, UiState};
use crate::ui::kaomoji;

/// 桌宠窗口尺寸(32x32 网格 x 10px)
const PET_W: f32 = 320.0;
const PET_H: f32 = 320.0;

pub struct VoxApp {
    state: SharedState,
    t: f64,
}

impl VoxApp {
    pub fn new(state: SharedState) -> Self {
        Self { state, t: 0.0 }
    }

    fn phase_color(phase: Phase) -> egui::Color32 {
        match phase {
            Phase::Idle => egui::Color32::from_rgb(0x5b, 0xc0, 0xbe),
            Phase::Listening => egui::Color32::from_rgb(0xff, 0xd1, 0x66),
            Phase::Thinking => egui::Color32::from_rgb(0xff, 0x7e, 0x67),
            Phase::Speaking => egui::Color32::from_rgb(0x7a, 0xd0, 0xff),
            Phase::Working => egui::Color32::from_rgb(0xa7, 0x8b, 0xfa),
            Phase::Error => egui::Color32::from_rgb(0xff, 0x5d, 0x5d),
        }
    }

    fn state(&self) -> UiState {
        match self.state.lock() {
            Ok(s) => s.clone(),
            Err(_) => UiState::default(),
        }
    }

    /// 浮动偏移(按状态)
    fn bob(&self, phase: Phase) -> f32 {
        let t = self.t as f32;
        match phase {
            Phase::Idle => (t * 2.0).sin() * 3.0,
            Phase::Listening => (t * 2.4).sin() * 4.0,
            Phase::Thinking => (t * 3.0).sin() * 2.0,
            Phase::Speaking => (t * 6.0).sin().abs() * 5.0,
            Phase::Working => (t * 5.0).sin() * 3.0,
            Phase::Error => 0.0,
        }
    }

    fn draw_kaomoji(
        &self,
        painter: &egui::Painter,
        center: egui::Pos2,
        text: &str,
        color: egui::Color32,
        size: f32,
    ) {
        // 霓虹发光: 两层大字号低透明度叠底
        for (s, a) in [(size + 14.0, 22), (size + 7.0, 46)] {
            let glow = egui::Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), a);
            let g = painter.layout_no_wrap(text.to_string(), egui::FontId::proportional(s), glow);
            let r = egui::Rect::from_center_size(center, g.size());
            painter.galley(r.min, g, glow);
        }
        let galley = painter.layout_no_wrap(text.to_string(), egui::FontId::proportional(size), color);
        let rect = egui::Rect::from_center_size(center, galley.size());
        painter.galley(rect.min, galley, color);
    }

    /// 屏幕内装饰(思考气泡/音符/键盘)
    fn draw_deco(&self, painter: &egui::Painter, phase: Phase, cx: f32, cy: f32) {
        let t = self.t as f32;
        match phase {
            Phase::Thinking => {
                let bob = (t * 2.0).sin() * 2.0;
                for (i, r) in [(0.0, 4.0), (16.0, 6.0), (34.0, 8.0)].iter().enumerate() {
                    let yy = cy - 20.0 * i as f32 - bob;
                    painter.circle_filled(
                        egui::pos2(cx + r.0, yy),
                        r.1,
                        egui::Color32::from_rgb(0xff, 0xd1, 0x66),
                    );
                }
            }
            Phase::Speaking => {
                let bob = (t * 4.0).sin() * 3.0;
                let c = egui::Color32::from_rgb(0x7a, 0xd0, 0xff);
                painter.circle_filled(egui::pos2(cx + 22.0, cy - 20.0 + bob), 3.5, c);
                painter.rect_filled(
                    egui::Rect::from_min_size(egui::pos2(cx + 25.5, cy - 24.0 + bob), egui::vec2(3.0, 12.0)),
                    1.5,
                    c,
                );
                painter.circle_filled(egui::pos2(cx + 7.0, cy - 36.0 - bob), 3.5, c);
                painter.rect_filled(
                    egui::Rect::from_min_size(egui::pos2(cx + 10.5, cy - 40.0 - bob), egui::vec2(3.0, 12.0)),
                    1.5,
                    c,
                );
            }
            Phase::Working => {
                let kx = cx - 40.0;
                let ky = cy + 56.0;
                painter.rect_filled(
                    egui::Rect::from_min_size(egui::pos2(kx, ky), egui::vec2(80.0, 26.0)),
                    3.0,
                    egui::Color32::from_rgb(0x3a, 0x4a, 0x6b),
                );
                for r in 0..2 {
                    for c in 0..5 {
                        painter.rect_filled(
                            egui::Rect::from_min_size(
                                egui::pos2(kx + 5.0 + c as f32 * 15.0, ky + 5.0 + r as f32 * 10.0),
                                egui::vec2(10.0, 6.0),
                            ),
                            1.5,
                            egui::Color32::from_rgb(0x5a, 0x6a, 0x9b),
                        );
                    }
                }
            }
            _ => {}
        }
    }

    fn status_text(&self, st: &UiState) -> String {
        if !st.reply_text.is_empty() {
            return st.reply_text.clone();
        }
        match st.phase {
            Phase::Idle => "对着麦克风跟我说点什么吧~".into(),
            Phase::Listening => {
                if st.asr_partial.is_empty() { "我在听呢..." } else { &st.asr_partial }
            }
            .into(),
            Phase::Thinking => "嗯...让我想想~".into(),
            Phase::Speaking => "正在说话...".into(),
            Phase::Working => "收到,开始干活!".into(),
            Phase::Error => st.error.clone().unwrap_or_else(|| "出错了".into()),
        }
    }

    /// 桌宠: 32x32 像素网格电视,颜文字在屏幕里发光
    fn draw_pet(&mut self, ui: &mut egui::Ui) {
        let st = self.state();
        let accent = Self::phase_color(st.phase);
        let bob = self.bob(st.phase);
        let painter = ui.painter();
        let t = self.t as f32;
        // 32x32 网格,每格 10px
        const G: f32 = 10.0;
        let g = |n: f32| n * G;
        let shell_top = egui::Color32::from_rgb(0x48, 0x4c, 0x68);
        let shell_mid = egui::Color32::from_rgb(0x3c, 0x3f, 0x56);
        let shell_bot = egui::Color32::from_rgb(0x2e, 0x30, 0x42);
        let edge = egui::Color32::from_rgb(0x5a, 0x5f, 0x82);

        // ---- 外壳(格 1..31,像素圆角) ----
        let tv = egui::Rect::from_min_max(egui::pos2(g(1.0), g(1.0)), egui::pos2(g(31.0), g(31.0)));
        painter.rect_filled(tv, egui::CornerRadius::same(10), shell_mid);
        for (y0, y1, c) in [(1.0, 10.0, shell_top), (10.0, 24.0, shell_mid), (24.0, 31.0, shell_bot)] {
            let r = egui::Rect::from_min_max(egui::pos2(g(1.0), g(y0)), egui::pos2(g(31.0), g(y1)));
            painter.rect_filled(r, egui::CornerRadius::ZERO, c);
        }
        painter.rect_stroke(tv, egui::CornerRadius::same(10), egui::Stroke::new(2.0, edge), egui::StrokeKind::Inside);
        // 顶部高光带
        painter.rect_filled(
            egui::Rect::from_min_max(egui::pos2(g(2.0), g(1.2)), egui::pos2(g(30.0), g(1.8))),
            egui::CornerRadius::same(2),
            egui::Color32::from_rgba_unmultiplied(220, 225, 255, 36),
        );

        // ---- 屏幕(格 2..30 x 3..25) ----
        let screen = egui::Rect::from_min_max(egui::pos2(g(2.0), g(3.0)), egui::pos2(g(30.0), g(25.0)));
        painter.rect_filled(screen, egui::CornerRadius::same(6), egui::Color32::from_rgb(0x0a, 0x0c, 0x16));
        let screen_in = egui::Rect::from_min_max(egui::pos2(g(3.0), g(4.0)), egui::pos2(g(29.0), g(24.0)));
        painter.rect_filled(screen_in, egui::CornerRadius::same(4), egui::Color32::from_rgb(0x11, 0x13, 0x22));
        if st.phase == Phase::Working {
            let pulse = 40 + 26 * (t * 8.0).sin().abs() as u8;
            painter.rect_stroke(
                screen_in,
                egui::CornerRadius::same(4),
                egui::Stroke::new(2.0, egui::Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), pulse)),
                egui::StrokeKind::Inside,
            );
        }
        // 屏幕底部辉光
        for (i, a) in [(0u8, 10u8), (1, 18), (2, 26)] {
            let gh = egui::Rect::from_min_max(
                egui::pos2(g(4.0), g(21.0) + i as f32 * 8.0),
                egui::pos2(g(28.0), g(22.0) + i as f32 * 8.0),
            );
            painter.rect_filled(
                gh,
                egui::CornerRadius::same(4),
                egui::Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), a),
            );
        }

        // ---- 颜文字(屏幕中央,发光) + 装饰 ----
        let cx = screen_in.center().x;
        let cy = screen_in.center().y - 12.0 + bob;
        let face = kaomoji::kaomoji(st.phase, t);
        self.draw_kaomoji(painter, egui::pos2(cx, cy), face, accent, 52.0);
        self.draw_deco(painter, st.phase, cx, g(5.0) + 26.0);

        // CRT 扫描线
        for i in 0..4 {
            let sy = g(5.0) + i as f32 * g(4.0);
            painter.line_segment(
                [egui::pos2(g(4.0), sy), egui::pos2(g(28.0), sy)],
                egui::Stroke::new(1.0, egui::Color32::from_rgba_unmultiplied(255, 255, 255, 12)),
            );
        }
        // 玻璃反光
        painter.rect_filled(
            egui::Rect::from_min_max(egui::pos2(g(4.0), g(4.5)), egui::pos2(g(14.0), g(5.2))),
            egui::CornerRadius::same(2),
            egui::Color32::from_rgba_unmultiplied(190, 200, 240, 15),
        );

        // ---- 字幕(屏幕内底部) ----
        let text = self.status_text(&st);
        let sub = painter.layout_no_wrap(
            text,
            egui::FontId::proportional(13.0),
            egui::Color32::from_rgb(0xcf, 0xd4, 0xf0),
        );
        let sub_w = sub.size().x.min(g(24.0));
        painter.rect_filled(
            egui::Rect::from_min_max(egui::pos2(g(3.0), g(22.0)), egui::pos2(g(29.0), g(23.5))),
            egui::CornerRadius::same(3),
            egui::Color32::from_rgba_unmultiplied(0, 0, 0, 90),
        );
        painter.galley(
            egui::pos2(screen_in.center().x - sub_w / 2.0, g(22.3)),
            sub,
            egui::Color32::from_rgb(0xcf, 0xd4, 0xf0),
        );

        // ---- 底部控制条(格 25..31): 电源灯 + 品牌 + 旋钮 ----
        let ctrl_y = g(28.0);
        let led = egui::pos2(g(4.5), ctrl_y);
        let led_alpha = if st.phase == Phase::Error {
            (0.35 + 0.65 * (0.5 + 0.5 * (t * 5.0).sin())) as u8 * 255
        } else {
            255
        };
        painter.circle_filled(
            led,
            3.0,
            egui::Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), led_alpha),
        );
        painter.circle_filled(led, 1.4, egui::Color32::from_rgba_unmultiplied(255, 255, 255, 70));
        painter.text(
            egui::pos2(g(16.0), ctrl_y),
            egui::Align2::CENTER_CENTER,
            "voxelf",
            egui::FontId::proportional(11.0),
            egui::Color32::from_rgb(0x8a, 0x8f, 0xb0),
        );
        let knob = egui::pos2(g(27.5), ctrl_y);
        painter.circle_filled(knob, 6.0, shell_bot);
        painter.circle_stroke(knob, 6.0, egui::Stroke::new(1.5, edge));
        painter.circle_filled(knob, 2.2, accent);

        // 拖动: 按住电视移动窗口
        let ctx = ui.ctx().clone();
        let pointer = ui.input(|i| i.pointer.hover_pos());
        let pressed = ui.input(|i| i.pointer.primary_pressed());
        if pressed && pointer.is_some_and(|p| tv.contains(p)) {
            ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
    }
}

impl eframe::App for VoxApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // 动画需要持续重绘(egui 默认只在有输入时重绘,鼠标不动会冻结)
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(16));
        let dt = ui.input(|i| i.stable_dt).min(0.1) as f64;
        self.t += dt;
        self.draw_pet(ui);
    }
}

/// 注册 CJK 字体(SimHei 优先,Noto 兜底)。
fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    for (name, path) in [
        ("simhei", "assets/fonts/SimHei.ttf"),
        ("noto", "assets/fonts/NotoSansSC.ttf"),
    ] {
        let Ok(bytes) = std::fs::read(path) else { continue };
        fonts
            .font_data
            .insert(name.to_string(), Arc::new(egui::FontData::from_owned(bytes)));
        for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            fonts.families.entry(family).or_default().push(name.to_string());
        }
        tracing::info!("eframe 字体加载: {path}");
        break; // 第一个成功即可
    }
    ctx.set_fonts(fonts);
}

/// 启动 eframe 桌宠窗口。阻塞当前线程。
pub fn run_ui(state: SharedState) -> eframe::Result<()> {
    let viewport = egui::ViewportBuilder::default()
        .with_title("voxelf - 语音像素伙伴")
        .with_inner_size([PET_W, PET_H])
        .with_resizable(false)
        .with_transparent(true)
        .with_decorations(false)
        .with_always_on_top()
        .with_window_level(egui::WindowLevel::AlwaysOnTop);
    let native_options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "voxelf",
        native_options,
        Box::new(move |cc| {
            setup_fonts(&cc.egui_ctx);
            Ok(Box::new(VoxApp::new(state)))
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::VoxApp;
    use crate::state::Phase;

    #[test]
    fn all_phases_have_colors_and_bob() {
        for ph in [
            Phase::Idle, Phase::Listening, Phase::Thinking,
            Phase::Speaking, Phase::Working, Phase::Error,
        ] {
            let _ = VoxApp::phase_color(ph);
            let _ = ph.label();
        }
    }
}
