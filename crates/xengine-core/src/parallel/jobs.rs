//! Zero-dependency job system: frame task queue, worker pool, scoped
//! `parallel_for`.
//!
//! Design notes (see `openspec/changes/core-parallel-jobs/design.md`):
//! - The frame queue is a **frame-scoped** queue: [`JobSystem::begin_frame`] /
//!   [`JobSystem::end_frame`] bracket a frame and `end_frame` is a barrier.
//! - Execution categories mirror NeoX `xjobsystem`:
//!   [`JobCategory::Compute`] (worker pool), [`JobCategory::Current`] (inline on
//!   the calling thread) and [`JobCategory::Main`] (owner-thread local queue
//!   that may carry `!Send` payloads).
//! - Waits are *helping*: a waiting thread executes available work instead of
//!   idling, exactly like NeoX's `wait` vs `blocked_wait` split.
//! - Rust-specific gap NeoX does not have: a panicking worker is caught and its
//!   payload is re-raised on the waiting thread, so a panic can never leak the
//!   completion counter (which would deadlock every waiter).

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::ops::Range;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, Weak};
use std::thread::{self, JoinHandle, ThreadId};
use std::time::{Duration, Instant};

/// Default chunk size used by `parallel_for` when `grain_size == 0`.
pub const DEFAULT_GRAIN_SIZE: u32 = 64;

/// Recursion cap for helping waits (mirrors NeoX `XJOB_MAX_RECURSIVE_WAIT_LEVEL`).
const MAX_HELP_DEPTH: u32 = 64;

/// Poll interval for blocking completion waits.
const WAIT_POLL: Duration = Duration::from_millis(10);

/// Inline capacity of [`JobGroup`] (no heap allocation below this size).
const GROUP_INLINE: usize = 4;

static NEXT_SYSTEM_ID: AtomicU64 = AtomicU64::new(1);

thread_local! {
    /// `(system id, slot index, slot generation)` of the job running on this
    /// thread, used for self-wait detection and `current_job()`.
    static CURRENT_JOB: Cell<Option<(u64, u32, u32)>> = const { Cell::new(None) };
    /// Re-entrancy guard for helping waits.
    static HELP_DEPTH: Cell<u32> = const { Cell::new(0) };
    /// Owner-thread main-job queues, keyed by system id. Payloads here may be
    /// `!Send`, which is exactly why they never leave the owning thread.
    static MAIN_QUEUES: RefCell<HashMap<u64, VecDeque<MainJob>>> = RefCell::new(HashMap::new());
}

// ── configuration ───────────────────────────────────────────────────────────

/// Thread budget for a [`JobSystem`], aligned with NeoX `InitializeParam`.
///
/// `hint > 1.0` is a thread count, `hint <= 1.0` is a fraction of the hardware
/// threads (`1.0` = all of them). `reserve` is subtracted from that result and
/// stays available for dedicated long-task threads (see
/// [`crate::parallel::ThreadPool`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JobConfig {
    /// Requested worker count (`> 1.0`) or hardware-thread fraction (`<= 1.0`).
    pub hint: f32,
    /// Threads carved out of the worker budget for dedicated long tasks.
    pub reserve: u32,
    /// Lower bound for the resulting worker count.
    pub min: u32,
    /// Upper bound for the resulting worker count.
    pub max: u32,
}

impl Default for JobConfig {
    fn default() -> Self {
        Self {
            hint: 1.0,
            reserve: 2,
            min: 1,
            max: 8,
        }
    }
}

/// Resolves the worker count for a config: `clamp(f(hint) - reserve, min, max)`,
/// never below 1.
pub fn worker_count_for(config: &JobConfig) -> usize {
    let hardware = thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1) as f32;
    let wanted = if config.hint > 1.0 {
        config.hint
    } else {
        config.hint.max(0.0) * hardware
    };
    let min = config.min.max(1) as f32;
    let max = config.max.max(config.min.max(1)) as f32;
    (wanted - config.reserve as f32).clamp(min, max) as usize
}

/// Non-preemptive job priority. `High` is taken before `Normal` before `Low`;
/// equal priorities keep submission order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum JobPriority {
    /// Taken first.
    High,
    /// Default priority.
    Normal,
    /// Taken last.
    Low,
}

impl JobPriority {
    const COUNT: usize = 3;

    fn index(self) -> usize {
        match self {
            Self::High => 0,
            Self::Normal => 1,
            Self::Low => 2,
        }
    }
}

/// Where a job is allowed to run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobCategory {
    /// Worker pool.
    Compute,
    /// Inline on the submitting thread.
    Current,
    /// Owner thread only ([`JobSystem::spawn_main`]); may carry `!Send` data.
    Main,
}

/// Per-frame statistics returned by [`JobSystem::end_frame`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameStats {
    /// Frame index passed to `begin_frame`.
    pub frame: u64,
    /// Jobs submitted during this frame.
    pub jobs: usize,
    /// Jobs completed during this frame.
    pub completed: usize,
    /// Wall time spent in the frame barrier.
    pub duration: Duration,
}

// ── internal state ──────────────────────────────────────────────────────────

/// Visibility of a job slot for a `(index, generation)` handle pair.
#[derive(Clone, Copy, PartialEq, Eq)]
enum JobState {
    Pending,
    Done,
    /// The slot was recycled: the job finished in a previous frame.
    Expired,
}

struct MainJob {
    index: u32,
    generation: u32,
    f: Box<dyn FnOnce()>,
}

struct JobRecord {
    generation: u32,
    name: &'static str,
    live: bool,
    done: AtomicBool,
    payload: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    panic: Mutex<Option<Box<dyn Any + Send>>>,
}

/// Shared context handed to every chunk of one scoped `parallel_for` call.
///
/// The payload is a trait object rather than a generic parameter on purpose:
/// the lifetime erasure below must not force the caller's closure type to be
/// `'static` (closure types that capture borrows never are).
struct ScopedCtx<'a> {
    f: &'a (dyn Fn(u32, u32) + Send + Sync + 'a),
    end: usize,
    grain: usize,
    cursor: AtomicUsize,
}

fn claim_loop(ctx: &ScopedCtx<'_>) {
    loop {
        let start = ctx.cursor.fetch_add(ctx.grain, Ordering::Relaxed);
        if start >= ctx.end {
            return;
        }
        let end = (start + ctx.grain).min(ctx.end);
        (ctx.f)(start as u32, end as u32);
    }
}

struct Sched {
    slots: Vec<JobRecord>,
    free: Vec<u32>,
    queues: [VecDeque<u32>; JobPriority::COUNT],
    pending: usize,
}

