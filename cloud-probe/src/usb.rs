//! USB 场检测（探针扩展）
//!
//! 目标：诊断 USB 口接触不良（典型对象：充电宝／线缆／接口松动）。
//!
//! **诚实说明**：Windows 不向普通程序暴露 USB 端口上的原始电流／电压数据
//! （需专用硬件（USB 电流表）或厂商 EC 接口）。因此本模块采用**次级可观测信号**：
//!
//! - USB 设备拓扑（当前挂载的 USB 类设备数）——反映"场"的结构面（τ 维）
//! - 系统事件日志中的 USB / PnP 事件（最近 24h）——反映"场"的变动面（f 维）
//!
//! 若 USB 口接触不良，典型表现是**设备反复到达/移除**（churn），
//! 这会直接在事件计数上体现。判定以此为据，并明确标注为近似指标。
//!
//! 零第三方依赖：仅调用系统自带的管理 shell。

use std::process::Command;

/// USB 场检测结果。
#[derive(Clone, Debug, PartialEq)]
pub struct UsbReport {
    /// 当前 USB 类设备数（结构面，τ）。
    pub devices: u64,
    /// 最近 24h 内 USB / PnP 相关系统事件数（变动面，f）。
    pub events_24h: u64,
    /// 断连/变动频率（次/小时）。
    pub churn_per_hour: f64,
    /// 判定：normal / watch / contact_poor / unknown
    pub verdict: &'static str,
    /// 可读说明（含方法论与建议）。
    pub note: String,
}

impl UsbReport {
    /// 手写 JSON（零依赖；与探针其余输出风格一致）。
    pub fn to_json(&self) -> String {
        format!(
            "{{\"usb\":{{\"devices\":{},\"events_24h\":{},\"churn_per_hour\":{:.3},\
\"verdict\":\"{}\",\"note\":\"{}\"}}}}",
            self.devices,
            self.events_24h,
            self.churn_per_hour,
            self.verdict,
            self.note.replace('\\', "\\\\").replace('"', "\\\"")
        )
    }

    /// 七维中的两个相关分量（供上层并入场域向量参考）。
    /// τ_proxy：设备数（结构/连接）
    /// f_proxy：churn 频率（变动）
    pub fn tau_proxy(&self) -> f64 {
        self.devices as f64 / 10.0
    }
    pub fn f_proxy(&self) -> f64 {
        1.0 + self.churn_per_hour / 2.0
    }
}

/// 由观测值生成判定（纯函数——便于单元测试，不依赖平台）。
///
/// 阈值说明：24h 内 USB/PnP 事件含**正常枚举活动**（开机设备枚举、驱动加载等），
/// 因此不能一见事件就判"接触不良"。按实测标定：
///   ≤9   → normal     （正常枚举量级）
///   10–39 → watch      （偏多，需结合实际体验判断）
///   ≥40  → contact_poor（远超正常枚举，高度疑似反复断连）
pub fn verdict_of(events_24h: u64) -> &'static str {
    match events_24h {
        0..=9 => "normal",
        10..=39 => "watch",
        _ => "contact_poor",
    }
}

/// 采集 USB 场（仅 Windows；其余平台返回 Unsupported 说明）。
pub fn probe_usb() -> Result<UsbReport, String> {
    #[cfg(windows)]
    {
        win::sample()
    }
    #[cfg(not(windows))]
    {
        Err("cloud-probe: USB 场检测仅支持 Windows（当前平台无实现）".to_string())
    }
}

#[cfg(windows)]
mod win {
    use super::{verdict_of, UsbReport};
    use std::process::Command;

