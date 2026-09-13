//! Lane-scoped dedicated threads for long-running tasks.
//!
//! The counterpart of [`crate::parallel::JobSystem`]: frame jobs must never be
//! blocked by a long task, so long tasks run on threads that are **dedicated to
//! a named lane** and never take part in frame job execution. The lane thread
//! budget is the "reserved" part of the worker budget (`JobConfig::reserve`).

use std::any::Any;
use std::collections::VecDeque;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const WAIT_POLL: Duration = Duration::from_millis(10);

/// Specification of one lane: a name plus its dedicated thread count.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LaneSpec {
    /// Human-readable lane name (used in thread names).
    pub name: &'static str,
    /// Number of threads dedicated to this lane.
    pub threads: u32,
}

/// Index of a lane inside a [`ThreadPool`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LaneId(u32);

impl LaneId {
    /// Raw lane index.
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

struct LongTask {
    done: Mutex<bool>,
    cv: Condvar,
    panic: Mutex<Option<Box<dyn Any + Send>>>,
    body: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}

impl LongTask {
    fn new(body: Box<dyn FnOnce() + Send>) -> Arc<Self> {
        Arc::new(Self {
            done: Mutex::new(false),
            cv: Condvar::new(),
            panic: Mutex::new(None),
            body: Mutex::new(Some(body)),
        })
    }

    fn run(&self) {
        let body = self.body.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(body) = body
            && let Err(payload) = catch_unwind(AssertUnwindSafe(body))
        {
            let mut stored = self.panic.lock().unwrap_or_else(|e| e.into_inner());
            if stored.is_none() {
                *stored = Some(payload);
            }
        }
        self.finish();
    }

    /// Marks the task finished (also used to cancel queued tasks on shutdown).
    fn finish(&self) {
        *self.done.lock().unwrap_or_else(|e| e.into_inner()) = true;
        self.cv.notify_all();
    }

    fn is_finished(&self) -> bool {
        *self.done.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn wait_collect(&self) -> Option<Box<dyn Any + Send>> {
        let mut done = self.done.lock().unwrap_or_else(|e| e.into_inner());
        while !*done {
            done = self.cv.wait(done).unwrap_or_else(|e| e.into_inner());
        }
        drop(done);
        self.panic.lock().unwrap_or_else(|e| e.into_inner()).take()
    }
}

struct LaneShared {
    queue: Mutex<VecDeque<Arc<LongTask>>>,
    cv: Condvar,
    shutdown: AtomicBool,
    permits: Mutex<u32>,
    permit_cv: Condvar,
}

struct Lane {
    spec: LaneSpec,
    shared: Arc<LaneShared>,
    threads: Mutex<Vec<JoinHandle<()>>>,
}

/// A pool of named lanes, each owning dedicated threads for long tasks.
///
/// Dropping the pool joins every lane thread, so it waits for in-flight long
/// tasks to finish; tasks still queued at that point are cancelled and their
/// handles report completion without running their payload.
pub struct ThreadPool {
    lanes: Vec<Lane>,
}

impl ThreadPool {
    /// Starts one dedicated thread set per lane specification.
    pub fn new(specs: &[LaneSpec]) -> Self {
        let mut lanes = Vec::with_capacity(specs.len());
        for spec in specs {
            let shared = Arc::new(LaneShared {
                queue: Mutex::new(VecDeque::new()),
                cv: Condvar::new(),
                shutdown: AtomicBool::new(false),
                permits: Mutex::new(spec.threads),
                permit_cv: Condvar::new(),
            });
            let mut threads = Vec::with_capacity(spec.threads as usize);
            for index in 0..spec.threads {
                let worker = Arc::clone(&shared);
                let name = format!("xengine-{}-{index}", spec.name);
                let handle = thread::Builder::new()
                    .name(name)
                    .spawn(move || lane_worker(worker))
                    .expect("spawn lane thread");
                threads.push(handle);
            }
            lanes.push(Lane {
                spec: *spec,
                shared,
                threads: Mutex::new(threads),
            });
        }
        Self { lanes }
    }

    /// Number of lanes.
    pub fn lane_count(&self) -> usize {
        self.lanes.len()
    }

