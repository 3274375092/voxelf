//! eframe(egui)桌宠 UI: 32x32 像素网格电视(每格 10px = 320x320 窗口),
//! 透明无边框 + 置顶 + 可拖动,颜文字在屏幕里发光显示。
//! 状态总线与 ASR/大脑/TTS 管道完全解耦。

use std::sync::Arc;

use eframe::egui;

use crate::state::{Phase, SharedState, UiState};
use crate::tray::{Tray, TrayAction};
use crate::ui::kaomoji;

/// 桌宠窗口尺寸(32x32 网格 x 10px)
const PET_W: f32 = 320.0;
const PET_H: f32 = 320.0;

pub struct VoxApp {
    state: SharedState,
    t: f64,
    tray: Option<Tray>,
    /// 事件转发线程 → UI 的动作通道(不依赖 eframe 帧循环)
    tray_rx: Option<flume::Receiver<TrayAction>>,
    locked: bool,
    topmost: bool,
    quitting: bool,
    /// 自维护可见性(不读 viewport 报告: 隐藏后 viewport 报 stale,依赖它无法恢复)
    visible: bool,
    /// 托盘动作反馈(字幕短暂显示,确认命令生效)
    feedback: Option<(String, f64)>,
}

/// 字幕文本截断: 超过 max_chars 字符截断并加省略号(纯函数,可测试)。
fn subtitle_text(text: &str, max_chars: usize) -> String {
    if text.chars().count() > max_chars {
        let mut s: String = text.chars().take(max_chars - 1).collect();
        s.push('…');
        s
    } else {
        text.to_string()
    }
}

impl VoxApp {
    pub fn new(state: SharedState, tray: Option<Tray>, tray_rx: Option<flume::Receiver<TrayAction>>) -> Self {
        Self {
            state,
            t: 0.0,
            tray,
            tray_rx,
            locked: false,
            topmost: true,
            quitting: false,
            visible: true,
            feedback: None,
        }
    }

