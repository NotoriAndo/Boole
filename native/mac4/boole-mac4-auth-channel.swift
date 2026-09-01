// Closed-local MAC.4 host for one authenticated AF_VSOCK handshake.
//
// The Mac owns every durable task, replay and verdict fact. The guest receives
// only a fresh liveness challenge over the socket device belonging to this
// exact VM. No IP network, shared directory or writable disk is configured.

import CryptoKit
import Darwin
import Foundation
import Virtualization

let FIXED_CPU_COUNT = 2
let FIXED_MEMORY_BYTES: UInt64 = 2 * 1024 * 1024 * 1024
let VSOCK_PORT: UInt32 = 4050
let FRAME_BYTES = 108
let MAGIC = Data("BOOLE4V1".utf8)
let CONTRACT_DIGEST_HEX = "4f2ec110d72f628207ac383668daff7bda6b568449fd315d8376aeb20ae08bbd"

struct HostError: Error, CustomStringConvertible {
    let description: String
    init(_ description: String) { self.description = description }
}

func fail(_ message: String) -> Never {
    FileHandle.standardError.write(Data("mac4-channel: \(message)\n".utf8))
    exit(2)
}

func option(_ name: String, _ arguments: [String]) throws -> String {
    guard let index = arguments.firstIndex(of: "--\(name)"),
          index + 1 < arguments.count else {
        throw HostError("missing required option --\(name)")
    }
    let value = arguments[index + 1]
    if value.hasPrefix("--") { throw HostError("--\(name) has no value") }
    return value
}

func dataFromHex(_ value: String, named name: String) throws -> Data {
    guard value.count == 64 else { throw HostError("\(name) must be 32 bytes") }
    var output = Data()
    output.reserveCapacity(32)
    var index = value.startIndex
    for _ in 0..<32 {
        let next = value.index(index, offsetBy: 2)
        guard let byte = UInt8(value[index..<next], radix: 16) else {
            throw HostError("\(name) is not lowercase hexadecimal")
        }
        output.append(byte)
        index = next
    }
    guard !output.allSatisfy({ $0 == 0 }) else {
        throw HostError("\(name) must not be all zero")
    }
    return output
}

