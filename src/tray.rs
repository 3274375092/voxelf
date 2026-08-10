//! 系统托盘(Windows): 托盘图标 + 菜单(显示/锁定/置顶/自启/退出)。
//! 事件通过独立线程捕获并**直接执行**(不依赖 eframe 帧循环)。

use eframe::egui;

/// 托盘菜单动作(UI 每帧 poll 执行)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    /// 显示/隐藏窗口
    ToggleVisible,
    /// 锁定位置(不可拖动)
    ToggleLock,
    /// 置顶开关
    ToggleTopmost,
    /// 开机自启开关
    ToggleAutostart,
    /// 完全退出
    Quit,
}

/// 菜单 id → 动作(纯函数,可测试)
pub fn action_for_id(id: &str) -> Option<TrayAction> {
    match id {
        "toggle" => Some(TrayAction::ToggleVisible),
        "lock" => Some(TrayAction::ToggleLock),
        "topmost" => Some(TrayAction::ToggleTopmost),
        "autostart" => Some(TrayAction::ToggleAutostart),
        "quit" => Some(TrayAction::Quit),
        _ => None,
    }
}

pub struct Tray {
    _icon: tray_icon::TrayIcon,
    menu: muda::Menu,
    lock_item: muda::CheckMenuItem,
    topmost_item: muda::CheckMenuItem,
    autostart_item: muda::CheckMenuItem,
}

/// 生成 32x32 托盘图标(电视造型,程序绘制)
fn make_icon() -> tray_icon::Icon {
    let w = 32u32;
    let h = 32u32;
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    let put = |rgba: &mut Vec<u8>, x: i32, y: i32, r: u8, g: u8, b: u8, a: u8| {
        if x < 0 || y < 0 || x >= w as i32 || y >= h as i32 {
            return;
        }
        let i = ((y as u32 * w + x as u32) * 4) as usize;
        rgba[i] = r;
        rgba[i + 1] = g;
        rgba[i + 2] = b;
        rgba[i + 3] = a;
    };
    // 外壳(蓝灰) 2..30
    for y in 2..30 {
        for x in 2..30 {
            put(&mut rgba, x, y, 0x3c, 0x3f, 0x56, 255);
        }
    }
    // 屏幕(深色) 5..25
    for y in 5..26 {
        for x in 5..25 {
            put(&mut rgba, x, y, 0x11, 0x13, 0x22, 255);
        }
    }
    // 颜文字眼睛(accent 青)
    for (ex, ey) in [(11, 13), (12, 13), (20, 13), (21, 13), (11, 14), (12, 14), (20, 14), (21, 14)] {
        put(&mut rgba, ex, ey, 0x5b, 0xc0, 0xbe, 255);
    }
    // 嘴巴
    for x in 14..18 {
        put(&mut rgba, x, 17, 0x7a, 0xd0, 0xff, 255);
    }
    tray_icon::Icon::from_rgba(rgba, w, h).expect("托盘图标生成失败")
}

const AUTOSTART_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const AUTOSTART_NAME: &str = "voxelf";