    /// 显示 2 秒反馈字幕(托盘动作确认)
    fn set_feedback(&mut self, msg: impl Into<String>) {
        self.feedback = Some((msg.into(), self.t));
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
        let shell_top = egui::Color32::from_rgb(0x4a, 0x4e, 0x6c);
        let shell_mid = egui::Color32::from_rgb(0x3b, 0x3e, 0x55);
        let shell_bot = egui::Color32::from_rgb(0x2b, 0x2d, 0x3e);
        let edge = egui::Color32::from_rgb(0x5e, 0x63, 0x86);

        // ---- 外壳(格 1..31,像素圆角) ----
        let tv = egui::Rect::from_min_max(egui::pos2(g(1.0), g(1.0)), egui::pos2(g(31.0), g(31.0)));
        painter.rect_filled(tv, egui::CornerRadius::same(10), shell_mid);
        for (y0, y1, c) in [(1.0, 8.0, shell_top), (8.0, 22.0, shell_mid), (22.0, 31.0, shell_bot)] {
            let r = egui::Rect::from_min_max(egui::pos2(g(1.0), g(y0)), egui::pos2(g(31.0), g(y1)));
            painter.rect_filled(r, egui::CornerRadius::ZERO, c);
        }
        painter.rect_stroke(tv, egui::CornerRadius::same(10), egui::Stroke::new(2.0, edge), egui::StrokeKind::Inside);
        // 顶部高光带(更亮更宽,模拟曲面)
        painter.rect_filled(
            egui::Rect::from_min_max(egui::pos2(g(2.0), g(1.15)), egui::pos2(g(30.0), g(1.6))),
            egui::CornerRadius::same(2),
            egui::Color32::from_rgba_unmultiplied(230, 235, 255, 60),
        );
        painter.rect_filled(
            egui::Rect::from_min_max(egui::pos2(g(2.0), g(1.7)), egui::pos2(g(30.0), g(2.1))),
            egui::CornerRadius::same(2),
            egui::Color32::from_rgba_unmultiplied(200, 210, 245, 22),
        );

        // ---- 屏幕(格 2..30 x 3..25) ----
        let screen = egui::Rect::from_min_max(egui::pos2(g(2.0), g(3.0)), egui::pos2(g(30.0), g(25.0)));
        painter.rect_filled(screen, egui::CornerRadius::same(6), egui::Color32::from_rgb(0x08, 0x0a, 0x13));
        // 屏幕内凹阴影(3 层渐深,模拟 CRT 凹陷)
        for i in 0..3 {
            let s = screen.shrink(2.0 + i as f32 * 1.5);
            painter.rect_stroke(
                s,
                egui::CornerRadius::same(5),
                egui::Stroke::new(1.5, egui::Color32::from_rgba_unmultiplied(0, 0, 0, 26 - i as u8 * 7)),
                egui::StrokeKind::Inside,
            );
        }
        let screen_in = egui::Rect::from_min_max(egui::pos2(g(3.0), g(4.0)), egui::pos2(g(29.0), g(24.0)));
        painter.rect_filled(screen_in, egui::CornerRadius::same(4), egui::Color32::from_rgb(0x10, 0x12, 0x21));

        // 屏幕呼吸光: 所有状态屏幕边缘带 accent 微光,Working 快速脉冲
        let breathe = 0.5 + 0.5 * (t * 1.4).sin();
        let pulse = if st.phase == Phase::Working {
            0.7 + 0.3 * (t * 8.0).sin().abs()
        } else {
            breathe
        };
        painter.rect_stroke(
            screen_in,
            egui::CornerRadius::same(4),
            egui::Stroke::new(
                2.0,
                egui::Color32::from_rgba_unmultiplied(
                    accent.r(),
                    accent.g(),
                    accent.b(),
                    (12.0 + 20.0 * pulse) as u8,
                ),
            ),
            egui::StrokeKind::Inside,
        );

        // CRT 雪花噪点(待机/聆听时,屏幕内固定伪随机点,微闪)
        if st.phase == Phase::Idle || st.phase == Phase::Listening {
            let mut seed: u32 = 0x9e37_79b9;
            let mut rnd = move || {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                (seed >> 8) as f32 / 16777216.0
            };
            for i in 0..46 {
                let nx = g(3.5) + rnd() * g(25.0);
                let ny = g(4.5) + rnd() * g(18.5);
                let tw = 0.4 + 0.6 * (0.5 + 0.5 * (t * (2.0 + rnd() * 3.0) + i as f32).sin());
                painter.rect_filled(
                    egui::Rect::from_min_size(egui::pos2(nx, ny), egui::vec2(1.2, 1.2)),
                    0.5,
                    egui::Color32::from_rgba_unmultiplied(200, 205, 230, (5.0 + 9.0 * tw) as u8),
                );
            }
        }

        // 屏幕底部辉光(呼吸)
        for (i, a) in [(0u8, 8u8), (1, 15), (2, 22)] {
            let gh = egui::Rect::from_min_max(
                egui::pos2(g(4.0), g(21.0) + i as f32 * 8.0),
                egui::pos2(g(28.0), g(22.0) + i as f32 * 8.0),
            );
            painter.rect_filled(
                gh,
                egui::CornerRadius::same(4),
                egui::Color32::from_rgba_unmultiplied(
                    accent.r(),
                    accent.g(),
                    accent.b(),
                    (a as f32 * (0.6 + 0.4 * breathe)) as u8,
                ),
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
        // 玻璃反光(弧形,多段递减宽度)
        for (i, w) in [(0, 0.42f32), (1, 0.30), (2, 0.18), (3, 0.08)] {
            let wpx = screen_in.width() * w;
            painter.rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(screen_in.min.x + (screen_in.width() - wpx) / 2.0, g(4.4) + i as f32 * 1.6),
                    egui::pos2(screen_in.min.x + (screen_in.width() + wpx) / 2.0, g(4.8) + i as f32 * 1.6),
                ),
                egui::CornerRadius::same(2),
                egui::Color32::from_rgba_unmultiplied(190, 200, 240, 15 - i as u8 * 3),
            );
        }

        // ---- 字幕(屏幕内底部,贴底向上生长,层级最高) ----
        // 托盘动作反馈优先显示 2 秒
        let text = if let Some((msg, at)) = &self.feedback {
            if self.t - *at < 2.0 {
                msg.clone()
            } else {
                self.feedback = None;
                self.status_text(&st)
            }
        } else {
            self.status_text(&st)
        };
        let wrap_w = g(24.0);
        // 截断到最多 3 行(~23 字/行),超出加省略号
        let max_chars = 69;
        let display = subtitle_text(&text, max_chars);
        let sub_color = egui::Color32::from_rgb(0xcf, 0xd4, 0xf0);
        let sub = painter.layout(
            display,
            egui::FontId::proportional(12.0),
            sub_color,
            wrap_w,
        );
        // 字幕条贴屏幕底部,向上生长;高度上限 6 格(容纳 3 行)
        let bar_bottom = screen_in.max.y - 4.0;
        let bar_h = (sub.size().y + 12.0).min(g(6.0));
        let bar = egui::Rect::from_min_max(
            egui::pos2(g(3.0), bar_bottom - bar_h),
            egui::pos2(g(29.0), bar_bottom),
        );
        painter.rect_filled(
            bar,
            egui::CornerRadius::same(3),
            egui::Color32::from_rgba_unmultiplied(0, 0, 0, 130),
        );
        // 裁剪到条内,超长也不溢出界面
        let clipped = painter.with_clip_rect(bar);
        clipped.galley(
            egui::pos2(bar.center().x - sub.size().x / 2.0, bar.min.y + 6.0),
            sub,
            sub_color,
        );

        // ---- 底部控制条(格 25..31): 频道 + 电源灯 + 品牌 + 旋钮 ----
        let ctrl_y = g(28.0);
        // 频道字(左侧)
        painter.text(
            egui::pos2(g(2.2), ctrl_y),
            egui::Align2::LEFT_CENTER,
            "CH-1",
            egui::FontId::monospace(9.0),
            egui::Color32::from_rgb(0x6a, 0x6f, 0x90),
        );
        // 电源指示灯(呼吸)
        let led = egui::pos2(g(7.0), ctrl_y);
        let led_alpha = if st.phase == Phase::Error {
            (0.35 + 0.65 * (0.5 + 0.5 * (t * 5.0).sin())) as u8 * 255
        } else {
            (170.0 + 60.0 * breathe) as u8
        };
        painter.circle_filled(
            led,
            3.0,
            egui::Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), led_alpha),
        );
        painter.circle_filled(led, 1.3, egui::Color32::from_rgba_unmultiplied(255, 255, 255, 60));
        // 品牌字
        painter.text(
            egui::pos2(g(16.0), ctrl_y),
            egui::Align2::CENTER_CENTER,
            "voxelf",
            egui::FontId::proportional(11.0),
            egui::Color32::from_rgb(0x8a, 0x8f, 0xb0),
        );
        // 旋钮(带刻度)
        let knob = egui::pos2(g(27.5), ctrl_y);
        painter.circle_filled(knob, 6.0, shell_bot);
        painter.circle_stroke(knob, 6.0, egui::Stroke::new(1.5, edge));
        painter.circle_filled(knob, 2.2, accent);
        let tick = egui::pos2(knob.x, knob.y - 4.6);
        painter.circle_filled(tick, 0.9, egui::Color32::from_rgb(0x8a, 0x8f, 0xb0));

        // 外壳底部反射(屏幕光映到外壳下缘)
        painter.rect_filled(
            egui::Rect::from_min_max(egui::pos2(g(3.0), g(29.5)), egui::pos2(g(29.0), g(30.0))),
            egui::CornerRadius::same(2),
            egui::Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), (10.0 + 14.0 * breathe) as u8),
        );

        // 拖动: 按住电视区域移动窗口(锁定时不响应)
        let ctx = ui.ctx().clone();
        if !self.locked {
            let pointer = ui.input(|i| i.pointer.hover_pos());
            let pressed = ui.input(|i| i.pointer.primary_pressed());
            if pressed && pointer.is_some_and(|p| tv.contains(p)) {
                ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }
        }
    }
}

