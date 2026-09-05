//! Phase 2 集成测试：安全阀 × 正源系统 × 内核闭环 的场景验证。
//!
//! 场景覆盖（对应发起人验收标准）：
//! 1. **负输入拦截**：所有负值经安全阀归零，内核任何环节不出现负值；
//! 2. **资源枯竭**：Observer 超配额 → 强制休眠（非杀死）→ 正源系统自动介入回收；
//! 3. **自动调用链**：休眠回收后的元素进入正源库，可被 Searcher 再次检索到。

use meta_kernel_core::{
    energy::{energy_level_evaluate, Verdict, verdict_for},
    hourglass::BubbleHourglass,
    mirror::MirrorPool,
    ontology::{self, Element, Pattern},
    positive_source::{process_pattern, GranularityGovernor, MeritPool, PositiveSource, Searcher},
    sanitizer::{ManasMonitor, ObserverQuota, QuotaStatus, finalize, soft_clamp},
};

fn e(l: u8, v: f64) -> Element {
    Element::new(l, v)
}

#[test]
fn negative_input_is_annihilated_everywhere() {
    let mut hg = BubbleHourglass::new();
    let mut pool = MirrorPool::new();

    // NPB 层软钳位：负输入统一归零
    let seeds = [-5.0, -0.001, -1e9, 0.0, 0.5, 1.0, 3.0];
    for s in seeds {
        let clamped = soft_clamp(s);
        assert!((0.0..=1.0).contains(&clamped), "soft_clamp({s}) 越界");

        // 全链路喂入 clamped 值：沙漏与镜像池输出均合法非负
        let outs = hg.tick(Some(clamped));
        for o in &outs {
            assert!(o.is_finite() && *o >= 0.0 && *o <= 1.0, "hourglass out: {o}");
            pool.observe(*o);
        }
        if let Some(echo) = pool.tick(None) {
            assert!(echo.is_finite() && echo >= 0.0, "echo: {echo}");
        }
        assert_eq!(finalize(-0.5), 0.0);
    }
    // 末那识扰动种子同样安全
    let m = ManasMonitor::default();
    for salt in 0..20u64 {
        assert!((0.0..=1.0).contains(&m.disturbance_seed(salt)));
    }
}

#[test]
fn resource_exhaustion_sleeps_then_positive_source_recycles() {
    // ---- 阶段 1：Observer 资源枯竭 → 强制休眠（非杀死）----
    let mut quota = ObserverQuota::new(7, 10, 1024);
    let mut status = QuotaStatus::Active;
    for _ in 0..10 {
        status = quota.charge(1, 128);
    }
    assert_eq!(status, QuotaStatus::Dormant, "超预算应休眠而非杀死");
    assert!(quota.is_dormant());

    // ---- 阶段 2：系统压力升高，正源系统自动介入 ----
    let pressure = 0.95; // 高熵/高压 → 粗粒止损
    assert!(matches!(
        GranularityGovernor::granularity(pressure),
        meta_kernel_core::positive_source::Granularity::Coarse
    ));

    // 把休眠 Observer 的"遗留模式"（含负向/混沌痕迹）交给正源流水线
    let leftover = Pattern::new(vec![e(1, 0.05), e(1, 0.05), e(1, 0.02)]);
    let mut source = PositiveSource::new();
    let mut merit = MeritPool::new();
    let vigor = energy_level_evaluate(&leftover);
    assert!(matches!(verdict_for(vigor), Verdict::DecomposeToGranules | Verdict::RecycleLoop));
    let outcome = process_pattern(&mut source, &mut merit, &leftover, pressure);
    assert!(
        matches!(outcome, meta_kernel_core::positive_source::ProcessOutcome::RecycledToSource { .. }),
        "遗留负模式应回收入库: {outcome:?}"
    );
    assert!(!source.is_empty(), "正源库应非空");

    // ---- 阶段 3：正源库可被检索（回收材料可复用）----
    let query_schema = ontology::abstract_pattern(vec![e(1, 0.05), e(1, 0.05)]);
    let hit = Searcher::search(source.library(), &query_schema);
    assert!(hit.is_some(), "回收的基础元素应能被再次检索到");

    // ---- 阶段 4：配额可被唤醒（未被杀死，可重新服役）----
    quota.wake();
    assert!(!quota.is_dormant());
    assert_eq!(quota.charge(1, 1), QuotaStatus::Active);
}

#[test]
fn weave_reuses_ontology_decompose_pipeline() {
    // 编织器内部走的正是 ontology::decompose 的产物，验证无交叉依赖裂缝
    let p = Pattern::new(vec![e(6, 0.8), e(2, 0.4), e(2, 0.6)]);
    let dec = ontology::decompose(&p, 3);
    assert!(dec.iter().all(|x| x.level <= 3));
    let schema = ontology::abstract_pattern(dec);
    assert!(!schema.nodes.is_empty());
    // 重组到不同领域仍合法
    let recomposed = ontology::recompose(&schema, ontology::Domain::Wave);
    for el in &recomposed.elements {
        assert!((0.0..=1.0).contains(&el.intensity));
    }
}
