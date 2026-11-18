import AppKit
import CoreGraphics
import CryptoKit
import Darwin
import Foundation
import Metal

private struct Outcome {
    let status: String
    let detail: String
    let exitCode: Int32

    static func pass() -> Self {
        Self(
            status: "Pass",
            detail: "An on-screen child window was observed and the verified application quit via AppKit.",
            exitCode: 0
        )
    }

    static func blocked(_ detail: String) -> Self {
        Self(status: "Blocked", detail: detail, exitCode: 78)
    }

    static func fail(_ detail: String) -> Self {
        Self(status: "Fail", detail: detail, exitCode: 1)
    }
}

private func ownedWindow(in windows: [[String: Any]], pid: pid_t) -> [String: Any]? {
    for window in windows {
        guard (window[kCGWindowOwnerPID as String] as? NSNumber)?.int32Value == pid,
              (window[kCGWindowLayer as String] as? NSNumber)?.intValue == 0,
              let id = window[kCGWindowNumber as String] as? NSNumber,
              let bounds = window[kCGWindowBounds as String] as? [String: Any],
              let rect = CGRect(dictionaryRepresentation: bounds as CFDictionary),
              rect.width > 0, rect.height > 0 else { continue }
        return [
            "id": id, "owner_pid": pid,
            "width": Double(rect.width), "height": Double(rect.height), "on_screen": true,
        ]
    }
    return nil
}

private func waitForExit(_ child: Process, seconds: TimeInterval) -> Bool {
    let deadline = ProcessInfo.processInfo.systemUptime + seconds
    while child.isRunning && ProcessInfo.processInfo.systemUptime < deadline {
        Thread.sleep(forTimeInterval: 0.10)
    }
    return !child.isRunning
}

private func verifiedApplication(_ child: Process, binary: URL) -> NSRunningApplication? {
    guard child.isRunning,
          let app = NSRunningApplication(processIdentifier: child.processIdentifier),
          app.executableURL?.resolvingSymlinksInPath().path == binary.path,
          child.isRunning else { return nil }
    return app
}

private func observe(_ child: Process, binary: URL, desktopAvailable: Bool, evidence: inout [String: Any]) -> Outcome {
    let deadline = ProcessInfo.processInfo.systemUptime + 15
    var sawWindowList = false
    while child.isRunning && ProcessInfo.processInfo.systemUptime < deadline {
        let options: CGWindowListOption = [.optionOnScreenOnly, .excludeDesktopElements]
        if let windows = CGWindowListCopyWindowInfo(options, kCGNullWindowID) as? [[String: Any]] {
            sawWindowList = true
            evidence["window_list_available"] = true
            let pid = child.processIdentifier
            if let window = ownedWindow(in: windows, pid: pid) {
                evidence["window_observed"] = true
                evidence["window"] = window
                if let app = verifiedApplication(child, binary: binary) {
                    evidence["target_verified"] = true
                    let requested = app.terminate()
                    evidence["appkit_quit_requested"] = requested
                    guard requested else {
                        return .fail("AppKit rejected Quit for the verified child application.")
                    }
                    guard waitForExit(child, seconds: 8) else {
                        return .fail("The child did not exit within eight seconds of AppKit Quit.")
                    }
                    return child.terminationReason == .exit && child.terminationStatus == 0
                        ? .pass()
                        : .fail("The child exited abnormally after AppKit Quit.")
                }
            }
        }
        Thread.sleep(forTimeInterval: 0.25)
    }

    if evidence["window"] != nil {
        return .blocked("The child's window was on screen, but AppKit did not expose a matching executable for Quit.")
    }
    if !desktopAvailable || !sawWindowList {
        return .blocked("The runner did not provide usable Quartz window, display, and Metal evidence.")
    }
    return .fail(child.isRunning
        ? "The child did not show an on-screen window within fifteen seconds."
        : "The child exited before an on-screen window was observed.")
}

