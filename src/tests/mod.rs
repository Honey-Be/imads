use crate::core::DefaultBundle;
use crate::core::acceptance::AcceptanceEngine;
use crate::core::engine::{Engine, EngineConfig};
use crate::core::poll::DefaultPoll;
use crate::types::{
    Env, MeshGeometry, Smc, Tau, XMesh, XReal, mesh_to_real, quantize_real_to_mesh,
};

fn base_env() -> Env {
    Env {
        run_id: 1,
        config_hash: 2,
        data_snapshot_id: 3,
        rng_master_seed: 4,
    }
}

fn base_cfg() -> EngineConfig {
    EngineConfig::new(
        vec![Tau(100), Tau(10), Tau(1)],
        vec![Smc(16), Smc(64)],
        1.0,
        4,
        1,
        2,
        1,
        3,
        8,
        1,
        None,
        2,
        0.0,
        0.0,
        0.5,
        1e-12,
        1e-12,
        64,
        0.01,
        20,
        0.1,
        0.0,
        0.05,
        4096,
        25,
        0.90,
        0.2,
        2,
        2,
        false,
        true,
        Some(4),
        32,
        1,
        256,
        2_000,
        true,
    )
    .unwrap()
}

#[test]
fn determinism_across_workers_when_budget_fixed() {
    let env = base_env();
    let cfg = base_cfg();

    let mut e1 = Engine::<DefaultBundle>::default();
    let out1 = e1.run(&cfg, &env, 1);

    let mut e4 = Engine::<DefaultBundle>::default();
    let out4 = e4.run(&cfg, &env, 4);

    assert_eq!(out1.f_best, out4.f_best);
    assert_eq!(out1.x_best, out4.x_best);
}

#[test]
fn cache_consistency_second_run_hits_cache() {
    let env = base_env();
    let cfg = base_cfg();

    let mut engine = Engine::<DefaultBundle>::default();
    let out1 = engine.run(&cfg, &env, 2);
    let out2 = engine.run(&cfg, &env, 2);

    assert_eq!(out1.f_best, out2.f_best);
    assert_eq!(out1.x_best, out2.x_best);

    // Second run should benefit from cache.
    assert!(out2.stats.truth_eval_cache_hits + out2.stats.truth_decision_cache_hits > 0);
}

// A custom scheduler example that picks candidates in reverse order.
#[derive(Default)]
struct ReverseScheduler {
    workers: usize,
}

impl crate::policies::SchedulerPolicy for ReverseScheduler {
    fn configure(&mut self, workers: usize) {
        self.workers = workers.max(1);
    }

    fn batch_size(&self) -> usize {
        self.workers
    }

    fn select_next(
        &mut self,
        ready_view: &[crate::types::ReadyCandidateView],
    ) -> Vec<crate::types::CandidateId> {
        ready_view
            .iter()
            .rev()
            .take(self.batch_size())
            .map(|v| v.id)
            .collect()
    }

    fn on_complete(&mut self, _id: crate::types::CandidateId, _result: &crate::types::JobResult) {}

    fn should_cancel_inflight(&self, _new_incumbent: bool) -> crate::policies::CancelPolicy {
        crate::policies::CancelPolicy::Never
    }
}

#[test]
fn custom_scheduler_bundle_compiles_and_runs() {
    let env = base_env();
    let cfg = base_cfg();

    let mut eng = crate::core::engine::Engine::<
        crate::core::engine::CustomSchedulerBundle<ReverseScheduler>,
    >::with_custom_scheduler();
    let out = eng.run(&cfg, &env, 3);
    assert!(out.stats.truth_evals > 0);
}

#[test]
fn quantization_respects_mesh_mul_and_mapping_error_bound() {
    let geo = MeshGeometry {
        base_step: 0.5,
        mesh_mul: 4,
        mesh_mul_min: 1,
        refine_div: 2,
        poll_step_mult: 1,
    };

    let x = XReal::new(vec![1.1, -2.7].into_iter()).unwrap();
    let q = quantize_real_to_mesh(&x, &geo);

    // Coordinates must be multiples of mesh_mul in base-lattice units.
    for &u in &q.0 {
        assert_eq!(u % geo.mesh_mul, 0);
    }

    // Mapping back to continuous space stays within half a mesh cell per dimension.
    let xq = mesh_to_real(&q, geo.base_step).unwrap();
    let max_err = geo.current_step() * 0.5 + 1e-12;
    for (&a, &b) in x.0.iter().zip(xq.0.iter()) {
        assert!((f64::from(a) - f64::from(b)).abs() <= max_err);
    }
}

