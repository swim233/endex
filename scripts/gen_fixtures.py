#!/usr/bin/env python3
"""Generate synthetic search fixtures (fake screenshots, photos, notes, docx) plus eval queries.

All content is invented. Usage: gen_fixtures.py <out_dir>
Writes <out_dir>/{images,docs}/..., manifests images.txt / texts.txt, and queries.toml.
Requires Pillow and the Noto Sans CJK fonts.
"""

import math
import random
import sys
import zipfile
from pathlib import Path
from xml.sax.saxutils import escape

from PIL import Image, ImageDraw, ImageFilter, ImageFont

CJK = "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc"
SANS, MONO = 2, 7  # face indices of Noto Sans CJK SC / Noto Sans Mono CJK SC
W, H = 2560, 1600

SCREENSHOTS = {
    "屏幕截图_20250812_101500_terminal.png": (
        "dark",
        "Konsole — cargo build",
        [
            "$ cargo build --release",
            "   Compiling endex-core v0.1.0",
            "error[E0382]: borrow of moved value: `config`",
            "  --> src/main.rs:42:20",
            "   |",
            "40 |     let runner = Runner::new(config);",
            "   |                              ------ value moved here",
            '42 |     println!("{}", config.name);',
            "   |                    ^^^^^^^^^^^ value borrowed here after move",
            "error: could not compile `endex-core` due to 1 previous error",
        ],
    ),
    "屏幕截图_20250903_150210_chat.png": (
        "light",
        "微信 — 项目组",
        [
            "张经理：大家好，明天下午三点在三楼会议室开项目评审会。",
            "张经理：记得带上第三季度的报表和演示文稿。",
            "小李：收到，我会提前十分钟到。",
            "小王：评审会需要准备演示吗？",
            "张经理：每个小组准备五分钟的进度汇报。",
        ],
    ),
    "屏幕截图_20251015_083000_ticket.png": (
        "light",
        "12306 — 订单详情",
        [
            "火车票 订单已支付",
            "车次 G1234  北京南 → 上海虹桥",
            "发车时间 2026年5月1日 08:00",
            "二等座 05车 12F号",
            "票价 ¥553.0  请携带身份证件检票乘车",
        ],
    ),
    "屏幕截图_20251120_221145_editor.png": (
        "dark",
        "Kate — chunk.rs",
        [
            "fn parse_markdown_heading(line: &str) -> Option<usize> {",
            "    let level = line.chars().take_while(|c| *c == '#').count();",
            "    (1..=6).contains(&level).then_some(level)",
            "}",
            "",
            "pub fn split_sections(text: &str) -> Vec<Section> {",
            "    let mut sections = Vec::new();",
            "    // walk lines and start a new section at each heading",
        ],
    ),
    "屏幕截图_20251208_071530_weather.png": (
        "light",
        "天气",
        [
            "杭州市 天气预报",
            "今天 多云转小雨 18°C ~ 24°C",
            "湿度 76%  东南风 3级",
            "明天 小雨 16°C ~ 21°C  空气质量 良",
            "出门记得带伞",
        ],
    ),
    "屏幕截图_20260114_190000_wifi.png": (
        "light",
        "系统设置 — 网络",
        [
            "Wi-Fi 设置",
            "网络名称 HomeNet-5G  已连接",
            "IPv4 地址 192.168.1.23",
            "子网掩码 255.255.255.0  路由器 192.168.1.1",
            "DNS 服务器 223.5.5.5",
        ],
    ),
    "屏幕截图_20260222_123000_recipe.png": (
        "light",
        "Firefox — 家常菜谱",
        [
            "红烧肉的做法",
            "1. 五花肉切成三厘米见方的块，冷水下锅焯水。",
            "2. 锅中放少许油，加入冰糖小火炒出糖色。",
            "3. 放入肉块翻炒上色，加生抽、老抽、料酒。",
            "4. 加开水没过肉块，小火炖一个小时后大火收汁。",
        ],
    ),
    "屏幕截图_20260310_094512_error.png": (
        "light",
        "Dolphin",
        [
            "An error occurred",
            'Failed to mount volume "Backup Disk":',
            "permission denied (os error 13)",
            "Check that you are allowed to access this device.",
            "[ Details ]   [ OK ]",
        ],
    ),
    "屏幕截图_20260405_200800_calendar.png": (
        "light",
        "日历 — 2026年4月",
        [
            "2026年4月 日程安排",
            "周一 19:00 健身房 力量训练",
            "周三 10:30 牙医预约 复查",
            "周五 20:00 读书会 讨论《三体》",
            "周日 全天 去郊外徒步",
        ],
    ),
    "屏幕截图_20260518_163020_order.png": (
        "light",
        "购物 — 订单详情",
        [
            "订单已发货",
            "商品 机械键盘 茶轴 87键 无线版",
            "快递公司 顺丰速运",
            "快递单号 SF1234567890",
            "预计 5月20日 送达",
        ],
    ),
    "屏幕截图_20260702_111111_gitlog.png": (
        "dark",
        "Konsole — git log",
        [
            "commit 3f2a1b9c (HEAD -> main)",
            "    refactor: split indexer into parse, embed and write stages",
            "commit 9e8d7c6b",
            "    fix: keep heading path when chunking long markdown sections",
            "commit 1a2b3c4d",
            "    feat: add reciprocal rank fusion for hybrid search",
        ],
    ),
    "屏幕截图_20260905_080000_desktop.png": ("wallpaper", None, []),
}

