use crate::hwinfo::decode_text_auto;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub const STATIC_CATALOG_FILE_NAME: &str = "static_catalog.json";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FileEntry {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "IsDir")]
    pub is_dir: bool,
    #[serde(rename = "Size")]
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LaunchVariant {
    #[serde(rename = "Label")]
    pub label: String,
    #[serde(rename = "Path")]
    pub path: String,
    #[serde(rename = "Kind")]
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Tool {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    pub category_id: String,
    pub category_name: String,
    #[serde(default)]
    pub tag: String,
    #[serde(default)]
    pub description: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub warning: String,
    #[serde(default)]
    pub kind: String,
    #[serde(skip)]
    pub dir: PathBuf,
    #[serde(default)]
    pub rel_dir: String,
    #[serde(skip)]
    pub entry_path: PathBuf,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub rel_entry_path: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variants: Vec<LaunchVariant>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub password_note: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<FileEntry>,
    #[serde(default)]
    pub total_bytes: u64,
    #[serde(default)]
    pub file_count: usize,
}

impl Tool {
    pub fn match_query(&self, q: &str) -> bool {
        let q = q.trim().to_lowercase();
        if q.is_empty() {
            return true;
        }
        let entry_name = self
            .entry_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();

        if self.name.to_lowercase().contains(&q)
            || self.tag.to_lowercase().contains(&q)
            || self.description.to_lowercase().contains(&q)
            || self.category_name.to_lowercase().contains(&q)
            || entry_name.contains(&q)
        {
            return true;
        }
        self.aliases.iter().any(|a| a.to_lowercase().contains(&q))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Category {
    #[serde(rename = "ID")]
    pub id: String,
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Folder")]
    pub folder: String,
    #[serde(rename = "Tools")]
    pub tools: Vec<Tool>,
}

#[derive(Debug, Clone, Default)]
pub struct Catalog {
    #[allow(dead_code)]
    pub root: PathBuf,
    pub categories: Vec<Category>,
    pub all_tool_indices: Vec<(usize, usize)>,
    pub by_id: HashMap<String, (usize, usize)>,
}

impl Catalog {
    pub fn total_tools(&self) -> usize {
        self.all_tool_indices.len()
    }

    pub fn get(&self, id: &str) -> Option<&Tool> {
        let &(ci, ti) = self.by_id.get(id)?;
        self.get_by_idx((ci, ti))
    }

    pub fn get_by_idx(&self, idx: (usize, usize)) -> Option<&Tool> {
        self.categories.get(idx.0)?.tools.get(idx.1)
    }

    #[cfg(test)]
    pub fn iter_tools(&self) -> impl Iterator<Item = &Tool> {
        self.all_tool_indices
            .iter()
            .filter_map(|&idx| self.get_by_idx(idx))
    }

    fn rebuild_indices(&mut self) {
        self.all_tool_indices.clear();
        self.by_id.clear();
        for (ci, cat) in self.categories.iter().enumerate() {
            for (ti, tool) in cat.tools.iter().enumerate() {
                self.all_tool_indices.push((ci, ti));
                self.by_id.insert(tool.id.clone(), (ci, ti));
            }
        }
    }
}

#[derive(Serialize, Deserialize)]
struct StaticCatalogEnvelope {
    categories: Vec<Category>,
}

struct ToolSpec {
    id: &'static str,
    name: &'static str,
    aliases: &'static [&'static str],
    folder: &'static str,
    entry: &'static str,
    kind: &'static str,
    tag: &'static str,
    description: &'static str,
    warning: &'static str,
    is_tui: bool,
}

struct CategoryDef {
    id: &'static str,
    name: &'static str,
    folder: &'static str,
    specs: &'static [ToolSpec],
}

fn is_skipped_stub_folder(name: &str) -> bool {
    matches!(
        name.to_lowercase().as_str(),
        "msiafterburnersetup" | "游戏加加" | "iva" | "检查更新"
    )
}

const CATEGORY_DEFS: &[CategoryDef] = &[
    CategoryDef {
        id: "overview",
        name: "综合检测",
        folder: "综合检测",
        specs: &[
            ToolSpec {
                id: "tulading_hw",
                name: "图拉丁硬件检测",
                aliases: &["图拉丁硬件检测", "硬件检测", "图吧检测"],
                folder: "综合检测/图拉丁硬件检测",
                entry: "图拉丁硬件检测.exe",
                kind: "exe",
                tag: "本土检测",
                description: "图吧工具箱内置硬件快速识别工具，可读取 CPU、芯片组、内存 SPD、显卡、硬盘与显示器 EDID。",
                warning: "内含 WinRing0 底层硬件访问驱动，部分系统安全策略可能拦截，仅用于本地硬件检测。",
                is_tui: false,
            },
            ToolSpec {
                id: "rweverything",
                name: "RWEverything",
                aliases: &["rw", "rweverything", "pci", "ec", "acpi"],
                folder: "综合检测/RWEverything",
                entry: "Rw.exe",
                kind: "exe",
                tag: "底层调试",
                description: "读取与调试主板 PCI/PCIe、内存 SPD、ACPI 表、EC 寄存器与 IO 端口的硬核工程师工具。",
                warning: "底层 Ring0 寄存器读写工具，请勿随意修改未知寄存器数值，以免导致死机或硬件异常。",
                is_tui: false,
            },
        ],
    },
    CategoryDef {
        id: "cpu",
        name: "CPU工具",
        folder: "处理器工具",
        specs: &[
            ToolSpec {
                id: "prime95_bat",
                name: "Prime95 一键单烤",
                aliases: &["prime95", "p95", "一键烤鸡", "单烤cpu"],
                folder: "处理器工具/Prime95",
                entry: "start.bat",
                kind: "bat",
                tag: "一键烤机",
                description: "通过批处理一键启动 Prime95 极限负载测试 CPU 稳定性与散热能力，支持按任意键一键结束烤机。",
                warning: "极限负载烤机（负载高于 AIDA64 单烤 FPU），请务必同时开启 CoreTemp 或 HWiNFO 监控温度。",
                is_tui: false,
            },
            ToolSpec {
                id: "prime95_gui",
                name: "Prime95 完整版",
                aliases: &["prime95", "p95", "small fft", "blend"],
                folder: "处理器工具/Prime95",
                entry: "prime95x64.exe",
                kind: "exe",
                tag: "极限压力",
                description: "原生 Prime95 图形界面版，可自选 Small FFTs（烤 CPU）或 Blend（烤 CPU+内存）。",
                warning: "高发热压力测试工具，运行期间请注意 CPU 核心温度与供电散热。",
                is_tui: false,
            },
            ToolSpec {
                id: "linx",
                name: "LinX 浮点烤机",
                aliases: &["linx", "linpack", "浮点"],
                folder: "处理器工具/LinX",
                entry: "LinX.exe",
                kind: "exe",
                tag: "浮点压测",
                description: "基于 Intel Linpack 的高强度 CPU 浮点运算与超频稳定性验证工具，支持测算 GFlops 性能。",
                warning: "瞬时功耗极高，超频验证时建议分档加压并盯紧温度。",
                is_tui: false,
            },
            ToolSpec {
                id: "superpi",
                name: "SuperPI 单核测试",
                aliases: &["superpi", "pi", "圆周率", "单核"],
                folder: "处理器工具/superpi",
                entry: "Superpi.exe",
                kind: "exe",
                tag: "单核跑分",
                description: "经典的单线程圆周率计算跑分工具（常用 1M / 32M），用于评估 CPU 单核效能与内存延迟。",
                warning: "",
                is_tui: false,
            },
            ToolSpec {
                id: "wprime",
                name: "wPrime 多核测试",
                aliases: &["wprime", "多核跑分"],
                folder: "处理器工具/wPrime",
                entry: "wPrime.exe",
                kind: "exe",
                tag: "多核跑分",
                description: "多线程平方根计算基准测试工具（常用 32M / 1024M），用于测试多核心协同计算能力。",
                warning: "",
                is_tui: false,
            },
            ToolSpec {
                id: "xiangqi",
                name: "国际象棋基准 (Fritz)",
                aliases: &["xiangqi", "国际象棋", "fritz", "chess"],
                folder: "处理器工具/XIANGQI",
                entry: "xiangqi.exe",
                kind: "exe",
                tag: "经典跑分",
                description: "Fritz Chess Benchmark 国际象棋跑分，测试处理器整数AI计算吞吐量与多线程倍率。",
                warning: "",
                is_tui: false,
            },
            ToolSpec {
                id: "throttlestop",
                name: "ThrottleStop 降压调校",
                aliases: &["throttlestop", "降压", "功耗墙", "解锁"],
                folder: "处理器工具/ThrottleStop",
                entry: "ThrottleStop.exe",
                kind: "exe",
                tag: "电压/功耗",
                description: "针对 Intel 笔记本/桌面处理器的频率监控、降压降温、功耗墙解锁与 BD PROCHOT 调节利器。",
                warning: "调压或解锁功耗墙存在蓝屏风险，建议小步微调并先保存默认配置。",
                is_tui: false,
            },
            ToolSpec {
                id: "thread_bomb",
                name: "线程炸弹 (加密压缩包)",
                aliases: &["线程炸弹", "多线程"],
                folder: "处理器工具/线程炸弹",
                entry: "线程炸弹.zip",
                kind: "archive",
                tag: "极限压测",
                description: "多线程调度极限压测小工具，用于检验高线程并发下的系统响应。",
                warning: "加密压缩包（解压密码通常为 tulading），运行后可能造成系统短时卡顿，详见目录声明。",
                is_tui: false,
            },
        ],
    },
    CategoryDef {
        id: "mobo",
        name: "主板工具",
        folder: "主板工具",
        specs: &[],
    },
    CategoryDef {
        id: "ram",
        name: "内存工具",
        folder: "内存工具",
        specs: &[
            ToolSpec {
                id: "tm5",
                name: "TestMem5 (TM5)",
                aliases: &["tm5", "testmem5", "内存烤机", "超频测试"],
                folder: "内存工具/tm5",
                entry: "TM5.exe",
                kind: "exe",
                tag: "超频烤机",
                description: "内存超频玩家最常用的稳定性拷机软件，可快速抓出时序过紧或电压不足导致的错误。",
                warning: "",
                is_tui: false,
            },
            ToolSpec {
                id: "memtest64",
                name: "MemTest64",
                aliases: &["memtest64", "techpowerup"],
                folder: "内存工具/memtest64",
                entry: "MemTest64.exe",
                kind: "exe",
                tag: "64位检测",
                description: "TechPowerUp 出品的 64 位桌面内存稳定性测试工具，支持在 Windows 下大范围分配内存测试。",
                warning: "",
                is_tui: false,
            },
            ToolSpec {
                id: "memtest",
                name: "MemTest 经典版",
                aliases: &["memtest", "内存检测"],
                folder: "内存工具/memtest",
                entry: "memtest.exe",
                kind: "exe",
                tag: "经典检测",
                description: "轻量级 Windows 内存坏块与稳定性扫描工具，适合多开并发测试全容量。",
                warning: "",
                is_tui: false,
            },
            ToolSpec {
                id: "mem_clean",
                name: "内存整理 (加密压缩包)",
                aliases: &["内存整理", "sunlight", "清理内存"],
                folder: "内存工具/内存整理",
                entry: "内存整理.zip",
                kind: "archive",
                tag: "加密压缩",
                description: "轻量内存工作集清理释放工具（加密打包存储，密码见右侧密码说明）。",
                warning: "压缩包已加密（解压密码请查看右侧密码提示与目录声明）。",
                is_tui: false,
            },
        ],
    },
    CategoryDef {
        id: "gpu",
        name: "显卡工具",
        folder: "显卡工具",
        specs: &[
            ToolSpec {
                id: "gpuinfo",
                name: "GPUInfo (核心查看)",
                aliases: &["gpuinfo", "显卡信息"],
                folder: "显卡工具/gpuinfo",
                entry: "Gpuinfo.exe",
                kind: "exe",
                tag: "快速识别",
                description: "国内开发者制作的小巧显卡参数快速检测工具，直观展示显卡基础规格。",
                warning: "",
                is_tui: false,
            },
            ToolSpec {
                id: "ddu",
                name: "DDU 显卡驱动彻底卸载",
                aliases: &["ddu", "display driver uninstaller", "驱动卸载"],
                folder: "显卡工具/DDU v18.0.1.9",
                entry: "Display Driver Uninstaller.exe",
                kind: "exe",
                tag: "驱动清理",
                description: "彻底清除 NVIDIA / AMD / Intel 显卡驱动残留与注册表项，换卡或驱动异常必用（建议安全模式下使用）。",
                warning: "卸载驱动后屏幕将暂时回到基础分辨率，建议提前准备好目标驱动安装包。",
                is_tui: false,
            },
            ToolSpec {
                id: "gputest",
                name: "GpuTest 压力测试",
                aliases: &["gputest", "opengl", "烤机"],
                folder: "显卡工具/GpuTest_Windows x64",
                entry: "GpuTest_GUI.exe",
                kind: "exe",
                tag: "OpenGL压测",
                description: "集成 FurMark、TessMark、GiMark 等多种 OpenGL 场景的 64 位显卡基准测试与压力工具。",
                warning: "",
                is_tui: false,
            },
            ToolSpec {
                id: "nvinspector",
                name: "NVIDIA Inspector",
                aliases: &["nvidia", "inspector", "n卡", "超频", "驱动配置"],
                folder: "显卡工具/nvidiaInspector",
                entry: "nvidiaInspector.exe",
                kind: "exe",
                tag: "N卡调校",
                description: "NVIDIA 显卡高级状态查看、多档位帧率限制与驱动隐藏参数调校工具。",
                warning: "",
                is_tui: false,
            },
            ToolSpec {
                id: "amdclocktool",
                name: "AMD GPU Clock Tool",
                aliases: &["amd", "clock", "a卡"],
                folder: "显卡工具/AMDGPUClockTool",
                entry: "AMDGPUClockTool.exe",
                kind: "exe",
                tag: "A卡调校",
                description: "针对经典 AMD/ATI 显卡的频率查看与时钟控制辅助工具。",
                warning: "",
                is_tui: false,
            },
        ],
    },
    CategoryDef {
        id: "disk",
        name: "硬盘工具",
        folder: "硬盘工具",
        specs: &[
            ToolSpec {
                id: "crystaldiskinfo",
                name: "CrystalDiskInfo",
                aliases: &["cdi", "crystaldiskinfo", "smart", "通电时间", "健康度"],
                folder: "硬盘工具/CrystalDiskInfo",
                entry: "DiskInfo64.exe",
                kind: "exe",
                tag: "SMART健康",
                description: "查看硬盘 S.M.A.R.T. 健康状态、通电次数、累计通电时间、读写量、实时温度与 NVMe 接口协议。",
                warning: "",
                is_tui: false,
            },
            ToolSpec {
                id: "crystaldiskmark",
                name: "CrystalDiskMark",
                aliases: &["cdm", "crystaldiskmark", "测速", "4k", "跑分"],
                folder: "硬盘工具/CrystalDiskMark",
                entry: "DiskMarkx64.exe",
                kind: "exe",
                tag: "读写测速",
                description: "最常用的固态/机械硬盘顺序读写与 4K 随机读写性能基准跑分工具。",
                warning: "",
                is_tui: false,
            },
            ToolSpec {
                id: "asssd",
                name: "AS SSD Benchmark",
                aliases: &["asssd", "as ssd", "4k对齐", "ssd测速"],
                folder: "硬盘工具/ASSSDBenchmark",
                entry: "ASSSDBenchmark.exe",
                kind: "exe",
                tag: "4K对齐/测速",
                description: "专为 SSD 设计的测速与 4K 对齐检测工具，一眼核对分区是否 4K-OK 及驱动是否启用。",
                warning: "",
                is_tui: false,
            },
            ToolSpec {
                id: "atto",
                name: "ATTO Disk Benchmark",
                aliases: &["atto", "不同文件大小测速"],
                folder: "硬盘工具/ATTODISKBENCHMARK",
                entry: "ATTO 磁盘基准测试.exe",
                kind: "exe",
                tag: "多块测速",
                description: "按 512B 至 64MB 不同数据包大小分级测试硬盘读写性能，常用于验证标称峰值速度。",
                warning: "",
                is_tui: false,
            },
            ToolSpec {
                id: "h2testw",
                name: "H2testw (U盘扩容校验)",
                aliases: &["h2testw", "u盘校验", "扩容盘"],
                folder: "硬盘工具/H2testw",
                entry: "h2testw_1.4.exe",
                kind: "exe",
                tag: "扩容检测",
                description: "国际公认的 U 盘/存储卡实际容量写入校验工具，专门识破虚标扩容盘。",
                warning: "",
                is_tui: false,
            },
            ToolSpec {
                id: "mydisktest",
                name: "MyDiskTest 扩容检测",
                aliases: &["mydisktest", "扩容", "u盘"],
                folder: "硬盘工具/mydisktest",
                entry: "MyDiskTest_v298.exe",
                kind: "exe",
                tag: "快速扩容测",
                description: "U 盘/TF 卡快速扩容检测与坏块扫描工具，测试速度比 H2testw 更快。",
                warning: "",
                is_tui: false,
            },
            ToolSpec {
                id: "urwtest",
                name: "URWTest 全盘校验",
                aliases: &["urwtest", "全盘读写"],
                folder: "硬盘工具/URWTEST",
                entry: "urwtest_v18.exe",
                kind: "exe",
                tag: "读写校验",
                description: "全盘写入并回读校验工具，验证 SSD/U 盘缓存外真实性能与数据一致性。",
                warning: "涉及磁盘写入操作，请务必先确认目标盘符无重要数据或选择空闲测试。",
                is_tui: false,
            },
            ToolSpec {
                id: "spacesniffer",
                name: "SpaceSniffer 空间透视",
                aliases: &["spacesniffer", "磁盘空间", "大文件"],
                folder: "硬盘工具/SpaceSniffer",
                entry: "SpaceSniffer.exe",
                kind: "exe",
                tag: "空间分析",
                description: "以直观树状方块实时可视化硬盘各目录占用比例，快速揪出 C 盘爆满元凶。",
                warning: "",
                is_tui: false,
            },
            ToolSpec {
                id: "windirstat",
                name: "WinDirStat 空间统计",
                aliases: &["windirstat", "磁盘分析"],
                folder: "硬盘工具/windirstat",
                entry: "windirstat.exe",
                kind: "exe",
                tag: "空间统计",
                description: "经典的开源磁盘目录与文件类型占用统计软件，配合扩展名色块图精准定位大文件。",
                warning: "",
                is_tui: false,
            },
            ToolSpec {
                id: "defraggler",
                name: "Defraggler 碎片整理",
                aliases: &["defraggler", "碎片整理"],
                folder: "硬盘工具/Defraggler",
                entry: "Defraggler.exe",
                kind: "exe",
                tag: "机械整理",
                description: "机械硬盘（HDD）文件与空闲空间碎片整理工具（固态硬盘无需整理碎片）。",
                warning: "",
                is_tui: false,
            },
            ToolSpec {
                id: "ssdz",
                name: "SSD-Z",
                aliases: &["ssdz", "ssd-z", "主控"],
                folder: "硬盘工具/SSDZ",
                entry: "SSDZ.exe",
                kind: "exe",
                tag: "主控/颗粒",
                description: "类似 CPU-Z 风格的固态硬盘信息工具，用于查询主控、闪存制程与 TRIM 状态。",
                warning: "",
                is_tui: false,
            },
        ],
    },
    CategoryDef {
        id: "screen",
        name: "屏幕工具",
        folder: "显示器工具",
        specs: &[
            ToolSpec {
                id: "tui_screen_test",
                name: "内置纯色坏点检测 (TUI)",
                aliases: &["坏点", "纯色", "屏幕检测", "漏光", "tui"],
                folder: "",
                entry: "",
                kind: "tui",
                tag: "内置工具",
                description: "原生全屏纯色坏点与色阶渐变检测器，支持 红/绿/蓝/白/黑/灰阶/棋盘格/24-bit 平滑渐变全屏切换。",
                warning: "",
                is_tui: true,
            },
            ToolSpec {
                id: "monitorinfo",
                name: "色域与面板检测",
                aliases: &["monitorinfo", "色域", "edid", "面板"],
                folder: "显示器工具/色域检测",
                entry: "monitorinfo.exe",
                kind: "exe",
                tag: "色域/面板",
                description: "读取显示器 EDID 原生色域坐标（sRGB / NTSC / DCI-P3 覆盖计算）、出厂日期与面板型号。",
                warning: "",
                is_tui: false,
            },
            ToolSpec {
                id: "online_screen_test",
                name: "在线屏幕综合测试",
                aliases: &["在线屏幕测试", "屏幕测试", "响应时间"],
                folder: "显示器工具/在线屏幕测试",
                entry: "在线屏幕测试.bat",
                kind: "bat",
                tag: "网页测试",
                description: "调用浏览器打开在线屏幕测试页面，检查坏点、对比度、灰度渐变与拖影。",
                warning: "",
                is_tui: false,
            },
        ],
    },
    CategoryDef {
        id: "periph",
        name: "外设工具",
        folder: "外设工具",
        specs: &[
            ToolSpec {
                id: "keyboard_test",
                name: "Keyboard Test Utility",
                aliases: &["keyboard", "键盘测试", "连键", "无冲"],
                folder: "外设工具/Keyboard Test Utility",
                entry: "Keyboard Test Utility.exe",
                kind: "exe",
                tag: "键盘/无冲",
                description: "机械/薄膜键盘全键触发与多键无冲测试工具，快速排查失灵按键或连击轴体。",
                warning: "",
                is_tui: false,
            },
            ToolSpec {
                id: "double_click_test",
                name: "鼠标单击变双击测试器",
                aliases: &["双击", "微动", "鼠标测试"],
                folder: "外设工具/鼠标单机变双击测试器",
                entry: "鼠标单击变双击测试器V2.0.exe",
                kind: "exe",
                tag: "微动排查",
                description: "专门检测鼠标左右中键微动老化导致的连击/双击故障，统计极短间隔误触次数。",
                warning: "",
                is_tui: false,
            },
            ToolSpec {
                id: "mouserate",
                name: "MouseRate 鼠标回报率测试",
                aliases: &["mouserate", "回报率", "轮询率", "hz"],
                folder: "外设工具/MOUSERATE",
                entry: "MOUSERATE.EXE",
                kind: "exe",
                tag: "回报率Hz",
                description: "实时测定游戏鼠标 USB 回报率（125Hz / 500Hz / 1000Hz / 4000Hz 等）。",
                warning: "",
                is_tui: false,
            },
            ToolSpec {
                id: "areson_mouse",
                name: "AresonMouseTest 轨迹/画线",
                aliases: &["areson", "画线", "蚊香图", "鼠标测试"],
                folder: "外设工具/AresonMouseTest",
                entry: "鼠标测试软件AresonMouseTestProgram.exe",
                kind: "exe",
                tag: "轨迹/按键",
                description: "综合鼠标按键、滚轮与移动平滑度测试工具，支持绘制画线轨迹排查丢帧跳标。",
                warning: "",
                is_tui: false,
            },
        ],
    },
    CategoryDef {
        id: "other",
        name: "其他工具",
        folder: "其他工具",
        specs: &[
            ToolSpec {
                id: "cpu_tier",
                name: "CPU 性能天梯图",
                aliases: &["cpu天梯图", "天梯图", "处理器排行"],
                folder: "其他工具/天梯图",
                entry: "CPU天梯图.jpg",
                kind: "img",
                tag: "天梯图",
                description: "桌面与移动端处理器综合性能排行天梯大图（右侧面板支持预览缩略图，回车调用看图器打开原图）。",
                warning: "",
                is_tui: false,
            },
            ToolSpec {
                id: "gpu_tier",
                name: "显卡性能天梯图",
                aliases: &["显卡天梯图", "天梯图", "gpu排行"],
                folder: "其他工具/天梯图",
                entry: "显卡天梯图.jpg",
                kind: "img",
                tag: "天梯图",
                description: "NVIDIA / AMD / Intel 显卡桌面与笔记本性能排行天梯大图（右侧面板实时缩略预览，回车看原图）。",
                warning: "",
                is_tui: false,
            },
            ToolSpec {
                id: "procexp",
                name: "Process Explorer",
                aliases: &["procexp", "进程", "sysinternals"],
                folder: "其他工具/procexp",
                entry: "procexp.exe",
                kind: "exe",
                tag: "增强进程",
                description: "微软 Sysinternals 出品的高级任务管理器，可查看进程树、句柄锁占用、DLL 与 GPU 占用。",
                warning: "",
                is_tui: false,
            },
            ToolSpec {
                id: "bluescreenview",
                name: "BlueScreenView 蓝屏分析",
                aliases: &["bluescreenview", "蓝屏", "bsod", "dump"],
                folder: "其他工具/bluescreenview",
                entry: "BlueScreenViewx64.exe",
                kind: "exe",
                tag: "蓝屏排错",
                description: "自动扫描 Minidump 蓝屏转储文件，直接定位引发蓝屏的故障驱动或系统模块。",
                warning: "",
                is_tui: false,
            },
            ToolSpec {
                id: "windbg",
                name: "WinDbg 调试器",
                aliases: &["windbg", "dump分析", "调试"],
                folder: "其他工具/WinDbg",
                entry: "windbg.exe",
                kind: "exe",
                tag: "内核调试",
                description: "微软官方内核/蓝屏 Dump 深度分析调试器。",
                warning: "",
                is_tui: false,
            },
            ToolSpec {
                id: "rufus",
                name: "Rufus 启动盘制作",
                aliases: &["rufus", "启动盘", "重装系统", "iso"],
                folder: "其他工具/rufus",
                entry: "rufus.exe",
                kind: "exe",
                tag: "U盘启动",
                description: "小巧快速的 Windows/Linux ISO 系统安装启动 U 盘制作工具（支持绕过 TPM/Win11 限制）。",
                warning: "",
                is_tui: false,
            },
            ToolSpec {
                id: "ventoy",
                name: "Ventoy 多系统启动盘",
                aliases: &["ventoy", "多系统", "iso直启"],
                folder: "其他工具/ventoy",
                entry: "Ventoy2Disk.exe",
                kind: "exe",
                tag: "多ISO启动",
                description: "制作后只需直接把多个 ISO/WIM/VHD 镜像拷入 U 盘即可菜单直启，装机维修利器。",
                warning: "",
                is_tui: false,
            },
            ToolSpec {
                id: "desktopok",
                name: "DesktopOK 图标布局保存",
                aliases: &["desktopok", "桌面图标"],
                folder: "其他工具/DesktopOK",
                entry: "DesktopOK.exe",
                kind: "exe",
                tag: "图标备份",
                description: "一键备份与恢复 Windows 桌面图标位置，防止改分辨率或装驱动后桌面图标乱序。",
                warning: "",
                is_tui: false,
            },
            ToolSpec {
                id: "gifcam",
                name: "GifCam 动图录制",
                aliases: &["gifcam", "gif", "录屏"],
                folder: "其他工具/gifcam",
                entry: "GifCam.exe",
                kind: "exe",
                tag: "GIF录屏",
                description: "极轻量的桌面区域 GIF 动图录制与逐帧编辑小工具。",
                warning: "",
                is_tui: false,
            },
            ToolSpec {
                id: "batteryinfoview",
                name: "BatteryInfoView 电池损耗",
                aliases: &["battery", "电池", "损耗", "笔记本"],
                folder: "其他工具/BatteryInfoView",
                entry: "BatteryInfoView.zip",
                kind: "archive",
                tag: "加密压缩",
                description: "查看笔记本电池设计容量、当前充满容量、损耗度、充放电功率与循环次数。",
                warning: "压缩包已加密（解压密码请查看右侧密码提示与目录声明）。",
                is_tui: false,
            },
            ToolSpec {
                id: "chipgenius",
                name: "ChipGenius 主控芯片检测",
                aliases: &["chipgenius", "u盘主控", "usb"],
                folder: "其他工具/ChipGenius",
                entry: "chipgenius.zip",
                kind: "archive",
                tag: "加密压缩",
                description: "精准识别 U 盘主控型号、闪存 Flash 颗粒厂商与 USB 设备 VID/PID。",
                warning: "压缩包已加密（解压密码请查看右侧密码提示与目录声明）。",
                is_tui: false,
            },
        ],
    },
];

fn from_slash(rel: &str) -> PathBuf {
    let mut pb = PathBuf::new();
    for part in rel.split(['/', '\\']) {
        if !part.is_empty() {
            pb.push(part);
        }
    }
    pb
}

fn to_slash(root: &Path, full: &Path) -> String {
    let rel = full.strip_prefix(root).unwrap_or(full);
    rel.components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// Loads from pre-baked `static_catalog.json` if valid, otherwise scans `root/tools` and persists it.
pub fn load(root: &Path) -> io::Result<Catalog> {
    if !tools_changed_since_static(root) {
        if let Ok(cat) = load_from_static_file(root) {
            if cat.total_tools() > 0 {
                return Ok(cat);
            }
        }
    }
    let cat = scan_and_build(root)?;
    let _ = save_static_file(root, &cat);
    Ok(cat)
}

/// Returns true if `tools/` or any category folder (including ones not in
/// `CATEGORY_DEFS`) was modified after `static_catalog.json` was written,
/// i.e. a tool folder was added, removed or renamed. Only directory mtimes
/// are read (about 10 metadata calls), so the check costs well under 1 ms.
fn tools_changed_since_static(root: &Path) -> bool {
    let Ok(cache_time) = fs::metadata(root.join(STATIC_CATALOG_FILE_NAME)).and_then(|m| m.modified())
    else {
        return true;
    };
    let tools_root = root.join("tools");
    let newer = |p: &Path| {
        fs::metadata(p)
            .and_then(|m| m.modified())
            .map(|t| t > cache_time)
            .unwrap_or(false)
    };
    if newer(&tools_root) {
        return true;
    }
    match fs::read_dir(&tools_root) {
        Ok(entries) => entries
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .any(|e| newer(&e.path())),
        Err(_) => false,
    }
}

pub fn load_from_static_file(root: &Path) -> io::Result<Catalog> {
    let path = root.join(STATIC_CATALOG_FILE_NAME);
    let data = fs::read(&path)?;
    let env: StaticCatalogEnvelope =
        serde_json::from_slice(&data).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    if env.categories.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "empty static catalog"));
    }

    let mut cat = Catalog {
        root: root.to_path_buf(),
        categories: env.categories,
        all_tool_indices: Vec::new(),
        by_id: HashMap::new(),
    };

    for c in &mut cat.categories {
        for t in &mut c.tools {
            if t.kind != "tui" {
                if !t.rel_dir.is_empty() {
                    t.dir = root.join(from_slash(&t.rel_dir));
                }
                if !t.rel_entry_path.is_empty() {
                    t.entry_path = root.join(from_slash(&t.rel_entry_path));
                } else if let Some(first) = t.variants.first() {
                    t.entry_path = t.dir.join(&first.label);
                }
                for v in &mut t.variants {
                    if !v.label.is_empty() && !t.dir.as_os_str().is_empty() {
                        v.path = t.dir.join(&v.label).to_string_lossy().into_owned();
                    }
                }
            }
        }
    }
    cat.rebuild_indices();
    Ok(cat)
}

