use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::Path;
use std::ptr::null_mut;
use std::time::{SystemTime, UNIX_EPOCH};
use windows_sys::Win32::Globalization::MultiByteToWideChar;

#[derive(Debug, Clone, Default)]
pub struct MemoryModule {
    pub index: usize,
    pub capacity: String,
    pub speed: String,
    pub manufacturer: String,
    pub part_number: String,
    pub serial: String,
}

#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub struct GpuInfo {
    pub index: usize,
    pub model: String,
    pub vendor: String,
    pub driver_version: String,
    pub device_id: String,
    pub vram: String,
    pub subsystem: String,
    pub stepping: String,
}

#[derive(Debug, Clone, Default)]
pub struct DiskInfo {
    pub index: usize,
    pub model: String,
    pub capacity: String,
    pub actual_size: String,
    pub bus_type: String,
}

#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub struct MonitorInfo {
    pub index: usize,
    pub model: String,
    pub id: String,
    pub size: String,
    pub dimensions: String,
    pub manufacture: String,
    pub edid: String,
    pub serial: String,
}

#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub struct NetworkAdapter {
    pub index: usize,
    pub model: String,
    pub vendor: String,
    pub mac: String,
    pub speed: String,
    pub adapter_type: String,
}

#[derive(Debug, Clone, Default)]
pub struct HardwareReport {
    pub system_type: String,
    pub system_vendor: String,
    pub system_model: String,

    pub os_name: String,
    pub os_build: String,
    pub os_arch: String,

    pub cpu_model: String,
    pub cpu_arch: String,
    pub cpu_vendor: String,
    pub cpu_cores: String,
    pub cpu_threads: String,
    pub cpu_socket: String,
    pub cpu_bits: String,
    pub cpu_bclk: String,
    pub cpu_clock: String,
    pub cpu_count: String,

    pub chipset_model: String,
    pub chipset_desc: String,
    pub chipset_step: String,

    pub board_brand: String,
    pub board_model: String,

    pub bios_vendor: String,
    pub bios_version: String,
    pub bios_desc: String,
    pub bios_oem: String,

    pub memory_type: String,
    pub memory_modules: Vec<MemoryModule>,
    pub gpus: Vec<GpuInfo>,
    pub disks: Vec<DiskInfo>,
    pub monitors: Vec<MonitorInfo>,
    pub audio_devices: Vec<String>,
    pub net_adapters: Vec<NetworkAdapter>,

    pub source_label: String,
    pub updated_at: Option<SystemTime>,
}

impl HardwareReport {
    /// Sums the capacity of all memory modules in the static report (e.g. "20GB").
    pub fn total_memory_gb(&self) -> String {
        let mut total: u64 = 0;
        for m in &self.memory_modules {
            let s = m.capacity.trim().to_ascii_uppercase();
            let s = s.strip_suffix("GB").unwrap_or(&s);
            let s = s.strip_suffix('G').unwrap_or(s);
            if let Ok(n) = s.trim().parse::<u64>() {
                if n > 0 {
                    total += n;
                }
            }
        }
        if total > 0 {
            format!("{total}GB")
        } else {
            String::new()
        }
    }

    pub fn updated_at_string(&self) -> String {
        let Some(ts) = self.updated_at else {
            return String::new();
        };
        #[cfg(windows)]
        {
            use windows_sys::Win32::Foundation::{FILETIME, SYSTEMTIME};
            use windows_sys::Win32::Storage::FileSystem::FileTimeToLocalFileTime;
            use windows_sys::Win32::System::Time::FileTimeToSystemTime;
            let secs = ts.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
            let ticks = secs.saturating_add(11_644_473_600).saturating_mul(10_000_000);
            let ft = FILETIME {
                dwLowDateTime: (ticks & 0xFFFF_FFFF) as u32,
                dwHighDateTime: (ticks >> 32) as u32,
            };
            unsafe {
                let mut local_ft = FILETIME::default();
                let mut st = SYSTEMTIME::default();
                if FileTimeToLocalFileTime(&ft, &mut local_ft) != 0
                    && FileTimeToSystemTime(&local_ft, &mut st) != 0
                {
                    return format!(
                        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
                        st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute, st.wSecond
                    );
                }
            }
        }
        let secs = ts.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
        format!("{secs}s")
    }
}

