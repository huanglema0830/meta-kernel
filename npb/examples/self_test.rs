//! 自检摘要打印（原生侧；供跨平台一致性校验）。

fn main() {
    let digest = npb::self_test_digest();
    println!("DIGEST={digest}");
    println!("NPB_NATIVE_SELF_TEST_OK");
}