func sha256(ofFileAt path: String) throws -> String {
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

func helloFrame(nonce: Data, bootBinding: Data) throws -> Data {
    guard nonce.count == 32, bootBinding.count == 32 else {
        throw HostError("handshake inputs are not 32 bytes")
    }
    var frame = Data()
    frame.append(MAGIC)
    frame.append(1)
    frame.append(contentsOf: [0, 0, 0])
    frame.append(try dataFromHex(CONTRACT_DIGEST_HEX, named: "contract digest"))
    frame.append(nonce)
    frame.append(bootBinding)
    guard frame.count == FRAME_BYTES else { throw HostError("hello frame size differs") }
    return frame
}

func validateReady(_ response: Data, nonce: Data, bootBinding: Data) throws {
    guard response.count == FRAME_BYTES else { throw HostError("ready frame size differs") }
    guard response[0..<8] == MAGIC else { throw HostError("ready magic differs") }
    guard response[8] == 2 else { throw HostError("ready frame kind differs") }
    guard response[9..<12] == Data([0, 0, 0]) else {
        throw HostError("ready reserved bytes differ")
    }
    let contract = try dataFromHex(CONTRACT_DIGEST_HEX, named: "contract digest")
    guard response[12..<44] == contract else { throw HostError("ready contract differs") }
    guard response[44..<76] == nonce else { throw HostError("ready nonce differs") }
    guard response[76..<108] == bootBinding else {
        throw HostError("ready boot binding differs")
    }
}

func writeAll(_ descriptor: Int32, data: Data) throws {
    try data.withUnsafeBytes { raw in
        guard let base = raw.baseAddress else { throw HostError("empty frame") }
        var offset = 0
        while offset < raw.count {
            let count = Darwin.write(descriptor, base.advanced(by: offset), raw.count - offset)
            if count < 0 {
                if errno == EINTR { continue }
                throw HostError("vsock write failed: \(String(cString: strerror(errno)))")
            }
            if count == 0 { throw HostError("vsock write made no progress") }
            offset += count
        }
    }
}

func readExact(_ descriptor: Int32, count: Int) throws -> Data {
    var output = Data(count: count)
    try output.withUnsafeMutableBytes { raw in
        guard let base = raw.baseAddress else { throw HostError("empty response buffer") }
        var offset = 0
        while offset < count {
            let found = Darwin.read(descriptor, base.advanced(by: offset), count - offset)
            if found < 0 {
                if errno == EINTR { continue }
                throw HostError("vsock read failed: \(String(cString: strerror(errno)))")
            }
            if found == 0 { throw HostError("vsock response ended early") }
            offset += found
        }
    }
    return output
}

let arguments = Array(CommandLine.arguments.dropFirst())
let dryRun = arguments.contains("--dry-run")

let kernelPath: String
let rootDiskPath: String
let consolePath: String
let receiptPath: String
let commandLine: String
let expectedKernelDigest: String
let expectedRootDiskDigest: String
let nonce: Data
let bootBinding: Data
let timeoutSeconds: Double

do {
    kernelPath = try option("kernel", arguments)
    rootDiskPath = try option("root-disk", arguments)
    consolePath = try option("console", arguments)
    receiptPath = try option("receipt", arguments)
    commandLine = try option("cmdline", arguments)
    expectedKernelDigest = try option("kernel-sha256", arguments)
    expectedRootDiskDigest = try option("root-disk-sha256", arguments)
    nonce = try dataFromHex(try option("nonce-hex", arguments), named: "nonce")
    bootBinding = try dataFromHex(
        try option("boot-binding-hex", arguments), named: "boot binding"
    )
    timeoutSeconds = Double(try option("timeout", arguments)) ?? 0
} catch {
    fail("\(error)")
}
if timeoutSeconds <= 0 { fail("--timeout must be positive") }

let kernelDigest: String
let rootDiskDigest: String
do {
    kernelDigest = try sha256(ofFileAt: kernelPath)
    rootDiskDigest = try sha256(ofFileAt: rootDiskPath)
} catch {
    fail("\(error)")
}
if kernelDigest != expectedKernelDigest { fail("kernel digest differs") }
if rootDiskDigest != expectedRootDiskDigest { fail("root disk digest differs") }

let configuration = VZVirtualMachineConfiguration()
let bootLoader = VZLinuxBootLoader(kernelURL: URL(fileURLWithPath: kernelPath))
bootLoader.commandLine = commandLine
configuration.bootLoader = bootLoader
configuration.cpuCount = FIXED_CPU_COUNT
configuration.memorySize = FIXED_MEMORY_BYTES

FileManager.default.createFile(atPath: consolePath, contents: nil)
guard let consoleWriter = FileHandle(forWritingAtPath: consolePath) else {
    fail("cannot open console transcript")
}
let consoleInput = Pipe()
let serial = VZVirtioConsoleDeviceSerialPortConfiguration()
serial.attachment = VZFileHandleSerialPortAttachment(
    fileHandleForReading: consoleInput.fileHandleForReading,
    fileHandleForWriting: consoleWriter
)
configuration.serialPorts = [serial]

do {
    let attachment = try VZDiskImageStorageDeviceAttachment(
        url: URL(fileURLWithPath: rootDiskPath), readOnly: true
    )
    configuration.storageDevices = [VZVirtioBlockDeviceConfiguration(attachment: attachment)]
} catch {
    fail("cannot attach root disk read-only: \(error)")
}

configuration.networkDevices = []
configuration.directorySharingDevices = []
configuration.socketDevices = [VZVirtioSocketDeviceConfiguration()]
if !configuration.networkDevices.isEmpty { fail("IP network device reached configuration") }
if !configuration.directorySharingDevices.isEmpty { fail("shared directory reached configuration") }
if configuration.socketDevices.count != 1 { fail("expected exactly one vsock device") }
if configuration.storageDevices.count != 1 { fail("expected exactly one read-only disk") }
do { try configuration.validate() } catch { fail("invalid VM configuration: \(error)") }

func writeReceipt(outcome: String, detail: String, startedAt: Date?, stoppedAt: Date?) {
    var value: [String: Any] = [
        "schema": "boole.native-shadow.mac4-authenticated-channel-run.v1",
        "outcome": outcome,
        "detail": detail,
        "dryRun": dryRun,
        "kernel": ["path": kernelPath, "sha256": kernelDigest],
        "rootDisk": ["path": rootDiskPath, "sha256": rootDiskDigest, "attachedReadOnly": true],
        "nonceHex": nonce.map { String(format: "%02x", $0) }.joined(),
        "bootTupleBindingHex": bootBinding.map { String(format: "%02x", $0) }.joined(),
        "contractSha256": CONTRACT_DIGEST_HEX,
        "machine": [
            "cpuCount": FIXED_CPU_COUNT,
            "memoryBytes": FIXED_MEMORY_BYTES,
            "networkDevices": configuration.networkDevices.count,
            "sharedDirectories": configuration.directorySharingDevices.count,
            "socketDevices": configuration.socketDevices.count,
            "storageDevices": configuration.storageDevices.count,
            "serialPorts": configuration.serialPorts.count,
        ],
        "vsock": ["port": VSOCK_PORT, "handshakeComplete": outcome == "authenticated-channel-pass"],
        "timeoutSeconds": timeoutSeconds,
    ]
    if let start = startedAt, let stop = stoppedAt {
        value["ranForSeconds"] = stop.timeIntervalSince(start)
    }
    if let data = try? JSONSerialization.data(withJSONObject: value, options: [.prettyPrinted, .sortedKeys]) {
        try? data.write(to: URL(fileURLWithPath: receiptPath))
    }
}

if dryRun {
    writeReceipt(
        outcome: "dry-run-configuration-valid",
        detail: "one vsock device, no IP network or share; no VM started",
        startedAt: nil,
        stoppedAt: nil
    )
    try? consoleWriter.close()
    print("mac4-channel: dry run ok")
    exit(0)
}

let queue = DispatchQueue(label: "boole.mac4.authenticated-channel")
let machine = VZVirtualMachine(configuration: configuration, queue: queue)

final class StopWatcher: NSObject, VZVirtualMachineDelegate {
    var stopped = false
    var reason = ""
    func guestDidStop(_ virtualMachine: VZVirtualMachine) {
        stopped = true
        reason = "guest stopped"
    }
    func virtualMachine(_ virtualMachine: VZVirtualMachine, didStopWithError error: Error) {
        stopped = true
        reason = "VM stopped with error: \(error)"
    }
}

let watcher = StopWatcher()
queue.sync { machine.delegate = watcher }
let startedAt = Date()
var startError: String?
let started = DispatchSemaphore(value: 0)
queue.async {
    machine.start { result in
        if case .failure(let error) = result { startError = "\(error)" }
        started.signal()
    }
}
started.wait()
if let startError {
    writeReceipt(outcome: "did-not-start", detail: startError, startedAt: startedAt, stoppedAt: Date())
    fail("VM did not start: \(startError)")
}

let deadline = startedAt.addingTimeInterval(timeoutSeconds)
let request: Data
do { request = try helloFrame(nonce: nonce, bootBinding: bootBinding) } catch { fail("\(error)") }
var handshakeComplete = false
var handshakeDetail = "guest relay did not accept before timeout"

while Date() < deadline && !queue.sync(execute: { watcher.stopped }) && !handshakeComplete {
    let completed = DispatchSemaphore(value: 0)
    var connectionResult: Result<VZVirtioSocketConnection, Error>?
    queue.async {
        guard let socket = machine.socketDevices.first as? VZVirtioSocketDevice else {
            connectionResult = .failure(HostError("VM has no virtio socket device"))
            completed.signal()
            return
        }
        socket.connect(toPort: VSOCK_PORT) { result in
            connectionResult = result
            completed.signal()
        }
    }
    _ = completed.wait(timeout: .now() + 3)
    if case .success(let connection) = connectionResult {
        do {
            try writeAll(connection.fileDescriptor, data: request)
            let response = try readExact(connection.fileDescriptor, count: FRAME_BYTES)
            try validateReady(response, nonce: nonce, bootBinding: bootBinding)
            handshakeComplete = true
            handshakeDetail = "fresh nonce, boot tuple and protocol binding matched"
        } catch {
            handshakeDetail = "guest response refused: \(error)"
        }
        connection.close()
    }
    if !handshakeComplete {
        RunLoop.current.run(until: Date().addingTimeInterval(0.25))
    }
}

let stopRequested = DispatchSemaphore(value: 0)
queue.async {
    if machine.canRequestStop { try? machine.requestStop() }
    stopRequested.signal()
}
stopRequested.wait()
let graceDeadline = Date().addingTimeInterval(10)
while Date() < graceDeadline && !queue.sync(execute: { watcher.stopped }) {
    RunLoop.current.run(until: Date().addingTimeInterval(0.2))
}
if !queue.sync(execute: { watcher.stopped }) {
    let forced = DispatchSemaphore(value: 0)
    queue.async { machine.stop { _ in forced.signal() } }
    _ = forced.wait(timeout: .now() + 10)
}

let stoppedAt = Date()
try? consoleWriter.close()
writeReceipt(
    outcome: handshakeComplete ? "authenticated-channel-pass" : "authenticated-channel-fail",
    detail: handshakeDetail,
    startedAt: startedAt,
    stoppedAt: stoppedAt
)
if !handshakeComplete { fail(handshakeDetail) }
print("mac4-channel: authenticated handshake complete")