/// Decodes UTF-8 (stripping UTF-8 BOM) or falls back to GB18030/GBK via Win32 MultiByteToWideChar.
pub fn decode_text_auto(data: &[u8]) -> String {
    let data = data.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(data);
    if let Ok(s) = std::str::from_utf8(data) {
        return s.to_string();
    }
    if data.is_empty() {
        return String::new();
    }
    let Ok(len) = i32::try_from(data.len()) else {
        return String::from_utf8_lossy(data).into_owned();
    };
    for cp in [54936u32, 936u32] {
        let needed = unsafe { MultiByteToWideChar(cp, 0, data.as_ptr(), len, null_mut(), 0) };
        if needed > 0 {
            let mut wide = vec![0u16; needed as usize];
            let written = unsafe {
                MultiByteToWideChar(cp, 0, data.as_ptr(), len, wide.as_mut_ptr(), needed)
            };
            if written > 0 {
                return String::from_utf16_lossy(&wide[..written as usize]);
            }
        }
    }
    String::from_utf8_lossy(data).into_owned()
}

fn parse_ini_sections(text: &str) -> HashMap<String, HashMap<String, String>> {
    let mut sections: HashMap<String, HashMap<String, String>> = HashMap::new();
    let mut current = String::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') && line.len() >= 2 {
            current = line[1..line.len() - 1].trim().to_string();
            sections.entry(current.clone()).or_default();
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            sections
                .entry(current.clone())
                .or_default()
                .insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    sections
}

fn get_val(sec: &HashMap<String, String>, key: &str) -> String {
    sec.get(key).cloned().unwrap_or_default()
}

fn parse_count(raw: Option<&String>, sec: &HashMap<String, String>, prefix: &str, suffix: &str) -> usize {
    if let Some(r) = raw {
        if let Ok(n) = r.trim().parse::<usize>() {
            if n > 0 {
                return n;
            }
        }
    }
    for i in 1..=16 {
        let key = format!("{prefix}{i}{suffix}");
        if sec.get(&key).map(|v| v.is_empty()).unwrap_or(true) {
            return i - 1;
        }
    }
    0
}

fn format_link_speed(raw: &str) -> String {
    let raw = raw.trim();
    if raw.is_empty() {
        return String::new();
    }
    let Ok(v) = raw.parse::<u64>() else {
        return raw.to_string();
    };
    if v == 0 || v >= 1_000_000_000_000 {
        return "未连接/未知".to_string();
    }
    if v >= 1_000_000_000 {
        return format!("{:.1} Gbps", v as f64 / 1e9);
    }
    if v >= 1_000_000 {
        return format!("{:.0} Mbps", v as f64 / 1e6);
    }
    format!("{v} bps")
}

