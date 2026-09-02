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
let PROXY_VSOCK_PORT: UInt32 = 4051
let FRAME_BYTES = 108
let MAGIC = Data("BOOLE4V1".utf8)
let CONTRACT_DIGEST_HEX = "4f2ec110d72f628207ac383668daff7bda6b568449fd315d8376aeb20ae08bbd"
let PROXY_MAGIC = Data("BOOLE4P1".utf8)
let PROXY_FRAME_BYTES = 120
let PROXY_CONTRACT_DIGEST_HEX = "74d2f8c0be187a0b3ff0c9a1272bd5cef6943222448b4c6e7f7a97f209763613"
let CONTROLLER_MAGIC = Data("BOOLE4C1".utf8)
let CONTROLLER_VERSION: UInt8 = 1
let CONTROLLER_HEADER_BYTES = 96
let CONTROLLER_FRAME_CAP_BYTES = 524_288
let CONTROLLER_FRAME_COUNT_CAP = 3
let CONTROLLER_CONTRACT_DIGEST_HEX = "98095abde0cb32cb5fb27edeaf5bc6c67f3df796ad3cda07b16f8b4484b9b713"

struct HostError: Error, CustomStringConvertible {
    let description: String
    init(_ description: String) { self.description = description }
}

struct LauncherPeer: Equatable {
    let pid: UInt32
    let uid: UInt32
    let gid: UInt32

    var json: [String: Any] {
        ["pid": pid, "uid": uid, "gid": gid]
    }
}

enum ControllerCommand: UInt8 {
    case qualification = 1
    case execution = 2
    case shutdown = 3

    var responseKind: UInt8 { rawValue | 0x80 }
    var requestFrameCount: Int {
        switch self {
        case .qualification: return 1
        case .execution: return 2
        case .shutdown: return 0
        }
    }
}