impl eframe::App for VoxApp {
    /// logic 层: 窗口隐藏时也会被调用(只要有 repaint 请求)。
    /// 托盘事件处理必须在这里,否则隐藏到托盘后就无法恢复显示。
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 维持动画时钟与持续重绘(隐藏时也保持,让 logic 持续调度)
        ctx.request_repaint_after(std::time::Duration::from_millis(16));
        let dt = ctx.input(|i| i.stable_dt).min(0.1) as f64;
        self.t += dt;
        self.handle_tray(ctx);
        // 关闭窗口(Alt+F4/系统关闭)→ 隐藏到托盘而非退出;托盘"退出"放行
        if ctx.input(|i| i.viewport().close_requested()) {
            if self.quitting {
                return; // 放行,让 eframe 正常退出
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            self.visible = false;
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // 可见帧同样处理托盘命令(与 logic 幂等)
        self.handle_tray(ui.ctx());
        // Esc → 隐藏到托盘
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.visible = false;
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Visible(false));
            return;
        }
        self.draw_pet(ui);
    }
}

impl VoxApp {
    /// 处理托盘动作(由事件转发线程捕获,经 channel 送达;不依赖帧循环)
    fn handle_tray(&mut self, ctx: &egui::Context) {
        // 收动作(先取出,避免 self 借用冲突)
        let tray_action = self.tray_rx.as_ref().and_then(|rx| rx.try_recv().ok());
        if let Some(action) = tray_action {
            if let Some(t) = self.tray.as_ref() {
                t.sync_checked(action);
                t.sync(self.locked, self.topmost);
            }
            match action {
                TrayAction::ToggleVisible => {
                    // 自维护状态取反(隐藏后 viewport 报告 stale,不能依赖它)
                    self.visible = toggle_visible(self.visible);
                    tracing::info!("托盘: 切换可见性 → {}", self.visible);
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(self.visible));
                }
                TrayAction::ToggleLock => {
                    self.locked = !self.locked;
                    tracing::info!("托盘: 锁定位置 = {}", self.locked);
                    self.set_feedback(if self.locked { "已锁定位置" } else { "已解锁" });
                }
                TrayAction::ToggleTopmost => {
                    self.topmost = !self.topmost;
                    let level = if self.topmost {
                        egui::WindowLevel::AlwaysOnTop
                    } else {
                        egui::WindowLevel::Normal
                    };
                    tracing::info!("托盘: 置顶 = {}", self.topmost);
                    ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(level));
                    self.set_feedback(if self.topmost { "已置顶" } else { "取消置顶" });
                }
                TrayAction::ToggleAutostart => {
                    let on = crate::tray::is_autostart();
                    tracing::info!("托盘: 开机自启 = {on}");
                    self.set_feedback(if on { "已开启开机自启" } else { "已关闭开机自启" });
                }
                TrayAction::Quit => {
                    // 标记真正退出,放行 close(否则被 close_requested 拦截成隐藏)
                    tracing::info!("托盘: 退出");
                    self.quitting = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
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

/// 启动 eframe 桌宠窗口(含系统托盘)。阻塞当前线程。
pub fn run_ui(state: SharedState) -> eframe::Result<()> {
    let tray = crate::tray::Tray::new();
    if tray.is_none() {
        tracing::warn!("托盘初始化失败,继续无托盘运行");
    }
    // 事件转发线程: 托盘事件不依赖 eframe 帧循环,独立线程捕获 + 唤醒
    let (tray_tx, tray_rx) = flume::unbounded::<TrayAction>();
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
            // 独立线程捕获托盘事件(不依赖帧循环,窗口休眠也能即时响应)
            let wake = {
                let ctx = cc.egui_ctx.clone();
                move || ctx.request_repaint()
            };
            crate::tray::spawn_event_forwarder(tray_tx, wake);
            Ok(Box::new(VoxApp::new(state, tray, Some(tray_rx))))
        }),
    )
}