/// Reads static hwinfo.ini from `root` with zero WMI/COM polling overhead.
pub fn parse_hwinfo(root: &Path) -> io::Result<HardwareReport> {
    let mut rep = HardwareReport {
        source_label: "静态配置".to_string(),
        ..Default::default()
    };
    let path = root.join("hwinfo.ini");
    if let Ok(meta) = fs::metadata(&path) {
        rep.updated_at = meta.modified().ok();
    }
    let data = fs::read(&path)?;
    let sec = parse_ini_sections(&decode_text_auto(&data));

    if let Some(s) = sec.get("类型") {
        rep.system_type = get_val(s, "类型");
        rep.system_vendor = get_val(s, "厂商");
        rep.system_model = get_val(s, "型号");
    }
    if let Some(s) = sec.get("系统") {
        rep.os_name = get_val(s, "版本");
        rep.os_build = get_val(s, "Build");
        rep.os_arch = get_val(s, "系统位宽");
    }
    if let Some(s) = sec.get("处理器") {
        rep.cpu_model = get_val(s, "处理器型号").trim().to_string();
        rep.cpu_arch = get_val(s, "处理器架构_数据");
        rep.cpu_vendor = get_val(s, "处理器厂商");
        rep.cpu_cores = get_val(s, "处理器核心数");
        rep.cpu_threads = get_val(s, "处理器线程数");
        rep.cpu_socket = get_val(s, "处理器接口");
        rep.cpu_bits = get_val(s, "处理器位宽");
        rep.cpu_bclk = get_val(s, "处理器外频");
        rep.cpu_clock = get_val(s, "处理器主频");
        rep.cpu_count = get_val(s, "处理器数量");
    }
    if let Some(s) = sec.get("芯片组") {
        rep.chipset_model = get_val(s, "型号");
        rep.chipset_desc = get_val(s, "描述");
        rep.chipset_step = get_val(s, "步进");
    }
    if let Some(s) = sec.get("主板") {
        rep.board_brand = get_val(s, "品牌");
        rep.board_model = get_val(s, "型号");
    }
    if let Some(s) = sec.get("BIOS") {
        rep.bios_vendor = get_val(s, "厂商");
        rep.bios_version = get_val(s, "版本");
        rep.bios_desc = get_val(s, "说明");
        rep.bios_oem = get_val(s, "OEM版本");
    }
    if let Some(s) = sec.get("内存") {
        rep.memory_type = get_val(s, "类型");
        let count = parse_count(s.get("数量"), s, "内存", "容量");
        for i in 1..=count {
            let p = format!("内存{i}");
            let cap = get_val(s, &(p.clone() + "容量"));
            let part = get_val(s, &(p.clone() + "型号")).trim().to_string();
            if cap.is_empty() && part.is_empty() {
                continue;
            }
            rep.memory_modules.push(MemoryModule {
                index: i,
                capacity: cap,
                speed: get_val(s, &(p.clone() + "速度")),
                manufacturer: get_val(s, &(p.clone() + "厂商")),
                part_number: part,
                serial: get_val(s, &(p + "序列号")),
            });
        }
    }
    if let Some(s) = sec.get("显卡") {
        let count = parse_count(s.get("数量"), s, "显卡", "型号");
        for i in 1..=count {
            let p = format!("显卡{i}");
            let model = get_val(s, &(p.clone() + "型号"));
            if model.is_empty() {
                continue;
            }
            rep.gpus.push(GpuInfo {
                index: i,
                model,
                vendor: get_val(s, &(p.clone() + "厂商")),
                driver_version: get_val(s, &(p.clone() + "驱动版本")),
                device_id: get_val(s, &(p.clone() + "ID")),
                vram: get_val(s, &(p.clone() + "显存")),
                subsystem: get_val(s, &(p.clone() + "制造商")),
                stepping: get_val(s, &(p + "步进")),
            });
        }
    }
    if let Some(s) = sec.get("磁盘") {
        let count = parse_count(s.get("数量"), s, "磁盘", "型号");
        for i in 1..=count {
            let p = format!("磁盘{i}");
            let model = get_val(s, &(p.clone() + "型号"));
            if model.is_empty() {
                continue;
            }
            rep.disks.push(DiskInfo {
                index: i,
                model,
                capacity: get_val(s, &(p.clone() + "容量")),
                actual_size: get_val(s, &(p.clone() + "实际容量")),
                bus_type: get_val(s, &(p + "类型")),
            });
        }
    }
    if let Some(s) = sec.get("显示器") {
        let count = parse_count(s.get("数量"), s, "显示器", "ID");
        for i in 1..=count {
            let p = format!("显示器{i}");
            let mut model = get_val(s, &(p.clone() + "型号_测试版")).trim().to_string();
            if model.is_empty() {
                model = get_val(s, &(p.clone() + "型号")).trim().to_string();
            }
            let id = get_val(s, &(p.clone() + "ID"));
            if model.is_empty() && id.is_empty() {
                continue;
            }
            let w = get_val(s, &(p.clone() + "宽度"));
            let h = get_val(s, &(p.clone() + "高度"));
            let dimensions = if !w.is_empty() && !h.is_empty() {
                format!("{w} x {h}")
            } else {
                String::new()
            };
            rep.monitors.push(MonitorInfo {
                index: i,
                model,
                id,
                size: get_val(s, &(p.clone() + "尺寸")),
                dimensions,
                manufacture: get_val(s, &(p.clone() + "生产日期")),
                edid: get_val(s, &(p.clone() + "edid版本")),
                serial: get_val(s, &(p + "序列号_测试版")),
            });
        }
    }
    if let Some(s) = sec.get("声卡") {
        let count = parse_count(s.get("数量"), s, "声卡", "型号");
        for i in 1..=count {
            let m = get_val(s, &format!("声卡{i}型号"));
            if !m.is_empty() {
                rep.audio_devices.push(m);
            }
        }
    }
    if let Some(s) = sec.get("网卡") {
        let count = parse_count(s.get("数量"), s, "网卡", "型号");
        for i in 1..=count {
            let p = format!("网卡{i}");
            let model = get_val(s, &(p.clone() + "型号"));
            if model.is_empty() {
                continue;
            }
            rep.net_adapters.push(NetworkAdapter {
                index: i,
                model,
                vendor: get_val(s, &(p.clone() + "厂商")),
                mac: get_val(s, &(p.clone() + "MAC")),
                speed: format_link_speed(&get_val(s, &(p.clone() + "速度"))),
                adapter_type: get_val(s, &(p + "类型")),
            });
        }
    }

    Ok(rep)
}

/// Reads static hwinfo.ini from `data_root`, falling back to a labelled empty report if missing.
pub fn load_initial_hardware(data_root: &Path) -> HardwareReport {
    match parse_hwinfo(data_root) {
        Ok(rep) => rep,
        Err(_) => HardwareReport {
            source_label: "未找到 hwinfo.ini".to_string(),
            ..Default::default()
        },
    }
}

