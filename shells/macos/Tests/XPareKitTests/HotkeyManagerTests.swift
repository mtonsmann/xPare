import Carbon.HIToolbox
import Foundation
import Testing

@testable import XPareKit

/// Covers the global-hotkey glue: the Carbon constant derivation, the process-wide
/// id/handler dispatch table, and the `HotkeyManager` register/unregister lifecycle.
/// `RegisterEventHotKey` succeeds headlessly on the test host, so the real Carbon
/// registration path is exercised — only the C event callback (driven by an actual
/// key press) is out of reach, and its sole effect, `HotkeyDispatch.fire(id:)`, is
/// tested directly.
@MainActor
@Suite struct HotkeyManagerTests {

    @Test func defaultComboMatchesCarbonConstants() {
        #expect(HotkeyManager.defaultKeyCode == UInt32(kVK_ANSI_V))
        #expect(HotkeyManager.defaultModifiers == UInt32(cmdKey | optionKey))
        // And the Carbon-free Settings mirror agrees with the Carbon-derived values.
        #expect(HotkeyCombo.defaultCombo.keyCode == HotkeyManager.defaultKeyCode)
        #expect(HotkeyCombo.defaultCombo.modifiers == HotkeyManager.defaultModifiers)
    }

    @Test func dispatchSignatureIsTheFourCharCodeSfSt() {
        // 'SfSt' packed big-endian into an OSType.
        let expected: OSType = Array("SfSt".utf8).reduce(OSType(0)) { ($0 << 8) | OSType($1) }
        #expect(HotkeyDispatch.signature == expected)
    }

    @Test func allocatedIdsAreProcessUnique() {
        let a = HotkeyDispatch.nextID()
        let b = HotkeyDispatch.nextID()
        let c = HotkeyDispatch.nextID()
        #expect(a != b)
        #expect(b != c)
        #expect(a != c)
    }

    @Test func fireRoutesToTheRegisteredHandlerById() {
        let id = HotkeyDispatch.nextID()
        var fires = 0
        HotkeyDispatch.shared.register(id: id) { fires += 1 }
        defer { HotkeyDispatch.shared.unregister(id: id) }

        HotkeyDispatch.shared.fire(id: id)
        HotkeyDispatch.shared.fire(id: id)
        #expect(fires == 2)
    }

    @Test func fireForAnUnregisteredIdIsANoOp() {
        let id = HotkeyDispatch.nextID()
        // Never registered → firing must not crash and must do nothing.
        HotkeyDispatch.shared.fire(id: id)
    }

    @Test func unregisterStopsRoutingToTheHandler() {
        let id = HotkeyDispatch.nextID()
        var fires = 0
        HotkeyDispatch.shared.register(id: id) { fires += 1 }
        HotkeyDispatch.shared.fire(id: id)
        HotkeyDispatch.shared.unregister(id: id)
        HotkeyDispatch.shared.fire(id: id)
        #expect(fires == 1, "no fires should land after unregister")
    }

    @Test func registerActivatesAndUnregisterTearsDown() {
        let manager = HotkeyManager(onFire: {})
        #expect(manager.isRegistered == false)

        #expect(manager.register() == true)
        #expect(manager.isRegistered == true)

        manager.unregister()
        #expect(manager.isRegistered == false)
    }

    @Test func unregisterIsIdempotent() {
        let manager = HotkeyManager(onFire: {})
        manager.register()
        manager.unregister()
        manager.unregister()  // second call must be a harmless no-op
        #expect(manager.isRegistered == false)
    }

    @Test func reRegisteringReplacesThePriorRegistrationWithoutLeaking() {
        let manager = HotkeyManager(onFire: {})
        #expect(manager.register(keyCode: UInt32(kVK_ANSI_V), modifiers: UInt32(cmdKey)) == true)
        // Registering again with a different combo must tear down the old one first and
        // still report a single live registration.
        #expect(
            manager.register(keyCode: UInt32(kVK_ANSI_B), modifiers: UInt32(cmdKey | optionKey))
                == true)
        #expect(manager.isRegistered == true)
        manager.unregister()
        #expect(manager.isRegistered == false)
    }

    // MARK: - The Carbon C trampoline

    /// Build a real `kEventHotKeyPressed` Carbon event carrying `hotKeyID`, or fail.
    private func makeHotKeyEvent(id: UInt32) throws -> EventRef {
        var event: EventRef?
        let created = CreateEvent(
            nil, OSType(kEventClassKeyboard), UInt32(kEventHotKeyPressed), 0, 0, &event)
        #expect(created == noErr)
        let ev = try #require(event)
        var hk = EventHotKeyID(signature: HotkeyDispatch.signature, id: id)
        let set = SetEventParameter(
            ev, EventParamName(kEventParamDirectObject), EventParamType(typeEventHotKeyID),
            MemoryLayout<EventHotKeyID>.size, &hk)
        #expect(set == noErr)
        return ev
    }

    /// Drive the C event handler the way Carbon would on a physical keypress: a
    /// synthesized event whose direct-object param is our `EventHotKeyID` must route
    /// through the dispatch table to the registered handler and report `noErr`.
    @Test func synthesizedHotKeyEventRoutesToTheRegisteredHandler() throws {
        let id = HotkeyDispatch.nextID()
        var fired = 0
        HotkeyDispatch.shared.register(id: id) { fired += 1 }
        defer { HotkeyDispatch.shared.unregister(id: id) }

        let status = hotKeyEventHandler(nil, try makeHotKeyEvent(id: id), nil)
        #expect(status == noErr)
        #expect(fired == 1)
    }

    @Test func nilEventIsReportedAsNotHandled() {
        #expect(hotKeyEventHandler(nil, nil, nil) == OSStatus(eventNotHandledErr))
    }

    @Test func eventWithoutHotKeyIdParamReturnsTheParameterError() throws {
        // An event of the right class/kind but missing the direct-object param: the
        // handler must surface GetEventParameter's failure status, not crash.
        var event: EventRef?
        let created = CreateEvent(
            nil, OSType(kEventClassKeyboard), UInt32(kEventHotKeyPressed), 0, 0, &event)
        #expect(created == noErr)
        let ev = try #require(event)
        #expect(hotKeyEventHandler(nil, ev, nil) != noErr)
    }
}