private func runProbe(binary: URL, evidence: inout [String: Any]) throws -> Outcome {
    let environment = ProcessInfo.processInfo.environment
    let version = ProcessInfo.processInfo.operatingSystemVersion
    #if arch(arm64)
    let architecture = "arm64"
    #elseif arch(x86_64)
    let architecture = "x86_64"
    #else
    let architecture = "unknown"
    #endif
    evidence["probe_architecture"] = architecture
    evidence["runner_architecture"] = environment["RUNNER_ARCH"]
    evidence["runner_image_version"] = environment["ImageVersion"]
    evidence["os_version"] = ProcessInfo.processInfo.operatingSystemVersionString
    evidence["commit"] = environment["THREADLINE_PROBE_COMMIT"]
    let session = CGSessionCopyCurrentDictionary() as? [String: Any]
    evidence["quartz_session_available"] = session != nil
    evidence["session_on_console"] = session?[kCGSessionOnConsoleKey] as? Bool
    evidence["session_login_complete"] = session?[kCGSessionLoginDoneKey] as? Bool
    var displayCount: UInt32 = 0
    let displayResult = CGGetActiveDisplayList(0, nil, &displayCount)
    evidence["display_query_status"] = displayResult.rawValue
    evidence["active_displays"] = displayCount
    let metal = MTLCreateSystemDefaultDevice()
    evidence["metal_device_available"] = metal != nil
    evidence["metal_device_name"] = metal?.name
    let desktopAvailable = session != nil && displayResult == .success && displayCount > 0 && metal != nil

    let bytes = try Data(contentsOf: binary, options: .mappedIfSafe)
    evidence["binary_bytes"] = bytes.count
    evidence["binary_sha256"] = SHA256.hash(data: bytes).map { String(format: "%02x", $0) }.joined()
    guard architecture == "arm64", version.majorVersion == 15 else {
        return .blocked("This probe requires a macOS 15 arm64 process.")
    }

    let files = FileManager.default
    let scratch = files.temporaryDirectory.appendingPathComponent("threadline-macos-probe-\(UUID().uuidString)", isDirectory: true)
    try files.createDirectory(at: scratch, withIntermediateDirectories: false)
    var retainScratch = false
    defer {
        if !retainScratch { try? files.removeItem(at: scratch) }
    }

    let profile = scratch.appendingPathComponent("profile", isDirectory: true)
    try files.createDirectory(at: profile, withIntermediateDirectories: false)
    let child = Process()
    child.executableURL = binary
    child.currentDirectoryURL = scratch
    child.arguments = ["--e2e-report", scratch.appendingPathComponent("ui-observation.json").path]
    var childEnvironment = environment
    childEnvironment["THREADLINE_PROFILE_DIR"] = profile.path
    childEnvironment["THREADLINE_TEST_SEED"] = "12345"
    child.environment = childEnvironment
    child.standardInput = FileHandle.nullDevice
    child.standardOutput = FileHandle.nullDevice
    child.standardError = FileHandle.nullDevice
    try child.run()
    evidence["child_pid"] = child.processIdentifier
    let outcome = observe(child, binary: binary, desktopAvailable: desktopAvailable, evidence: &evidence)

    if child.isRunning {
        child.terminate()
        evidence["cleanup_signal_sent"] = true
        if !waitForExit(child, seconds: 3) {
            let app = verifiedApplication(child, binary: binary)
            evidence["cleanup_force_target_verified"] = app != nil
            if let app {
                evidence["cleanup_appkit_force_requested"] = app.forceTerminate()
                _ = waitForExit(child, seconds: 3)
            }
        }
        let exited = !child.isRunning
        evidence["cleanup_child_exited"] = exited
        guard exited else {
            retainScratch = true
            evidence["cleanup_retained_directory"] = scratch.path
            return .fail("The test child remained running after bounded cleanup. Its working directory was retained.")
        }
    }
    evidence["child_termination_status"] = child.terminationStatus
    evidence["child_termination_reason"] = child.terminationReason == .exit ? "exit" : "signal"
    let database = profile.appendingPathComponent("threadline.sqlite3")
    let databaseHeader = sqliteHeaderMatches(database)
    evidence["isolated_profile_sqlite_header"] = databaseHeader
    if outcome.status == "Pass" && !databaseHeader {
        return .fail("The app completed its window and Quit probe without creating an isolated SQLite database.")
    }
    return outcome
}

private func sqliteHeaderMatches(_ database: URL) -> Bool {
    guard let stream = try? FileHandle(forReadingFrom: database) else { return false }
    defer { try? stream.close() }
    guard let bytes = try? stream.read(upToCount: 16) else { return false }
    return bytes == Data("SQLite format 3\0".utf8)
}

private func main() -> Int32 {
    guard CommandLine.arguments.count == 3 else {
        print("Usage: xcrun swift macos-window-probe.swift <app binary> <JSON report>")
        return 1
    }
    let binary = URL(fileURLWithPath: CommandLine.arguments[1]).resolvingSymlinksInPath()
    let report = URL(fileURLWithPath: CommandLine.arguments[2])
    var evidence: [String: Any] = [
        "schema": 1,
        "scope": "Early macOS native window and AppKit quit probe",
        "window_list_available": false,
        "window_observed": false,
        "target_verified": false,
        "appkit_quit_requested": false,
    ]
    let outcome: Outcome
    do {
        outcome = try runProbe(binary: binary, evidence: &evidence)
    } catch {
        outcome = .fail("Probe setup or process launch failed with code \((error as NSError).code).")
    }
    evidence["status"] = outcome.status
    evidence["detail"] = outcome.detail
    do {
        let bytes = try JSONSerialization.data(withJSONObject: evidence, options: [.prettyPrinted, .sortedKeys])
        try bytes.write(to: report, options: .atomic)
    } catch {
        print("Could not write macOS probe evidence: \(error.localizedDescription)")
        return 1
    }
    print("macOS native window probe \(outcome.status). \(outcome.detail)")
    return outcome.exitCode
}

exit(main())
