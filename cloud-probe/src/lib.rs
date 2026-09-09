//! cloud-probe 库（感知 → 七维翻译；逻辑与平台采集分离）。

use meta_kernel_core::l5_senses::FieldReading;

/// 采集并翻译当前场域 → FieldReading（运行即退：单一采样，~0.5s 完成）。
pub fn collect() -> Result<FieldReading, String> {
    #[cfg(windows)]
    {
        win::sample()
    }
    #[cfg(not(windows))]
    {
        Err("cloud-probe: 非 Windows 平台暂无采集器（fallback）".to_string())
    }
}

#[cfg(windows)]
mod win {
    use super::FieldReading;

    #[repr(C)]
    #[derive(Default)]
    struct MemStatusEx {
        dw_length: u32,
        dw_memory_load: u32,
        ull_total_phys: u64,
        ull_avail_phys: u64,
        ull_total_page: u64,
        ull_avail_page: u64,
        ull_total_virtual: u64,
        ull_avail_virtual: u64,
        ull_avail_ext: u64,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct FileTime {
        dw_low: u32,
        dw_high: u32,
    }
    impl FileTime {
        fn to_u64(self) -> u64 {
            ((self.dw_high as u64) << 32) | self.dw_low as u64
        }
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GlobalMemoryStatusEx(lp: *mut MemStatusEx) -> i32;
        fn GetSystemTimes(lp_idle: *mut FileTime, lp_kernel: *mut FileTime, lp_user: *mut FileTime) -> i32;
        fn GetDiskFreeSpaceExW(dir: *const u16, free: *mut u64, total: *mut u64, avail: *mut u64) -> i32;
    }

    pub fn sample() -> Result<FieldReading, String> {
        let mut ms = MemStatusEx::default();
        ms.dw_length = std::mem::size_of::<MemStatusEx>() as u32;
        if unsafe { GlobalMemoryStatusEx(&mut ms) } == 0 {
            return Err("GlobalMemoryStatusEx 失败".into());
        }
        let mem_pct = ms.dw_memory_load as f64;

        let cpu1 = cpu_pct()?;
        std::thread::sleep(std::time::Duration::from_millis(180));
        let cpu2 = cpu_pct()?;

        let free_pct = disk_free_pct()?;
        let (procs, threads) = count_processes_threads()?;

        let t = cpu2.max(cpu1) / 40.0;
        let f = (cpu2 + 15.0) / (cpu1 + 15.0);
        let a = mem_pct / 40.0;
        let jitter = (cpu2 - cpu1).abs() / cpu1.max(1.0);
        let phi = 1.0 + jitter * 8.0;
        let x = (free_pct / 100.0) / 0.5;
        let h = procs as f64 / 100.0;
        let tau = threads as f64 / 300.0;

        Ok(FieldReading {
            s: [t, f, a, phi, x, h, tau],
            at: None,
        })
    }

    fn cpu_pct() -> Result<f64, String> {
        let mut idle = FileTime::default();
        let mut ker = FileTime::default();
        let mut user = FileTime::default();
        if unsafe { GetSystemTimes(&mut idle, &mut ker, &mut user) } == 0 {
            return Err("GetSystemTimes 失败".into());
        }
        let total = ker.to_u64().saturating_add(user.to_u64());
        let busy = total.saturating_sub(idle.to_u64());
        Ok(busy as f64 / total.max(1) as f64 * 100.0)
    }

    fn disk_free_pct() -> Result<f64, String> {
        let cur = std::env::current_dir().map_err(|e| e.to_string())?;
        let drive = cur.to_string_lossy().chars().next().unwrap_or('C');
        let root = format!("{drive}:\\");
        let wide: Vec<u16> = root.encode_utf16().chain(std::iter::once(0)).collect();
        let mut free: u64 = 0;
        let mut total: u64 = 0;
        if unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut free, &mut total, std::ptr::null_mut()) } == 0 || total == 0 {
            return Err("GetDiskFreeSpaceExW 失败".into());
        }
        Ok(free as f64 / total as f64 * 100.0)
    }

    fn count_processes_threads() -> Result<(u64, u64), String> {
        // Windows 自带管理 shell 计数进程与线程总数
        let script = "$p=Get-Process -ErrorAction SilentlyContinue; Write-Output $p.Count; Write-Output (($p | ForEach-Object { $_.Threads.Count } | Measure-Object -Sum).Sum)";
        let sh = std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", script])
            .output()
            .map_err(|e| format!("ps: {e}"))?;
        let text = String::from_utf8_lossy(&sh.stdout);
        let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
        if lines.len() < 2 {
            return Err("ps 输出异常".into());
        }
        Ok((lines[0].trim().parse().unwrap_or(80), lines[1].trim().parse().unwrap_or(400)))
    }
}