    /// Threads dedicated to a lane.
    pub fn lane_threads(&self, lane: LaneId) -> u32 {
        self.lane(lane).spec.threads
    }

    /// Lane name.
    pub fn lane_name(&self, lane: LaneId) -> &'static str {
        self.lane(lane).spec.name
    }

    /// Looks a lane up by name.
    pub fn lane_id(&self, name: &str) -> Option<LaneId> {
        self.lanes
            .iter()
            .position(|lane| lane.spec.name == name)
            .map(|index| LaneId(index as u32))
    }

    /// Lane at a positional index.
    pub fn lane_id_at(&self, index: usize) -> Option<LaneId> {
        (index < self.lanes.len()).then_some(LaneId(index as u32))
    }

    /// Submits a long-running task to a lane. The task may outlive the frame
    /// and is executed by the lane's dedicated threads only.
    pub fn spawn_long(
        &self,
        lane: LaneId,
        name: &'static str,
        f: impl FnOnce() + Send + 'static,
    ) -> LongTaskHandle {
        let task = LongTask::new(Box::new(f));
        let shared = Arc::clone(&self.lane(lane).shared);
        {
            let mut queue = shared.queue.lock().unwrap_or_else(|e| e.into_inner());
            queue.push_back(Arc::clone(&task));
        }
        shared.cv.notify_one();
        LongTaskHandle { task, name }
    }

    /// Borrows one dedicated thread from a lane for the whole duration of a
    /// long task. Blocks while every thread of the lane is already borrowed;
    /// the thread is returned when the guard is dropped.
    pub fn acquire(&self, lane: LaneId) -> OwnedThread<'_> {
        let name = self.lane_name(lane);
        let shared = &self.lane(lane).shared;
        let mut permits = shared.permits.lock().unwrap_or_else(|e| e.into_inner());
        while *permits == 0 && !shared.shutdown.load(Ordering::Acquire) {
            permits = shared
                .permit_cv
                .wait(permits)
                .unwrap_or_else(|e| e.into_inner());
        }
        assert!(
            !shared.shutdown.load(Ordering::Acquire),
            "xengine thread pool: lane '{name}' is shut down"
        );
        *permits -= 1;
        OwnedThread {
            shared: Arc::clone(shared),
            lane,
            _pool: std::marker::PhantomData,
        }
    }

    fn lane(&self, lane: LaneId) -> &Lane {
        &self.lanes[lane.index()]
    }
}

impl Drop for ThreadPool {
    fn drop(&mut self) {
        for lane in &self.lanes {
            lane.shared.shutdown.store(true, Ordering::Release);
            // Cancel queued tasks so handles never wait forever.
            let queued: Vec<Arc<LongTask>> = {
                let mut queue = lane.shared.queue.lock().unwrap_or_else(|e| e.into_inner());
                queue.drain(..).collect()
            };
            for task in queued {
                task.finish();
            }
            lane.shared.cv.notify_all();
            lane.shared.permit_cv.notify_all();
            let threads: Vec<JoinHandle<()>> = {
                let mut threads = lane.threads.lock().unwrap_or_else(|e| e.into_inner());
                std::mem::take(&mut *threads)
            };
            for handle in threads {
                let _ = handle.join();
            }
        }
    }
}

fn lane_worker(shared: Arc<LaneShared>) {
    loop {
        let task = {
            let mut queue = shared.queue.lock().unwrap_or_else(|e| e.into_inner());
            loop {
                if let Some(task) = queue.pop_front() {
                    break Some(task);
                }
                if shared.shutdown.load(Ordering::Acquire) {
                    break None;
                }
                let (guard, _) = shared
                    .cv
                    .wait_timeout(queue, WAIT_POLL)
                    .unwrap_or_else(|e| e.into_inner());
                queue = guard;
            }
        };
        match task {
            Some(task) => task.run(),
            None => return,
        }
    }
}

/// Handle to a long task submitted with [`ThreadPool::spawn_long`].
pub struct LongTaskHandle {
    task: Arc<LongTask>,
    name: &'static str,
}