PHOTOS = ["sunset.jpg", "red_apple.png", "snow_mountain.jpg", "night_city.jpg", "forest.webp", "ocean.jpg"]

MARKDOWN = {
    "notes/rust_ownership.md": """# Rust 所有权笔记

## 所有权规则
每个值都有一个所有者。所有者离开作用域时，值会被释放。赋值或传参会移动所有权，移动之后原变量不能再使用。

## 借用规则
可以有任意多个不可变引用，或者恰好一个可变引用，二者不能同时存在。引用必须始终有效。

## 生命周期
生命周期标注描述引用之间的关系，编译器用它检查悬垂引用。大多数情况下可以省略标注。
""",
    "notes/kde_setup.md": """# KDE Plasma 配置记录

## 快捷键
Meta+E 打开 Dolphin，Meta+Enter 打开 Konsole，Ctrl+Alt+T 也可以打开终端。

## Wayland 下的输入法
在系统设置的虚拟键盘里选择 fcitx5，然后在 fcitx5 配置里添加拼音输入法。Chrome 需要加上 --enable-wayland-ime 参数。

## 显示缩放
笔记本屏幕使用 150% 缩放，外接显示器使用 100%。
""",
    "recipes/hongshaorou.md": """# 红烧肉

## 材料
五花肉 500 克，冰糖 30 克，生抽两勺，老抽一勺，料酒，葱姜，八角两个。

## 步骤
五花肉切块后冷水下锅焯水，捞出洗净。小火炒糖色，下肉块翻炒上色，加调料和开水，小火炖一小时，最后大火收汁。
""",
    "travel/kyoto_trip.md": """# 京都旅行计划

## 第一天 清水寺
早上去清水寺，然后沿着二年坂、三年坂散步，下午去祇园看看花见小路。

## 第二天 岚山
上午去岚山竹林小径和天龙寺，中午吃汤豆腐，下午坐小火车看保津川峡谷。

## 预算
住宿每晚约一万日元，交通使用巴士一日券。
""",
    "work/weekly_meeting_2026_03.md": """# 项目周会纪要 2026-03-16

## 进展
索引模块完成了 Markdown 和 docx 的解析，搜索延迟降低到 50 毫秒以内。

## 决定
混合检索使用 RRF 融合，k 取 60。图片结果和文档结果分开展示。

## 待办事项
- 补充评测集的查询
- 调研截图文字识别的准确率
- 下周三前完成命令行的 status 子命令
""",
    "reading/three_body.md": """# 《三体》读书笔记

## 黑暗森林
宇宙就是一座黑暗森林，每个文明都是带枪的猎人。暴露自己的位置就可能招致毁灭。

## 人物
叶文洁、汪淼、罗辑、章北海。罗辑提出了黑暗森林法则，成为面壁者。
""",
    "english/vector_search.md": """# Vector search basics

## Embeddings
Text and images are mapped into a shared vector space; cosine similarity of normalized vectors ranks candidates.

## Exact versus approximate search
Exact flat search scans every vector and is fine for a few hundred thousand rows. HNSW and IVF-PQ trade recall for speed.

## BM25 and hybrid ranking
BM25 scores keyword overlap with term frequency saturation and document length normalization. Hybrid ranking fuses the keyword list and the vector list with reciprocal rank fusion.
""",
}

