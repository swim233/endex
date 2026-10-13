# Endex

本地检索文档和图片（包括截图里的文字），中英文都能搜。适用于 KDE Plasma 6（Wayland）。

## 依赖

- Rust ≥ 1.95、`just`、`curl`、CMake、Ninja
- Qt ≥ 6.8、LayerShellQt、KF6（Kirigami、KIO、KWindowSystem）、qqc2-desktop-style

  Arch：

  ```sh
  sudo pacman -S --needed rust just curl cmake ninja qt6-base qt6-declarative qt6-wayland \
      layer-shell-qt kirigami kio kwindowsystem qqc2-desktop-style
  ```

- 嵌入模型 EmbeddingGemma 2：LiteRT-LM 格式的 `.litertlm` 文件，以及同一模型的 `tokenizer.json`
- LiteRT-LM 运行库：用 `litert-lm` Python 包里带的 `liblitert-lm.so`

## 安装

```sh
# 1. LiteRT-LM 运行库（Endex 会自动在这个位置找它）
python -m venv ~/.venvs/litert-lm
~/.venvs/litert-lm/bin/pip install litert-lm

# 2. 把模型放到默认位置。放在别处的话，第一次运行后改 ~/.config/endex/config.toml 里的路径
#    ~/models/embeddinggemma-2-litert/embeddinggemma-2-740m.litertlm
#    ~/models/embeddinggemma-2/tokenizer.json

# 3. 下载 OCR 模型，编译并安装到 ~/.local
just models
just install
systemctl --user enable --now endexd   # 可选：登录时启动
```

卸载：`just uninstall`。配置和索引会保留。

## 快捷键

Endex 不会改 KDE 的快捷键设置，需要手动绑定一次：

1. 打开系统设置 → 键盘 → 快捷键 → 添加新项 → 命令或脚本。
2. 命令填完整路径，用 `echo "$HOME/.local/bin/endex toggle"` 打印出来的那一行。
3. 设一个快捷键，例如 Meta+Space。

## 开始使用

1. 按快捷键唤出浮窗。
2. 第一次使用时，点「打开设置 › 索引目录」，添加要索引的文件夹，点「应用」。
3. 设置窗口也可以从应用菜单里的「Endex 设置」打开。

更多说明在 [docs/guide.md](docs/guide.md)，包括命令行、设置项、外观、theme.toml 和 D-Bus 接口。
