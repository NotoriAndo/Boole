import Foundation

@main
struct StopStateTests {
    static func require(_ condition: @autoclosure () -> Bool, _ message: String) {
        if !condition() {
            FileHandle.standardError.write(Data("stop-state test failed: \(message)\n".utf8))
            exit(1)
        }
    }

    static func main() {
        var noCallback = StopState()
        require(!noCallback.stopped, "no callback must not imply a stop")

        var failedCallback = StopState()
        failedCallback.recordForcedStopCompletion(NSError(domain: "test", code: 1))
        require(!failedCallback.stopped, "an error callback must not confirm a stop")

        var successfulCallback = StopState()
        successfulCallback.recordForcedStopCompletion(nil)
        require(successfulCallback.stopped, "a nil-error callback confirms the forced stop")
        require(!successfulCallback.failed, "a successful forced stop is not failed")

        var guestFailure = StopState()
        guestFailure.confirmStopped("guest error", failed: true)
        require(guestFailure.stopped && guestFailure.failed, "guest error remains terminal failure")
    }
}