LONG_SECTION = (
    "这是一段用于测试分块的长文本。系统需要把很长的章节切成不超过上限的小块，"
    "同时保留标题路径，方便在搜索结果里显示位置。The chunker must respect the token budget "
    "and split on sentence boundaries whenever possible, so that each piece stays meaningful. "
)

TEXTS = {
    "code/parse_config.py": '''"""Load and validate user settings."""
import tomllib


def load_user_settings(path):
    with open(path, "rb") as handle:
        settings = tomllib.load(handle)
    if "max_retry_count" not in settings:
        settings["max_retry_count"] = 3
    return settings


def merge_default_profile(settings, defaults):
    return {**defaults, **settings}
''',
    "code/RetryingHttpClient.java": """package example.net;

public class RetryingHttpClient {
    private final int maxRetryCount;
    private final long backoffMillis;

    public RetryingHttpClient(int maxRetryCount, long backoffMillis) {
        this.maxRetryCount = maxRetryCount;
        this.backoffMillis = backoffMillis;
    }

    public Response sendWithRetry(Request request) throws IOException {
        IOException lastError = null;
        for (int attempt = 0; attempt <= maxRetryCount; attempt++) {
            try {
                return send(request);
            } catch (IOException e) {
                lastError = e;
                sleep(backoffMillis * (1L << attempt));
            }
        }
        throw lastError;
    }
}
""",
    "logs/backup_job.txt": """2026-04-02 03:00:01 backup job started
2026-04-02 03:00:05 snapshot of /home created
2026-04-02 03:12:40 uploaded 1532 files to the NAS
2026-04-02 03:12:41 backup job finished, 0 errors
""",
}

DOCX = {
    "reports/quarterly_sales_report.docx": [
        "2026年第一季度销售报告",
        "本季度总销售额同比增长 12%，其中华东区增长最快，达到 18%。",
        "华南区受物流影响略有下滑，下季度将重点优化仓储和配送。",
        "新产品线智能台灯贡献了 9% 的营收。",
    ],
    "letters/cover_letter.docx": [
        "Cover letter",
        "Dear hiring team,",
        "I am applying for the backend engineer position. I have five years of experience building search services in Rust and Python.",
        "In my last role I reduced query latency by forty percent by introducing hybrid keyword and vector ranking.",
        "Kind regards, A. Example",
    ],
    "manuals/router_manual.docx": [
        "无线路由器使用说明",
        "首次使用时，用网线连接电脑和路由器的 LAN 口，在浏览器中打开 192.168.1.1 进行设置。",
        "恢复出厂设置：在通电状态下长按 RESET 键 10 秒，指示灯全部闪烁后松开。",
        "建议定期升级固件以获得安全更新。",
    ],
}

QUERIES = [
    ("所有权和借用规则", ["docs/notes/rust_ownership.md"], "text"),
    ("fcitx5 输入法 Wayland", ["docs/notes/kde_setup.md"], "text"),
    ("怎么做红烧肉", ["docs/recipes/hongshaorou.md", "images/屏幕截图_20260222_123000_recipe.png"], "text"),
    ("京都 竹林", ["docs/travel/kyoto_trip.md"], "text"),
    ("周会的待办事项", ["docs/work/weekly_meeting_2026_03.md"], "text"),
    ("黑暗森林法则", ["docs/reading/three_body.md"], "text"),
    ("hybrid ranking with BM25", ["docs/english/vector_search.md"], "text"),
    ("load_user_settings", ["docs/code/parse_config.py"], "text"),
    ("http client retry count", ["docs/code/RetryingHttpClient.java"], "text"),
    ("华东区 季度销售", ["docs/reports/quarterly_sales_report.docx"], "text"),
    ("路由器恢复出厂设置", ["docs/manuals/router_manual.docx"], "text"),
    ("router_man", ["docs/manuals/router_manual.docx"], "text"),
    ("borrow of moved value", ["images/屏幕截图_20250812_101500_terminal.png"], "visual"),
    ("会议室 项目评审会", ["images/屏幕截图_20250903_150210_chat.png"], "visual"),
    ("去上海虹桥的火车票", ["images/屏幕截图_20251015_083000_ticket.png"], "visual"),
    ("机械键盘 快递单号", ["images/屏幕截图_20260518_163020_order.png"], "visual"),
    ("mount permission denied", ["images/屏幕截图_20260310_094512_error.png"], "visual"),
    ("杭州天气预报", ["images/屏幕截图_20251208_071530_weather.png"], "visual"),
    ("sunset", ["images/sunset.jpg"], "visual"),
    ("a red apple", ["images/red_apple.png"], "visual"),
    ("snowy mountains", ["images/snow_mountain.jpg"], "visual"),
    ("city lights at night", ["images/night_city.jpg"], "visual"),
]