impl Sched {
    fn new() -> Self {
        Self {
            slots: Vec::new(),
            free: Vec::new(),
            queues: [VecDeque::new(), VecDeque::new(), VecDeque::new()],
            pending: 0,
        }
    }

    fn alloc(&mut self, name: &'static str) -> (u32, u32) {
        if let Some(index) = self.free.pop() {
            let slot = &mut self.slots[index as usize];
            slot.generation = slot.generation.wrapping_add(1).max(1);
            slot.name = name;
            slot.live = true;
            slot.done.store(false, Ordering::Release);
            *slot.payload.lock().unwrap_or_else(|e| e.into_inner()) = None;
            *slot.panic.lock().unwrap_or_else(|e| e.into_inner()) = None;
            (index, slot.generation)
        } else {
            let index = self.slots.len() as u32;
            self.slots.push(JobRecord {
                generation: 1,
                name,
                live: true,
                done: AtomicBool::new(false),
                payload: Mutex::new(None),
                panic: Mutex::new(None),
            });
            (index, 1)
        }
    }

    fn pop_job(&mut self) -> Option<u32> {
        for queue in &mut self.queues {
            if let Some(index) = queue.pop_front() {
                return Some(index);
            }
        }
        None
    }

    fn state(&self, index: u32, generation: u32) -> JobState {
        match self.slots.get(index as usize) {
            Some(slot) if slot.live && slot.generation == generation => {
                if slot.done.load(Ordering::Acquire) {
                    JobState::Done
                } else {
                    JobState::Pending
                }
            }
            _ => JobState::Expired,
        }
    }

    /// Releases every finished slot back to the free list (called from the end
    /// of a frame barrier, when no frame job can still be running).
    fn recycle(&mut self) {
        for index in 0..self.slots.len() {
            let slot = &self.slots[index];
            if slot.live && slot.done.load(Ordering::Acquire) {
                self.slots[index].live = false;
                *self.slots[index]
                    .payload
                    .lock()
                    .unwrap_or_else(|e| e.into_inner()) = None;
                self.free.push(index as u32);
            }
        }
    }
}

struct JobsInner {
    id: u64,
    owner: ThreadId,
    worker_count: usize,
    sched: Mutex<Sched>,
    cv: Condvar,
    shutdown: AtomicBool,
    workers: Mutex<Vec<JoinHandle<()>>>,
    frame: AtomicU64,
    frame_jobs: AtomicUsize,
    frame_done: AtomicUsize,
}

impl JobsInner {
    fn start(worker_count: usize) -> Arc<Self> {
        let inner = Arc::new(Self {
            id: NEXT_SYSTEM_ID.fetch_add(1, Ordering::Relaxed),
            owner: thread::current().id(),
            worker_count,
            sched: Mutex::new(Sched::new()),
            cv: Condvar::new(),
            shutdown: AtomicBool::new(false),
            workers: Mutex::new(Vec::new()),
            frame: AtomicU64::new(0),
            frame_jobs: AtomicUsize::new(0),
            frame_done: AtomicUsize::new(0),
        });
        let mut handles = Vec::with_capacity(worker_count);
        for index in 0..worker_count {
            let weak = Arc::downgrade(&inner);
            let handle = thread::Builder::new()
                .name(format!("xengine-job-{index}"))
                .spawn(move || worker_loop(weak))
                .expect("spawn job worker thread");
            handles.push(handle);
        }
        *inner.workers.lock().unwrap_or_else(|e| e.into_inner()) = handles;
        inner
    }

    fn lock(&self) -> MutexGuard<'_, Sched> {
        self.sched.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn is_owner_thread(&self) -> bool {
        thread::current().id() == self.owner
    }

    fn state(&self, index: u32, generation: u32) -> JobState {
        self.lock().state(index, generation)
    }

    fn take_panic(&self, index: u32, generation: u32) -> Option<Box<dyn Any + Send>> {
        let sched = self.lock();
        match sched.slots.get(index as usize) {
            Some(slot) if slot.live && slot.generation == generation => {
                slot.panic.lock().unwrap_or_else(|e| e.into_inner()).take()
            }
            _ => None,
        }
    }

