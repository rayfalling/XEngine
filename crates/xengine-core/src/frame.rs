//! Frame-driven engine loop (Unity-style model) and frame-time modes.

use std::sync::Arc;
use std::time::Duration;

use crate::go::Scene;
use crate::parallel::JobSystem;
use crate::render::RenderSnapshot;
use crate::schedule::Schedule;
use crate::system::Stage;

/// Fixed logical step boundary (user decision: 1/60s; configurable).
pub const DEFAULT_FIXED_STEP: Duration = Duration::from_nanos(1_000_000_000 / 60);

/// Anti-spiral safety valve: max FixedUpdate runs per rendered frame.
pub const MAX_FIXED_STEPS_PER_FRAME: usize = 16;

/// Frame rate mode.
///
/// `Capped` aligns the update dt with the target frame rate (frame limiting).
/// `Uncapped` uses the measured frame time, clamped by `max_dt`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameMode {
    /// Limit the frame rate: Update dt becomes ~1/target_fps.
    Capped { target_fps: u32 },
    /// No frame limit: use the measured frame time, clamped by `max_dt`.
    Uncapped { max_dt: Duration },
}

impl FrameMode {
    /// Resolves the current frame dt for a measured frame time.
    pub fn frame_dt(&self, measured: Duration) -> Duration {
        match *self {
            FrameMode::Capped { target_fps } => {
                if target_fps == 0 {
                    measured
                } else {
                    Duration::from_secs_f64(1.0 / target_fps as f64)
                }
            }
            FrameMode::Uncapped { max_dt } => measured.min(max_dt),
        }
    }
}

/// Per-frame time bookkeeping published as a resource (`Res<TimeState>`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimeState {
    /// Current rendered frame index (1-based).
    pub frame: u64,
    /// Frame dt handed to Update/PostUpdate this frame.
    pub dt: Duration,
    /// Fixed logical step.
    pub fixed_step: Duration,
    /// How many FixedUpdate runs happened this frame.
    pub fixed_runs: usize,
}

/// Statistics of one tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunStats {
    pub frame: u64,
    pub dt: Duration,
    pub fixed_runs: usize,
}

/// The engine: owns the scene (and its world) and runs the schedule per frame.
pub struct Engine {
    scene: crate::go::scene::SceneHandle,
    schedule: Schedule,
    mode: FrameMode,
    fixed_step: Duration,
    accumulator: Duration,
    frame: u64,
    jobs: Option<Arc<JobSystem>>,
}

impl Engine {
    /// Creates an engine with the default fixed step (1/60s).
    ///
    /// Without [`Engine::with_jobs`] the engine runs single-threaded: no frame
    /// barrier is needed and no job system is involved.
    pub fn new(scene: crate::go::scene::SceneHandle, schedule: Schedule, mode: FrameMode) -> Self {
        Self {
            scene,
            schedule,
            mode,
            fixed_step: DEFAULT_FIXED_STEP,
            accumulator: Duration::ZERO,
            frame: 0,
            jobs: None,
        }
    }

    /// Attaches a job system: `tick` then brackets every frame with
    /// `begin_frame` / `end_frame`, so a frame barrier makes `tick` return only
    /// after all jobs submitted during that frame have completed.
    pub fn with_jobs(mut self, jobs: Arc<JobSystem>) -> Self {
        self.jobs = Some(jobs);
        self
    }

    /// The attached job system, if any.
    pub fn jobs(&self) -> Option<&Arc<JobSystem>> {
        self.jobs.as_ref()
    }

    /// Sets the fixed step (FixedUpdate period).
    pub fn with_fixed_step(mut self, step: Duration) -> Self {
        assert!(!step.is_zero(), "fixed step must be non-zero");
        self.fixed_step = step;
        self
    }

    pub fn fixed_step(&self) -> Duration {
        self.fixed_step
    }

    pub fn mode(&self) -> FrameMode {
        self.mode
    }

    pub fn set_mode(&mut self, mode: FrameMode) {
        self.mode = mode;
    }

    pub fn scene(&self) -> &Scene {
        &self.scene
    }

    pub fn scene_mut(&mut self) -> &mut Scene {
        &mut self.scene
    }

    /// Resets the fixed-step accumulator (e.g. after a long pause).
    pub fn reset_accumulator(&mut self) {
        self.accumulator = Duration::ZERO;
    }