    pub fn sample() -> Result<UsbReport, String> {
        // 两行输出由 Rust 解析：
        //   USB_DEV=<当前 USB 类设备数>
        //   USB_EVENTS=<最近 24h USB/PnP 事件数>
        let script = concat!(
            "$since=(Get-Date).AddHours(-24); ",
            "$dev=@(Get-CimInstance Win32_PnPEntity -ErrorAction SilentlyContinue | ",
            "Where-Object { $_.PNPClass -eq 'USB' -or $_.Service -eq 'USBHUB3' -or $_.Service -eq 'usbhub' }); ",
            "Write-Output ('USB_DEV=' + $dev.Count); ",
            "$ev=@(Get-WinEvent -FilterHashtable @{LogName='System';StartTime=$since} -ErrorAction SilentlyContinue | ",
            "Where-Object { $_.ProviderName -match 'PnP|USB' -or ($_.Message -ne $null -and $_.Message -match 'USB') }); ",
            "Write-Output ('USB_EVENTS=' + $ev.Count)"
        );

        let out = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .output()
            .map_err(|e| format!("powershell: {e}"))?;

        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        let mut devices: u64 = 0;
        let mut events: u64 = 0;
        let mut got_dev = false;
        let mut got_ev = false;
        for line in text.lines() {
            let l = line.trim();
            if let Some(v) = l.strip_prefix("USB_DEV=") {
                devices = v.trim().parse().unwrap_or(0);
                got_dev = true;
            } else if let Some(v) = l.strip_prefix("USB_EVENTS=") {
                events = v.trim().parse().unwrap_or(0);
                got_ev = true;
            }
        }
        if !got_dev && !got_ev {
            return Err("USB 场采集失败：无法读取设备/事件（权限或系统限制）".to_string());
        }

        let churn = events as f64 / 24.0;
        let verdict = verdict_of(events);
        let note = match verdict {
            "normal" => format!(
                "USB 变动处于正常枚举量级（24h 内 {events} 次事件，当前 {devices} 个 USB 设备），\
未见接触不良特征。"
            ),
            "watch" => format!(
                "USB 变动偏多（24h 内 {events} 次事件，当前 {devices} 个设备）——\
可能是正常枚举，也可能是接口偶发松动。建议结合实际使用体验判断：\
若充电/传输时经常中断，按 ①重插拔并清洁触点 ②换数据线 ③换 USB 口 逐一交叉验证。"
            ),
            _ => format!(
                "USB 变动非常频繁（24h 内 {events} 次事件，当前 {devices} 个设备）——\
高度疑似接口反复断连/接触不良。建议：①重新插拔并清洁触点（酒精/橡皮）\
②更换数据线 ③换一个 USB 口 ④若仍频繁，可能为端口虚焊，需送修。"
            ),
        };
        let note = format!(
            "{note}［方法说明］系统不向普通程序暴露 USB 原始电流/电压，\
本判定基于系统事件日志的 USB/PnP 变动频率，属**近似指标**；\
可与本底场机制结合（正常时的变动率作为基线，偏离时告警）以提升准确度。"
        );

        Ok(UsbReport {
            devices,
            events_24h: events,
            churn_per_hour: churn,
            verdict,
            note,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verdict_thresholds() {
        assert_eq!(verdict_of(0), "normal");
        assert_eq!(verdict_of(9), "normal");
        assert_eq!(verdict_of(10), "watch");
        assert_eq!(verdict_of(39), "watch");
        assert_eq!(verdict_of(40), "contact_poor");
        assert_eq!(verdict_of(999), "contact_poor");
    }

    #[test]
    fn json_shape_is_stable() {
        let r = UsbReport {
            devices: 12,
            events_24h: 3,
            churn_per_hour: 0.125,
            verdict: "normal",
            note: "ok".to_string(),
        };
        let j = r.to_json();
        assert!(j.starts_with("{\"usb\":{"), "{j}");
        assert!(j.contains("\"devices\":12"), "{j}");
        assert!(j.contains("\"verdict\":\"normal\""), "{j}");
    }

    #[test]
    fn json_escapes_quotes_in_note() {
        let r = UsbReport {
            devices: 1,
            events_24h: 20,
            churn_per_hour: 0.833,
            verdict: "contact_poor",
            note: "建议\"换线\"并清触点".to_string(),
        };
        let j = r.to_json();
        assert!(j.contains("\\\"换线\\\""), "{j}");
    }

    #[test]
    fn proxies_are_sane() {
        let r = UsbReport {
            devices: 10,
            events_24h: 24,
            churn_per_hour: 1.0,
            verdict: "contact_poor",
            note: String::new(),
        };
        assert!((r.tau_proxy() - 1.0).abs() < 1e-9);
        assert!((r.f_proxy() - 1.5).abs() < 1e-9);
    }
}