impl LongTaskHandle {
    /// Whether the task finished (or was cancelled by pool shutdown).
    pub fn is_finished(&self) -> bool {
        self.task.is_finished()
    }

    /// Waits for completion and re-raises a panic payload on this thread.
    pub fn wait(&self) {
        if let Some(payload) = self.task.wait_collect() {
            resume_unwind(payload);
        }
    }

    /// The name the task was submitted with.
    pub fn name(&self) -> &'static str {
        self.name
    }
}

impl std::fmt::Debug for LongTaskHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LongTaskHandle")
            .field("name", &self.name)
            .field("finished", &self.is_finished())
            .finish()
    }
}

/// RAII reservation of one dedicated lane thread.
pub struct OwnedThread<'a> {
    shared: Arc<LaneShared>,
    lane: LaneId,
    // Ties the guard to the pool lifetime.
    _pool: std::marker::PhantomData<&'a ThreadPool>,
}

impl OwnedThread<'_> {
    /// The lane this thread was borrowed from.
    pub fn lane(&self) -> LaneId {
        self.lane
    }

    /// Runs `f` on the reserved lane thread and returns its result.
    pub fn run<R: Send + 'static>(&self, f: impl FnOnce() -> R + Send + 'static) -> R {
        let (tx, rx) = mpsc::channel();
        let task = LongTask::new(Box::new(move || {
            let _ = tx.send(f());
        }));
        {
            let mut queue = self.shared.queue.lock().unwrap_or_else(|e| e.into_inner());
            queue.push_back(Arc::clone(&task));
        }
        self.shared.cv.notify_one();
        if let Some(payload) = task.wait_collect() {
            resume_unwind(payload);
        }
        rx.recv().expect("owned thread result")
    }
}

