/// Non-blocking notification state for freestanding hosts.
///
/// Embedded hosts drive progress explicitly and cannot block on an operating
/// system condition variable. Native ingress is therefore polled by the host.
pub(crate) struct ProgressSignal;

impl ProgressSignal {
    pub(crate) const fn new() -> Self {
        Self
    }

    pub(crate) fn notify(&self) {
        // The host owns polling cadence; no blocking waiter observes this.
    }
}
