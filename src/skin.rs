//! 参考图 → 像素皮肤 转换管线(方案 B)。
//! 步骤: 居中裁剪为正方形 → 下采样到 64x64 → 中位切分(median cut)
//! 调色板量化 → (可选)边缘描边增强 → 输出 PNG 皮肤(assets/sprites/vox.png 格式)。
use anyhow::{Context, Result};
use image::Rgba;
use image::RgbaImage;

/// 皮肤画布边长(与 sprite::SIZE 一致)
pub const SKIN_SIZE: u32 = 64;

/// 量化后的调色板颜色数(像素画感: 颜色少、色块大)
const PALETTE_COLORS: usize = 24;

/// 调色板中的一个色箱(median cut 的分割单元)
#[derive(Clone)]
struct ColorBox {
    pixels: Vec<[u8; 4]>,
}

impl ColorBox {
    /// 通道范围最大的维度
    fn widest_channel(&self) -> usize {
        let mut min = [255u8; 4];
        let mut max = [0u8; 4];
        for p in &self.pixels {
            for c in 0..4 {
                min[c] = min[c].min(p[c]);
                max[c] = max[c].max(p[c]);
            }
        }
        let mut widest = 0;
        let mut span = 0;
        for c in 0..4 {
            let s = max[c] as u16 - min[c] as u16;
            if s > span {
                span = s;
                widest = c;
            }
        }
        widest
    }

    /// 平均色(作为该箱的代表色)
    fn average(&self) -> Rgba<u8> {
        let n = self.pixels.len().max(1) as u32;
        let mut sum = [0u32; 4];
        for p in &self.pixels {
            for c in 0..4 {
                sum[c] += p[c] as u32;
            }
        }
        Rgba([
            (sum[0] / n) as u8,
            (sum[1] / n) as u8,
            (sum[2] / n) as u8,
            (sum[3] / n) as u8,
        ])
    }
}

/// 中位切分: 反复把"通道范围最大"的箱按最长通道中位数切成两半,直到数量达标。
pub fn median_cut(pixels: impl IntoIterator<Item = [u8; 4]>, max_colors: usize) -> Vec<Rgba<u8>> {
    let mut boxes = vec![ColorBox { pixels: pixels.into_iter().collect() }];
    while boxes.len() < max_colors {
        // 找像素最多的箱来切(优先切大块,避免碎片箱)
        let idx = boxes
            .iter()
            .enumerate()
            .max_by_key(|(_, b)| (b.pixels.len(), b.widest_channel()))
            .map(|(i, _)| i);
        let Some(idx) = idx else { break };
        let b = &boxes[idx];
        if b.pixels.len() < 2 || b.widest_channel() == 0 && b.pixels.len() == 2 {
            break;
        }
        let ch = b.widest_channel();
        let mut sorted = b.pixels.clone();
        sorted.sort_by_key(|p| p[ch]);
        let mid = sorted.len() / 2;
        let (lo, hi) = sorted.split_at(mid);
        boxes[idx] = ColorBox { pixels: lo.to_vec() };
        boxes.push(ColorBox { pixels: hi.to_vec() });
    }
    boxes.iter().map(|b| b.average()).collect()
}

/// 在调色板中找最近色(简单 RGB 距离,忽略 alpha 差异)
fn nearest(palette: &[Rgba<u8>], c: Rgba<u8>) -> Rgba<u8> {
    palette
        .iter()
        .min_by_key(|p| {
            let dr = p[0] as i32 - c[0] as i32;
            let dg = p[1] as i32 - c[1] as i32;
            let db = p[2] as i32 - c[2] as i32;
            dr * dr + dg * dg + db * db
        })
        .copied()
        .unwrap_or(c)
}

