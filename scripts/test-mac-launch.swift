// Launch the exact executable on the verified, mounted distribution image.
import CoreGraphics
import Foundation

let app = Process()
app.executableURL = URL(fileURLWithPath: CommandLine.arguments[1])
let dataDirectory = CommandLine.arguments[2]
var environment = ProcessInfo.processInfo.environment
environment["KLADDE_DATA_DIR"] = dataDirectory
app.environment = environment
let start = ProcessInfo.processInfo.systemUptime
try app.run()
defer {
    if app.isRunning { app.terminate(); app.waitUntilExit() }
}

func hasMainWindow() -> Bool {
    let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID)
    guard let windows = list as? [[String: Any]] else { return false }
    return windows.contains { window in
        let pid = window[kCGWindowOwnerPID as String] as? NSNumber
        let bounds = window[kCGWindowBounds as String] as? [String: NSNumber]
        return pid?.int32Value == app.processIdentifier && (bounds?["Width"]?.intValue ?? 0) >= 600
    }
}

while app.isRunning && !hasMainWindow() && ProcessInfo.processInfo.systemUptime - start < 30 {
    Thread.sleep(forTimeInterval: 0.05)
}
guard app.isRunning && hasMainWindow() else { throw NSError(domain: "Packaged Kladde did not open its window", code: 1) }
print("Packaged Mac app launch to visible window (ms)", Int((ProcessInfo.processInfo.systemUptime - start) * 1000))
let file = URL(fileURLWithPath: dataDirectory).appendingPathComponent("kladde.json")
let json = try JSONSerialization.jsonObject(with: Data(contentsOf: file)) as? [String: Any]
guard let subjects = json?["subjects"] as? [[String: Any]], !subjects.isEmpty else {
    throw NSError(domain: "Packaged Kladde did not initialize its study store", code: 2)
}