    /// Runs one frame: FixedUpdate (0..N) -> Update -> PostUpdate.
    ///
    /// The measured frame time is resolved per the current [`FrameMode`].
    pub fn tick(&mut self, measured: Duration) -> RunStats {
        let dt = self.mode.frame_dt(measured);
        self.frame += 1;
        if let Some(jobs) = &self.jobs {
            jobs.begin_frame(self.frame);
        }
        let mut fixed_runs = 0;
        self.accumulator += dt;
        while self.accumulator >= self.fixed_step {
            if fixed_runs >= MAX_FIXED_STEPS_PER_FRAME {
                // Anti-spiral: drop the remainder instead of a spiral.
                self.accumulator = Duration::ZERO;
                break;
            }
            self.accumulator -= self.fixed_step;
            fixed_runs += 1;
            let scene = &mut self.scene;
            self.schedule
                .run_stage(scene.world_mut(), Stage::FixedUpdate);
        }
        // Publish time bookkeeping before the frame-update systems.
        let scene = &mut self.scene;
        scene.world_mut().insert_resource(TimeState {
            frame: self.frame,
            dt,
            fixed_step: self.fixed_step,
            fixed_runs,
        });
        let scene = &mut self.scene;
        self.schedule.run_stage(scene.world_mut(), Stage::Update);
        let scene = &mut self.scene;
        self.schedule
            .run_stage(scene.world_mut(), Stage::PostUpdate);
        // Frame barrier: every job submitted during this frame is done before
        // the frame is considered over (render snapshot included).
        if let Some(jobs) = &self.jobs {
            let _ = jobs.end_frame();
        }
        RunStats {
            frame: self.frame,
            dt,
            fixed_runs,
        }
    }

