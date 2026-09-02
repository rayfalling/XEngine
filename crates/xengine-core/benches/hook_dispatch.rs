//! Spike benchmark: hook-dispatch safety vs performance comparison.
//!
//! Answers: how expensive is each dispatch implementation for component
//! lifecycle hooks, and does the `SceneHandle` (DerefMut wrapper) cost
//! anything at all?
//!
//! - A. fn-pointer bridge (current design: `fn(*mut u8, *mut ())`)
//! - B. trait-object dispatch (type-safe callback registry style)
//! - C. TypeId-keyed table lookup, then fn-pointer call (typed-registry style)
//! - D. DerefMut wrapper vs direct reference access (SceneHandle cost check)

use std::collections::HashMap;
use std::time::Instant;

const N: usize = 1_000_000;

// ── A: type-erased fn-pointer bridge (current) ───────────────────────────

type HookFn = fn(*mut u8, *mut ());
fn noop_hook(data: *mut u8, ctx: *mut ()) {
    std::hint::black_box(data);
    std::hint::black_box(ctx);
}

// ── B: trait-object dispatch (type-safe callback registry alternative) ───

struct DynHook {
    f: Box<dyn FnMut(*mut u8, *mut ())>,
}
fn dyn_noop(data: *mut u8, ctx: *mut ()) {
    std::hint::black_box(data);
    std::hint::black_box(ctx);
}

// ── C: TypeId-keyed table + fn-pointer call (typed dispatch path) ────────

type Table = HashMap<std::any::TypeId, HookFn>;

// ── D: DerefMut wrapper vs direct reference (SceneHandle cost probe) ─────

use std::ops::DerefMut;

struct Wrap(Box<u64>);
impl Wrap {
    fn new() -> Self {
        Wrap(Box::new(0))
    }
}
impl std::ops::Deref for Wrap {
    type Target = u64;
    fn deref(&self) -> &u64 {
        &self.0
    }
}
impl DerefMut for Wrap {
    fn deref_mut(&mut self) -> &mut u64 {
        &mut self.0
    }
}

fn main() {
    println!("hook dispatch comparison (N={N})\n");

    // A: direct fn pointer (current bridge)
    {
        let h: HookFn = noop_hook;
        let mut sum = 0usize;
        let start = Instant::now();
        let data = std::ptr::null_mut();
        let data = std::hint::black_box(data);
        let ctx = std::ptr::null_mut();
        for _ in 0..N {
            h(data, ctx);
            sum = sum.wrapping_add(1);
        }
        let el = start.elapsed();
        println!(
            "A. fn-ptr bridge     {:>7.2} ns/call   [{:.2} ms total]",
            el.as_nanos() as f64 / N as f64,
            el.as_secs_f64() * 1e3
        );
        std::hint::black_box(sum);
    }

    // B: trait-object dispatch
    {
        let mut hook = DynHook {
            f: Box::new(dyn_noop),
        };
        let mut sum = 0usize;
        let start = Instant::now();
        let data = std::ptr::null_mut();
        let data = std::hint::black_box(data);
        let ctx = std::ptr::null_mut();
        for _ in 0..N {
            (hook.f)(data, ctx);
            sum = sum.wrapping_add(1);
        }
        let el = start.elapsed();
        println!(
            "B. trait-object      {:>7.2} ns/call   [{:.2} ms total]",
            el.as_nanos() as f64 / N as f64,
            el.as_secs_f64() * 1e3
        );
        std::hint::black_box(sum);
    }

    // C: TypeId table lookup + call
    {
        let mut table: Table = HashMap::new();
        table.insert(std::any::TypeId::of::<crate::Dummy>(), noop_hook);
        let id = std::any::TypeId::of::<crate::Dummy>();
        let mut sum = 0usize;
        let start = Instant::now();
        let data = std::ptr::null_mut();
        let data = std::hint::black_box(data);
        let ctx = std::ptr::null_mut();
        for _ in 0..N {
            let h = table[&id];
            h(data, ctx);
            sum = sum.wrapping_add(1);
        }
        let el = start.elapsed();
        println!(
            "C. table+fn-ptr      {:>7.2} ns/call   [{:.2} ms total]",
            el.as_nanos() as f64 / N as f64,
            el.as_secs_f64() * 1e3
        );
        std::hint::black_box(sum);
    }

    // D: DerefMut wrapper vs direct reference
    {
        let mut w = Wrap::new();
        let mut sum = 0u64;
        let start = Instant::now();
        for i in 0..N {
            let i = std::hint::black_box(i);
            let r = w.deref_mut().wrapping_add(i as u64);
            *w = r;
            sum = sum.wrapping_add(*w);
        }
        let wrap_el = start.elapsed();

        let mut raw = Box::new(0u64);
        let mut sum2 = 0u64;
        let start2 = Instant::now();
        for i in 0..N {
            let i = std::hint::black_box(i);
            *raw = raw.wrapping_add(i as u64);
            sum2 = sum2.wrapping_add(*raw);
        }
        let raw_el = start2.elapsed();
        println!(
            "D. deref-mut wrap    {:>7.2} ns/op     direct ref {:>7.2} ns/op  (diff {:+.2})",
            wrap_el.as_nanos() as f64 / N as f64,
            raw_el.as_nanos() as f64 / N as f64,
            (wrap_el.as_nanos() as f64 - raw_el.as_nanos() as f64) / N as f64
        );
        std::hint::black_box(sum + sum2);
    }
}

struct Dummy;
