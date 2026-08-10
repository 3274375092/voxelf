//! 颜文字(kaomoji)表情: 用文字表情替代像素小人,按状态/时间驱动动画。
//! 字符选择约束: 只用 SimHei 中字形索引正常的字符(fontdue idx != 0,
//! 见 tests/font_diag.rs)。半角假名(｡･)、韩文(ㅅ)在 SimHei 中是 .notdef,
//! 渲染会空白,故回避; 用 ^ - ω ▽ > < ; ゞ 等全支持字符组合。

use crate::state::Phase;

/// 按状态返回当前帧的颜文字。
/// t: 秒,驱动眨眼/嘴形等时间动画。
pub fn kaomoji(phase: Phase, t: f32) -> &'static str {
    match phase {
        // 待机: 偶尔眨眼(每 3.2s 周期末闭眼 0.2s)
        Phase::Idle => {
            let blink = (t % 3.2) > 3.0;
            if blink { "(-ω-)" } else { "(^ω^)" }
        }
        // 聆听: 耳朵竖起
        Phase::Listening => "(>ω<)",
        // 思考: 半闭眼
        Phase::Thinking => "(-ω-)",
        // 说话: 嘴形开合(ω 闭 / ▽ 张)
        Phase::Speaking => {
            let open = (t * 6.0).sin().abs() > 0.4;
            if open { "(^▽^)" } else { "(^ω^)" }
        }
        // 干活: 干劲满满
        Phase::Working => "(^ω^)ゞ",
        // 出错: 委屈
        Phase::Error => "(;ω;)",
    }
}

#[cfg(test)]
mod tests {
    use super::kaomoji;
    use crate::state::Phase;

    /// 防回归: 颜文字用到的每个字符必须在 SimHei 中有真实字形
    /// (idx != 0,否则 macroquad 渲染为空白)。这是之前"没有正常显示"的根因。
    #[test]
    fn all_kaomoji_chars_exist_in_simhei() {
        let bytes = std::fs::read("assets/fonts/SimHei.ttf").expect("SimHei.ttf 应存在");
        let font = fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
            .expect("fontdue 解析失败");
        let mut checked = std::collections::BTreeSet::new();
        for ph in [
            Phase::Idle, Phase::Listening, Phase::Thinking,
            Phase::Speaking, Phase::Working, Phase::Error,
        ] {
            for (i, t) in [0.0, 0.1, 3.1, 0.5].iter().enumerate() {
                for ch in kaomoji(ph, *t).chars() {
                    if !checked.insert(ch) {
                        continue;
                    }
                    let idx = font.lookup_glyph_index(ch);
                    assert_ne!(
                        idx, 0,
                        "{ph:?} 颜文字字符 {ch:?} U+{:04X} 在 SimHei 中无字形(会渲染空白)",
                        ch as u32
                    );
                }
                let _ = i;
            }
        }
    }

    #[test]
    fn all_phases_have_kaomoji() {
        for ph in [
            Phase::Idle, Phase::Listening, Phase::Thinking,
            Phase::Speaking, Phase::Working, Phase::Error,
        ] {
            let k = kaomoji(ph, 0.0);
            assert!(!k.is_empty(), "{ph:?} 应有颜文字");
            assert!(k.starts_with('(') && k.ends_with(')') || k.ends_with('ゞ'), "颜文字应成对括号: {k}");
        }
    }

    #[test]
    fn idle_blinks_periodically() {
        // t=0 睁眼, t=3.1(周期末眨眼窗口)闭眼, t=3.0 睁眼
        assert_eq!(kaomoji(Phase::Idle, 0.0), "(^ω^)");
        assert_eq!(kaomoji(Phase::Idle, 3.1), "(-ω-)");
        assert_eq!(kaomoji(Phase::Idle, 3.0), "(^ω^)");
    }

    #[test]
    fn speaking_switches_mouth() {
        let mut seen_open = false;
        let mut seen_closed = false;
        for i in 0..60 {
            let k = kaomoji(Phase::Speaking, i as f32 * 0.1);
            if k.contains('▽') { seen_open = true; }
            if k.contains('ω') { seen_closed = true; }
        }
        assert!(seen_open && seen_closed, "说话时应交替张嘴/闭嘴");
    }
}