    fn job_name(&self, index: u32) -> &'static str {
        let sched = self.lock();
        sched
            .slots
            .get(index as usize)
            .map(|slot| slot.name)
            .unwrap_or("job")
    }

    fn record_panic(&self, index: u32, payload: Box<dyn Any + Send>) {
        let sched = self.lock();
        if let Some(slot) = sched.slots.get(index as usize) {
            let mut stored = slot.panic.lock().unwrap_or_else(|e| e.into_inner());
            if stored.is_none() {
                *stored = Some(payload);
            }
        }
    }

    fn finish_job(&self, index: u32) {
        {
            let mut sched = self.lock();
            if let Some(slot) = sched.slots.get(index as usize)
                && !slot.done.swap(true, Ordering::AcqRel)
            {
                sched.pending = sched.pending.saturating_sub(1);
            }
        }
        self.frame_done.fetch_add(1, Ordering::AcqRel);
        self.cv.notify_all();
    }

    fn run_job_body<F: FnOnce()>(&self, index: u32, generation: u32, body: F) {
        CURRENT_JOB.with(|c| c.set(Some((self.id, index, generation))));
        let result = catch_unwind(AssertUnwindSafe(body));
        CURRENT_JOB.with(|c| c.set(None));
        if let Err(payload) = result {
            self.record_panic(index, payload);
        }
        self.finish_job(index);
    }

    fn run_compute_job(&self, index: u32) {
        let (generation, payload) = {
            let sched = self.lock();
            match sched.slots.get(index as usize) {
                Some(slot) if slot.live => (
                    slot.generation,
                    slot.payload
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .take(),
                ),
                _ => return,
            }
        };
        match payload {
            Some(f) => self.run_job_body(index, generation, f),
            None => self.finish_job(index),
        }
    }

    fn run_main_job(&self, job: MainJob) {
        self.run_job_body(job.index, job.generation, job.f);
    }

    /// Executes one queued main-thread job; returns whether one was available.
    fn run_one_local_inner(&self) -> bool {
        if !self.is_owner_thread() {
            return false;
        }
        let job = MAIN_QUEUES.with(|queues| {
            queues
                .borrow_mut()
                .get_mut(&self.id)
                .and_then(|queue| queue.pop_front())
        });
        match job {
            Some(job) => {
                self.run_main_job(job);
                true
            }
            None => false,
        }
    }

    /// Executes one unit of available work (main queue first, then compute
    /// queue). Returns whether anything ran.
    fn help_once(&self) -> bool {
        if self.run_one_local_inner() {
            return true;
        }
        let job = self.lock().pop_job();
        match job {
            Some(index) => {
                self.run_compute_job(index);
                true
            }
            None => false,
        }
    }

    fn wait_collect(&self, index: u32, generation: u32, help: bool) -> Option<Box<dyn Any + Send>> {
        loop {
            match self.state(index, generation) {
                JobState::Done => return self.take_panic(index, generation),
                JobState::Expired => return None,
                JobState::Pending => {}
            }
            if let Some((system, current, _)) = CURRENT_JOB.with(|c| c.get())
                && system == self.id
                && current == index
            {
                panic!(
                    "xengine job system: self-wait detected on job '{}' (a job may not wait for itself)",
                    self.job_name(index)
                );
            }
            if help {
                let depth = HELP_DEPTH.with(|d| d.get());
                if depth < MAX_HELP_DEPTH {
                    HELP_DEPTH.with(|d| d.set(depth + 1));
                    let progressed = self.help_once();
                    HELP_DEPTH.with(|d| d.set(depth));
                    if progressed {
                        continue;
                    }
                }
            }
            let sched = self.lock();
            if !matches!(sched.state(index, generation), JobState::Pending) {
                continue;
            }
            let _ = self
                .cv
                .wait_timeout(sched, WAIT_POLL)
                .unwrap_or_else(|e| e.into_inner());
        }
    }

    fn spawn(
        self: &Arc<Self>,
        name: &'static str,
        priority: JobPriority,
        f: impl FnOnce() + Send + 'static,
    ) -> JobHandle {
        let (index, generation) = {
            let mut sched = self.lock();
            let (index, generation) = sched.alloc(name);
            {
                let slot = &sched.slots[index as usize];
                *slot.payload.lock().unwrap_or_else(|e| e.into_inner()) = Some(Box::new(f));
            }
            sched.pending += 1;
            sched.queues[priority.index()].push_back(index);
            (index, generation)
        };
        self.frame_jobs.fetch_add(1, Ordering::AcqRel);
        let handle = JobHandle {
            inner: Arc::clone(self),
            index,
            generation,
            name,
        };
        if self.worker_count == 0 {
            self.run_compute_job(index);
        } else {
            self.cv.notify_one();
        }
        handle
    }

    fn spawn_at(
        self: &Arc<Self>,
        category: JobCategory,
        name: &'static str,
        priority: JobPriority,
        f: impl FnOnce() + Send + 'static,
    ) -> JobHandle {
        match category {
            JobCategory::Compute => self.spawn(name, priority, f),
            JobCategory::Current => {
                let (index, generation) = {
                    let mut sched = self.lock();
                    let (index, generation) = sched.alloc(name);
                    sched.pending += 1;
                    (index, generation)
                };
                self.frame_jobs.fetch_add(1, Ordering::AcqRel);
                self.run_job_body(index, generation, f);
                JobHandle {
                    inner: Arc::clone(self),
                    index,
                    generation,
                    name,
                }
            }
            JobCategory::Main => panic!(
                "xengine job system: use spawn_main() for main-thread jobs (they may capture !Send data)"
            ),
        }
    }

    /// Enqueues a main-thread job. The payload may be `!Send`; it is only ever
    /// touched by the owner thread.
    fn spawn_main(self: &Arc<Self>, name: &'static str, f: impl FnOnce() + 'static) -> JobHandle {
        assert!(
            self.is_owner_thread(),
            "xengine job system: spawn_main must be called from the JobSystem owner thread; use spawn() for cross-thread submission"
        );
        let (index, generation) = {
            let mut sched = self.lock();
            let (index, generation) = sched.alloc(name);
            sched.pending += 1;
            (index, generation)
        };
        self.frame_jobs.fetch_add(1, Ordering::AcqRel);
        MAIN_QUEUES.with(|queues| {
            queues
                .borrow_mut()
                .entry(self.id)
                .or_default()
                .push_back(MainJob {
                    index,
                    generation,
                    f: Box::new(f),
                })
        });
        JobHandle {
            inner: Arc::clone(self),
            index,
            generation,
            name,
        }
    }

    fn parallel_for<F>(self: &Arc<Self>, range: Range<u32>, grain_size: u32, f: &F)
    where
        F: Fn(u32, u32) + Send + Sync,
    {
        if let Some(payload) = self.parallel_for_collect(range, grain_size, f) {
            resume_unwind(payload);
        }
    }

    /// Runs a scoped fork-join parallel loop and returns the first panic payload
    /// (if any) **after** every chunk job has completed, so the caller's
    /// borrowed data is guaranteed to outlive all jobs.
    fn parallel_for_collect<F>(
        self: &Arc<Self>,
        range: Range<u32>,
        grain_size: u32,
        f: &F,
    ) -> Option<Box<dyn Any + Send>>
    where
        F: Fn(u32, u32) + Send + Sync,
    {
        let grain = if grain_size == 0 {
            DEFAULT_GRAIN_SIZE
        } else {
            grain_size
        };
        let start = range.start;
        let end = range.end;
        if start >= end {
            return None;
        }
        let chunks = (end - start).div_ceil(grain.max(1)) as usize;
        if self.worker_count == 0 || chunks == 1 {
            for chunk in 0..chunks as u32 {
                let chunk_start = start + chunk * grain;
                (f)(chunk_start, (chunk_start + grain).min(end));
            }
            return None;
        }
        let ctx: Arc<ScopedCtx<'_>> = Arc::new(ScopedCtx {
            f,
            end: end as usize,
            grain: grain as usize,
            cursor: AtomicUsize::new(start as usize),
        });
        // --- the single lifetime-erasure unsafe of this module -------------
        // Safety: `ctx` is owned by this stack frame and outlives every job
        // created below, because this function joins all of them (and runs its
        // own claim loop) before returning; the erased `'static` is therefore
        // never observed after the real borrow ends. `ScopedCtx<'_>` is
        // `Send + Sync` (it stores a `&dyn Fn + Send + Sync` plus atomics), so
        // the erased `Arc` may legally be captured by `Send + 'static` payloads.
        let ctx: Arc<ScopedCtx<'static>> =
            unsafe { std::mem::transmute::<Arc<ScopedCtx<'_>>, Arc<ScopedCtx<'static>>>(ctx) };
        let jobs = self.worker_count.min(chunks);
        let mut handles = Vec::with_capacity(jobs);
        for _ in 0..jobs {
            let ctx = Arc::clone(&ctx);
            handles.push(
                self.spawn("xengine.parallel_for", JobPriority::Normal, move || {
                    claim_loop(&ctx)
                }),
            );
        }
        let local = catch_unwind(AssertUnwindSafe(|| claim_loop(&ctx)));
        let mut first = local.err();
        for handle in &handles {
            if let Some(payload) = self.wait_collect(handle.index, handle.generation, true)
                && first.is_none()
            {
                first = Some(payload);
            }
        }
        first
    }

    fn spawn_parallel_for<F>(
        self: &Arc<Self>,
        name: &'static str,
        priority: JobPriority,
        range: Range<u32>,
        grain_size: u32,
        f: F,
    ) -> JobHandle
    where
        F: Fn(u32, u32) + Send + Sync + 'static,
    {
        let inner = Arc::clone(self);
        self.spawn(name, priority, move || {
            inner.parallel_for(range, grain_size, &f)
        })
    }

    fn begin_frame(&self, frame: u64) {
        self.frame.store(frame, Ordering::Release);
        self.frame_jobs.store(0, Ordering::Release);
        self.frame_done.store(0, Ordering::Release);
    }

    /// Frame barrier: pumps the main queue, drains all pending work and returns
    /// the frame statistics.
    fn end_frame(&self) -> FrameStats {
        let started = Instant::now();
        while self.run_one_local_inner() {}
        loop {
            let mut sched = self.lock();
            if sched.pending == 0 {
                break;
            }
            if let Some(index) = sched.pop_job() {
                drop(sched);
                self.run_compute_job(index);
                continue;
            }
            let _ = self
                .cv
                .wait_timeout(sched, WAIT_POLL)
                .unwrap_or_else(|e| e.into_inner());
        }
        let jobs = self.frame_jobs.swap(0, Ordering::AcqRel);
        let completed = self.frame_done.swap(0, Ordering::AcqRel);
        self.lock().recycle();
        FrameStats {
            frame: self.frame.load(Ordering::Acquire),
            jobs,
            completed,
            duration: started.elapsed(),
        }
    }

    fn current_job(self: &Arc<Self>) -> Option<JobHandle> {
        CURRENT_JOB
            .with(|c| c.get())
            .and_then(|(system, index, generation)| {
                if system == self.id {
                    Some(JobHandle {
                        inner: Arc::clone(self),
                        index,
                        generation,
                        name: self.job_name(index),
                    })
                } else {
                    None
                }
            })
    }

    fn shutdown_and_join(&self) {
        if self.shutdown.swap(true, Ordering::AcqRel) {
            return;
        }
        let queued: Vec<u32> = {
            let mut sched = self.lock();
            let mut queued = Vec::new();
            for queue in &mut sched.queues {
                queued.extend(queue.drain(..));
            }
            queued
        };
        for index in queued {
            {
                let sched = self.lock();
                if let Some(slot) = sched.slots.get(index as usize) {
                    *slot.payload.lock().unwrap_or_else(|e| e.into_inner()) = None;
                }
            }
            self.finish_job(index);
        }
        self.cv.notify_all();
        let handles: Vec<JoinHandle<()>> = {
            let mut workers = self.workers.lock().unwrap_or_else(|e| e.into_inner());
            std::mem::take(&mut *workers)
        };
        for handle in handles {
            let _ = handle.join();
        }
        if self.is_owner_thread() {
            MAIN_QUEUES.with(|queues| {
                queues.borrow_mut().remove(&self.id);
            });
        }
    }
}