def font(size, face=SANS):
    return ImageFont.truetype(CJK, size, index=face)


def screenshot(path, style, title, lines):
    if style == "wallpaper":
        img = gradient((W, H), (40, 60, 120), (150, 80, 160))
        draw = ImageDraw.Draw(img)
        for i in range(12):
            x, y, r = 200 + i * 190, 700 + int(250 * math.sin(i)), 60 + 10 * i
            draw.ellipse((x - r, y - r, x + r, y + r), outline=(255, 255, 255), width=3)
        draw.rectangle((0, H - 60, W, H), fill=(25, 25, 30))
        img.save(path)
        return
    dark = style == "dark"
    bg, fg = ((30, 31, 34), (220, 220, 220)) if dark else ((246, 246, 248), (30, 30, 30))
    img = Image.new("RGB", (W, H), (60, 70, 90))
    draw = ImageDraw.Draw(img)
    draw.rectangle((0, H - 60, W, H), fill=(25, 25, 30))  # panel
    draw.text((40, H - 50), "应用程序  文件  浏览器  终端", font=font(28), fill=(200, 200, 200))
    win = (240, 120, W - 240, H - 160)
    draw.rectangle(win, fill=bg)
    draw.rectangle((win[0], win[1], win[2], win[1] + 64), fill=(210, 212, 216) if not dark else (45, 46, 50))
    draw.text((win[0] + 30, win[1] + 12), title, font=font(30), fill=fg)
    face = MONO if dark else SANS
    y = win[1] + 120
    for line in lines:
        draw.text((win[0] + 60, y), line, font=font(44, face), fill=fg)
        y += 80
    img.save(path)


def gradient(size, top, bottom):
    w, h = size
    img = Image.new("RGB", size)
    draw = ImageDraw.Draw(img)
    for y in range(h):
        t = y / (h - 1)
        draw.line([(0, y), (w, y)], fill=tuple(int(a + (b - a) * t) for a, b in zip(top, bottom)))
    return img


