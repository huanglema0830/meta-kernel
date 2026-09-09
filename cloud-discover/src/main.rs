//! cloud-discover 入口：扫描本机 /24 → 状态 JSON（含在线设备与需人工介入项），运行即退。
//! 用法：`cloud-discover [--report <gateway-http>]`（可选注入网关地址到指引文案）。

fn main() {
    let report_gw = std::env::args().nth(2).unwrap_or_default();
    match cloud_discover::scan() {
        Ok(r) => {
            let mut json = r.to_json();
            if !report_gw.is_empty() {
                json = json.replace("\"manual_needed\"", &format!("\"gateway\":\"{report_gw}\",\"manual_needed\""));
            }
            println!("{json}");
            // 控制台可读摘要
            for h in &r.online {
                println!("[在线] {} ({})", h.ip, h.hostname.clone().unwrap_or_else(|| "名称未知".into()));
            }
            if r.online.is_empty() {
                println!("[提示] 未发现其他在线设备（仅本机网段自身）");
            }
            println!("[探针指引] 在目标设备运行：cloud-probe.exe --report http://<云操作系统主机>:3000");
        }
        Err(e) => {
            eprintln!("cloud-discover error: {e}");
            std::process::exit(1);
        }
    }
}