/// Strips redundant marketing noise and truncates to `max_chars` characters.
pub fn shorten_model_name(s: &str, max_chars: usize) -> String {
    let mut cleaned = s.trim().to_string();
    for noise in [
        "with Radeon Graphics",
        "NVIDIA GeForce ",
        "AMD Radeon ",
        "Intel(R) Core(TM) ",
        " Laptop GPU",
        "Processor",
        "AMD ",
    ] {
        cleaned = cleaned.replace(noise, "");
    }
    let collapsed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let chars: Vec<char> = collapsed.chars().collect();
    if max_chars > 1 && chars.len() > max_chars {
        let mut out: String = chars.into_iter().take(max_chars - 1).collect();
        out.push('~');
        out
    } else {
        collapsed
    }
}

/// Formats a clean UTF-8 static hardware configuration summary for export/clipboard.
pub fn format_plain_text(rep: &HardwareReport) -> String {
    let mut out = String::new();
    out.push_str("==================== Trust工具箱 硬件配置报告 ====================\n");
    out.push_str(&format!("数据来源: {} (hwinfo.ini)\n", rep.source_label));
    let ts = rep.updated_at_string();
    if !ts.is_empty() {
        out.push_str(&format!("更新时间: {ts}\n"));
    }

    out.push_str("\n[系统与机型]\n");
    out.push_str(&format!("  操作系统 : {} (Build {} / {})\n", rep.os_name, rep.os_build, rep.os_arch));
    out.push_str(&format!("  机型信息 : {} {} ({})\n", rep.system_vendor, rep.system_model, rep.system_type));
    out.push_str(&format!("  主板型号 : {} {}\n", rep.board_brand, rep.board_model));
    out.push_str(&format!("  BIOS信息 : {} / {} ({})\n", rep.bios_vendor, rep.bios_version, rep.bios_desc));

    out.push_str("\n[处理器]\n");
    out.push_str(&format!("  CPU型号  : {}\n", rep.cpu_model));
    out.push_str(&format!(
        "  规格参数 : {}核心 {}线程 | 主频 {} | 外频 {} | 接口 {}\n",
        rep.cpu_cores, rep.cpu_threads, rep.cpu_clock, rep.cpu_bclk, rep.cpu_socket
    ));

    out.push_str("\n[内存]\n");
    for m in &rep.memory_modules {
        out.push_str(&format!(
            "  插槽#{}  : {} {} | {} | 型号: {} (S/N: {})\n",
            m.index, m.capacity, m.speed, m.manufacturer, m.part_number, m.serial
        ));
    }

    out.push_str("\n[显卡]\n");
    for g in &rep.gpus {
        out.push_str(&format!(
            "  显卡#{}  : {} | 显存: {} | 驱动: {}\n",
            g.index, g.model, g.vram, g.driver_version
        ));
    }

    out.push_str("\n[硬盘]\n");
    for d in &rep.disks {
        out.push_str(&format!(
            "  硬盘#{}  : {} | 标称: {} (实际: {}) | 接口: {}\n",
            d.index, d.model, d.capacity, d.actual_size, d.bus_type
        ));
    }

    out.push_str("\n[显示器]\n");
    for mon in &rep.monitors {
        out.push_str(&format!(
            "  屏幕#{}  : {} ({}) | 尺寸: {} | 物理: {} | 出厂: {}\n",
            mon.index, mon.model, mon.id, mon.size, mon.dimensions, mon.manufacture
        ));
    }

    out.push_str("\n[声卡与网卡]\n");
    for (i, a) in rep.audio_devices.iter().enumerate() {
        out.push_str(&format!("  声卡#{}  : {}\n", i + 1, a));
    }
    for n in &rep.net_adapters {
        out.push_str(&format!(
            "  网卡#{}  : {} | {} | MAC: {} | 速率: {}\n",
            n.index, n.model, n.adapter_type, n.mac, n.speed
        ));
    }
    out.push_str("=================================================================\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn workspace_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    #[test]
    fn parse_static_hwinfo() {
        let root = workspace_root();
        assert!(root.join("hwinfo.ini").exists(), "hwinfo.ini missing");

        let rep = parse_hwinfo(&root).expect("parse_hwinfo failed");
        assert!(rep.cpu_model.contains("14650HX"), "got {}", rep.cpu_model);
        assert!(rep.os_name.contains("Windows"), "got {}", rep.os_name);
        assert_eq!(rep.total_memory_gb(), "20GB");
        assert!(!rep.gpus.is_empty());
        assert!(rep.gpus[0].model.contains("4050"));
        assert_eq!(rep.gpus[0].vram, "6GB");
        assert!(!rep.disks.is_empty());

        let txt = format_plain_text(&rep);
        assert!(txt.contains(&rep.cpu_model));
    }
}
