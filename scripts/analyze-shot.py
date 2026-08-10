# 分析 voxelf smoke 截图: 统计各区域的亮色(accent)像素,判断颜文字/文字是否渲染
from PIL import Image
import sys

img = Image.open(sys.argv[1] if len(sys.argv) > 1 else 'kaomoji_preview.png').convert('RGB')
W, H = img.size
px = img.load()
print(f'尺寸 {W}x{H}')

# 颜文字/文字使用的 accent 色(phase_color)与背景
ACCENTS = {
    'Idle青(91,192,190)': (91, 192, 190),
    'Listening黄(255,209,102)': (255, 209, 102),
    'Thinking橙(255,126,103)': (255, 126, 103),
    'Speaking蓝(122,208,255)': (122, 208, 255),
    'Working紫(167,139,250)': (167, 139, 250),
    'Error红(255,93,93)': (255, 93, 93),
    '星紫蓝(150,155,210)': (150, 155, 210),
}

def near(a, b, tol=40):
    return abs(a[0]-b[0]) <= tol and abs(a[1]-b[1]) <= tol and abs(a[2]-b[2]) <= tol

# 区域: 主角色(中央偏左) 与 底部六预览(每格 135px 间距)
regions = {
    '主角色区(300x150 @ 中心36%,40%)': (W*0.36-150, H*0.40-75, W*0.36+150, H*0.40+75),
}
for i in range(6):
    cx = 90 + i * 135
    regions[f'预览{i}(90+{i}*135)'] = (cx-70, H-240, cx+70, H-100)

for name, (x0, y0, x1, y1) in regions.items():
    x0, y0, x1, y1 = int(x0), int(y0), int(x1), int(y1)
    counts = {k: 0 for k in ACCENTS}
    total = 0
    for y in range(max(0, y0), min(H, y1)):
        for x in range(max(0, x0), min(W, x1)):
            c = px[x, y]
            total += 1
            for k, a in ACCENTS.items():
                if near(c, a):
                    counts[k] += 1
    hit = {k: v for k, v in counts.items() if v > 0}
    print(f'{name}: 总{total}px, 命中 {hit if hit else "无(纯背景/暗色)"}')