#[test]
fn nested_mesh_property_holds_for_refinement() {
    let coarse = MeshGeometry {
        base_step: 1.0,
        mesh_mul: 4,
        mesh_mul_min: 1,
        refine_div: 2,
        poll_step_mult: 1,
    };
    let fine = MeshGeometry {
        mesh_mul: 2,
        ..coarse.clone()
    };

    // Pick a point that is exactly on the coarse mesh.
    let x_coarse = XMesh(vec![8, -4, 0]);
    let xr = mesh_to_real(&x_coarse, coarse.base_step).unwrap();

    // Quantizing onto the finer mesh must keep the same canonical base-lattice coordinate.
    let x_fine_q = quantize_real_to_mesh(&xr, &fine);
    assert_eq!(x_fine_q, x_coarse);
}

#[test]
fn mesh_refine_is_integer_only_and_monotone() {
    let mut geo = MeshGeometry {
        base_step: 1.0,
        mesh_mul: 8,
        mesh_mul_min: 2,
        refine_div: 2,
        poll_step_mult: 1,
    };

    geo.refine();
    assert_eq!(geo.mesh_mul, 4);

    geo.refine();
    assert_eq!(geo.mesh_mul, 2);

    // Should clamp at mesh_mul_min.
    geo.refine();
    assert_eq!(geo.mesh_mul, 2);
}

#[test]
fn poll_generates_axis_points_with_step() {
    let center = XMesh(vec![10, -5, 0]);
    let pts = DefaultPoll::generate_points(&center, 3);
    assert_eq!(pts.len(), 6);
    // Each point differs from center in exactly one coordinate by ±step.
    for p in pts {
        let mut diffs = 0;
        for (a, b) in p.0.iter().zip(center.0.iter()) {
            let d = a - b;
            if d != 0 {
                diffs += 1;
                assert!(d == 3 || d == -3);
            }
        }
        assert_eq!(diffs, 1);
    }
}

#[test]
fn acceptance_filter_and_barrier_behave() {
    use crate::core::acceptance::{AcceptanceConfig, DefaultAcceptance, TruthDecision};

    let cfg = AcceptanceConfig {
        h0: 10.0,
        h_min: 0.0,
        h_shrink: 0.5,
        eps_f: 1e-9,
        eps_v: 1e-9,
        filter_cap: 64,
    };
    let mut acc = DefaultAcceptance::new(cfg);

    let x = XMesh(vec![0, 0]);

    assert_eq!(acc.decide_truth(&x, 10.0, 1.0), TruthDecision::Accept);
    assert_eq!(acc.decide_truth(&x, 9.0, 2.0), TruthDecision::Accept);

    // Dominated by (10,1): worse f and not better v.
    assert_eq!(acc.decide_truth(&x, 11.0, 1.0), TruthDecision::Reject);

    // Dominates others: should accept and shrink filter set.
    assert_eq!(acc.decide_truth(&x, 8.0, 0.5), TruthDecision::Accept);
    assert!(acc.state.filter.points.len() <= 2);

    // Barrier tightens on poll-fail boundary.
    let h_before = acc.state.barrier.h;
    acc.on_iteration_end(true, false);
    assert!(acc.state.barrier.h <= h_before + 1e-12);
}