fn worker_loop(weak: Weak<JobsInner>) {
    loop {
        let Some(inner) = weak.upgrade() else {
            return;
        };
        let job = {
            let mut sched = inner.lock();
            loop {
                if let Some(index) = sched.pop_job() {
                    break Some(index);
                }
                if inner.shutdown.load(Ordering::Acquire) {
                    break None;
                }
                let (guard, _) = inner
                    .cv
                    .wait_timeout(sched, WAIT_POLL)
                    .unwrap_or_else(|e| e.into_inner());
                sched = guard;
            }
        };
        match job {
            Some(index) => inner.run_compute_job(index),
            None => return,
        }
    }
}

// ── handles and groups ──────────────────────────────────────────────────────

/// A submitted job. Cheap to clone; keeps its job system state alive.
pub struct JobHandle {
    inner: Arc<JobsInner>,
    index: u32,
    generation: u32,
    name: &'static str,
}

impl JobHandle {
    /// Whether the job completed (jobs from recycled frames report `true`).
    pub fn is_finished(&self) -> bool {
        matches!(
            self.inner.state(self.index, self.generation),
            JobState::Done | JobState::Expired
        )
    }

    /// Helping wait: the calling thread executes available work while waiting
    /// and re-raises the job's panic payload on completion.
    pub fn wait(&self) {
        if let Some(payload) = self.inner.wait_collect(self.index, self.generation, true) {
            resume_unwind(payload);
        }
    }

    /// Blocking wait: never executes other work (for contexts that must not
    /// re-enter the scheduler).
    pub fn wait_blocking(&self) {
        if let Some(payload) = self.inner.wait_collect(self.index, self.generation, false) {
            resume_unwind(payload);
        }
    }

    /// The name the job was submitted with.
    pub fn name(&self) -> &'static str {
        self.name
    }
}

impl fmt::Debug for JobHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JobHandle")
            .field("name", &self.name)
            .field("index", &self.index)
            .field("generation", &self.generation)
            .field("finished", &self.is_finished())
            .finish()
    }
}

impl Clone for JobHandle {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
            index: self.index,
            generation: self.generation,
            name: self.name,
        }
    }
}

impl PartialEq for JobHandle {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index
            && self.generation == other.generation
            && Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl Eq for JobHandle {}

/// A collection of job handles waited on together. Up to [`GROUP_INLINE`]
/// handles are stored inline (no heap allocation).
pub struct JobGroup {
    inline: [Option<JobHandle>; GROUP_INLINE],
    len: usize,
    spill: Vec<JobHandle>,
}

impl Default for JobGroup {
    fn default() -> Self {
        Self::new()
    }
}

impl JobGroup {
    /// Creates an empty group.
    pub fn new() -> Self {
        Self {
            inline: std::array::from_fn(|_| None),
            len: 0,
            spill: Vec::new(),
        }
    }

    /// Adds a handle to the group.
    pub fn push(&mut self, handle: JobHandle) {
        if self.len < GROUP_INLINE {
            self.inline[self.len] = Some(handle);
            self.len += 1;
        } else {
            self.spill.push(handle);
        }
    }