/// 边缘描边增强: 与 8 邻域平均色差超过阈值的像素压暗(像素画轮廓感)。
fn edge_enhance(img: &mut RgbaImage, threshold: u32) {
    let (w, h) = (img.width() as i32, img.height() as i32);
    let mut out = img.clone();
    for y in 0..h {
        for x in 0..w {
            let c = img.get_pixel(x as u32, y as u32);
            let mut diff_sum = 0u32;
            let mut count = 0u32;
            for dy in -1..=1 {
                for dx in -1..=1 {
                    if dx == 0 && dy == 0 {
                        continue;
                    }
                    let (nx, ny) = (x + dx, y + dy);
                    if nx < 0 || ny < 0 || nx >= w || ny >= h {
                        continue;
                    }
                    let n = img.get_pixel(nx as u32, ny as u32);
                    let dr = c[0] as i32 - n[0] as i32;
                    let dg = c[1] as i32 - n[1] as i32;
                    let db = c[2] as i32 - n[2] as i32;
                    diff_sum += (dr * dr + dg * dg + db * db) as u32;
                    count += 1;
                }
            }
            let avg = if count > 0 { diff_sum / count } else { 0 };
            if avg > threshold {
                // 压暗但不全黑,保留一点原色
                out.put_pixel(
                    x as u32,
                    y as u32,
                    Rgba([
                        (c[0] as u32 * 60 / 100) as u8,
                        (c[1] as u32 * 60 / 100) as u8,
                        (c[2] as u32 * 60 / 100) as u8,
                        c[3],
                    ]),
                );
            }
        }
    }
    *img = out;
}

/// 参考图 → 像素皮肤主流程。
/// top_frac: 只取图顶部比例(立绘脸部通常在顶部,全身图直接下采样脸会太小);
/// 1.0 表示全图居中裁剪。
pub fn convert(input: &str, output: &str, outline: bool, top_frac: f32) -> Result<()> {
    let img = image::ImageReader::open(input)
        .with_context(|| format!("打开参考图失败: {input}"))?
        .with_guessed_format()
        .with_context(|| format!("无法识别参考图格式: {input}"))?
        .decode()
        .with_context(|| format!("解码参考图失败: {input}"))?
        .to_rgba8();
    let (w, h) = (img.width(), img.height());
    if w == 0 || h == 0 {
        anyhow::bail!("参考图尺寸无效");
    }
    // 可选: 先取顶部区域(立绘头部)
    let crop_h = (h as f32 * top_frac.clamp(0.1, 1.0)) as u32;
    let side = w.min(crop_h);
    // top 取景: 从顶部向下; 全图: 垂直居中
    let y0 = if top_frac >= 1.0 { (h - side) / 2 } else { 0 };
    // 水平居中裁剪为正方形(取景框内)
    let x0 = (w - side) / 2;
    let cropped = image::imageops::crop_imm(&img, x0, y0, side, side).to_image();
    // 下采样到皮肤尺寸(三角形滤波折中锐利与平滑)
    let mut small = image::imageops::resize(
        &cropped,
        SKIN_SIZE,
        SKIN_SIZE,
        image::imageops::FilterType::Triangle,
    );
    // 调色板量化
    let palette = median_cut(small.pixels().map(|p| p.0), PALETTE_COLORS);
    for p in small.pixels_mut() {
        *p = nearest(&palette, *p);
    }
    if outline {
        edge_enhance(&mut small, 3000);
    }
    small
        .save(output)
        .with_context(|| format!("保存皮肤失败: {output}"))?;
    tracing::info!("已生成像素皮肤 {output} ({}x{}, {PALETTE_COLORS} 色)", SKIN_SIZE, SKIN_SIZE);
    Ok(())
}

/// 语义色分类表(ASCII 预览用): 字符 → 代表色
const SEMANTIC: &[(&str, [u8; 3])] = &[
    ("O", [45, 35, 45]),    // 深/黑(眼睛/头发深部/阴影)
    ("W", [252, 252, 252]), // 白(衬衫/高光)
    ("S", [255, 222, 196]), // 肤
    ("P", [255, 168, 190]), // 粉(腮红/粉发)
    ("R", [222, 64, 64]),   // 红(蝴蝶结/红发饰)
    ("Y", [244, 204, 84]),  // 黄/金(金发/黄领带)
    ("B", [84, 116, 222]),  // 蓝(蓝眼/蓝衣)
    ("G", [92, 162, 92]),   // 绿
    ("C", [84, 190, 190]),  // 青
    ("V", [154, 92, 182]),  // 紫
    ("N", [142, 112, 82]),  // 棕(棕发/木色)
    ("L", [190, 190, 200]), // 浅灰(阴影/银)
];

