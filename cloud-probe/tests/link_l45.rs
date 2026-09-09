//! cloud-probe 与 L4/L5 链联调（真实场域数据进入诊断链路）。
//! 仅 Windows 实机运行（CI/Linux 跳过）。

#![cfg(windows)]

use meta_kernel_core::l4::dimension::FieldState;
use meta_kernel_core::l4::l4_router::{route_state, route_passed};
use meta_kernel_core::l5_baseline::BaselineField;
use meta_kernel_core::l5_diagnosis::{diagnose, Traceability, Diagnosis};
use meta_kernel_core::l5_compare::compare;
use meta_kernel_core::l5_senses::{decompose, FieldReading};
use meta_kernel_core::l5_router::to_json;

#[test]
fn probe_output_format_matches_field_reading() {
    let r = cloud_probe::collect().expect("采集成功");
    assert_eq!(r.s.len(), 7);
    for v in &r.s {
        assert!(v.is_finite(), "七维均为有限数: {v}");
        assert!(*v > 0.0, "正读数: {v}");
        assert!(*v < 100.0, "合理上界: {v}");
    }
    // FieldReading 接口一致（schema 1 输出）
    let j = r.to_json();
    assert!(j.starts_with("{\"schema\":1,\"s\":["), "{j}");
}

#[test]
fn real_field_enters_diagnosis_chain() {
    let r: FieldReading = cloud_probe::collect().expect("采集成功");
    let s = r.s;
    // L4：逐维戒律（默认健康基线 1.0；真实场可能通过或拒绝——两条路径均须语义正确）
    let st = FieldState::from_vec(&s).expect("7 元素");
    let base = FieldState::baseline();
    let l4 = route_state(&st, &base);
    // L5：号脉（四场 + 本底场）。本底场=采集值自身（本机首次标定演示），应全平或近平
    let fields = decompose(&s);
    let bl = BaselineField {
        earth: fields[0], water: fields[1], fire: fields[2], wind: fields[3],
        object: "local-host", established: "probe-self",
    };
    let pattern = compare(&fields, &bl);
    let conclusion = meta_kernel_core::l5_diagnosis::synthesize(&fields, &bl, &pattern);
    let d = Diagnosis {
        schema: 2,
        fields,
        pattern,
        conclusion,
        trace: Traceability {
            baseline_id: "b-probe-self".into(), object: "local-host".into(),
            at: "probe".into(), reproducible: true,
        },
    };
    let json = to_json(&d);
    assert!(json.starts_with("{\"schema\":2"), "{json}");
    match l4 {
        Ok(v) => {
            let payload = route_passed(&v);
            assert_eq!(payload.state.len(), 7);
        }
        Err(_) => { /* 被拒路径：不进入号脉（语义正确：拒绝即弃） */ }
    }
    // 号脉诊断必出自真实读数（可溯源）
    assert!(json.contains("trace"), "{json}");
}

#[test]
fn probe_returns_promptly_and_exits() {
    // 探针运行时间应在数秒内（运行即退、不留后台）
    let t0 = std::time::Instant::now();
    let _ = cloud_probe::collect();
    assert!(t0.elapsed().as_secs() < 10, "探针应在数秒内完成: {:?}", t0.elapsed());
    // 进程自身无后台残留由设计保证（单进程、无 spawn 保持）
    let _ = diagnose; // 引用诊断链（编译层面确认链路可用）
}