impl Drop for OwnedThread<'_> {
    fn drop(&mut self) {
        let mut permits = self
            .shared
            .permits
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        *permits += 1;
        self.shared.permit_cv.notify_one();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parallel::{JobConfig, JobSystem};
    use std::panic::AssertUnwindSafe;
    use std::sync::atomic::AtomicUsize;

    #[derive(Debug, PartialEq, Eq)]
    enum Outcome {
        Ok,
        Timeout,
    }

    /// Runs `f` on a helper thread so a pool bug fails the test instead of
    /// hanging the whole suite.
    fn timed<F: FnOnce() + Send + 'static>(ms: u64, f: F) -> Outcome {
        let (tx, rx) = std::sync::mpsc::channel();
        let _ = thread::spawn(move || {
            let _ = catch_unwind(AssertUnwindSafe(f));
            let _ = tx.send(Outcome::Ok);
        });
        rx.recv_timeout(Duration::from_millis(ms))
            .unwrap_or(Outcome::Timeout)
    }

    fn io_pool(threads: u32) -> ThreadPool {
        ThreadPool::new(&[
            LaneSpec {
                name: "io",
                threads,
            },
            LaneSpec {
                name: "compile",
                threads: 1,
            },
        ])
    }

    #[test]
    fn lane_metadata_is_reported() {
        let pool = io_pool(3);
        assert_eq!(pool.lane_count(), 2);
        let io = pool.lane_id("io").expect("io lane");
        let compile = pool.lane_id("compile").expect("compile lane");
        assert_eq!(pool.lane_threads(io), 3);
        assert_eq!(pool.lane_threads(compile), 1);
        assert_eq!(pool.lane_name(io), "io");
        assert_eq!(pool.lane_id("missing"), None);
        assert_eq!(pool.lane_id_at(0), Some(io));
        assert_eq!(pool.lane_id_at(2), None);
    }

    #[test]
    fn spawn_long_runs_on_a_lane_thread() {
        let pool = io_pool(1);
        let lane = pool.lane_id("io").expect("io lane");
        let caller = thread::current().id();
        let observed = Arc::new(Mutex::new(None));
        let target = Arc::clone(&observed);
        let handle = pool.spawn_long(lane, "load", move || {
            *target.lock().unwrap_or_else(|e| e.into_inner()) = Some(thread::current().id());
        });
        handle.wait();
        assert!(handle.is_finished());
        assert_eq!(handle.name(), "load");
        let worker = observed
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .expect("task ran");
        assert_ne!(worker, caller, "long tasks run off the submitting thread");
    }

    #[test]
    fn long_task_panic_propagates_and_pool_survives() {
        let pool = io_pool(1);
        let lane = pool.lane_id("io").expect("io lane");
        let bad = pool.spawn_long(lane, "boom", || panic!("long boom"));
        let result = catch_unwind(AssertUnwindSafe(|| bad.wait()));
        assert!(result.is_err(), "the panic must reach the waiting thread");

        let counter = Arc::new(AtomicUsize::new(0));
        let target = Arc::clone(&counter);
        let good = pool.spawn_long(lane, "after", move || {
            target.fetch_add(1, Ordering::AcqRel);
        });
        good.wait();
        assert_eq!(counter.load(Ordering::Acquire), 1);
    }

    #[test]
    fn acquire_blocks_until_a_permit_is_returned() {
        let pool = Arc::new(io_pool(1));
        let lane = pool.lane_id("io").expect("io lane");
        let guard = pool.acquire(lane);
        let acquired = Arc::new(AtomicBool::new(false));
        let waiter = {
            let pool = Arc::clone(&pool);
            let acquired = Arc::clone(&acquired);
            thread::spawn(move || {
                let _second = pool.acquire(lane);
                acquired.store(true, Ordering::Release);
            })
        };
        thread::sleep(Duration::from_millis(100));
        assert!(
            !acquired.load(Ordering::Acquire),
            "the only permit is still borrowed"
        );
        drop(guard);
        waiter.join().expect("waiter thread");
        assert!(acquired.load(Ordering::Acquire));
    }

    #[test]
    fn owned_thread_runs_work_on_its_lane_thread() {
        let pool = io_pool(1);
        let lane = pool.lane_id("io").expect("io lane");
        let guard = pool.acquire(lane);
        assert_eq!(guard.lane(), lane);
        let caller = thread::current().id();
        let (value, worker) = guard.run(move || (41 + 1, thread::current().id()));
        assert_eq!(value, 42);
        assert_ne!(worker, caller, "the reserved thread runs the closure");
    }

    #[test]
    fn owned_thread_run_propagates_panics() {
        let pool = io_pool(1);
        let lane = pool.lane_id("io").expect("io lane");
        let guard = pool.acquire(lane);
        let result = catch_unwind(AssertUnwindSafe(|| {
            guard.run(|| panic!("owned boom"));
        }));
        assert!(result.is_err(), "the panic must reach the caller");
    }

    #[test]
    fn idle_pool_drop_is_clean() {
        let outcome = timed(4000, || {
            for _ in 0..4 {
                let pool = io_pool(2);
                drop(pool);
            }
        });
        assert_eq!(outcome, Outcome::Ok);
    }

    #[test]
    fn long_tasks_do_not_starve_frame_jobs() {
        let jobs = Arc::new(JobSystem::new(JobConfig {
            hint: 2.0,
            reserve: 0,
            min: 2,
            max: 2,
        }));
        let pool = Arc::new(io_pool(2));
        let lane = pool.lane_id("io").expect("io lane");
        let gate = Arc::new(crate::parallel::JobConditional::new());
        let started = Arc::new(AtomicUsize::new(0));
        for _ in 0..2 {
            let gate = Arc::clone(&gate);
            let started = Arc::clone(&started);
            let _ = pool.spawn_long(lane, "block", move || {
                started.fetch_add(1, Ordering::AcqRel);
                gate.wait();
            });
        }
        while started.load(Ordering::Acquire) < 2 {
            thread::sleep(Duration::from_millis(5));
        }

        let chunks = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&chunks);
        let runner = Arc::clone(&jobs);
        let outcome = timed(4000, move || {
            runner.parallel_for(0..1024, 64, move |_start, _end| {
                counter.fetch_add(1, Ordering::AcqRel);
            });
        });
        assert_eq!(outcome, Outcome::Ok, "frame jobs must not wait for lanes");
        assert_eq!(chunks.load(Ordering::Acquire), 16);
        gate.notify_all();
        drop(pool);
    }
}