struct ControllerEnvelope {
    let command: ControllerCommand
    let requestID: Data
    let frames: [Data]
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

func optionalOption(_ name: String, _ arguments: [String]) throws -> String? {
    guard let index = arguments.firstIndex(of: "--\(name)") else { return nil }
    guard index + 1 < arguments.count else { throw HostError("--\(name) has no value") }
    let value = arguments[index + 1]
    if value.hasPrefix("--") { throw HostError("--\(name) has no value") }
    return value
}

func exactFrame(at path: String, cap: Int, named name: String) throws -> Data {
    let frame = try Data(contentsOf: URL(fileURLWithPath: path))
    guard frame.count >= 4 else { throw HostError("\(name) frame header is truncated") }
    let declared = frame.prefix(4).reduce(0) { ($0 << 8) | Int($1) }
    guard declared <= cap else { throw HostError("\(name) frame exceeds cap") }
    guard frame.count == declared + 4 else { throw HostError("\(name) frame length differs") }
    return frame
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

func appendUInt16(_ value: UInt16, to data: inout Data) {
    var bigEndian = value.bigEndian
    withUnsafeBytes(of: &bigEndian) { data.append(contentsOf: $0) }
}

func appendUInt32(_ value: UInt32, to data: inout Data) {
    var bigEndian = value.bigEndian
    withUnsafeBytes(of: &bigEndian) { data.append(contentsOf: $0) }
}

func uint16(_ data: Data, at offset: Int) -> UInt16 {
    data[offset..<offset + 2].reduce(0) { ($0 << 8) | UInt16($1) }
}

func uint32(_ data: Data, at offset: Int) -> UInt32 {
    data[offset..<offset + 4].reduce(0) { ($0 << 8) | UInt32($1) }
}

func controllerRequestID(command: ControllerCommand, frames: [Data]) -> Data {
    var content = Data([command.rawValue])
    for frame in frames {
        appendUInt32(UInt32(frame.count), to: &content)
        content.append(frame)
    }
    return Data(SHA256.hash(data: content))
}

func readHandleExact(_ handle: FileHandle, count: Int, allowCleanEOF: Bool = false) throws -> Data? {
    var output = Data()
    while output.count < count {
        let next = try handle.read(upToCount: count - output.count) ?? Data()
        if next.isEmpty {
            if allowCleanEOF && output.isEmpty { return nil }
            throw HostError("controller stream ended before exact frame")
        }
        output.append(next)
    }
    return output
}

func readControllerEnvelope(_ handle: FileHandle) throws -> ControllerEnvelope? {
    guard let header = try readHandleExact(
        handle, count: CONTROLLER_HEADER_BYTES, allowCleanEOF: true
    ) else { return nil }
    guard header[0..<8] == CONTROLLER_MAGIC,
          header[8] == CONTROLLER_VERSION,
          header[48..<80] == (try dataFromHex(
              CONTROLLER_CONTRACT_DIGEST_HEX, named: "controller contract digest"
          )),
          header[80..<96].allSatisfy({ $0 == 0 }) else {
        throw HostError("controller request header differs")
    }
    guard let command = ControllerCommand(rawValue: header[9]) else {
        throw HostError("controller request command differs")
    }
    let frameCount = Int(uint16(header, at: 10))
    let payloadBytes = Int(uint32(header, at: 12))
    guard frameCount == command.requestFrameCount,
          frameCount <= CONTROLLER_FRAME_COUNT_CAP,
          payloadBytes <= CONTROLLER_FRAME_COUNT_CAP * (CONTROLLER_FRAME_CAP_BYTES + 4) else {
        throw HostError("controller request shape exceeds contract")
    }
    let requestID = Data(header[16..<48])
    let payload = try readHandleExact(handle, count: payloadBytes) ?? Data()
    var offset = 0
    var frames: [Data] = []
    for _ in 0..<frameCount {
        guard payload.count - offset >= 4 else {
            throw HostError("controller embedded frame header is truncated")
        }
        let length = Int(uint32(payload, at: offset))
        offset += 4
        guard length <= CONTROLLER_FRAME_CAP_BYTES, payload.count - offset >= length else {
            throw HostError("controller embedded frame is truncated or oversized")
        }
        frames.append(Data(payload[offset..<offset + length]))
        offset += length
    }
    guard offset == payload.count else { throw HostError("controller request has trailing bytes") }
    guard requestID == controllerRequestID(command: command, frames: frames) else {
        throw HostError("controller request binding differs")
    }
    return ControllerEnvelope(command: command, requestID: requestID, frames: frames)
}

func writeControllerResponse(
    _ handle: FileHandle,
    request: ControllerEnvelope,
    launcherPeer: LauncherPeer?,
    frames: [Data]
) throws {
    let expectedFrames: Int
    switch request.command {
    case .qualification: expectedFrames = 2
    case .execution: expectedFrames = 3
    case .shutdown: expectedFrames = 0
    }
    guard frames.count == expectedFrames, frames.count <= CONTROLLER_FRAME_COUNT_CAP else {
        throw HostError("controller response frame count differs")
    }
    if request.command == .shutdown {
        guard launcherPeer == nil else { throw HostError("shutdown response has launcher peer") }
    } else {
        guard let peer = launcherPeer, peer.pid != 0, peer.uid == 0, peer.gid == 0 else {
            throw HostError("controller response launcher peer is not root")
        }
    }
    var payload = Data()
    for frame in frames {
        guard frame.count <= CONTROLLER_FRAME_CAP_BYTES else {
            throw HostError("controller response frame exceeds cap")
        }
        appendUInt32(UInt32(frame.count), to: &payload)
        payload.append(frame)
    }
    var header = Data()
    header.append(CONTROLLER_MAGIC)
    header.append(CONTROLLER_VERSION)
    header.append(request.command.responseKind)
    appendUInt16(UInt16(frames.count), to: &header)
    appendUInt32(UInt32(payload.count), to: &header)
    header.append(request.requestID)
    header.append(try dataFromHex(
        CONTROLLER_CONTRACT_DIGEST_HEX, named: "controller contract digest"
    ))
    if let peer = launcherPeer {
        appendUInt32(peer.pid, to: &header)
        appendUInt32(peer.uid, to: &header)
        appendUInt32(peer.gid, to: &header)
    } else {
        header.append(contentsOf: [UInt8](repeating: 0, count: 12))
    }
    header.append(contentsOf: [UInt8](repeating: 0, count: 4))
    guard header.count == CONTROLLER_HEADER_BYTES else {
        throw HostError("controller response header size differs")
    }
    try handle.write(contentsOf: header + payload)
}

func validateEmbeddedLauncherFrame(_ frame: Data, cap: Int, named name: String) throws {
    guard frame.count >= 4 else { throw HostError("\(name) frame header is truncated") }
    let declared = Int(uint32(frame, at: 0))
    guard declared <= cap else { throw HostError("\(name) frame exceeds cap") }
    guard frame.count == declared + 4 else { throw HostError("\(name) frame length differs") }
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

func proxyNonce(base: Data, phase: UInt8) -> Data {
    var value = Data(base)
    value.append(phase)
    return Data(SHA256.hash(data: value))
}

func proxyOpenFrame(nonce: Data, bootBinding: Data, phase: UInt8) throws -> Data {
    guard phase == 1 || phase == 2 else { throw HostError("proxy phase differs") }
    var frame = Data()
    frame.append(PROXY_MAGIC)
    frame.append(1)
    frame.append(phase)
    frame.append(contentsOf: [0, 0])
    frame.append(try dataFromHex(PROXY_CONTRACT_DIGEST_HEX, named: "proxy contract digest"))
    frame.append(nonce)
    frame.append(bootBinding)
    frame.append(contentsOf: [UInt8](repeating: 0, count: 12))
    guard frame.count == PROXY_FRAME_BYTES else { throw HostError("proxy open size differs") }
    return frame
}

func validateProxyReady(
    _ response: Data, nonce: Data, bootBinding: Data, phase: UInt8
) throws -> LauncherPeer {
    guard response.count == PROXY_FRAME_BYTES else { throw HostError("proxy ready size differs") }
    guard response[0..<8] == PROXY_MAGIC, response[8] == 2, response[9] == phase else {
        throw HostError("proxy ready identity differs")
    }
    guard response[10..<12] == Data([0, 0]) else { throw HostError("proxy reserved bytes differ") }
    let contract = try dataFromHex(PROXY_CONTRACT_DIGEST_HEX, named: "proxy contract digest")
    guard response[12..<44] == contract else { throw HostError("proxy contract differs") }
    guard response[44..<76] == nonce, response[76..<108] == bootBinding else {
        throw HostError("proxy response belongs to another attempt or boot")
    }
    func word(_ offset: Int) -> UInt32 {
        response[offset..<(offset + 4)].reduce(0) { ($0 << 8) | UInt32($1) }
    }
    let peer = LauncherPeer(pid: word(108), uid: word(112), gid: word(116))
    guard peer.pid != 0, peer.uid == 0, peer.gid == 0 else {
        throw HostError("proxy launcher peer is not the root supervisor")
    }
    return peer
}

func controllerDryRunProxyReady(phase: UInt8, peer: LauncherPeer) throws -> Data {
    var frame = Data()
    frame.append(PROXY_MAGIC)
    frame.append(2)
    frame.append(phase)
    frame.append(contentsOf: [0, 0])
    frame.append(try dataFromHex(PROXY_CONTRACT_DIGEST_HEX, named: "proxy contract digest"))
    frame.append(contentsOf: [UInt8](repeating: 0x11, count: 32))
    frame.append(contentsOf: [UInt8](repeating: 0x22, count: 32))
    appendUInt32(peer.pid, to: &frame)
    appendUInt32(peer.uid, to: &frame)
    appendUInt32(peer.gid, to: &frame)
    guard frame.count == PROXY_FRAME_BYTES else { throw HostError("proxy ready size differs") }
    return frame
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

func readFrame(_ descriptor: Int32, cap: Int, named name: String) throws -> Data {
    let header = try readExact(descriptor, count: 4)
    let declared = header.reduce(0) { ($0 << 8) | Int($1) }
    guard declared <= cap else { throw HostError("\(name) frame exceeds cap") }
    var frame = Data(header)
    frame.append(try readExact(descriptor, count: declared))
    return frame
}

func requireSocketEOF(_ descriptor: Int32, named name: String) throws {
    var byte: UInt8 = 0
    while true {
        let count = Darwin.read(descriptor, &byte, 1)
        if count == 0 { return }
        if count < 0, errno == EINTR { continue }
        if count < 0 { throw HostError("\(name) EOF read failed: \(String(cString: strerror(errno)))") }
        throw HostError("\(name) returned trailing bytes")
    }
}

let arguments = Array(CommandLine.arguments.dropFirst())
let dryRun = arguments.contains("--dry-run")
let proxyDryRun = arguments.contains("--proxy-dry-run")
let controllerProtocolDryRun = arguments.contains("--controller-protocol-dry-run")
let controllerStdio = arguments.contains("--controller-stdio")

if controllerProtocolDryRun {
    let peer = LauncherPeer(pid: 4242, uid: 0, gid: 0)
    do {
        while let request = try readControllerEnvelope(.standardInput) {
            switch request.command {
            case .qualification:
                try writeControllerResponse(
                    .standardOutput,
                    request: request,
                    launcherPeer: peer,
                    frames: [
                        try controllerDryRunProxyReady(phase: 1, peer: peer),
                        request.frames[0],
                    ]
                )
            case .execution:
                try writeControllerResponse(
                    .standardOutput,
                    request: request,
                    launcherPeer: peer,
                    frames: [
                        try controllerDryRunProxyReady(phase: 2, peer: peer),
                        request.frames[0],
                        request.frames[1],
                    ]
                )
            case .shutdown:
                try writeControllerResponse(
                    .standardOutput, request: request, launcherPeer: nil, frames: []
                )
                exit(0)
            }
        }
        exit(0)
    } catch {
        fail("controller protocol dry run failed: \(error)")
    }
}

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
let proxyQualificationHelloPath: String?
let proxyExecutionHelloPath: String?
let proxyExecutionRequestPath: String?
let proxyQualificationReadyPath: String?
let proxyExecutionReadyPath: String?
let proxyExecutionReportPath: String?

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
    proxyQualificationHelloPath = try optionalOption("proxy-qualification-hello", arguments)
    proxyExecutionHelloPath = try optionalOption("proxy-execution-hello", arguments)
    proxyExecutionRequestPath = try optionalOption("proxy-execution-request", arguments)
    proxyQualificationReadyPath = try optionalOption("proxy-qualification-ready-out", arguments)
    proxyExecutionReadyPath = try optionalOption("proxy-execution-ready-out", arguments)
    proxyExecutionReportPath = try optionalOption("proxy-execution-report-out", arguments)
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

let proxyPaths = [
    proxyQualificationHelloPath,
    proxyExecutionHelloPath,
    proxyExecutionRequestPath,
]
let proxyConfigured = proxyPaths.allSatisfy { $0 != nil }
if proxyPaths.contains(where: { $0 != nil }) && !proxyConfigured {
    fail("all three proxy frame paths must be supplied together")
}
if proxyDryRun && (!dryRun || !proxyConfigured) {
    fail("--proxy-dry-run requires --dry-run and all proxy frame paths")
}
let proxyOutputPaths = [
    proxyQualificationReadyPath,
    proxyExecutionReadyPath,
    proxyExecutionReportPath,
]
if proxyConfigured && !proxyDryRun && !proxyOutputPaths.allSatisfy({ $0 != nil }) {
    fail("a real proxy run requires all three proxy output paths")
}
if !proxyConfigured && proxyOutputPaths.contains(where: { $0 != nil }) {
    fail("proxy output paths require all three proxy input frames")
}
if controllerStdio && (dryRun || proxyConfigured || proxyOutputPaths.contains(where: { $0 != nil })) {
    fail("--controller-stdio cannot be combined with dry-run or one-shot proxy paths")
}
if proxyConfigured {
    do {
        _ = try exactFrame(
            at: proxyQualificationHelloPath!, cap: 131_072, named: "qualification hello"
        )
        _ = try exactFrame(
            at: proxyExecutionHelloPath!, cap: 131_072, named: "execution hello"
        )
        _ = try exactFrame(
            at: proxyExecutionRequestPath!, cap: 131_072, named: "execution request"
        )
    } catch {
        fail("\(error)")
    }
}

if proxyDryRun {
    let value: [String: Any] = [
        "schema": "boole.native-shadow.mac4-authenticated-channel-run.v1",
        "outcome": "proxy-dry-run-inputs-valid",
        "dryRun": true,
        "executionProxy": [
            "configured": true,
            "persistentController": false,
            "port": PROXY_VSOCK_PORT,
            "sessions": ["qualification", "execution"],
        ],
    ]
    if let data = try? JSONSerialization.data(
        withJSONObject: value, options: [.prettyPrinted, .sortedKeys]
    ) {
        try? data.write(to: URL(fileURLWithPath: receiptPath))
    }
    print("mac4-channel: proxy dry run ok")
    exit(0)
}

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

var proxyLauncherPeer: LauncherPeer?

func writeReceipt(outcome: String, detail: String, startedAt: Date?, stoppedAt: Date?) {
    let persistentControllerConfigured = controllerStdio
    var executionProxy: [String: Any] = [
        "configured": proxyConfigured || persistentControllerConfigured,
        "port": PROXY_VSOCK_PORT,
        "sessions": (proxyConfigured || persistentControllerConfigured)
            ? ["qualification", "execution"] : [],
        "persistentController": persistentControllerConfigured,
    ]
    if let peer = proxyLauncherPeer {
        executionProxy["launcherPeer"] = peer.json
    }
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
        "executionProxy": executionProxy,
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

func connectGuest(port: UInt32) throws -> VZVirtioSocketConnection {
    let completed = DispatchSemaphore(value: 0)
    var connectionResult: Result<VZVirtioSocketConnection, Error>?
    queue.async {
        guard let socket = machine.socketDevices.first as? VZVirtioSocketDevice else {
            connectionResult = .failure(HostError("VM has no virtio socket device"))
            completed.signal()
            return
        }
        socket.connect(toPort: port) { result in
            connectionResult = result
            completed.signal()
        }
    }
    guard completed.wait(timeout: .now() + 3) == .success else {
        throw HostError("guest vsock port \(port) did not connect before timeout")
    }
    guard let result = connectionResult else { throw HostError("guest connection has no result") }
    return try result.get()
}

func openProxy(
    phase: UInt8, nonceBase: Data? = nil
) throws -> (VZVirtioSocketConnection, LauncherPeer, Data) {
    let connection = try connectGuest(port: PROXY_VSOCK_PORT)
    do {
        let fresh = proxyNonce(base: nonceBase ?? nonce, phase: phase)
        try writeAll(
            connection.fileDescriptor,
            data: try proxyOpenFrame(nonce: fresh, bootBinding: bootBinding, phase: phase)
        )
        let ready = try readExact(connection.fileDescriptor, count: PROXY_FRAME_BYTES)
        let peer = try validateProxyReady(
            ready, nonce: fresh, bootBinding: bootBinding, phase: phase
        )
        return (connection, peer, ready)
    } catch {
        connection.close()
        throw error
    }
}

func runPersistentController() throws -> LauncherPeer? {
    var qualifiedPeer: LauncherPeer?
    while let request = try readControllerEnvelope(.standardInput) {
        switch request.command {
        case .qualification:
            guard qualifiedPeer == nil else { throw HostError("controller qualified twice") }
            try validateEmbeddedLauncherFrame(
                request.frames[0], cap: 131_072, named: "qualification hello"
            )
            let (connection, peer, proxyReady) = try openProxy(
                phase: 1, nonceBase: request.requestID
            )
            do {
                try writeAll(connection.fileDescriptor, data: request.frames[0])
                try closeWrite(connection, named: "qualification")
                let launcherReady = try readFrame(
                    connection.fileDescriptor, cap: 65_536, named: "qualification ready"
                )
                try requireSocketEOF(connection.fileDescriptor, named: "qualification proxy")
                connection.close()
                qualifiedPeer = peer
                try writeControllerResponse(
                    .standardOutput,
                    request: request,
                    launcherPeer: peer,
                    frames: [proxyReady, launcherReady]
                )
            } catch {
                connection.close()
                throw error
            }
        case .execution:
            guard let qualifiedPeer else { throw HostError("execution preceded qualification") }
            try validateEmbeddedLauncherFrame(
                request.frames[0], cap: 131_072, named: "execution hello"
            )
            try validateEmbeddedLauncherFrame(
                request.frames[1], cap: 131_072, named: "execution request"
            )
            let (connection, peer, proxyReady) = try openProxy(
                phase: 2, nonceBase: request.requestID
            )
            guard peer == qualifiedPeer else {
                connection.close()
                throw HostError("proxy launcher peer changed after qualification")
            }
            do {
                try writeAll(connection.fileDescriptor, data: request.frames[0])
                let launcherReady = try readFrame(
                    connection.fileDescriptor, cap: 65_536, named: "execution ready"
                )
                try writeAll(connection.fileDescriptor, data: request.frames[1])
                try closeWrite(connection, named: "execution")
                let launcherReport = try readFrame(
                    connection.fileDescriptor, cap: 65_536, named: "execution report"
                )
                try requireSocketEOF(connection.fileDescriptor, named: "execution proxy")
                connection.close()
                try writeControllerResponse(
                    .standardOutput,
                    request: request,
                    launcherPeer: peer,
                    frames: [proxyReady, launcherReady, launcherReport]
                )
            } catch {
                connection.close()
                throw error
            }
        case .shutdown:
            try writeControllerResponse(
                .standardOutput, request: request, launcherPeer: nil, frames: []
            )
            return qualifiedPeer
        }
    }
    throw HostError("controller input ended before explicit shutdown")
}

func closeWrite(_ connection: VZVirtioSocketConnection, named name: String) throws {
    if Darwin.shutdown(connection.fileDescriptor, SHUT_WR) != 0 {
        throw HostError("\(name) write shutdown failed: \(String(cString: strerror(errno)))")
    }
}

func atomicWrite(_ data: Data, to path: String) throws {
    try data.write(to: URL(fileURLWithPath: path), options: .atomic)
}

let deadline = startedAt.addingTimeInterval(timeoutSeconds)
let request: Data
do { request = try helloFrame(nonce: nonce, bootBinding: bootBinding) } catch { fail("\(error)") }
var handshakeComplete = false
var handshakeDetail = "guest relay did not accept before timeout"

while Date() < deadline && !queue.sync(execute: { watcher.stopped }) && !handshakeComplete {
    do {
        let connection = try connectGuest(port: VSOCK_PORT)
        try writeAll(connection.fileDescriptor, data: request)
        let response = try readExact(connection.fileDescriptor, count: FRAME_BYTES)
        try validateReady(response, nonce: nonce, bootBinding: bootBinding)
        handshakeComplete = true
        handshakeDetail = "fresh nonce, boot tuple and protocol binding matched"
        connection.close()
    } catch {
        handshakeDetail = "guest response refused: \(error)"
    }
    if !handshakeComplete {
        RunLoop.current.run(until: Date().addingTimeInterval(0.25))
    }
}

var proxyComplete = !proxyConfigured && !controllerStdio
if handshakeComplete && controllerStdio {
    do {
        proxyLauncherPeer = try runPersistentController()
        proxyComplete = true
        handshakeDetail += "; persistent controller stopped cleanly"
    } catch {
        proxyComplete = false
        handshakeDetail += "; persistent controller failed: \(error)"
    }
} else if handshakeComplete && proxyConfigured {
    do {
        let (qualification, qualificationPeer, _) = try openProxy(phase: 1)
        try writeAll(
            qualification.fileDescriptor,
            data: try exactFrame(
                at: proxyQualificationHelloPath!, cap: 131_072, named: "qualification hello"
            )
        )
        try closeWrite(qualification, named: "qualification")
        let qualificationReady = try readFrame(
            qualification.fileDescriptor, cap: 65_536, named: "qualification ready"
        )
        try requireSocketEOF(qualification.fileDescriptor, named: "qualification proxy")
        qualification.close()
        try atomicWrite(qualificationReady, to: proxyQualificationReadyPath!)

        let (execution, executionPeer, _) = try openProxy(phase: 2)
        guard qualificationPeer == executionPeer else {
            execution.close()
            throw HostError("proxy launcher peer changed after qualification")
        }
        proxyLauncherPeer = qualificationPeer
        try writeAll(
            execution.fileDescriptor,
            data: try exactFrame(
                at: proxyExecutionHelloPath!, cap: 131_072, named: "execution hello"
            )
        )
        let executionReady = try readFrame(
            execution.fileDescriptor, cap: 65_536, named: "execution ready"
        )
        try writeAll(
            execution.fileDescriptor,
            data: try exactFrame(
                at: proxyExecutionRequestPath!, cap: 131_072, named: "execution request"
            )
        )
        try closeWrite(execution, named: "execution")
        let executionReport = try readFrame(
            execution.fileDescriptor, cap: 65_536, named: "execution report"
        )
        try requireSocketEOF(execution.fileDescriptor, named: "execution proxy")
        execution.close()
        try atomicWrite(executionReady, to: proxyExecutionReadyPath!)
        try atomicWrite(executionReport, to: proxyExecutionReportPath!)
        proxyComplete = true
        handshakeDetail += "; qualification and execution proxy frames completed"
    } catch {
        proxyComplete = false
        handshakeDetail += "; execution proxy failed: \(error)"
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
    outcome: handshakeComplete && proxyComplete
        ? "authenticated-channel-pass" : "authenticated-channel-fail",
    detail: handshakeDetail,
    startedAt: startedAt,
    stoppedAt: stoppedAt
)
if !handshakeComplete || !proxyComplete { fail(handshakeDetail) }
if !controllerStdio { print("mac4-channel: authenticated handshake complete") }