#[test]
fn dids_assignment_updates_from_history() {
    use crate::policies::calibrator::CalibState;
    use crate::policies::dids::{DefaultDids, DidsPolicy};
    use crate::types::{EnvRev, Estimates, EvalMeta, JobResult, Phi, PolicyRev, Smc, Tau};

    let mut dids = DefaultDids::default();
    dids.init(1);

    let phi = Phi {
        tau: Tau(100),
        smc: Smc(16),
    };
    let meta = EvalMeta {
        phi,
        env_rev: EnvRev(0),
        policy_rev: PolicyRev(0),
        runtime_cost: 0.0,
    };
    let est = Estimates {
        f_hat: 0.0,
        f_se: 0.0,
        c_hat: vec![1.0],
        c_se: vec![0.0],
        tau_scale: 1.0,
    };

    for k in 0..10 {
        let x = XMesh(vec![k]);
        let jr = JobResult::EarlyInfeasible {
            violated_j: 0,
            estimates: est.clone(),
            meta: meta.clone(),
        };
        dids.record(x, phi, 0, &jr);
    }

    let calib = CalibState {
        delta_rel: vec![0.005],
        ..Default::default()
    };

    let (a, delta) = dids.update_assignment(5, &calib);
    assert_eq!(a.len(), 1);
    // Conservative rule: with small sample size, level 1 becomes 2.
    assert_eq!(a[0], 2);
    assert_eq!(delta.0, 1);
}

#[test]
fn dids_assignment_increases_when_delta_high() {
    use crate::policies::calibrator::CalibState;
    use crate::policies::dids::{DefaultDids, DidsPolicy};
    use crate::types::{EnvRev, Estimates, EvalMeta, JobResult, Phi, PolicyRev, Smc, Tau, XMesh};

    let phi = Phi {
        tau: Tau(100),
        smc: Smc(16),
    };
    let meta = EvalMeta {
        phi,
        env_rev: EnvRev(0),
        policy_rev: PolicyRev(0),
        runtime_cost: 0.0,
    };
    let est = Estimates {
        f_hat: 0.0,
        f_se: 0.0,
        c_hat: vec![1.0],
        c_se: vec![0.0],
        tau_scale: 1.0,
    };

    let mut dids_low = DefaultDids::default();
    dids_low.init(1);
    for k in 0..10 {
        let x = XMesh(vec![k]);
        let jr = JobResult::EarlyInfeasible {
            violated_j: 0,
            estimates: est.clone(),
            meta: meta.clone(),
        };
        dids_low.record(x, phi, 0, &jr);
    }
    let calib_low = CalibState {
        delta_rel: vec![0.005],
        ..Default::default()
    };
    let (a_low, _) = dids_low.update_assignment(5, &calib_low);

    let mut dids_high = DefaultDids::default();
    dids_high.init(1);
    for k in 0..10 {
        let x = XMesh(vec![k]);
        let jr = JobResult::EarlyInfeasible {
            violated_j: 0,
            estimates: est.clone(),
            meta: meta.clone(),
        };
        dids_high.record(x, phi, 0, &jr);
    }
    let calib_high = CalibState {
        delta_rel: vec![0.02],
        ..Default::default()
    };
    let (a_high, _) = dids_high.update_assignment(5, &calib_high);

    assert_eq!(a_low.len(), 1);
    assert_eq!(a_high.len(), 1);
    assert!(a_high[0] >= a_low[0]);
}

#[test]
fn delta_calibrator_increases_delta_on_false_infeasible() {
    use crate::policies::calibrator::{AuditOf, CalibEvent, CalibratorPolicy, DeltaCalibrator};
    use crate::types::{
        CandidateId, EnvRev, Estimates, EvalMeta, JobResult, Phi, PolicyRev, Smc, Tau,
    };

    let mut cal = DeltaCalibrator::default();
    cal.init(1);

    let phi = Phi {
        tau: Tau(100),
        smc: Smc(16),
    };
    let meta = EvalMeta {
        phi,
        env_rev: EnvRev(0),
        policy_rev: PolicyRev(0),
        runtime_cost: 0.0,
    };
    let est = Estimates {
        f_hat: 0.0,
        f_se: 0.0,
        c_hat: vec![1.0],
        c_se: vec![0.0],
        tau_scale: 1.0,
    };

    let mut events: Vec<CalibEvent> = Vec::new();
    for i in 0..25u64 {
        let id = CandidateId(i);
        // audited early-infeasible
        let jr_e = JobResult::EarlyInfeasible {
            violated_j: 0,
            estimates: est.clone(),
            meta: meta.clone(),
        };
        events.push(CalibEvent {
            id,
            result: jr_e,
            audited: true,
            audit_of: Some(AuditOf {
                violated_j: 0,
                phi_at_cut: phi,
                phi_idx_at_cut: 0,
            }),
            paired_sample: None,
        });
        // later truth says feasible -> false infeasible
        let jr_t = JobResult::Truth {
            f: 0.0,
            c: vec![-1.0],
            feasible: true,
            v: 0.0,
            meta: meta.clone(),
        };
        events.push(CalibEvent {
            id,
            result: jr_t,
            audited: false,
            audit_of: None,
            paired_sample: None,
        });
    }
    events.sort_by_key(|e| e.id);

    let before = cal.state().delta_rel[0];
    let d = cal.update(&events);
    assert!(d.0 >= 1);
    let after = cal.state().delta_rel[0];
    assert!(after > before);
}

