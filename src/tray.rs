//! 系统托盘(Windows): 托盘图标 + 菜单(显示/锁定/置顶/自启/退出)。
//! 事件通过全局 channel 在 UI 帧里 poll,与 eframe 事件循环解耦。

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

    /// 每帧 poll 托盘事件,返回需要执行的动作(最多一个)
    pub fn poll(&mut self) -> Option<TrayAction> {
        // 菜单点击
        if let Ok(ev) = muda::MenuEvent::receiver().try_recv() {
            return match ev.id.0.as_str() {
                "toggle" => Some(TrayAction::ToggleVisible),
                "lock" => {
                    let next = !self.lock_item.is_checked();
                    self.lock_item.set_checked(next);
                    Some(TrayAction::ToggleLock)
                }
                "topmost" => {
                    let next = !self.topmost_item.is_checked();
                    self.topmost_item.set_checked(next);
                    Some(TrayAction::ToggleTopmost)
                }
                "autostart" => {
                    let next = !self.autostart_item.is_checked();
                    let _ = set_autostart(next);
                    self.autostart_item.set_checked(next);
                    Some(TrayAction::ToggleAutostart)
                }
                "quit" => Some(TrayAction::Quit),
                _ => None,
            };
        }
        // 双击托盘图标 → 显示/隐藏
        if let Ok(ev) = tray_icon::TrayIconEvent::receiver().try_recv() {
            if matches!(ev, tray_icon::TrayIconEvent::DoubleClick { .. }) {
                return Some(TrayAction::ToggleVisible);
            }
        }
        None
    }

    /// 同步 UI 侧状态到菜单(锁定/置顶被 UI 改变时)
    pub fn sync(&self, locked: bool, topmost: bool) {
        if self.lock_item.is_checked() != locked {
            self.lock_item.set_checked(locked);
        }
        if self.topmost_item.is_checked() != topmost {
            self.topmost_item.set_checked(topmost);
        }
    }

    #[allow(dead_code)]
    fn _keep_menu_alive(&self) -> &muda::Menu {
        &self.menu
    }
}