    /// Number of handles in the group.
    pub fn len(&self) -> usize {
        self.len + self.spill.len()
    }

    /// Whether the group is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Iterates over the handles in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = &JobHandle> {
        self.inline[..self.len]
            .iter()
            .flatten()
            .chain(self.spill.iter())
    }

    /// Helping-waits for every member.
    pub fn wait(&self) {
        for handle in self.iter() {
            handle.wait();
        }
    }

    /// Blocking-waits for every member.
    pub fn wait_blocking(&self) {
        for handle in self.iter() {
            handle.wait_blocking();
        }
    }
}

/// Sequential dependency chain: each `then` step runs after the previous one.
pub struct JobChain<'a> {
    jobs: &'a JobSystem,
    tail: Option<JobHandle>,
    priority: JobPriority,
}

impl<'a> JobChain<'a> {
    /// Starts a chain of jobs at `priority`.
    pub fn new(jobs: &'a JobSystem, priority: JobPriority) -> Self {
        Self {
            jobs,
            tail: None,
            priority,
        }
    }

    /// Appends a step that runs after the current tail.
    pub fn then(mut self, name: &'static str, f: impl FnOnce() + Send + 'static) -> Self {
        let previous = self.tail.take();
        let handle = self.jobs.spawn(name, self.priority, move || {
            if let Some(previous) = previous {
                previous.wait();
            }
            f();
        });
        self.tail = Some(handle);
        self
    }

    /// The handle of the last appended step.
    pub fn tail(&self) -> Option<JobHandle> {
        self.tail.clone()
    }

    /// Waits for the whole chain to finish.
    pub fn then_wait(self) {
        if let Some(tail) = self.tail {
            tail.wait();
        }
    }
}

// ── public job system ───────────────────────────────────────────────────────

/// The frame task queue plus its worker pool.
///
/// Owns the worker threads: dropping the `JobSystem` shuts them down and joins
/// them. Handles keep the underlying state alive, so they can still be probed
/// after the system is dropped.
pub struct JobSystem {
    inner: Arc<JobsInner>,
}

impl JobSystem {
    /// Creates a job system with the worker count derived from `config`.
    pub fn new(config: JobConfig) -> Self {
        Self {
            inner: JobsInner::start(worker_count_for(&config)),
        }
    }

    /// Creates a job system without worker threads: every job runs inline on
    /// the submitting thread, with identical results.
    pub fn single_threaded() -> Self {
        Self {
            inner: JobsInner::start(0),
        }
    }

    /// Number of worker threads (`0` for the single-threaded mode).
    pub fn worker_count(&self) -> usize {
        self.inner.worker_count
    }

    /// Jobs submitted but not finished yet.
    pub fn active_job_count(&self) -> usize {
        self.inner.lock().pending
    }

    /// The job currently running on this thread, if any.
    pub fn current_job(&self) -> Option<JobHandle> {
        self.inner.current_job()
    }

    /// Marks the start of a frame.
    pub fn begin_frame(&self, frame: u64) {
        self.inner.begin_frame(frame);
    }

    /// Frame barrier: pumps main-thread jobs, waits for every pending job and
    /// returns this frame's statistics.
    pub fn end_frame(&self) -> FrameStats {
        self.inner.end_frame()
    }

    /// Submits a compute job to the worker pool.
    pub fn spawn(
        &self,
        name: &'static str,
        priority: JobPriority,
        f: impl FnOnce() + Send + 'static,
    ) -> JobHandle {
        self.inner.spawn(name, priority, f)
    }

    /// Submits a job to a specific execution category.
    ///
    /// `Main` payloads may be `!Send`, so they are submitted through
    /// [`JobSystem::spawn_main`]; this method panics for that category.
    pub fn spawn_at(
        &self,
        category: JobCategory,
        name: &'static str,
        priority: JobPriority,
        f: impl FnOnce() + Send + 'static,
    ) -> JobHandle {
        self.inner.spawn_at(category, name, priority, f)
    }

    /// Submits a main-thread job; the payload may capture `!Send` data and is
    /// only ever executed by the owner thread, in submission order.
    pub fn spawn_main(&self, name: &'static str, f: impl FnOnce() + 'static) -> JobHandle {
        self.inner.spawn_main(name, f)
    }

    /// Fork-join parallel loop: splits `range` into `grain_size` chunks, runs
    /// them on the pool (the calling thread participates) and returns once all
    /// chunks completed. Results are per-chunk deterministic, so they do not
    /// depend on the worker count. Panics are re-raised on the calling thread
    /// after every chunk has stopped.
    pub fn parallel_for<F>(&self, range: Range<u32>, grain_size: u32, f: F)
    where
        F: Fn(u32, u32) + Send + Sync,
    {
        self.inner.parallel_for(range, grain_size, &f);
    }

    /// Fork-only variant of [`JobSystem::parallel_for`]: returns a handle to
    /// wait on later so the loop can overlap with other work. The payload owns
    /// its data (`'static`), because the call returns before the chunks run.
    pub fn spawn_parallel_for<F>(
        &self,
        name: &'static str,
        priority: JobPriority,
        range: Range<u32>,
        grain_size: u32,
        f: F,
    ) -> JobHandle
    where
        F: Fn(u32, u32) + Send + Sync + 'static,
    {
        self.inner
            .spawn_parallel_for(name, priority, range, grain_size, f)
    }

    /// Executes one queued main-thread job (owner thread only).
    pub fn run_one_local(&self) -> bool {
        self.inner.run_one_local_inner()
    }

    /// Pumps the main-thread queue; returns the number of jobs executed.
    pub fn pump(&self) -> usize {
        let mut ran = 0;
        while self.inner.run_one_local_inner() {
            ran += 1;
        }
        ran
    }
}

impl Drop for JobSystem {
    fn drop(&mut self) {
        self.inner.shutdown_and_join();
    }
}