pub fn save_static_file(root: &Path, cat: &Catalog) -> io::Result<()> {
    let mut categories = cat.categories.clone();
    for c in &mut categories {
        for t in &mut c.tools {
            if t.kind != "tui" && !t.entry_path.as_os_str().is_empty() {
                t.rel_entry_path = to_slash(root, &t.entry_path);
            }
            for v in &mut t.variants {
                if !v.path.is_empty() {
                    v.path = to_slash(root, Path::new(&v.path));
                }
            }
        }
    }
    let env = StaticCatalogEnvelope { categories };
    let mut data = serde_json::to_vec_pretty(&env)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    data.push(b'\n');

    let path = root.join(STATIC_CATALOG_FILE_NAME);
    let tmp = root.join(format!("{STATIC_CATALOG_FILE_NAME}.tmp"));
    fs::write(&tmp, &data)?;
    if let Err(e) = fs::rename(&tmp, &path) {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

pub fn scan_and_build(root: &Path) -> io::Result<Catalog> {
    let tools_root = root.join("tools");
    let is64 = std::env::consts::ARCH.contains("64");
    let mut covered_dirs: HashSet<String> = HashSet::new();
    let mut categories = Vec::new();

    for cdef in CATEGORY_DEFS {
        let mut cat_tools = Vec::new();

        for spec in cdef.specs {
            if spec.is_tui {
                cat_tools.push(Tool {
                    id: spec.id.to_string(),
                    name: spec.name.to_string(),
                    aliases: spec.aliases.iter().map(|s| (*s).to_string()).collect(),
                    category_id: cdef.id.to_string(),
                    category_name: cdef.name.to_string(),
                    tag: spec.tag.to_string(),
                    description: spec.description.to_string(),
                    warning: spec.warning.to_string(),
                    kind: "tui".to_string(),
                    rel_dir: "内置 TUI 模块".to_string(),
                    ..Default::default()
                });
                continue;
            }

            let dir = tools_root.join(from_slash(spec.folder));
            if !dir.is_dir() {
                continue;
            }
            covered_dirs.insert(dir.to_string_lossy().to_lowercase());

            if let Some(tool) = build_tool_from_dir(
                spec.id,
                spec.name,
                spec.aliases,
                spec.entry,
                spec.kind,
                spec.tag,
                spec.description,
                spec.warning,
                cdef.id,
                cdef.name,
                root,
                &dir,
                is64,
            ) {
                cat_tools.push(tool);
            }
        }

        let cat_dir = tools_root.join(cdef.folder);
        if let Ok(entries) = fs::read_dir(&cat_dir) {
            let mut subdirs: Vec<(String, PathBuf)> = entries
                .filter_map(|e| e.ok())
                .filter_map(|e| {
                    let ft = e.file_type().ok()?;
                    if !ft.is_dir() {
                        return None;
                    }
                    let name = e.file_name().to_string_lossy().into_owned();
                    if is_skipped_stub_folder(&name) {
                        return None;
                    }
                    Some((name, e.path()))
                })
                .collect();
            subdirs.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));

            for (name, sub_dir) in subdirs {
                let key = sub_dir.to_string_lossy().to_lowercase();
                if !covered_dirs.insert(key) {
                    continue;
                }
                let dyn_id = format!("dyn_{}_{}", cdef.id, sanitize_id(&name));
                let dyn_desc = format!("从目录 tools/{}/{} 自动发现的工具。", cdef.folder, name);
                let alias = [name.as_str()];
                if let Some(tool) = build_tool_from_dir(
                    &dyn_id,
                    &name,
                    &alias,
                    "",
                    "",
                    "本地工具",
                    &dyn_desc,
                    "",
                    cdef.id,
                    cdef.name,
                    root,
                    &sub_dir,
                    is64,
                ) {
                    cat_tools.push(tool);
                }
            }
        }

        if !cat_tools.is_empty() {
            categories.push(Category {
                id: cdef.id.to_string(),
                name: cdef.name.to_string(),
                folder: cdef.folder.to_string(),
                tools: cat_tools,
            });
        }
    }

    let mut cat = Catalog {
        root: root.to_path_buf(),
        categories,
        all_tool_indices: Vec::new(),
        by_id: HashMap::new(),
    };
    cat.rebuild_indices();
    Ok(cat)
}

