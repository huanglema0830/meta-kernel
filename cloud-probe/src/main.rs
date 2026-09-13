//! cloud-probe 入口：感知对象场域状态 → 输出七维 JSON（FieldReading 契约），运行即退。
//! 用法：
//!   cloud-probe                    采集并打印七维 JSON
//!   cloud-probe --report http://127.0.0.1:3000   采集后 POST 到网关 /v1/probe 并回显确认
//!   cloud-probe --file out.json    采集并写入文件（供共享传输）
//!   cloud-probe --diagnose-file in.json   读取场域 JSON → L4/L5 诊断（schema2 多语言）
//!   cloud-probe --usb              仅做 USB 场检测（不采集七维）
//!   cloud-probe --usb-report http://127.0.0.1:3000    USB 场检测并回传 /v1/probe/usb

fn main() {
    let args: Vec<String> = std::env::args().collect();

    // ---- USB 场检测（独立路径：不触发整机七维采集）----
    if args.len() >= 2 && (args[1] == "--usb" || args[1] == "--usb-report") {
        match cloud_probe::usb::probe_usb() {
            Ok(r) => {
                let json = r.to_json();
                if args[1] == "--usb-report" && args.len() >= 3 {
                    match cloud_probe::post_probe(&args[2], &json) {
                        Ok(msg) => println!("{msg}\n{json}"),
                        Err(e) => {
                            eprintln!("cloud-probe usb report error: {e}");
                            std::process::exit(1);
                        }
                    }
                } else {
                    println!("{json}");
                }
            }
            Err(e) => {
                eprintln!("cloud-probe usb error: {e}");
                std::process::exit(1);
            }
        }
        return;
    }

    match cloud_probe::collect() {
        Ok(r) => {
            let json = r.to_json();
            if args.len() >= 3 && args[1] == "--report" {
                match cloud_probe::post_probe(&args[2], &json) {
                    Ok(msg) => println!("{msg}
{json}"),
                    Err(e) => {
                        eprintln!("cloud-probe report error: {e}");
                        eprintln!("需人工介入：请确认网关地址可达（{}
{json})", &args[2]);
                        std::process::exit(1);
                    }
                }
            } else if args.len() >= 3 && args[1] == "--diagnose-file" {
                match std::fs::read_to_string(&args[2]) {
                    Ok(txt) => match cloud_probe::parse_reading(&txt).and_then(|s7| cloud_probe::diagnose_s(s7).ok()) {
                        Some(d) => println!("{d}"),
                        None => {
                            eprintln!("无法解析场域 JSON 或诊断失败: {}", &args[2]);
                            std::process::exit(1);
                        }
                    },
                    Err(e) => {
                        eprintln!("读取失败: {e}");
                        std::process::exit(1);
                    }
                }
            } else if args.len() >= 3 && args[1] == "--file" {
                if let Err(e) = std::fs::write(&args[2], &json) {
                    eprintln!("cloud-probe write error: {e}");
                    std::process::exit(1);
                }
                println!("written: {} ({json})", &args[2]);
            } else {
                println!("{json}");
            }
        }
        Err(e) => {
            eprintln!("cloud-probe error: {e}");
            std::process::exit(1);
        }
    }
}
