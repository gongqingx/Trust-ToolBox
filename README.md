# Trust 工具箱 (Trust-ToolBox)

一个使用 Rust 编写的 Windows 硬件检测工具箱启动器。纯 Win32 + GDI 绘制界面，无运行时依赖，单个 exe 即可运行，体积小、启动快。

它把常用的第三方硬件检测、拷机与维护工具按类别整理在一起，并显示本机硬件概览，一键启动对应工具。

## ✨ 功能

- 🖥️ **硬件概览**：启动时自动读取 CPU、内存、显卡、硬盘等信息
- 🗂️ **分类管理**：综合检测 / 处理器 / 显卡 / 内存 / 硬盘 / 显示器 / 外设 / 其他工具
- 🚀 **一键启动**：直接运行 `tools/` 目录下的第三方工具
- ⌨️ **键盘操作**：支持方向键、Tab、Enter、Esc 等快捷导航
- 📐 **高 DPI 适配**：支持多显示器与缩放变化
- 🪶 **轻量**：原生 Win32 实现，release 构建开启 LTO 与体积优化

## 📊 性能数据

| 指标 | 数值 |
|---|---|
| 程序体积 | **795 KB**（单个 exe，无需安装运行库） |
| 启动时间（启动 → 主窗口出现） | **约 60 ms**（热启动；冷启动约 130 ms） |
| 空闲内存占用 | 工作集 **约 1 MB**，私有内存 **约 5 MB** |

> 测试环境：Windows x64，release 构建，连续 5 次取值。实际数据因硬件不同会有差异。

## 💻 系统要求

| 系统 | 支持情况 |
|---|---|
| Windows 11 x64 | ✅ 支持 |
| Windows 10 x64（1703 及以上） | ✅ 支持 |
| Windows 10 1607 及更早 / Windows 7 / 8 / 8.1 | ❌ 不支持 |
| 32 位 Windows | ❌ 不支持 |
| Windows 11 ARM64 | ⚠️ 可通过 x64 仿真运行启动器，部分底层驱动类工具不可用 |

> 第三方工具的兼容性取决于各工具本身。部分使用底层驱动（如 WinRing0）的工具在开启"内核隔离 / 内存完整性"时可能被拦截，部分工具需要管理员权限。

## 📦 下载使用

前往 [Releases](https://github.com/gongqingx/Trust-ToolBox/releases) 下载完整包（启动器 + 全部第三方工具），解压后运行 exe 即可。

> 部分硬件工具需要管理员权限；少数底层驱动类工具可能被 Windows Defender 拦截，详见 [tools/DISCLAIMER.md](tools/DISCLAIMER.md)。

## 🔧 从源码构建

本仓库只包含启动器源码，不含第三方工具（`tools/` 目录请从 Release 包中获取）。

环境要求：

- Windows 10/11 x64
- [Rust](https://www.rust-lang.org/) 稳定版

```bash
git clone https://github.com/gongqingx/Trust-ToolBox.git
cd Trust-ToolBox
cargo build --release
```

产物位于 `target/release/trust-toolbox.exe`，将其与 `tools/`、`static_catalog.json` 等文件放在同一目录下运行。

> `.cargo/config.toml` 中配置了 `x86_64-pc-windows-gnu` 目标的本地 llvm-mingw 链接器路径。使用默认 MSVC 工具链构建时不受影响；如需使用 GNU 目标，请改成你自己的路径。

## 📁 目录结构

```
├── src/                  启动器源码
│   ├── main.rs           窗口与消息循环
│   ├── ui_render.rs      界面绘制
│   ├── gdi.rs            GDI 绘图封装
│   ├── hwinfo.rs         硬件信息读取
│   ├── catalog.rs        工具目录加载
│   ├── launcher.rs       工具启动
│   └── ...
├── static_catalog.json   工具清单
├── tui_config.json       界面配置
├── hwinfo.ini            硬件信息配置
└── tools/                第三方工具（仅在 Release 包中提供）
```

## ⚠️ 免责声明

`tools/` 中的所有第三方软件版权归其原作者所有，仅供个人学习与硬件检测使用。使用这些工具造成的任何后果由使用者自行承担。详见 [tools/DISCLAIMER.md](tools/DISCLAIMER.md)。

## 📄 许可证

本项目源码基于 [GPL-3.0](LICENSE) 许可证开源。该许可证**仅适用于本项目源码**，第三方工具遵循各自原有的许可证。