fn sanitize_id(s: &str) -> String {
    let mut out = String::new();
    for ch in s.to_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
        } else if ch == ' ' || ch == '-' || ch == '_' {
            out.push('_');
        } else {
            out.push_str(&format!("{:x}", ch as u32));
        }
    }
    out
}

fn walk_dir_stats(dir: &Path) -> (u64, usize) {
    let mut total_bytes = 0u64;
    let mut file_count = 0usize;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(curr) = stack.pop() {
        let Ok(entries) = fs::read_dir(&curr) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(ft) = entry.file_type() else {
                continue;
            };
            if ft.is_dir() {
                stack.push(path);
            } else if ft.is_file() {
                file_count += 1;
                if let Ok(meta) = entry.metadata() {
                    total_bytes += meta.len();
                }
            }
        }
    }
    (total_bytes, file_count)
}

#[allow(clippy::too_many_arguments)]
fn build_tool_from_dir(
    id: &str,
    name: &str,
    aliases: &[&str],
    preferred_entry: &str,
    preferred_kind: &str,
    tag: &str,
    description: &str,
    warning: &str,
    cat_id: &str,
    cat_name: &str,
    root: &Path,
    dir: &Path,
    is64: bool,
) -> Option<Tool> {
    let entries = fs::read_dir(dir).ok()?;
    let (total_bytes, file_count) = walk_dir_stats(dir);

    let mut files = Vec::new();
    let mut variants = Vec::new();
    let mut pw_note = String::new();

    for entry in entries.flatten() {
        let fname = entry.file_name().to_string_lossy().into_owned();
        let meta = entry.metadata().ok();
        let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);
        let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
        files.push(FileEntry {
            name: fname.clone(),
            is_dir,
            size,
        });
        if is_dir {
            continue;
        }
        let full_path = dir.join(&fname);
        let lower = fname.to_lowercase();
        let ext = Path::new(&lower)
            .extension()
            .and_then(|s| s.to_str())
            .map(|s| format!(".{s}"))
            .unwrap_or_default();

        if fname.contains("密码") && ext == ".txt" && pw_note.is_empty() {
            if let Ok(raw) = fs::read(&full_path) {
                pw_note = decode_text_auto(&raw).trim().to_string();
            }
        }

        if let Some(kind) = classify_ext(&ext) {
            variants.push(LaunchVariant {
                label: fname,
                path: full_path.to_string_lossy().into_owned(),
                kind: kind.to_string(),
            });
        }
    }

    files.sort_by(|a, b| {
        if a.is_dir != b.is_dir {
            b.is_dir.cmp(&a.is_dir)
        } else {
            a.name.to_lowercase().cmp(&b.name.to_lowercase())
        }
    });

    let mut entry_path = PathBuf::new();
    let mut entry_kind = preferred_kind.to_string();
    if !preferred_entry.is_empty() {
        let cand = dir.join(from_slash(preferred_entry));
        if cand.is_file() {
            entry_path = cand.clone();
            if entry_kind.is_empty() {
                let ext = cand
                    .extension()
                    .and_then(|s| s.to_str())
                    .map(|s| format!(".{}", s.to_lowercase()))
                    .unwrap_or_default();
                if let Some(k) = classify_ext(&ext) {
                    entry_kind = k.to_string();
                }
            }
        }
    }

    if entry_path.as_os_str().is_empty() {
        let mut best_score = -1i32;
        for v in &variants {
            let s = score_variant(v, is64);
            if s > best_score {
                best_score = s;
                entry_path = PathBuf::from(&v.path);
                entry_kind = v.kind.clone();
            }
        }
    }

    if entry_path.as_os_str().is_empty() && variants.is_empty() {
        return None;
    }

    if !entry_path.as_os_str().is_empty() {
        let ep_str = entry_path.to_string_lossy().into_owned();
        if let Some(pos) = variants
            .iter()
            .position(|v| v.path.eq_ignore_ascii_case(&ep_str))
        {
            if pos != 0 {
                variants.swap(0, pos);
            }
        } else {
            let label = entry_path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            variants.insert(
                0,
                LaunchVariant {
                    label,
                    path: ep_str,
                    kind: entry_kind.clone(),
                },
            );
        }
    }

    let rel_dir = to_slash(root, dir);
    let rel_entry_path = if !entry_path.as_os_str().is_empty() {
        to_slash(root, &entry_path)
    } else {
        String::new()
    };

    Some(Tool {
        id: id.to_string(),
        name: name.to_string(),
        aliases: aliases.iter().map(|s| (*s).to_string()).collect(),
        category_id: cat_id.to_string(),
        category_name: cat_name.to_string(),
        tag: tag.to_string(),
        description: description.to_string(),
        warning: warning.to_string(),
        kind: entry_kind,
        dir: dir.to_path_buf(),
        rel_dir,
        entry_path,
        rel_entry_path,
        variants,
        password_note: pw_note,
        files,
        total_bytes,
        file_count,
    })
}

