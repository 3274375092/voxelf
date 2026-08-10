// 诊断: 检查颜文字(kaomoji)字符在字体中的字形覆盖,
// 决定 UI 能安全使用哪些颜文字组合。
#[cfg(test)]
mod tests {
    #[test]
    fn kaomoji_glyph_coverage() {
        for path in ["assets/fonts/SimHei.ttf", "assets/fonts/NotoSansSC.ttf"] {
            let Ok(bytes) = std::fs::read(path) else {
                println!("{path}: 文件不存在");
                continue;
            };
            let font = fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
                .expect("fontdue 解析失败");
            println!("--- {path}: {} glyphs", font.glyph_count());
            // 候选颜文字字符(半角假名/符号/日文假名)
            for ch in ['｡', '･', 'ω', 'ㅅ', 'ㅂ', '▽', '≧', '≦', '；', '｀', '´', '・', 'ゝ', 'ゞ', 'へ', 'ノ', '°', 'ヽ', 'ヾ', '_', '^', 'o', 'O', 'T', '(', ')', '0', '3', '9', '>', '<', '☆', '★'] {
                let idx = font.lookup_glyph_index(ch);
                let (m, bmp) = font.rasterize(ch, 48.0);
                let ink = bmp.iter().filter(|&&b| b > 0).count();
                let ok = if ink > 0 { "OK " } else { "MISS" };
                println!("  {ok} '{ch}' U+{:04X} idx={idx} ink={ink}", ch as u32);
            }
        }
    }
}
