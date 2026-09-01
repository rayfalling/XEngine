//! Render snapshot interface placeholder.
//!
//! The core layer defines the contract the device layer will consume; this
//! change deliberately provides no platform graphics logic.

/// Frame-end render submission data contract (placeholder).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RenderSnapshot {
    frame: u64,
}

impl RenderSnapshot {
    /// A snapshot for a given frame index.
    pub fn new(frame: u64) -> Self {
        Self { frame }
    }

    /// The frame index this snapshot belongs to.
    pub fn frame(&self) -> u64 {
        self.frame
    }
}

/// Sink for render snapshots (implemented by the device layer later).
pub trait RenderSink {
    /// Receives one frame's snapshot.
    fn submit(&mut self, snapshot: &RenderSnapshot);
}

/// No-op sink used by tests and the core-only path.
pub struct NullRenderSink;

impl RenderSink for NullRenderSink {
    fn submit(&mut self, _snapshot: &RenderSnapshot) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_carries_frame_and_null_sink_accepts() {
        let snap = RenderSnapshot::new(7);
        assert_eq!(snap.frame(), 7);
        let mut sink = NullRenderSink;
        sink.submit(&snap);
        // Core has no platform dependency by construction (no deps at all).
    }
}