/// 开机自启: 写/删 HKCU\...\Run
pub fn set_autostart(enabled: bool) -> std::io::Result<()> {
    let hkcu = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
    let run = hkcu.open_subkey_with_flags(
        AUTOSTART_KEY,
        winreg::enums::KEY_READ | winreg::enums::KEY_SET_VALUE,
    )?;
    if enabled {
        let exe = std::env::current_exe()?.to_string_lossy().into_owned();
        run.set_value(AUTOSTART_NAME, &format!("\"{exe}\""))?;
    } else {
        match run.delete_value(AUTOSTART_NAME) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// 开机自启当前状态
pub fn is_autostart() -> bool {
    let Ok(hkcu) = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER).open_subkey(AUTOSTART_KEY)
    else {
        return false;
    };
    hkcu.get_value::<String, _>(AUTOSTART_NAME).is_ok()
}

/// 托盘动作反馈(字幕短暂显示,确认命令生效)
pub type SharedTrayState = std::sync::Arc<std::sync::Mutex<TrayState>>;

/// 托盘/UI 共享状态(forwarder 线程直接更新,UI 每帧读取)。
/// 不依赖 eframe 帧循环: 事件在独立线程立即执行窗口命令。
#[derive(Debug, Clone, Default)]
pub struct TrayState {
    pub locked: bool,
    pub topmost: bool,
    pub visible: bool,
    pub quitting: bool,
    /// 开机自启状态(菜单勾选同步用)
    pub autostart: bool,
    /// 动作反馈(文本, 到达时间)
    pub feedback: Option<(String, std::time::Instant)>,
    /// 上次动作(去抖: Windows 一次点击产生 2 个 MenuEvent)
    pub last_action: Option<(TrayAction, std::time::Instant)>,
}

/// 独立线程: 持续轮询托盘/菜单事件,捕获后**直接执行动作**
/// (窗口命令跨线程发送,egui Context 线程安全),完全绕开 eframe 帧循环。
/// 实测: request_repaint 跨线程唤醒在 eframe 0.36 + Windows 上失效,
/// 事件要等鼠标移动触发帧才被处理; 这里改为直接执行,零延迟。
pub fn spawn_event_forwarder(ctx: egui::Context, state: SharedTrayState) {
    std::thread::spawn(move || loop {
        if let Ok(ev) = muda::MenuEvent::receiver().try_recv()
            && let Some(a) = action_for_id(ev.id.0.as_str())
        {
            tracing::debug!("托盘: 菜单事件 {:?} (id={})", a, ev.id.0);
            apply_action(&ctx, &state, a);
        }
        if let Ok(ev) = tray_icon::TrayIconEvent::receiver().try_recv()
            && matches!(ev, tray_icon::TrayIconEvent::DoubleClick { .. })
        {
            tracing::debug!("托盘: 双击事件");
            apply_action(&ctx, &state, TrayAction::ToggleVisible);
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    });
}

/// 执行托盘动作(在独立线程,直接生效,不等 UI 帧)。
/// 注意: tray-icon/muda 是 !Send,菜单勾选状态由主线程 UI 帧同步。
fn apply_action(ctx: &egui::Context, state: &SharedTrayState, action: TrayAction) {
    let mut s = state.lock().expect("tray state 锁");
    let now = std::time::Instant::now();
    // 去抖: Windows 一次菜单点击会产生 2 个 MenuEvent
    if let Some((a, at)) = s.last_action
        && a == action
        && now.duration_since(at).as_millis() < 150
    {
        tracing::debug!("托盘: 忽略重复事件 {:?}", action);
        return;
    }
    s.last_action = Some((action, now));

    match action {
        TrayAction::ToggleVisible => {
            s.visible = !s.visible;
            tracing::info!("托盘: 切换可见性 → {}", s.visible);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(s.visible));
            s.feedback = Some((if s.visible { "已显示".to_string() } else { "已隐藏到托盘".to_string() }, std::time::Instant::now()));
        }
        TrayAction::ToggleLock => {
            s.locked = !s.locked;
            tracing::info!("托盘: 锁定位置 = {}", s.locked);
            s.feedback = Some((if s.locked { "已锁定位置".to_string() } else { "已解锁".to_string() }, std::time::Instant::now()));
        }
        TrayAction::ToggleTopmost => {
            s.topmost = !s.topmost;
            let level = if s.topmost {
                egui::WindowLevel::AlwaysOnTop
            } else {
                egui::WindowLevel::Normal
            };
            tracing::info!("托盘: 置顶 = {}", s.topmost);
            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(level));
            s.feedback = Some((if s.topmost { "已置顶".to_string() } else { "取消置顶".to_string() }, std::time::Instant::now()));
        }
        TrayAction::ToggleAutostart => {
            let on = !is_autostart();
            let _ = set_autostart(on);
            s.autostart = on;
            tracing::info!("托盘: 开机自启 = {on}");
            s.feedback = Some((if on { "已开启开机自启".to_string() } else { "已关闭开机自启".to_string() }, std::time::Instant::now()));
        }
        TrayAction::Quit => {
            tracing::info!("托盘: 退出");
            s.quitting = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

impl Tray {
    pub fn new() -> Option<Self> {
        // 菜单项 id 需稳定字符串
        let lock_item =
            muda::CheckMenuItem::with_id("lock", "锁定位置", true, false, None);
        let topmost_item =
            muda::CheckMenuItem::with_id("topmost", "窗口置顶", true, true, None);
        let autostart_item =
            muda::CheckMenuItem::with_id("autostart", "开机自启", true, is_autostart(), None);
        let menu = muda::Menu::with_items(&[
            &muda::MenuItem::with_id("toggle", "显示 / 隐藏", true, None),
            &muda::PredefinedMenuItem::separator(),
            &lock_item,
            &topmost_item,
            &autostart_item,
            &muda::PredefinedMenuItem::separator(),
            &muda::MenuItem::with_id("quit", "退出", true, None),
        ])
        .ok()?;

        let icon = make_icon();
        let tray = tray_icon::TrayIconBuilder::new()
            .with_menu(Box::new(menu.clone()))
            .with_tooltip("voxelf - 语音像素伙伴")
            .with_icon(icon)
            .build()
            .ok()?;
        Some(Self {
            _icon: tray,
            menu,
            lock_item,
            topmost_item,
            autostart_item,
        })
    }

    /// 同步 UI 侧状态到菜单勾选(主线程调用;tray-icon/muda 是 !Send)
    pub fn sync(&self, locked: bool, topmost: bool, autostart: bool) {
        if self.lock_item.is_checked() != locked {
            self.lock_item.set_checked(locked);
        }
        if self.topmost_item.is_checked() != topmost {
            self.topmost_item.set_checked(topmost);
        }
        if self.autostart_item.is_checked() != autostart {
            self.autostart_item.set_checked(autostart);
        }
    }

    #[allow(dead_code)]
    fn _keep_menu_alive(&self) -> &muda::Menu {
        &self.menu
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 菜单 id → 动作映射必须与菜单项 id 一致(不一致 = 点了没反应)
    #[test]
    fn menu_id_mapping_works() {
        assert_eq!(action_for_id("toggle"), Some(TrayAction::ToggleVisible));
        assert_eq!(action_for_id("lock"), Some(TrayAction::ToggleLock));
        assert_eq!(action_for_id("topmost"), Some(TrayAction::ToggleTopmost));
        assert_eq!(action_for_id("autostart"), Some(TrayAction::ToggleAutostart));
        assert_eq!(action_for_id("quit"), Some(TrayAction::Quit));
        assert_eq!(action_for_id("unknown"), None);
        assert_eq!(action_for_id(""), None);
    }

    /// 开机自启注册表往返: 写入 → 读回 true → 删除 → 读回 false。
    /// 测试结束恢复原状,不影响用户设置。
    #[test]
    fn autostart_registry_roundtrip() {
        let original = is_autostart();
        set_autostart(true).expect("写入注册表应成功");
        assert!(is_autostart(), "写入后应检测到自启");
        set_autostart(false).expect("删除注册表应成功");
        assert!(!is_autostart(), "删除后应检测不到自启");
        // 恢复用户原状
        if original {
            set_autostart(true).expect("恢复自启");
        }
    }
}