def photo(path, name, rng):
    size = (1600, 1200)
    if name.startswith("sunset"):
        img = gradient(size, (70, 40, 120), (250, 140, 60))
        d = ImageDraw.Draw(img)
        d.ellipse((650, 650, 950, 950), fill=(255, 200, 80))
        d.rectangle((0, 820, 1600, 1200), fill=(40, 50, 90))
        for y in range(840, 1200, 30):
            d.line([(rng.randint(0, 400), y), (rng.randint(1200, 1600), y)], fill=(250, 160, 80), width=3)
    elif name.startswith("red_apple"):
        img = Image.new("RGB", size, (245, 245, 240))
        d = ImageDraw.Draw(img)
        d.ellipse((520, 380, 1080, 940), fill=(200, 20, 30))
        d.ellipse((620, 450, 760, 560), fill=(235, 90, 90))
        d.line([(800, 400), (820, 260)], fill=(90, 60, 30), width=22)
        d.ellipse((820, 250, 1000, 340), fill=(60, 150, 50))
    elif name.startswith("snow_mountain"):
        img = gradient(size, (80, 140, 220), (200, 225, 250))
        d = ImageDraw.Draw(img)
        for x0, peak, base in [(-200, 300, 1000), (400, 220, 1100), (900, 360, 1000)]:
            d.polygon([(x0, 1200), (x0 + 450, peak), (x0 + 900, 1200)], fill=(110, 110, 125))
            d.polygon([(x0 + 330, peak + 140), (x0 + 450, peak), (x0 + 570, peak + 140)], fill=(250, 250, 255))
        d.rectangle((0, 1050, 1600, 1200), fill=(240, 245, 250))
    elif name.startswith("night_city"):
        img = gradient(size, (5, 10, 40), (30, 30, 70))
        d = ImageDraw.Draw(img)
        x = 0
        while x < 1600:
            bw, bh = rng.randint(80, 200), rng.randint(300, 900)
            d.rectangle((x, 1200 - bh, x + bw, 1200), fill=(20, 20, 30))
            for wy in range(1200 - bh + 20, 1180, 40):
                for wx in range(x + 10, x + bw - 20, 30):
                    if rng.random() < 0.6:
                        d.rectangle((wx, wy, wx + 14, wy + 20), fill=(255, 220, 120))
            x += bw + 10
    elif name.startswith("forest"):
        img = gradient(size, (150, 200, 230), (90, 140, 80))
        d = ImageDraw.Draw(img)
        for _ in range(60):
            x, y, s = rng.randint(0, 1600), rng.randint(500, 1200), rng.randint(80, 200)
            d.polygon([(x - s // 2, y), (x, y - 2 * s), (x + s // 2, y)], fill=(20, rng.randint(80, 130), 40))
            d.rectangle((x - 10, y, x + 10, y + 40), fill=(80, 50, 20))
    else:  # ocean
        img = gradient(size, (120, 190, 240), (10, 60, 130))
        d = ImageDraw.Draw(img)
        for y in range(500, 1200, 40):
            for x in range(0, 1600, 120):
                d.arc((x, y, x + 120, y + 40), 200, 340, fill=(230, 240, 255), width=4)
    img.filter(ImageFilter.GaussianBlur(1.5)).save(path)


def docx(path, paragraphs):
    body = "".join(f"<w:p><w:r><w:t>{escape(p)}</w:t></w:r></w:p>" for p in paragraphs)
    document = (
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
        '<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">'
        f"<w:body>{body}</w:body></w:document>"
    )
    content_types = (
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
        '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
        '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>'
        '<Default Extension="xml" ContentType="application/xml"/>'
        '<Override PartName="/word/document.xml" '
        'ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>'
        "</Types>"
    )
    rels = (
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
        '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
        '<Relationship Id="rId1" '
        'Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" '
        'Target="word/document.xml"/></Relationships>'
    )
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as z:
        z.writestr("[Content_Types].xml", content_types)
        z.writestr("_rels/.rels", rels)
        z.writestr("word/document.xml", document)


def main():
    out = Path(sys.argv[1]).resolve()
    rng = random.Random(7)
    images, docs = out / "images", out / "docs"
    images.mkdir(parents=True, exist_ok=True)
    image_paths, text_paths = [], []
    for name, (style, title, lines) in SCREENSHOTS.items():
        screenshot(images / name, style, title, lines)
        image_paths.append(images / name)
    for name in PHOTOS:
        photo(images / name, name, rng)
        image_paths.append(images / name)

    long_md = "# 长文档\n\n## 第一章 很长的章节\n\n" + LONG_SECTION * 40 + "\n\n## 第二章 短章节\n\n这里很短。\n"
    files = {**MARKDOWN, "long/long_handbook.md": long_md, **TEXTS}
    for rel, text in files.items():
        path = docs / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")
        text_paths.append(path)
    for rel, paragraphs in DOCX.items():
        path = docs / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        docx(path, paragraphs)
        text_paths.append(path)

    (out / "images.txt").write_text("".join(f"{p}\n" for p in image_paths), encoding="utf-8")
    (out / "texts.txt").write_text("".join(f"{p}\n" for p in text_paths), encoding="utf-8")
    with open(out / "queries.toml", "w", encoding="utf-8") as f:
        for text, expect, kind in QUERIES:
            paths = ", ".join(f'"{out / e}"' for e in expect)
            f.write(f'[[query]]\ntext = "{text}"\nexpect = [{paths}]\nkind = "{kind}"\n\n')
    print(f"{len(image_paths)} images, {len(text_paths)} documents, {len(QUERIES)} queries in {out}")


if __name__ == "__main__":
    main()
