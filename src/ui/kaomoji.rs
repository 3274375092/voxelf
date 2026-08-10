//! 颜文字(kaomoji)表情: 用文字表情替代像素小人,按状态/时间驱动动画。
//! 所有字符已经 fontdue 验证在 SimHei/NotoSansSC 中有字形(见 tests/font_diag.rs)。

use crate::state::Phase;

/// 按状态返回当前帧的颜文字。
/// t: 秒,驱动眨眼/嘴形等时间动画。
pub fn kaomoji(phase: Phase, t: f32) -> &'static str {
    match phase {
        // 待机: 偶尔眨眼(每 3.2s 周期末闭眼 0.2s)
        Phase::Idle => {
            let blink = (t % 3.2) > 3.0;
            if blink { "(｡-ω-｡)" } else { "(｡･ω･｡)" }
        }
        // 聆听: 耳朵竖起(ㅅ 像竖起的猫耳)
        Phase::Listening => "(｡>ㅅ<｡)",
        // 思考: 半闭眼
        Phase::Thinking => "(｡･_･｡)",
        // 说话: 嘴形开合(ω 闭 / ▽ 张)
        Phase::Speaking => {
            let open = (t * 6.0).sin().abs() > 0.4;
            if open { "(｡･▽･｡)" } else { "(｡･ω･｡)" }
        }
        // 干活: 干劲满满
        Phase::Working => "(｀・ω・´)",
        // 出错: 委屈
        Phase::Error => "(；ω；)",
    }
}

#[cfg(test)]
mod tests {
    use super::kaomoji;
    use crate::state::Phase;

    #[test]
    fn all_phases_have_kaomoji() {
        for ph in [
            Phase::Idle, Phase::Listening, Phase::Thinking,
            Phase::Speaking, Phase::Working, Phase::Error,
        ] {
            let k = kaomoji(ph, 0.0);
            assert!(!k.is_empty(), "{ph:?} 应有颜文字");
            assert!(k.starts_with('(') && k.ends_with(')'), "颜文字应成对括号: {k}");
        }
    }

    #[test]
    fn idle_blinks_periodically() {
        // t=0 睁眼, t=3.1(周期末眨眼窗口)闭眼, t=3.0 睁眼
        assert_eq!(kaomoji(Phase::Idle, 0.0), "(｡･ω･｡)");
        assert_eq!(kaomoji(Phase::Idle, 3.1), "(｡-ω-｡)");
        assert_eq!(kaomoji(Phase::Idle, 3.0), "(｡･ω･｡)");
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
