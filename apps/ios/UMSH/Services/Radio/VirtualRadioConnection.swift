import Foundation
import OSLog
import UMSHMobileCore

/// The LAN virtual radio, hosted in this process.
///
/// A companion radio with no hardware: the production device session,
/// running in the mobile core, whose PHY is the LAN's multicast group.
/// The app attaches to it with the same HDLC-Lite frames a serial link
/// carries, so the session above cannot tell it from a T1000-E. Frames
/// it sends reach the mesh through whichever radios on the network
/// forward them. Its group is a setting of its own, managed like any
/// radio's.
///
/// In-process hosting is what the Simulator and an unlisted network get:
/// the device runs only while the app does. It keeps running across a
/// detach, acknowledging and queueing, until the connection is released.
final class VirtualRadioConnection: UlcpRadioSession, RadioConnection, UlcpFrameLink,
    @unchecked Sendable {
    /// How long to wait before trying again when the device cannot start.
    private static let retryDelaySeconds: TimeInterval = 2

    /// The one virtual radio's identifier, fixed so the app remembers it
    /// like any other radio.
    private static let identifier = UUID(uuidString: "5B0E7C1A-8D3F-4E62-9A4B-6C1D2E3F4A5B")!

    private let stateDirectory: URL
    private var identifier: UUID { Self.identifier }
    private let decoder = MobileHdlcDecoder()
    private var radio: MobileVirtualRadio?
    private var attached = false
    /// False once the user disconnects, so a deliberate teardown does
    /// not immediately start again.
    private var wantsConnection = false

    init(stateDirectory: URL = VirtualRadioConnection.defaultStateDirectory) {
        self.stateDirectory = stateDirectory
        super.init(sessionQueue: DispatchQueue(
            label: "com.voria.umsh.radio.virtual",
            qos: .userInitiated
        ))
        adopt(link: self)
        snapshot.localIdentifier = Self.identifier
    }

    /// Where the virtual radio keeps its identity and saved
    /// configuration. Excluded from backups: the identity belongs to this
    /// device, as a radio's does to its own flash.
    static var defaultStateDirectory: URL {
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)
            .first ?? URL(fileURLWithPath: NSTemporaryDirectory())
        return base.appendingPathComponent("lan-radio", isDirectory: true)
    }

    private static let displayName = "Local network"

    // ------------------------------------------------------------------
    // Connection lifecycle
    // ------------------------------------------------------------------

    func connect() async throws {
        await withCheckedContinuation { result in
            sessionQueue.async { [self] in
                Self.logger.notice("action: connect to the LAN virtual radio")
                startAttaching()
                result.resume()
            }
        }
    }

    func autoConnect() async {
        await withCheckedContinuation { result in
            sessionQueue.async { [self] in
                startAttaching()
                result.resume()
            }
        }
    }

    func reconnect() async {
        await autoConnect()
    }

    func disconnect() async {
        await withCheckedContinuation { result in
            sessionQueue.async { [self] in
                Self.logger.notice("action: user pressed Disconnect")
                wantsConnection = false
                detach(problem: nil)
                result.resume()
            }
        }
    }

    /// The virtual radio's state is its own, like a radio's flash, and
    /// stays; forgetting it is disconnecting.
    func forget() async {
        await disconnect()
    }

    // ------------------------------------------------------------------
    // Discovery
    // ------------------------------------------------------------------

    /// One virtual radio. The list is a single entry so the existing
    /// picker works unchanged.
    func discoverRadios() async -> AsyncStream<[DiscoveredRadio]> {
        AsyncStream { continuation in
            continuation.yield([DiscoveredRadio(
                id: identifier,
                name: Self.displayName,
                // The sentinel the rest of the app already reads as
                // "no signal to report".
                rssiDBm: 127,
                isRemembered: true
            )])
            continuation.finish()
        }
    }

    func selectRadio(_ id: UUID) async throws {
        guard id == identifier else {
            throw RadioConnectionError.radioNotFound
        }
        try await connect()
    }

    func stopDiscovery() async {}

    // ------------------------------------------------------------------
    // UlcpFrameLink
    // ------------------------------------------------------------------

    var linkIsReady: Bool { attached }

    var linkID: UUID? { identifier }

    var linkName: String? { snapshot.name ?? Self.displayName }

    var linkIsBoundRadio: Bool { true }

    var linkCanReconnect: Bool { wantsConnection }

    func linkSend(frame: Data, rawTransactionID: UInt8?) {
        guard attached, let radio else { return }
        do {
            radio.receive(bytes: try UMSHMobileCore.ulcpHdlcEncode(frame: frame))
        } catch {
            terminateConnectionForFatalProtocolError(
                "The ULCP session produced an unsendable frame",
                name: linkName
            )
        }
    }

    func linkResetFraming() {
        decoder.reset()
    }

    func linkInvalidate(retrying: Bool) {
        if !retrying { wantsConnection = false }
        detach(problem: nil, preservingPublishedFailure: true)
        if retrying { scheduleRetry() }
    }

    func linkDidAttach() {}

    func linkDidReportName(_ name: String) {}

    func linkAbandonBinding() {
        wantsConnection = false
    }

    func linkDidEnterDfu() {
        wantsConnection = false
        detach(problem: nil)
    }

    // ------------------------------------------------------------------
    // The device
    // ------------------------------------------------------------------

    private func startAttaching() {
        wantsConnection = true
        guard !attached else { return }
        if radio == nil {
            do {
                try FileManager.default.createDirectory(
                    at: stateDirectory,
                    withIntermediateDirectories: true,
                    attributes: [.protectionKey: FileProtectionType.completeUntilFirstUserAuthentication]
                )
                // Choosing this radio is choosing the LAN, so it comes up
                // listening whatever it saved.
                radio = try MobileVirtualRadio.start(
                    stateDir: stateDirectory.path,
                    enablePhy: true,
                    listener: VirtualRadioListener(connection: self)
                )
            } catch {
                snapshot.linkState = .connecting
                snapshot.name = Self.displayName
                snapshot.localIdentifier = identifier
                snapshot.problemDescription = Self.describe(error)
                publish(snapshot)
                scheduleRetry()
                return
            }
        }
        snapshot.linkState = .connecting
        snapshot.name = Self.displayName
        snapshot.localIdentifier = identifier
        publish(snapshot)
        decoder.reset()
        attached = true
        radio?.attach()
        linkDidBecomeReady()
    }

    /// Detach from the device, which carries on queueing for as long as
    /// this connection keeps it.
    private func detach(problem: String?, preservingPublishedFailure: Bool = false) {
        guard attached else { return }
        attached = false
        radio?.detach()
        decoder.reset()
        if preservingPublishedFailure {
            sessionDidLoseLink()
        } else {
            linkDidClose(problem: problem)
        }
    }

    private func scheduleRetry() {
        sessionQueue.asyncAfter(deadline: .now() + Self.retryDelaySeconds) { [weak self] in
            guard let self, self.wantsConnection, !self.attached else { return }
            self.startAttaching()
        }
    }

    private static func describe(_ error: Error) -> String {
        switch error as? MobileVirtualRadioError {
        case .StateInUse:
            "The local network radio is in use elsewhere"
        default:
            "The local network radio could not start"
        }
    }

    /// Bytes from the device, on its worker thread.
    fileprivate func deviceDidSend(_ bytes: Data) {
        sessionQueue.async { [self] in
            guard attached else { return }
            for frame in decoder.push(bytes: bytes) {
                linkDidReceive(frame: frame)
            }
        }
    }
}

/// Carries the device's output onto the connection's queue. Holds the
/// connection weakly so a released one cannot be kept alive by its
/// device.
private final class VirtualRadioListener: MobileVirtualRadioListener, @unchecked Sendable {
    private weak var connection: VirtualRadioConnection?

    init(connection: VirtualRadioConnection) {
        self.connection = connection
    }

    func onWire(bytes: Data) {
        connection?.deviceDidSend(bytes)
    }

    /// In-process, the device queues only while the app is running and
    /// detached, and the app notices its own messages when it drains.
    func onQueued(notice: MobileQueuedNoticeRecord) {}
}
