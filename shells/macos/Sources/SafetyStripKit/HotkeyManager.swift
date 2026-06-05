import Foundation
import Carbon.HIToolbox

/// Registers a single global hotkey using the Carbon `RegisterEventHotKey` API.
///
/// We use Carbon here **on purpose**: it is the one global-hotkey mechanism that
/// needs neither the Accessibility nor the Input Monitoring entitlement, both of
/// which the project forbids. `CGEventTap` and `NSEvent.addGlobalMonitorForEvents`
/// would require those permissions and are therefore not used.
///
/// `@MainActor` because the Carbon event target is the main run loop's
/// dispatcher and the fire callback drives UI/clipboard work.
@MainActor
public final class HotkeyManager {
    /// Carbon constants for the default ⌥⌘V combo, re-derived from the headers so
    /// they always match the platform's real key/modifier values.
    public static var defaultKeyCode: UInt32 { UInt32(kVK_ANSI_V) }
    public static var defaultModifiers: UInt32 { UInt32(cmdKey | optionKey) }

    /// Called on the main actor each time the hotkey fires.
    private let onFire: () -> Void

    private var hotKeyRef: EventHotKeyRef?
    private var eventHandlerRef: EventHandlerRef?

    /// A process-unique id tying our `EventHotKeyID` to this instance so the
    /// shared C dispatcher can route the event back to the right manager.
    private let hotKeyID: UInt32

    /// Whether a hotkey is currently registered.
    public var isRegistered: Bool { hotKeyRef != nil }

    public init(onFire: @escaping () -> Void) {
        self.onFire = onFire
        self.hotKeyID = HotkeyDispatch.nextID()
    }

    /// Register the global hotkey. Re-registering first tears down any prior
    /// registration so we never leak handlers. Returns `false` if Carbon
    /// rejected the registration.
    @discardableResult
    public func register(
        keyCode: UInt32 = HotkeyManager.defaultKeyCode,
        modifiers: UInt32 = HotkeyManager.defaultModifiers
    ) -> Bool {
        unregister()

        // Install one process-wide handler for hot-key-pressed events the first
        // time any manager registers; route fires through the shared dispatch
        // table keyed by our id.
        HotkeyDispatch.shared.ensureHandlerInstalled(into: &eventHandlerRef)
        HotkeyDispatch.shared.register(id: hotKeyID) { [weak self] in
            self?.onFire()
        }

        var ref: EventHotKeyRef?
        let signature: OSType = HotkeyDispatch.signature
        let id = EventHotKeyID(signature: signature, id: hotKeyID)
        let status = RegisterEventHotKey(
            keyCode,
            modifiers,
            id,
            GetEventDispatcherTarget(),
            0,
            &ref
        )
        guard status == noErr, let ref else {
            HotkeyDispatch.shared.unregister(id: hotKeyID)
            return false
        }
        hotKeyRef = ref
        return true
    }

    /// Unregister the hotkey and drop our dispatch entry. Idempotent.
    public func unregister() {
        if let ref = hotKeyRef {
            UnregisterEventHotKey(ref)
            hotKeyRef = nil
        }
        HotkeyDispatch.shared.unregister(id: hotKeyID)
    }

    // Note: like `ClipboardMonitor`, teardown is the caller's responsibility via
    // `unregister()`. We avoid touching main-actor state from a `deinit`.
}

/// Process-wide glue between Carbon's single C event handler and the set of
/// `HotkeyManager` instances. Carbon hands us a C function pointer (no captured
/// context closures), so we keep an id -> handler table here and look up the
/// fired hot key by its `EventHotKeyID.id`.
@MainActor
final class HotkeyDispatch {
    static let shared = HotkeyDispatch()

    /// Four-char signature identifying our hot keys (`'SfSt'`).
    static let signature: OSType = {
        let chars: [UInt8] = Array("SfSt".utf8)
        return chars.reduce(OSType(0)) { ($0 << 8) | OSType($1) }
    }()

    private var handlers: [UInt32: () -> Void] = [:]
    private var nextHotKeyID: UInt32 = 1
    private var handlerInstalled = false

    /// Allocate a fresh, process-unique hot-key id.
    static func nextID() -> UInt32 {
        shared.allocateID()
    }

    private func allocateID() -> UInt32 {
        let id = nextHotKeyID
        nextHotKeyID &+= 1
        return id
    }

    func register(id: UInt32, handler: @escaping () -> Void) {
        handlers[id] = handler
    }

    func unregister(id: UInt32) {
        handlers.removeValue(forKey: id)
    }

    /// Dispatch a fired hot key by id. Called from the Carbon C callback.
    func fire(id: UInt32) {
        handlers[id]?()
    }

    /// Install the single Carbon event handler for `kEventHotKeyPressed` once.
    /// `outRef` receives the per-call handler ref so the owning manager can keep
    /// it alive; in practice all managers share one installed handler.
    func ensureHandlerInstalled(into outRef: inout EventHandlerRef?) {
        guard !handlerInstalled else { return }

        var eventType = EventTypeSpec(
            eventClass: OSType(kEventClassKeyboard),
            eventKind: UInt32(kEventHotKeyPressed)
        )

        var ref: EventHandlerRef?
        let status = InstallEventHandler(
            GetEventDispatcherTarget(),
            hotKeyEventHandler,
            1,
            &eventType,
            nil,
            &ref
        )
        if status == noErr {
            handlerInstalled = true
            eventHandlerRef = ref
            outRef = ref
        }
    }

    /// Held so the installed handler is never released for the process lifetime.
    private var eventHandlerRef: EventHandlerRef?
}

/// The C callback Carbon invokes for `kEventHotKeyPressed`. It extracts the
/// fired hot key's id and routes it through the main-actor dispatch table.
///
/// This is a top-level function (no captures) so it can be passed as an
/// `EventHandlerUPP`. Carbon delivers the event on the main run loop, so hopping
/// onto the main actor via `assumeIsolated` is safe.
private func hotKeyEventHandler(
    _ callRef: EventHandlerCallRef?,
    _ event: EventRef?,
    _ userData: UnsafeMutableRawPointer?
) -> OSStatus {
    guard let event else { return OSStatus(eventNotHandledErr) }

    var firedID = EventHotKeyID()
    let status = GetEventParameter(
        event,
        EventParamName(kEventParamDirectObject),
        EventParamType(typeEventHotKeyID),
        nil,
        MemoryLayout<EventHotKeyID>.size,
        nil,
        &firedID
    )
    guard status == noErr else { return status }

    let id = firedID.id
    MainActor.assumeIsolated {
        HotkeyDispatch.shared.fire(id: id)
    }
    return noErr
}