#[test]
fn mesh_to_real_returns_err_on_overflow() {
    let x = XMesh(vec![i64::MAX]);
    let res = mesh_to_real(&x, 1e308);
    assert!(res.is_err());
}

#[test]
fn dids_level_bucket_false_signal_pushes_a_higher() {
    use crate::policies::calibrator::CalibState;
    use crate::policies::dids::{DefaultDids, DidsPolicy};
    use crate::types::{EnvRev, Estimates, EvalMeta, JobResult, Phi, PolicyRev, Smc, Tau, XMesh};

    let phi_l1 = Phi {
        tau: Tau(100),
        smc: Smc(16),
    };
    let phi_l2 = Phi {
        tau: Tau(10),
        smc: Smc(64),
    };
    let meta1 = EvalMeta {
        phi: phi_l1,
        env_rev: EnvRev(0),
        policy_rev: PolicyRev(0),
        runtime_cost: 0.0,
    };
    let meta2 = EvalMeta {
        phi: phi_l2,
        env_rev: EnvRev(0),
        policy_rev: PolicyRev(0),
        runtime_cost: 0.0,
    };
    let est = Estimates {
        f_hat: 0.0,
        f_se: 0.0,
        c_hat: vec![1.0],
        c_se: vec![0.0],
        tau_scale: 1.0,
    };

    let mut dids = DefaultDids::default();
    dids.init(1);
    for k in 0..20 {
        let x = XMesh(vec![k]);
        let jr = if k % 2 == 0 {
            JobResult::EarlyInfeasible {
                violated_j: 0,
                estimates: est.clone(),
                meta: meta1.clone(),
            }
        } else {
            JobResult::EarlyInfeasible {
                violated_j: 0,
                estimates: est.clone(),
                meta: meta2.clone(),
            }
        };
        let phi = if k % 2 == 0 { phi_l1 } else { phi_l2 };
        let idx = if k % 2 == 0 { 0 } else { 1 };
        dids.record(x, phi, idx, &jr);
    }

    let calib = CalibState {
        delta_rel: vec![0.005],
        target_false: 0.01,
        min_audits: 5,
        audit_n_by_phi_idx: vec![vec![10, 10]],
        false_infeas_n_by_phi_idx: vec![vec![8, 0]],
        audit_n: vec![20],
        false_infeas_n: vec![8],
        ..Default::default()
    };

    let (a, _) = dids.update_assignment(3, &calib);
    assert_eq!(a.len(), 1);
    assert!(a[0] >= 2);
}

