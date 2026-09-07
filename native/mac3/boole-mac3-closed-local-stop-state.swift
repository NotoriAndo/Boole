import Foundation

/// The stop state shared by VZ callbacks and timeout handling.  This is kept
/// independent from Virtualization so its terminal-state rules can be tested
/// without constructing a VM.
struct StopState {
    private(set) var stopped = false
    private(set) var reason = ""
    private(set) var failed = false

    mutating func confirmStopped(_ detail: String, failed: Bool = false) {
        stopped = true
        if !self.failed { reason = detail }
        self.failed = self.failed || failed
    }

    mutating func recordForcedStopCompletion(_ error: Error?) {
        // A stop callback is not proof of a stop when it carries an error.
        // Leave `stopped` false so the host records stop-unconfirmed.
        guard error == nil else { return }
        confirmStopped("the host confirmed a forced stop")
    }
}