impl fmt::Debug for JobSystem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JobSystem")
            .field("workers", &self.worker_count())
            .field("active", &self.active_job_count())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parallel::JobConditional;
    use std::cell::RefCell;
    use std::rc::Rc;

    const EXACT_ONE: JobConfig = JobConfig {
        hint: 1.0,
        reserve: 0,
        min: 1,
        max: 1,
    };
    const EXACT_TWO: JobConfig = JobConfig {
        hint: 2.0,
        reserve: 0,
        min: 2,
        max: 2,
    };
    const EXACT_FOUR: JobConfig = JobConfig {
        hint: 4.0,
        reserve: 0,
        min: 4,
        max: 4,
    };

    #[derive(Debug, PartialEq, Eq)]
    enum Outcome {
        Ok,
        Panic,
        Timeout,
    }

    /// Runs `f` on a helper thread so a scheduler bug fails the test instead of
    /// hanging the whole suite.
    fn timed<F: FnOnce() + Send + 'static>(ms: u64, f: F) -> Outcome {
        let (tx, rx) = std::sync::mpsc::channel();
        let _ = thread::spawn(move || {
            let result = catch_unwind(AssertUnwindSafe(f));
            let _ = tx.send(if result.is_ok() {
                Outcome::Ok
            } else {
                Outcome::Panic
            });
        });
        rx.recv_timeout(Duration::from_millis(ms))
            .unwrap_or(Outcome::Timeout)
    }

    fn panic_text(payload: &(dyn Any + Send)) -> String {
        if let Some(text) = payload.downcast_ref::<&'static str>() {
            (*text).to_string()
        } else if let Some(text) = payload.downcast_ref::<String>() {
            text.clone()
        } else {
            "<non-string panic>".to_string()
        }
    }

    fn pattern_output(jobs: &JobSystem, n: u32) -> Vec<u32> {
        let out = Mutex::new(vec![0u32; n as usize]);
        jobs.parallel_for(0..n, 32, |start, end| {
            let mut guard = out.lock().unwrap_or_else(|e| e.into_inner());
            for i in start..end {
                guard[i as usize] = i.wrapping_mul(2_654_435_761) ^ (i >> 3);
            }
        });
        out.into_inner().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn default_config_matches_neox_defaults() {
        let config = JobConfig::default();
        assert_eq!(config.hint, 1.0);
        assert_eq!(config.reserve, 2);
        assert_eq!(config.min, 1);
        assert_eq!(config.max, 8);
    }

    #[test]
    fn worker_count_respects_hint_reserve_and_bounds() {
        assert_eq!(
            worker_count_for(&JobConfig {
                hint: 4.0,
                reserve: 0,
                min: 1,
                max: 8
            }),
            4
        );
        assert_eq!(
            worker_count_for(&JobConfig {
                hint: 4.0,
                reserve: 2,
                min: 1,
                max: 8
            }),
            2
        );
        assert_eq!(
            worker_count_for(&JobConfig {
                hint: 64.0,
                reserve: 0,
                min: 1,
                max: 8
            }),
            8
        );
        let hardware = thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1) as i64;
        assert_eq!(
            worker_count_for(&JobConfig::default()),
            (hardware - 2).clamp(1, 8) as usize
        );
    }

    #[test]
    fn single_threaded_runs_inline_without_workers() {
        let jobs = JobSystem::single_threaded();
        assert_eq!(jobs.worker_count(), 0);
        let flag = Arc::new(AtomicBool::new(false));
        let target = Arc::clone(&flag);
        let handle = jobs.spawn("inline", JobPriority::Normal, move || {
            target.store(true, Ordering::Release);
        });
        assert!(flag.load(Ordering::Acquire), "inline execution");
        assert!(handle.is_finished());
    }

    #[test]
    fn main_jobs_run_in_submission_order_with_non_send_payloads() {
        let jobs = JobSystem::new(EXACT_ONE);
        let log = Rc::new(RefCell::new(Vec::new()));
        for tag in ["a", "b", "c"] {
            let log = Rc::clone(&log);
            let _ = jobs.spawn_main("main-step", move || log.borrow_mut().push(tag));
        }
        assert_eq!(jobs.pump(), 3);
        assert_eq!(&*log.borrow(), &["a", "b", "c"]);
    }

    #[test]
    fn spawn_main_outside_the_owner_thread_panics() {
        let jobs = Arc::new(JobSystem::single_threaded());
        let other = Arc::clone(&jobs);
        let handle = thread::spawn(move || {
            let _ = other.spawn_main("nope", || {});
        });
        assert!(
            handle.join().is_err(),
            "spawn_main must reject non-owner threads"
        );
    }

    #[test]
    fn current_category_runs_inline() {
        let jobs = JobSystem::single_threaded();
        let flag = Arc::new(AtomicBool::new(false));
        let target = Arc::clone(&flag);
        let handle = jobs.spawn_at(
            JobCategory::Current,
            "inline",
            JobPriority::Normal,
            move || target.store(true, Ordering::Release),
        );
        assert!(flag.load(Ordering::Acquire));
        assert!(handle.is_finished());
    }

    #[test]
    #[should_panic(expected = "spawn_main")]
    fn main_category_through_spawn_at_is_rejected() {
        let jobs = JobSystem::single_threaded();
        let _ = jobs.spawn_at(JobCategory::Main, "nope", JobPriority::Normal, || {});
    }

    #[test]
    fn inline_panic_payload_is_preserved() {
        let jobs = JobSystem::single_threaded();
        let handle = jobs.spawn("inline-boom", JobPriority::Normal, || {
            panic!("inline payload")
        });
        assert!(handle.is_finished());
        let result = catch_unwind(AssertUnwindSafe(|| handle.wait()));
        let payload = result.expect_err("panic expected");
        assert_eq!(panic_text(payload.as_ref()), "inline payload");
    }

    #[test]
    fn high_priority_jumps_ahead_of_low() {
        let jobs = JobSystem::new(EXACT_ONE);
        assert_eq!(jobs.worker_count(), 1);
        let started = Arc::new(JobConditional::new());
        let release = Arc::new(JobConditional::new());
        let log = Arc::new(Mutex::new(Vec::new()));

        let _ = jobs.spawn("blocker", JobPriority::Low, {
            let started = Arc::clone(&started);
            let release = Arc::clone(&release);
            move || {
                started.notify_all();
                release.wait();
            }
        });
        started.wait();

        let _ = jobs.spawn("high", JobPriority::High, {
            let log = Arc::clone(&log);
            move || log.lock().unwrap_or_else(|e| e.into_inner()).push("high")
        });
        let _ = jobs.spawn("low", JobPriority::Low, {
            let log = Arc::clone(&log);
            move || log.lock().unwrap_or_else(|e| e.into_inner()).push("low")
        });
        release.notify_all();
        let _ = jobs.end_frame();
        assert_eq!(
            &*log.lock().unwrap_or_else(|e| e.into_inner()),
            &["high", "low"]
        );
    }

    #[test]
    fn same_priority_keeps_submission_order() {
        let jobs = JobSystem::new(EXACT_ONE);
        let started = Arc::new(JobConditional::new());
        let release = Arc::new(JobConditional::new());
        let log = Arc::new(Mutex::new(Vec::new()));

        let _ = jobs.spawn("blocker", JobPriority::Normal, {
            let started = Arc::clone(&started);
            let release = Arc::clone(&release);
            move || {
                started.notify_all();
                release.wait();
            }
        });
        started.wait();
        for tag in ["first", "second", "third"] {
            let _ = jobs.spawn("queued", JobPriority::Normal, {
                let log = Arc::clone(&log);
                move || log.lock().unwrap_or_else(|e| e.into_inner()).push(tag)
            });
        }
        release.notify_all();
        let _ = jobs.end_frame();
        assert_eq!(
            &*log.lock().unwrap_or_else(|e| e.into_inner()),
            &["first", "second", "third"]
        );
    }

    #[test]
    fn parallel_for_covers_every_chunk_exactly_once() {
        let jobs = JobSystem::new(EXACT_FOUR);
        let n = 1000u32;
        let grain = 64u32;
        let counters = Mutex::new(vec![0u32; n.div_ceil(grain) as usize]);
        jobs.parallel_for(0..n, grain, |start, end| {
            let mut guard = counters.lock().unwrap_or_else(|e| e.into_inner());
            guard[(start / grain) as usize] += 1;
            assert!(end > start, "chunk must be non-empty");
            assert!(end - start <= grain, "chunk must respect the grain size");
            assert_eq!(start % grain, 0, "chunks start on grain boundaries");
        });
        let guard = counters.lock().unwrap_or_else(|e| e.into_inner());
        assert!(guard.iter().all(|count| *count == 1));
        assert_eq!(guard.len(), 16);
    }

    #[test]
    fn parallel_for_empty_range_and_zero_grain_are_safe() {
        let jobs = JobSystem::new(EXACT_TWO);
        let calls = AtomicUsize::new(0);
        jobs.parallel_for(0..0, 64, |_, _| {
            calls.fetch_add(1, Ordering::Relaxed);
        });
        assert_eq!(calls.load(Ordering::Relaxed), 0);

        let seen = Mutex::new(Vec::new());
        jobs.parallel_for(0..10, 0, |start, end| {
            seen.lock()
                .unwrap_or_else(|e| e.into_inner())
                .push((start, end));
        });
        let mut ranges = seen.into_inner().unwrap_or_else(|e| e.into_inner());
        ranges.sort_unstable();
        assert_eq!(ranges, vec![(0, 10)], "grain 0 falls back to the default");
    }

    #[test]
    fn results_are_independent_of_worker_count() {
        let n = 2048u32;
        let baseline = pattern_output(&JobSystem::single_threaded(), n);
        for i in 0..n {
            assert_eq!(
                baseline[i as usize],
                i.wrapping_mul(2_654_435_761) ^ (i >> 3),
                "reference implementation"
            );
        }
        for config in [EXACT_ONE, EXACT_TWO, EXACT_FOUR] {
            let jobs = JobSystem::new(config);
            assert_eq!(jobs.worker_count(), config.max as usize);
            assert_eq!(pattern_output(&jobs, n), baseline, "config {config:?}");
        }
    }

    #[test]
    fn fork_only_parallel_for_completes_and_overlaps() {
        let jobs = Arc::new(JobSystem::new(EXACT_FOUR));
        let out = Arc::new(Mutex::new(vec![0u32; 512]));
        let target = Arc::clone(&out);
        let handle =
            jobs.spawn_parallel_for("pf", JobPriority::Normal, 0..512, 64, move |start, end| {
                let mut guard = target.lock().unwrap_or_else(|e| e.into_inner());
                for i in start..end {
                    guard[i as usize] = i + 1;
                }
            });
        let flag = Arc::new(AtomicBool::new(false));
        let target = Arc::clone(&flag);
        let other = jobs.spawn("overlap", JobPriority::Normal, move || {
            target.store(true, Ordering::Release);
        });
        handle.wait();
        other.wait();
        assert!(flag.load(Ordering::Acquire));
        let guard = out.lock().unwrap_or_else(|e| e.into_inner());
        assert!(guard.iter().enumerate().all(|(i, v)| *v == i as u32 + 1));
    }

    #[test]
    fn helping_wait_consumes_the_main_queue() {
        let jobs = Arc::new(JobSystem::new(EXACT_ONE));
        let gate = Arc::new(JobConditional::new());
        let released = Arc::new(AtomicBool::new(false));
        let watchdog_fired = Arc::new(AtomicBool::new(false));
        let blocker = jobs.spawn("blocker", JobPriority::Normal, {
            let gate = Arc::clone(&gate);
            move || gate.wait()
        });
        let _ = jobs.spawn_main("release", {
            let gate = Arc::clone(&gate);
            let released = Arc::clone(&released);
            move || {
                released.store(true, Ordering::Release);
                gate.notify_all();
            }
        });
        // Watchdog: only the owner thread may run main jobs, so this wait must
        // happen here. If the helping wait fails to consume the queue, the
        // watchdog unblocks the test (and the assertions fail) instead of
        // hanging the whole suite.
        let _watchdog = {
            let gate = Arc::clone(&gate);
            let fired = Arc::clone(&watchdog_fired);
            thread::spawn(move || {
                thread::sleep(Duration::from_secs(5));
                if !gate.is_notified() {
                    fired.store(true, Ordering::Release);
                    gate.notify_all();
                }
            })
        };
        blocker.wait();
        assert!(
            released.load(Ordering::Acquire),
            "the helping wait must run queued main jobs"
        );
        assert!(
            !watchdog_fired.load(Ordering::Acquire),
            "the helping wait must not fall back to the watchdog"
        );
    }

    #[test]
    fn self_wait_is_detected() {
        let jobs = Arc::new(JobSystem::new(EXACT_ONE));
        let inspector = Arc::clone(&jobs);
        let handle = jobs.spawn("self-wait", JobPriority::Normal, move || {
            let current = inspector.current_job().expect("a running job");
            current.wait();
        });
        let outcome = timed(4000, move || handle.wait());
        assert_eq!(outcome, Outcome::Panic, "self-wait must fail loudly");
    }

    #[test]
    fn job_panic_propagates_and_system_survives() {
        let jobs = Arc::new(JobSystem::new(EXACT_ONE));
        let bad = jobs.spawn("boom", JobPriority::Normal, || panic!("boom payload"));
        let outcome = timed(4000, {
            let bad = bad.clone();
            move || bad.wait()
        });
        assert_eq!(outcome, Outcome::Panic);

        let done = Arc::new(AtomicUsize::new(0));
        let target = Arc::clone(&done);
        let ok = jobs.spawn("after", JobPriority::Normal, move || {
            target.fetch_add(1, Ordering::AcqRel);
        });
        ok.wait();
        assert_eq!(done.load(Ordering::Acquire), 1);
        let stats = jobs.end_frame();
        assert_eq!(stats.completed, 2);
    }

    #[test]
    fn parallel_for_chunk_panic_does_not_hang() {
        let jobs = Arc::new(JobSystem::new(EXACT_FOUR));
        let panicking = Arc::clone(&jobs);
        let outcome = timed(4000, move || {
            panicking.parallel_for(0..256, 32, |start, _end| {
                if start == 64 {
                    panic!("chunk boom");
                }
            });
        });
        assert_eq!(outcome, Outcome::Panic);

        let after = Arc::clone(&jobs);
        let outcome = timed(4000, move || {
            after.parallel_for(0..64, 32, |_, _| {});
        });
        assert_eq!(
            outcome,
            Outcome::Ok,
            "the system stays usable after a panic"
        );
    }

    #[test]
    fn end_frame_is_a_barrier_and_reports_stats() {
        let jobs = JobSystem::new(EXACT_TWO);
        let counter = Arc::new(AtomicUsize::new(0));
        jobs.begin_frame(1);
        for _ in 0..8 {
            let counter = Arc::clone(&counter);
            let _ = jobs.spawn("work", JobPriority::Normal, move || {
                counter.fetch_add(1, Ordering::AcqRel);
            });
        }
        let stats = jobs.end_frame();
        assert_eq!(counter.load(Ordering::Acquire), 8);
        assert_eq!(stats.frame, 1);
        assert_eq!(stats.jobs, 8);
        assert_eq!(stats.completed, 8);
        assert_eq!(jobs.active_job_count(), 0);

        jobs.begin_frame(2);
        let stats = jobs.end_frame();
        assert_eq!(stats.frame, 2);
        assert_eq!(stats.jobs, 0, "no leftovers from the previous frame");
        assert_eq!(stats.completed, 0);
    }

    #[test]
    fn end_frame_pumps_main_jobs() {
        let jobs = JobSystem::new(EXACT_ONE);
        let flag = Arc::new(AtomicBool::new(false));
        let target = Arc::clone(&flag);
        jobs.begin_frame(7);
        let _ = jobs.spawn_main("main", move || target.store(true, Ordering::Release));
        let stats = jobs.end_frame();
        assert!(flag.load(Ordering::Acquire));
        assert_eq!(stats.jobs, 1);
        assert_eq!(stats.completed, 1);
    }

    #[test]
    fn repeated_empty_frames_are_safe() {
        let jobs = JobSystem::new(EXACT_ONE);
        for frame in 1..=5 {
            jobs.begin_frame(frame);
            let stats = jobs.end_frame();
            assert_eq!(stats.frame, frame);
            assert_eq!(stats.jobs, 0);
        }
    }

    #[test]
    fn active_job_count_tracks_pending_work() {
        let jobs = Arc::new(JobSystem::new(EXACT_ONE));
        let gate = Arc::new(JobConditional::new());
        let started = Arc::new(JobConditional::new());
        let handle = jobs.spawn("hold", JobPriority::Normal, {
            let gate = Arc::clone(&gate);
            let started = Arc::clone(&started);
            move || {
                started.notify_all();
                gate.wait();
            }
        });
        started.wait();
        assert_eq!(jobs.active_job_count(), 1);
        gate.notify_all();
        handle.wait();
        assert_eq!(jobs.active_job_count(), 0);
    }

    #[test]
    fn current_job_reports_the_running_job() {
        let jobs = Arc::new(JobSystem::single_threaded());
        assert!(jobs.current_job().is_none());
        let inside = Arc::new(Mutex::new(None));
        let target = Arc::clone(&inside);
        let inspector = Arc::clone(&jobs);
        let handle = jobs.spawn("inspect", JobPriority::Normal, move || {
            *target.lock().unwrap_or_else(|e| e.into_inner()) = inspector
                .current_job()
                .map(|job| (job.name(), job.is_finished()));
        });
        handle.wait();
        assert_eq!(
            *inside.lock().unwrap_or_else(|e| e.into_inner()),
            Some(("inspect", false))
        );
    }

    #[test]
    fn job_group_waits_for_all_members() {
        let jobs = JobSystem::new(EXACT_TWO);
        let counter = Arc::new(AtomicUsize::new(0));
        let mut group = JobGroup::new();
        for _ in 0..5 {
            let counter = Arc::clone(&counter);
            group.push(jobs.spawn("group-member", JobPriority::Normal, move || {
                counter.fetch_add(1, Ordering::AcqRel);
            }));
        }
        assert_eq!(group.len(), 5);
        assert!(!group.is_empty());
        group.wait();
        assert_eq!(counter.load(Ordering::Acquire), 5);
    }

    #[test]
    fn job_chain_runs_steps_in_order() {
        let jobs = JobSystem::new(EXACT_TWO);
        let log = Arc::new(Mutex::new(Vec::new()));
        let mut chain = JobChain::new(&jobs, JobPriority::Normal);
        for tag in ["one", "two", "three"] {
            let log = Arc::clone(&log);
            chain = chain.then("chain-step", move || {
                log.lock().unwrap_or_else(|e| e.into_inner()).push(tag)
            });
        }
        assert!(chain.tail().is_some());
        chain.then_wait();
        assert_eq!(
            &*log.lock().unwrap_or_else(|e| e.into_inner()),
            &["one", "two", "three"]
        );
    }

    #[test]
    fn drop_with_pending_main_jobs_is_clean() {
        let outcome = timed(4000, || {
            let jobs = JobSystem::new(EXACT_ONE);
            let flag = Arc::new(AtomicBool::new(false));
            let target = Arc::clone(&flag);
            let _ = jobs.spawn_main("never pumped", move || {
                target.store(true, Ordering::Release);
            });
            drop(jobs);
            assert!(!flag.load(Ordering::Acquire), "payload must not run");
        });
        assert_eq!(outcome, Outcome::Ok);
    }

    #[test]
    fn repeated_create_and_drop_is_clean() {
        let outcome = timed(8000, || {
            for _ in 0..8 {
                let jobs = JobSystem::new(JobConfig::default());
                assert!(jobs.worker_count() >= 1);
                drop(jobs);
            }
        });
        assert_eq!(outcome, Outcome::Ok);
    }
}