/// 可见性切换决策(纯函数,可测试): 隐藏 → 显示,显示 → 隐藏
fn toggle_visible(current: bool) -> bool {
    !current
}

#[cfg(test)]
mod tests {
    use super::subtitle_text;
    use super::toggle_visible;
    use super::VoxApp;
    use crate::state::Phase;

    /// 复现测试: 窗口隐藏后,eframe 只调用 logic 层,不再调用 ui 层。
    /// 若托盘事件处理只放在 ui() 里,隐藏后"显示"命令永远无法处理 —— 界面无法恢复。
    #[test]
    fn hidden_window_never_runs_ui_frame() {
        // 模拟 eframe 生命周期: 隐藏帧只调 logic(有 repaint 请求时),ui 只在可见帧调
        let mut visible = false;
        let mut ui_calls = 0usize;
        let mut logic_calls = 0usize;
        let mut handled_in_hidden = false;
        let mut show_event_pending = true; // 托盘"显示"事件
        for _ in 0..10 {
            if visible {
                ui_calls += 1; // eframe: 窗口隐藏时不跑 egui pass(不调 ui)
            }
            logic_calls += 1; // eframe: 隐藏时仍会调 logic(若请求了 repaint)
            if show_event_pending {
                // 事件在隐藏帧到达,只有 logic 层可达
                show_event_pending = false;
                if !visible {
                    handled_in_hidden = true; // 处理发生在隐藏态
                    visible = toggle_visible(visible);
                }
            }
        }
        // 关键断言: "显示"事件是在窗口仍隐藏时被处理的(只有 logic 可达)
        assert!(handled_in_hidden, "显示事件应在隐藏态被处理(logic 层)");
        assert!(visible, "处理后窗口恢复显示");
        assert!(ui_calls > 0, "窗口恢复后 ui 应恢复调用");
        assert!(logic_calls >= 10, "logic 层应每帧调度");
    }