/// 皮肤 ASCII 预览: 每个像素映射到最近语义色字符,辅助人工定位
/// 眼睛/嘴/猫耳等动画锚点(模型/人看不到彩色图时用)。
pub fn ascii_preview(input: &str) -> Result<()> {
    let img = image::open(input)
        .with_context(|| format!("打开皮肤失败: {input}"))?
        .to_rgba8();
    let (w, h) = (img.width(), img.height());
    for y in 0..h {
        let mut line = String::with_capacity(w as usize);
        for x in 0..w {
            let p = img.get_pixel(x, y);
            if p[3] < 128 {
                line.push('.');
                continue;
            }
            let c = [p[0], p[1], p[2]];
            let (ch, _) = SEMANTIC
                .iter()
                .min_by_key(|(_, sc)| {
                    let dr = sc[0] as i32 - c[0] as i32;
                    let dg = sc[1] as i32 - c[1] as i32;
                    let db = sc[2] as i32 - c[2] as i32;
                    dr * dr + dg * dg + db * db
                })
                .unwrap();
            line.push_str(ch);
        }
        println!("{line}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn median_cut_produces_requested_colors() {
        // 32 个不同颜色 → 量化到 8 色
        let pixels: Vec<[u8; 4]> = (0..32)
            .map(|i| [i as u8 * 8, (255 - i as u8 * 8), (i as u8 * 4), 255])
            .collect();
        let pal = median_cut(pixels, 8);
        assert_eq!(pal.len(), 8, "应量化到 8 色");
    }

    #[test]
    fn median_cut_fewer_colors_than_request() {
        let pixels: Vec<[u8; 4]> = vec![[10, 20, 30, 255], [11, 21, 31, 255]];
        let pal = median_cut(pixels, 24);
        assert!(pal.len() <= 2, "输入只有 2 色时不能凭空造出 24 色");
    }

    #[test]
    fn nearest_picks_closest() {
        let pal = vec![
            Rgba([255, 0, 0, 255]),
            Rgba([0, 255, 0, 255]),
            Rgba([0, 0, 255, 255]),
        ];
        assert_eq!(nearest(&pal, Rgba([250, 5, 5, 255])), Rgba([255, 0, 0, 255]));
        assert_eq!(nearest(&pal, Rgba([3, 250, 8, 255])), Rgba([0, 255, 0, 255]));
    }

    /// 端到端: 程序生成测试图 → 转换 → 输出尺寸与像素数正确。
    #[test]
    fn convert_pipeline_works() {
        let tmp = std::env::temp_dir().join("voxelf_skin_test");
        let _ = std::fs::create_dir_all(&tmp);
        let src = tmp.join("src.png");
        let dst = tmp.join("dst.png");
        // 128x128 渐变 + 色块测试图
        let mut img = RgbaImage::new(128, 128);
        for y in 0..128 {
            for x in 0..128 {
                img.put_pixel(x, y, Rgba([x as u8, y as u8, (x + y) as u8, 255]));
            }
        }
        img.save(&src).unwrap();
        convert(src.to_str().unwrap(), dst.to_str().unwrap(), true, 1.0).unwrap();
        let out = image::open(&dst).unwrap().to_rgba8();
        assert_eq!((out.width(), out.height()), (SKIN_SIZE, SKIN_SIZE));
        let colors: std::collections::HashSet<[u8; 4]> =
            out.pixels().map(|p| p.0).collect();
        assert!(colors.len() <= PALETTE_COLORS + 1, "量化后颜色应受限");
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