fn classify_ext(ext: &str) -> Option<&'static str> {
    match ext {
        ".exe" => Some("exe"),
        ".bat" | ".cmd" => Some("bat"),
        ".jpg" | ".jpeg" | ".png" | ".gif" | ".bmp" => Some("img"),
        ".zip" | ".rar" | ".7z" => Some("archive"),
        ".html" | ".htm" => Some("html"),
        _ => None,
    }
}

fn score_variant(v: &LaunchVariant, is64: bool) -> i32 {
    let lower = v.label.to_lowercase();
    let mut score = match v.kind.as_str() {
        "exe" => 100,
        "bat" => 80,
        "img" => 70,
        "html" => 60,
        "archive" => 50,
        _ => 0,
    };
    if is64 && (lower.contains("64") || lower.contains("x64")) {
        score += 20;
    }
    if lower.contains("gui") {
        score += 10;
    }
    score
}

pub fn extract_password(note: &str) -> String {
    let normalized = note.replace("\r\n", "\n");
    for line in normalized.lines() {
        let line = line.trim();
        if line.contains("密码") {
            for sep in ['：', ':'] {
                if let Some((_, val)) = line.split_once(sep) {
                    return val.trim().to_string();
                }
            }
        }
    }
    note.trim().to_string()
}

pub fn format_bytes(n: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * KB;
    const GB: f64 = 1024.0 * MB;
    let nf = n as f64;
    if nf >= GB {
        format!("{:.2} GB", nf / GB)
    } else if nf >= MB {
        format!("{:.1} MB", nf / MB)
    } else if nf >= KB {
        format!("{:.1} KB", nf / KB)
    } else {
        format!("{n} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    #[test]
    fn load_catalog_matches_disk() {
        let root = workspace_root();
        assert!(root.join("tools").is_dir(), "tools directory missing");

        let cat = load(&root).expect("load catalog");
        assert_eq!(cat.categories.len(), 8, "expected 8 non-empty categories");
        for c in &cat.categories {
            assert!(!c.tools.is_empty(), "empty category {}", c.name);
        }

        for forbidden in [
            "afterburner",
            "gamepp",
            "iva",
            "check_update",
            "aida64",
            "cpuz",
            "gpuz",
            "tulading_kms",
            "win10_act",
            "microkms",
            "oem7",
            "samsung_win7",
            "other_win7",
            "ultraiso",
            "memtestpro",
            "dismpp",
            "diskgenius",
            "furmark_bat",
            "hwmonitor",
        ] {
            assert!(cat.get(forbidden).is_none(), "removed/stub {forbidden} should be absent");
        }

        assert!(
            cat.total_tools() >= 48 && cat.total_tools() <= 49,
            "got {} tools",
            cat.total_tools()
        );

        for tool in cat.iter_tools() {
            if tool.kind == "tui" {
                continue;
            }
            assert!(tool.entry_path.is_file(), "missing entry_path for {}: {:?}", tool.id, tool.entry_path);
        }

        for id in ["tulading_hw", "rweverything", "thread_bomb", "mem_clean", "batteryinfoview", "chipgenius"] {
            assert!(cat.get(id).is_some(), "restored medium-risk tool {id} missing");
        }

        for id in ["batteryinfoview", "chipgenius", "mem_clean", "thread_bomb"] {
            let tool = cat.get(id).unwrap_or_else(|| panic!("missing tool {id}"));
            assert!(!tool.password_note.is_empty(), "empty password_note for {id}");
            assert!(!tool.password_note.contains('\u{FFFD}'), "garbled password for {id}");
            assert_eq!(extract_password(&tool.password_note), "tulading");
        }

        let cdi = cat.get("crystaldiskinfo").expect("crystaldiskinfo missing");
        assert!(cdi.match_query("crystaldiskinfo"));
        assert!(cdi.match_query("硬盘"));
    }
}
