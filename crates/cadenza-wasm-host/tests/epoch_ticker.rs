//! Regression for issue #62: the epoch deadline set by `new_store` is useless
//! unless something advances the engine epoch. `ComponentRuntime` owns a
//! background ticker that calls `Engine::increment_epoch` on a fixed cadence,
//! so a CPU-bound guest traps near the configured `epoch_timeout_ms` instead of
//! running forever.
//!
//! Cadence-vs-resolution tradeoff: the ticker requests a 1ms sleep, but OS
//! thread scheduling wakes it every ~1-15ms, so `epoch_timeout_ms` is an
//! approximate *floor*, not an exact wall-clock deadline. The assertions below
//! use generous slack accordingly — the invariant under test is "an infinite
//! loop terminates", not "it terminates at exactly N ms".

use std::sync::mpsc;
use std::time::{Duration, Instant};

use cadenza_wasm_host::{
    ComponentRuntime, RequestContext, WasmHostError, WasmRuntimeLimits, classify_trap,
};
use wasmtime::component::{Component, Linker};

/// A minimal component whose single export spins forever. Without an engine
/// epoch ticker this never returns; with one it traps via epoch interruption.
const SPIN_FOREVER_COMPONENT: &str = r#"
(component
  (core module $m
    (func (export "run")
      (loop $l (br $l))))
  (core instance $i (instantiate $m))
  (func $run (canon lift (core func $i "run")))
  (export "run" (func $run)))
"#;

#[test]
fn infinite_loop_guest_traps_with_timeout_near_budget() {
    // Small budget so the test is quick: deadline = epoch_timeout_ms ticks,
    // advanced ~1 tick/ms, so the loop should trap in well under a second.
    let limits = WasmRuntimeLimits {
        epoch_timeout_ms: 100,
        ..Default::default()
    };
    let runtime = ComponentRuntime::new(limits).expect("engine init");

    let component =
        Component::new(runtime.engine(), SPIN_FOREVER_COMPONENT).expect("assemble spin component");
    let mut store = runtime.new_store(RequestContext::default());
    let engine = runtime.engine().clone();

    // Run the (potentially non-terminating) guest call off-thread so that a
    // regression — the ticker missing, leaving the epoch frozen — fails this
    // test cleanly via the recv timeout instead of hanging the suite forever.
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let linker = Linker::new(&engine);
        let instance = linker
            .instantiate(&mut store, &component)
            .expect("instantiate spin component");
        let func = instance
            .get_typed_func::<(), ()>(&mut store, "run")
            .expect("typed run export");
        let started = Instant::now();
        let outcome = func.call(&mut store, ());
        let elapsed = started.elapsed();
        let _ = tx.send((outcome.map_err(classify_trap), elapsed));
    });

    // The real trap lands in ~100-300ms; this 10s bound is the deterministic
    // barrier that distinguishes "trapped" from "ran forever (no ticker)".
    match rx.recv_timeout(Duration::from_secs(10)) {
        Ok((Err(WasmHostError::Timeout), elapsed)) => {
            // Trapped via epoch interruption — proves the engine epoch was
            // advanced. Generous upper bound (2s) keeps this well clear of the
            // 10s "ran forever" barrier while tolerating sleep granularity.
            assert!(
                elapsed < Duration::from_secs(2),
                "guest trapped but took {elapsed:?}, far beyond the ~100ms budget",
            );
        }
        Ok((other, elapsed)) => {
            panic!("expected WasmHostError::Timeout, got {other:?} after {elapsed:?}");
        }
        Err(_) => {
            panic!(
                "guest never trapped within 10s: the engine epoch is not being \
                 advanced, so epoch_timeout_ms can never fire (issue #62)",
            );
        }
    }
}
