import Foundation

/// Where the LAN virtual radio lives, shared by the app and the network
/// extension that hosts it.
///
/// Both sit in the App Group container, which only this team's apps and
/// extensions can reach: the state, so the radio is the same device
/// wherever it runs, and the socket, whose reach is the security binding
/// of the link to it.
enum VirtualRadioPaths {
    static let appGroup = "group.com.voria.umsh"

    /// The App Group container, or this process's Application Support
    /// when the group is not available to it.
    private static var base: URL {
        if let group = FileManager.default.containerURL(
            forSecurityApplicationGroupIdentifier: appGroup
        ) {
            return group
        }
        return FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)
            .first ?? URL(fileURLWithPath: NSTemporaryDirectory())
    }

    /// The radio's saved configuration. Excluded from backups: it belongs
    /// to this phone, as a radio's does to its own flash.
    static var stateDirectory: URL {
        base.appendingPathComponent("lan-radio", isDirectory: true)
    }

    /// The extension's local socket. Kept short: a socket's path is
    /// limited to about 104 bytes, and the container's own path takes
    /// most of that.
    static var socketPath: String {
        base.appendingPathComponent("ulcp").path
    }
}