    /// Produces the frame-end render snapshot (interface placeholder).
    pub fn snapshot(&self) -> RenderSnapshot {
        RenderSnapshot::new(self.frame)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::go::SceneHandle;
    use crate::schedule::Schedule;
    use crate::system::System;

    fn empty_schedule() -> Schedule {
        Schedule::build(vec![]).unwrap()
    }

    #[test]
    fn capped_mode_uses_target_dt() {
        let mode = FrameMode::Capped { target_fps: 60 };
        let dt = mode.frame_dt(Duration::from_millis(100));
        assert_eq!(dt, Duration::from_secs_f64(1.0 / 60.0));
    }

    #[test]
    fn uncapped_mode_uses_measured_clamped() {
        let mode = FrameMode::Uncapped {
            max_dt: Duration::from_millis(50),
        };
        assert_eq!(
            mode.frame_dt(Duration::from_millis(12)),
            Duration::from_millis(12)
        );
        assert_eq!(
            mode.frame_dt(Duration::from_millis(120)),
            Duration::from_millis(50)
        );
    }

    #[test]
    fn slow_frame_runs_fixed_multiple_times_then_update_post() {
        use std::cell::RefCell;
        use std::rc::Rc;
        let ticks = Rc::new(RefCell::new(Vec::new()));
        let fixed = {
            let t = ticks.clone();
            System::new("fixed", Stage::FixedUpdate, move |_| {
                t.borrow_mut().push("F");
            })
        };
        let update = {
            let t = ticks.clone();
            System::new("update", Stage::Update, move |_| {
                t.borrow_mut().push("U");
            })
        };
        let post = {
            let t = ticks.clone();
            System::new("post", Stage::PostUpdate, move |_| {
                t.borrow_mut().push("P");
            })
        };
        let schedule = Schedule::build(vec![fixed, update, post]).unwrap();
        let mut engine = Engine::new(
            SceneHandle::new(),
            schedule,
            FrameMode::Uncapped {
                max_dt: Duration::from_secs(1),
            },
        )
        .with_fixed_step(Duration::from_millis(20));
        let stats = engine.tick(Duration::from_millis(100));
        assert_eq!(stats.fixed_runs, 5);
        let log = ticks.borrow();
        assert_eq!(log.iter().filter(|x| **x == "F").count(), 5);
        assert_eq!(log.iter().filter(|x| **x == "U").count(), 1);
        assert_eq!(log.iter().filter(|x| **x == "P").count(), 1);
    }

    #[test]
    fn fast_frame_runs_zero_fixed() {
        let stats = Engine::new(
            SceneHandle::new(),
            empty_schedule(),
            FrameMode::Uncapped {
                max_dt: Duration::from_secs(1),
            },
        )
        .tick(Duration::from_millis(5));
        assert_eq!(stats.fixed_runs, 0);
        assert_eq!(stats.dt, Duration::from_millis(5));
    }

    #[test]
    fn mode_switch_keeps_fixed_step_constant() {
        let mut engine = Engine::new(
            SceneHandle::new(),
            empty_schedule(),
            FrameMode::Capped { target_fps: 60 },
        );
        let a = engine.tick(Duration::from_millis(30));
        engine.set_mode(FrameMode::Uncapped {
            max_dt: Duration::from_millis(50),
        });
        let b = engine.tick(Duration::from_millis(30));
        // Fixed step unchanged; dt follows the mode.
        assert_eq!(engine.fixed_step(), Duration::from_secs(1) / 60);
        assert_eq!(a.dt, Duration::from_secs_f64(1.0 / 60.0));
        assert_eq!(b.dt, Duration::from_millis(30));
    }

    #[test]
    fn time_state_is_published_and_snapshot_available() {
        let mut engine = Engine::new(
            SceneHandle::new(),
            empty_schedule(),
            FrameMode::Uncapped {
                max_dt: Duration::from_secs(1),
            },
        );
        engine.tick(Duration::from_millis(16));
        let ts = engine
            .scene()
            .world()
            .get_resource::<TimeState>()
            .unwrap()
            .unwrap();
        assert_eq!(ts.frame, 1);
        assert_eq!(ts.dt, Duration::from_millis(16));
        let snap = engine.snapshot();
        assert_eq!(snap.frame(), 1);
    }

    // ── job system integration ─────────────────────────────────────────────

    #[test]
    fn engine_without_jobs_matches_a_single_threaded_job_system() {
        use crate::parallel::JobSystem;
        use std::sync::Arc;

        let mut plain = Engine::new(
            SceneHandle::new(),
            empty_schedule(),
            FrameMode::Capped { target_fps: 60 },
        );
        let mut with_jobs = Engine::new(
            SceneHandle::new(),
            empty_schedule(),
            FrameMode::Capped { target_fps: 60 },
        )
        .with_jobs(Arc::new(JobSystem::single_threaded()));

        assert!(plain.jobs().is_none());
        assert!(with_jobs.jobs().is_some());
        for _ in 0..3 {
            assert_eq!(
                plain.tick(Duration::from_millis(30)),
                with_jobs.tick(Duration::from_millis(30))
            );
        }
    }

    #[test]
    fn tick_drains_frame_jobs_before_returning() {
        use crate::parallel::{JobConfig, JobSystem};
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let jobs = Arc::new(JobSystem::new(JobConfig {
            hint: 2.0,
            reserve: 0,
            min: 2,
            max: 2,
        }));
        let counter = Arc::new(AtomicUsize::new(0));
        let submitter = {
            let jobs = Arc::clone(&jobs);
            let counter = Arc::clone(&counter);
            System::new("parallel work", Stage::Update, move |_| {
                let counter = Arc::clone(&counter);
                jobs.parallel_for(0..256, 32, move |_start, _end| {
                    counter.fetch_add(1, Ordering::AcqRel);
                });
            })
        };
        let schedule = Schedule::build(vec![submitter]).unwrap();
        let mut engine = Engine::new(
            SceneHandle::new(),
            schedule,
            FrameMode::Uncapped {
                max_dt: Duration::from_secs(1),
            },
        )
        .with_jobs(Arc::clone(&jobs));

        engine.tick(Duration::from_millis(16));
        assert_eq!(counter.load(Ordering::Acquire), 8, "all chunks ran");
        assert_eq!(
            jobs.active_job_count(),
            0,
            "the frame barrier drains every job"
        );
        engine.tick(Duration::from_millis(16));
        assert_eq!(counter.load(Ordering::Acquire), 16);
    }

    #[test]
    fn parallel_jobs_do_not_change_frame_determinism() {
        use crate::parallel::{JobConfig, JobSystem};
        use std::sync::{Arc, Mutex};

        fn run_log(with_jobs: bool) -> Vec<&'static str> {
            let log = Arc::new(Mutex::new(Vec::new()));
            let make = |name: &'static str, log: &Arc<Mutex<Vec<&'static str>>>| {
                let log = Arc::clone(log);
                System::new(name, Stage::Update, move |_| {
                    log.lock().unwrap_or_else(|e| e.into_inner()).push(name);
                })
            };
            let systems = vec![make("a", &log), make("b", &log), make("c", &log)];
            let schedule = Schedule::build(systems).unwrap();
            let mut engine = Engine::new(
                SceneHandle::new(),
                schedule,
                FrameMode::Uncapped {
                    max_dt: Duration::from_secs(1),
                },
            );
            if with_jobs {
                engine = engine.with_jobs(Arc::new(JobSystem::new(JobConfig {
                    hint: 4.0,
                    reserve: 0,
                    min: 4,
                    max: 4,
                })));
            }
            for _ in 0..10 {
                engine.tick(Duration::from_millis(16));
            }
            log.lock().unwrap_or_else(|e| e.into_inner()).clone()
        }

        let plain = run_log(false);
        let parallel = run_log(true);
        assert_eq!(plain.len(), 30);
        assert_eq!(plain, parallel, "frame determinism is preserved");
    }
}