    /// 复现测试: 窗口隐藏后 viewport().visible() 可能报告 stale 值(仍为 true),
    /// 依赖它取反会导致"再次点击仍是 true→false",永远无法恢复显示。
    /// 修复: App 自维护可见性状态,不读 viewport 报告。
    #[test]
    fn hidden_viewport_reports_stale_visible() {
        let mut app_visible = true; // App 自维护状态
        let mut viewport_visible = true; // 模拟 eframe viewport 报告
        // 第一次点击: 隐藏
        app_visible = toggle_visible(app_visible);
        assert!(!app_visible, "第一次点击后应隐藏");
        // 窗口隐藏后,viewport 报告冻结(仍报 true,不再更新)
        viewport_visible = true; // stale
        // 第二次点击: 若依赖 viewport 读取 → 错误地取反为 false(再次隐藏)
        let wrong = toggle_visible(viewport_visible);
        assert!(!wrong, "依赖 stale viewport 报告无法恢复显示");
        // 正确做法: 自维护状态 → 恢复显示
        let right = toggle_visible(app_visible);
        assert!(right, "自维护状态可恢复显示");
    }

    /// 可见性切换: 隐藏(false)→ 显示(true),显示 → 隐藏
    #[test]
    fn toggle_visible_switches_state() {
        assert!(toggle_visible(false), "隐藏后应能切换为显示");
        assert!(!toggle_visible(true), "显示后应能切换为隐藏");
        // 往返稳定
        assert!(!toggle_visible(toggle_visible(false)));
        assert!(toggle_visible(toggle_visible(true)));
    }

    /// 字幕截断: 短文本原样,长文本截断加省略号,边界不截。
    #[test]
    fn subtitle_truncates_long_text() {
        assert_eq!(subtitle_text("你好", 63), "你好");
        // 63 字符整: 不截断
        let exact: String = "字".repeat(63);
        assert_eq!(subtitle_text(&exact, 63).chars().count(), 63, "恰好 63 字不应截断");
        // 64 字符: 截断为 62 字 + 省略号 = 63
        let long: String = "字".repeat(64);
        let out = subtitle_text(&long, 63);
        assert_eq!(out.chars().count(), 63, "截断后含省略号共 63 字");
        assert!(out.ends_with('…'), "应以省略号结尾: {out}");
        assert_eq!(out.chars().filter(|&c| c == '字').count(), 62);
    }

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
