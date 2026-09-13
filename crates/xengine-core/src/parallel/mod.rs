//! Zero-dependency parallel execution layer.
//!
//! Two independent objects (user decision):
//! - [`JobSystem`]: the **frame task queue** plus its worker pool, aligned with
//!   NeoX `xjobsystem` (`initialize` budget, priorities, execution categories,
//!   helping waits, scoped `parallel_for`).
//! - [`ThreadPool`]: lane-scoped **dedicated threads** for long-running tasks.
//!
//! `JobConfig::reserve` carves the dedicated-thread budget out of the frame
//! worker budget, so frame jobs and long tasks never compete for the same
//! threads.

pub mod jobs;
pub mod sync;
pub mod threads;

pub use jobs::{
    DEFAULT_GRAIN_SIZE, FrameStats, JobCategory, JobChain, JobConfig, JobGroup, JobHandle,
    JobPriority, JobSystem, worker_count_for,
};
pub use sync::JobConditional;
pub use threads::{LaneSpec, LongTaskHandle, OwnedThread, ThreadPool};
