//! cloud-probe 入口：感知对象场域状态 → 输出七维 JSON（FieldReading 契约），运行即退。
//! 用法：
//!   cloud-probe                    采集并打印七维 JSON
//!   cloud-probe --report http://127.0.0.1:3000   采集后 POST 到网关 /v1/probe 并回显确认
//!   cloud-probe --file out.json    采集并写入文件（供共享传输）

fn main() {
    let args: Vec<String> = std::env::args().collect();
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
