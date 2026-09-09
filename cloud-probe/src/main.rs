//! cloud-probe 入口：感知对象场域状态 → 输出七维 JSON（FieldReading 契约），运行即退。
//! 用法：`cloud-probe [--json]`（默认即 JSON 输出）。

fn main() {
    match cloud_probe::collect() {
        Ok(r) => println!("{}", r.to_json()),
        Err(e) => {
            eprintln!("cloud-probe error: {e}");
            std::process::exit(1);
        }
    }
}
