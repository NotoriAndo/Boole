// Closed-local development-Mac boot host for the MAC.3 qualification.
//
// This program boots one guest, once, from files it is handed by path, and
// writes down what it built and what happened. It has no default image path, no
// network device, no shared directory and no writable disk, and it refuses to
// start if the machine it assembled disagrees with any of that -- the isolation
// is checked against the built configuration rather than trusted because the
// code above it did not ask for anything.
//
// It is deliberately not a VM manager. There is no resume, no snapshot, no
// second boot and no way to hand it a disk it may write to.

import CryptoKit
import Foundation
import Virtualization

let FIXED_CPU_COUNT = 2
let FIXED_MEMORY_BYTES: UInt64 = 2 * 1024 * 1024 * 1024

struct HostError: Error, CustomStringConvertible {
    let description: String
    init(_ description: String) { self.description = description }
}

func fail(_ message: String) -> Never {
    FileHandle.standardError.write(Data("mac3-boot: \(message)\n".utf8))
    exit(2)
}

func option(_ name: String, _ arguments: [String]) throws -> String {
    guard let index = arguments.firstIndex(of: "--\(name)"),
          index + 1 < arguments.count else {
        throw HostError("missing required option --\(name)")
    }
    let value = arguments[index + 1]
    if value.hasPrefix("--") {
        throw HostError("option --\(name) was given another option as its value")
    }
    return value
}

func sha256(ofFileAt path: String) throws -> String {
    // Hashing is streamed: these files are over a gigabyte and reading one into
    // memory to check it would be its own failure mode.
    guard let handle = FileHandle(forReadingAtPath: path) else {
        throw HostError("cannot read \(path)")
    }
    defer { try? handle.close() }
    var hasher = SHA256()
    while true {
        let chunk = handle.readData(ofLength: 4 * 1024 * 1024)
        if chunk.isEmpty { break }
        hasher.update(data: chunk)
    }
    return hasher.finalize().map { String(format: "%02x", $0) }.joined()
}

let arguments = Array(CommandLine.arguments.dropFirst())

let kernelPath: String
let rootDiskPath: String
let commandLine: String
let consolePath: String
let receiptPath: String
let timeoutSeconds: Double
let dryRun = arguments.contains("--dry-run")

do {
    kernelPath = try option("kernel", arguments)
    rootDiskPath = try option("root-disk", arguments)
    commandLine = try option("cmdline", arguments)
    consolePath = try option("console", arguments)
    receiptPath = try option("receipt", arguments)
    timeoutSeconds = Double(try option("timeout", arguments)) ?? 0
} catch {
    fail("\(error)")
}

if timeoutSeconds <= 0 {
    fail("--timeout must be a positive number of seconds")
}

// The caller states the digests it intends to boot. This program recomputes
// them from the files it was actually handed and refuses on any disagreement,
// so a receipt naming a digest is a statement about bytes that were read here.
let expectedKernelDigest = (try? option("kernel-sha256", arguments)) ?? ""
let expectedRootDiskDigest = (try? option("root-disk-sha256", arguments)) ?? ""
if expectedKernelDigest.isEmpty || expectedRootDiskDigest.isEmpty {
    fail("--kernel-sha256 and --root-disk-sha256 are required")
}

let kernelDigest: String
let rootDiskDigest: String
do {
    kernelDigest = try sha256(ofFileAt: kernelPath)
    rootDiskDigest = try sha256(ofFileAt: rootDiskPath)
} catch {
    fail("\(error)")
}

if kernelDigest != expectedKernelDigest {
    fail("kernel digest mismatch: expected \(expectedKernelDigest), read \(kernelDigest)")
}
if rootDiskDigest != expectedRootDiskDigest {
    fail("root disk digest mismatch: expected \(expectedRootDiskDigest), read \(rootDiskDigest)")
}

let configuration = VZVirtualMachineConfiguration()

let bootLoader = VZLinuxBootLoader(kernelURL: URL(fileURLWithPath: kernelPath))
bootLoader.commandLine = commandLine
configuration.bootLoader = bootLoader
configuration.cpuCount = FIXED_CPU_COUNT
configuration.memorySize = FIXED_MEMORY_BYTES

FileManager.default.createFile(atPath: consolePath, contents: nil)
guard let consoleWriter = FileHandle(forWritingAtPath: consolePath) else {
    fail("cannot open the console transcript for writing at \(consolePath)")
}
// The guest gets a console it can write to and an input side nothing ever
// writes to. `FileHandle.nullDevice` is rejected here -- the attachment wants a
// real descriptor -- and the host's own stdin must not be handed over, since
// that would let whatever started this program type into the guest. A pipe held
// open for the process's lifetime is an input that stays silent.
let consoleInput = Pipe()
let serial = VZVirtioConsoleDeviceSerialPortConfiguration()
serial.attachment = VZFileHandleSerialPortAttachment(
    fileHandleForReading: consoleInput.fileHandleForReading,
    fileHandleForWriting: consoleWriter
)
configuration.serialPorts = [serial]

do {
    let attachment = try VZDiskImageStorageDeviceAttachment(
        url: URL(fileURLWithPath: rootDiskPath),
        readOnly: true
    )
    configuration.storageDevices = [VZVirtioBlockDeviceConfiguration(attachment: attachment)]
} catch {
    fail("cannot attach the sealed root disk read-only: \(error)")
}