#[test]
fn engine_config_validates_k_params_regression() {
    use crate::core::engine::{ConfigError, EngineConfig};
    use crate::types::{Smc, Tau};

    let err = EngineConfig::new(
        vec![Tau(10)],
        vec![Smc(16)],
        1.0,
        4,
        1,
        2,
        1,
        2,
        4,
        2,
        Some(4),
        1,
        0.0,
        0.0,
        0.5,
        1e-12,
        1e-12,
        64,
        0.01,
        20,
        0.1,
        0.0,
        0.05,
        0, // invalid k_window
        25,
        0.90,
        0.2,
        2,
        2,
        false,
        true,
        Some(4),
        32,
        1,
        256,
        2000,
        true,
    )
    .unwrap_err();
    assert!(err.contains(ConfigError::CalibratorKWindow));

    let err = EngineConfig::new(
        vec![Tau(10)],
        vec![Smc(16)],
        1.0,
        4,
        1,
        2,
        1,
        2,
        4,
        2,
        Some(4),
        1,
        0.0,
        0.0,
        0.5,
        1e-12,
        1e-12,
        64,
        0.01,
        20,
        0.1,
        0.0,
        0.05,
        16,
        32, // invalid: min_pairs > window
        0.90,
        0.2,
        2,
        2,
        false,
        true,
        Some(4),
        32,
        1,
        256,
        2000,
        true,
    )
    .unwrap_err();
    assert!(err.contains(ConfigError::CalibratorKMinPairs));

    let err = EngineConfig::new(
        vec![Tau(10)],
        vec![Smc(16)],
        1.0,
        4,
        1,
        2,
        1,
        2,
        4,
        2,
        Some(4),
        1,
        0.0,
        0.0,
        0.5,
        1e-12,
        1e-12,
        64,
        0.01,
        20,
        0.1,
        0.0,
        0.05,
        16,
        8,
        1.1, // invalid quantile
        0.2,
        2,
        2,
        false,
        true,
        Some(4),
        32,
        1,
        256,
        2000,
        true,
    )
    .unwrap_err();
    assert!(err.contains(ConfigError::CalibratorKQuantile));
}

#[test]
fn objective_pruning_gate_is_ladder_smc_aware_regression() {
    use crate::core::engine::objective_pruning_allowed;
    use crate::types::{Phi, Smc, Tau};

    let ladder = vec![
        Phi {
            tau: Tau(100),
            smc: Smc(8),
        },
        Phi {
            tau: Tau(100),
            smc: Smc(32),
        },
        Phi {
            tau: Tau(10),
            smc: Smc(32),
        },
        Phi {
            tau: Tau(1),
            smc: Smc(128),
        },
    ];

    let mut cfg = base_cfg();
    cfg.objective_prune_min_smc_rank = 2;
    cfg.objective_prune_min_level = 2;
    cfg.objective_prune_require_back_half = false;
    cfg.objective_prune_disable_for_audit = true;

    assert!(!objective_pruning_allowed(0, &ladder, false, &cfg));
    assert!(objective_pruning_allowed(1, &ladder, false, &cfg));
    assert!(objective_pruning_allowed(3, &ladder, false, &cfg));
    assert!(!objective_pruning_allowed(1, &ladder, true, &cfg));

    cfg.objective_prune_require_back_half = true;
    assert!(!objective_pruning_allowed(1, &ladder, false, &cfg));
    assert!(objective_pruning_allowed(3, &ladder, false, &cfg));
}

#[derive(Debug)]
struct NonFiniteSampleEvaluator;

impl crate::core::evaluator::Evaluator for NonFiniteSampleEvaluator {
    fn cheap_constraints(&self, _x: &XReal, _env: &Env) -> bool {
        true
    }
    fn mc_sample(
        &self,
        _x: &XReal,
        _phi: crate::types::Phi,
        _env: &Env,
        _k: u32,
    ) -> (f64, Vec<f64>) {
        (f64::NAN, vec![0.0])
    }
    fn solver_bias(&self, _x: &XReal, _tau: Tau, _env: &Env) -> (f64, Vec<f64>) {
        (0.0, vec![0.0])
    }
    fn num_constraints(&self) -> usize {
        1
    }
}

#[test]
fn evaluator_non_finite_outputs_are_rejected_without_panic_regression() {
    use std::sync::Arc;

    let env = base_env();
    let mut cfg = base_cfg();
    cfg.num_constraints = 1;
    cfg.max_iters = 2;
    cfg.max_steps_per_iter = Some(4);

    let mut engine = Engine::<DefaultBundle>::default();
    let out = engine.run_with_evaluator(&cfg, &env, 1, Arc::new(NonFiniteSampleEvaluator));
    assert!(out.f_best.is_none());
    assert_eq!(out.stats.cheap_rejects, 0);
    assert!(out.stats.invalid_eval_rejects > 0);
}

#[test]
fn mesh_to_real_overflow_is_non_panicking_regression() {
    let x = XMesh(vec![i64::MAX]);
    let res = mesh_to_real(&x, 1e308);
    assert!(res.is_err());
}

