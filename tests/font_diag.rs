// 诊断: 直接测试 fontdue 对中文字体的解析与光栅化。
#[cfg(test)]
mod tests {
    #[test]
    fn fontdue_cjk() {
        for path in ["assets/fonts/SimHei.ttf", "assets/fonts/NotoSansSC.ttf"] {
            let Ok(bytes) = std::fs::read(path) else {
                println!("{path}: 文件不存在");
                continue;
            };
            let font = fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
                .expect("fontdue 解析失败");
            println!("--- {path}: {} glyphs", font.glyph_count());
            for ch in ['你', '好', '测', '试', 'V', '1'] {
                let idx = font.lookup_glyph_index(ch);
                let (m, bmp) = font.rasterize(ch, 48.0);
                let ink = bmp.iter().filter(|&&b| b > 0).count();
                println!("  '{ch}' idx={idx} w={} h={} adv={} xmin={} ymin={} ink_px={ink}", m.width, m.height, m.advance_width, m.xmin, m.ymin);
            }
        }
    }
}