// Left empty on purpose, and then checked below rather than assumed.
configuration.networkDevices = []
configuration.directorySharingDevices = []
configuration.socketDevices = []

if !configuration.networkDevices.isEmpty {
    fail("a network device reached the configuration")
}
if !configuration.directorySharingDevices.isEmpty {
    fail("a shared directory reached the configuration")
}
if !configuration.socketDevices.isEmpty {
    fail("a socket device reached the configuration")
}
if configuration.storageDevices.count != 1 {
    fail("expected exactly one storage device, built \(configuration.storageDevices.count)")
}

do {
    try configuration.validate()
} catch {
    fail("the machine configuration is not valid: \(error)")
}

func writeReceipt(outcome: String, detail: String, startedAt: Date?, stoppedAt: Date?) {
    var receipt: [String: Any] = [
        "schema": "boole.native-shadow.mac3-closed-local-boot-run.v1",
        "outcome": outcome,
        "detail": detail,
        "dryRun": dryRun,
        "kernel": ["path": kernelPath, "sha256": kernelDigest],
        "rootDisk": [
            "path": rootDiskPath,
            "sha256": rootDiskDigest,
            "attachedReadOnly": true,
        ],
        "kernelCommandLine": commandLine,
        "machine": [
            "cpuCount": FIXED_CPU_COUNT,
            "memoryBytes": FIXED_MEMORY_BYTES,
            "networkDevices": configuration.networkDevices.count,
            "sharedDirectories": configuration.directorySharingDevices.count,
            "socketDevices": configuration.socketDevices.count,
            "storageDevices": configuration.storageDevices.count,
            "serialPorts": configuration.serialPorts.count,
        ],
        "console": ["path": consolePath],
        "timeoutSeconds": timeoutSeconds,
    ]
    if let startedAt, let stoppedAt {
        receipt["ranForSeconds"] = stoppedAt.timeIntervalSince(startedAt)
    }
    let data = try? JSONSerialization.data(
        withJSONObject: receipt,
        options: [.prettyPrinted, .sortedKeys]
    )
    if let data {
        try? data.write(to: URL(fileURLWithPath: receiptPath))
    }
}

if dryRun {
    // Everything above this point is the whole configuration path. A dry run
    // exercises it and stops, so the one allowed boot is not spent proving that
    // an option name was spelled correctly.
    writeReceipt(
        outcome: "dry-run-configuration-valid",
        detail: "the configuration was built and validated; no machine was started",
        startedAt: nil,
        stoppedAt: nil
    )
    print("mac3-boot: dry run ok")
    exit(0)
}

let queue = DispatchQueue(label: "boole.mac3.closed-local-boot")
let machine = VZVirtualMachine(configuration: configuration, queue: queue)

final class StopWatcher: NSObject, VZVirtualMachineDelegate {
    var stopped = false
    var reason = ""
    func guestDidStop(_ virtualMachine: VZVirtualMachine) {
        stopped = true
        reason = "the guest stopped itself"
    }
    func virtualMachine(_ virtualMachine: VZVirtualMachine, didStopWithError error: Error) {
        stopped = true
        reason = "the machine stopped with an error: \(error)"
    }
}

let watcher = StopWatcher()
queue.sync { machine.delegate = watcher }

let startedAt = Date()
var startError: String?
let started = DispatchSemaphore(value: 0)
queue.async {
    machine.start { result in
        if case .failure(let error) = result {
            startError = "\(error)"
        }
        started.signal()
    }
}
started.wait()

if let startError {
    writeReceipt(
        outcome: "did-not-start",
        detail: startError,
        startedAt: startedAt,
        stoppedAt: Date()
    )
    fail("the machine did not start: \(startError)")
}

let deadline = startedAt.addingTimeInterval(timeoutSeconds)
var stoppedByTimeout = false
while true {
    if queue.sync(execute: { watcher.stopped }) { break }
    if Date() >= deadline {
        stoppedByTimeout = true
        break
    }
    RunLoop.current.run(until: Date().addingTimeInterval(0.2))
}

if stoppedByTimeout {
    let done = DispatchSemaphore(value: 0)
    queue.async {
        if machine.canRequestStop {
            try? machine.requestStop()
        }
        done.signal()
    }
    done.wait()
    // A guest with no shutdown path of its own must not keep the host waiting
    // forever; the transcript up to here is the evidence either way.
    let graceDeadline = Date().addingTimeInterval(15)
    while Date() < graceDeadline && !queue.sync(execute: { watcher.stopped }) {
        RunLoop.current.run(until: Date().addingTimeInterval(0.2))
    }
    if !queue.sync(execute: { watcher.stopped }) {
        let forced = DispatchSemaphore(value: 0)
        queue.async {
            machine.stop { _ in forced.signal() }
        }
        _ = forced.wait(timeout: .now() + 15)
    }
}

let stoppedAt = Date()
try? consoleWriter.close()

let reason = queue.sync(execute: { watcher.reason })
writeReceipt(
    outcome: stoppedByTimeout ? "stopped-at-timeout" : "guest-stopped",
    detail: reason.isEmpty ? "the run reached its timeout and the host stopped it" : reason,
    startedAt: startedAt,
    stoppedAt: stoppedAt
)
print("mac3-boot: run complete")
exit(0)