#[test]
fn presets_validate_and_are_distinct() {
    use crate::presets::Preset;

    assert_eq!(Preset::ALL.len(), 4);
    let names: Vec<_> = Preset::ALL.iter().map(|p| p.name()).collect();
    let mut sorted = names.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), names.len());

    for p in Preset::ALL {
        let cfg = p.config();
        assert!(cfg.search_dim >= 1);
        assert!(cfg.mesh_base_step.is_finite() && cfg.mesh_base_step > 0.0);
    }
}

#[test]
fn delta_k_calibrator_updates_k_from_paired_audit() {
    use crate::policies::calibrator::{
        AuditOf, CalibEvent, CalibratorConfig, CalibratorPolicy, DeltaKCalibrator,
        PairedAuditSample,
    };
    use crate::types::{
        CandidateId, EnvRev, Estimates, EvalMeta, JobResult, Phi, PolicyRev, Smc, Tau,
    };

    let mut cal = DeltaKCalibrator::default();
    cal.init(1);
    cal.configure(&CalibratorConfig {
        target_false: 0.01,
        min_audits: 1,
        eta_delta: 0.1,
        delta_min: 0.0,
        delta_max: 0.05,
        k_window: 32,
        k_min_pairs: 1,
        k_quantile: 0.90,
        k_eta: 0.5,
    });

    let cut_phi = Phi {
        tau: Tau(100),
        smc: Smc(16),
    };
    let pair_phi = Phi {
        tau: Tau(10),
        smc: Smc(16),
    };
    let meta_cut = EvalMeta {
        phi: cut_phi,
        env_rev: EnvRev(0),
        policy_rev: PolicyRev(0),
        runtime_cost: 0.0,
    };
    let meta_truth = EvalMeta {
        phi: Phi {
            tau: Tau(1),
            smc: Smc(64),
        },
        env_rev: EnvRev(0),
        policy_rev: PolicyRev(0),
        runtime_cost: 0.0,
    };

    let cut_est = Estimates {
        f_hat: 10.0,
        f_se: 0.0,
        c_hat: vec![5.0],
        c_se: vec![0.0],
        tau_scale: 100.0,
    };
    let pair_est = Estimates {
        f_hat: 9.0,
        f_se: 0.0,
        c_hat: vec![1.0],
        c_se: vec![0.0],
        tau_scale: 10.0,
    };

    let events = vec![
        CalibEvent {
            id: CandidateId(1),
            result: JobResult::EarlyInfeasible {
                violated_j: 0,
                estimates: cut_est.clone(),
                meta: meta_cut.clone(),
            },
            audited: true,
            audit_of: Some(AuditOf {
                violated_j: 0,
                phi_at_cut: cut_phi,
                phi_idx_at_cut: 0,
            }),
            paired_sample: Some(PairedAuditSample {
                paired_phi: pair_phi,
                paired_phi_idx: 1,
                estimates: pair_est.clone(),
            }),
        },
        CalibEvent {
            id: CandidateId(1),
            result: JobResult::Truth {
                f: 0.0,
                c: vec![-1.0],
                feasible: true,
                v: 0.0,
                meta: meta_truth,
            },
            audited: false,
            audit_of: None,
            paired_sample: None,
        },
    ];

    let _ = cal.update(&events);
    let st = cal.state();
    assert!(st.k_f > 0.0 || st.k_c[0] > 0.0);
}

#[test]
fn engine_config_validates_objective_pruning_params() {
    use crate::core::engine::{ConfigError, EngineConfig};
    use crate::types::{Smc, Tau};

    let err = EngineConfig::new(
        vec![Tau(10)],
        vec![Smc(16)],
        1.0,
        4,
        1,
        2,
        1,
        2,
        4,
        2,
        Some(4),
        1,
        0.0,
        0.0,
        0.5,
        1e-12,
        1e-12,
        64,
        0.01,
        20,
        0.1,
        0.0,
        0.05,
        4096,
        25,
        0.90,
        0.2,
        0,
        0,
        false,
        true,
        Some(4),
        32,
        1,
        256,
        2000,
        true,
    )
    .unwrap_err();
    assert!(err.contains(ConfigError::ObjectivePruneMinSmcRank));
    assert!(err.contains(ConfigError::ObjectivePruneMinLevel));
}
