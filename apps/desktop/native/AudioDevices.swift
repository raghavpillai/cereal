import AudioToolbox
import CoreAudio
import Foundation

/// The microphone the person chose in the transcript panel, by Core Audio
/// device UID. `nil` follows the Mac's default input. Read when a capture's
/// microphone starts; a capture that is already running keeps its device.
private final class PreferredInput: @unchecked Sendable {
    private let lock = NSLock()
    private var uid: String?
    func set(_ value: String?) { lock.withLock { uid = value } }
    func get() -> String? { lock.withLock { uid } }
}
private let preferredInput = PreferredInput()

private func deviceAddress(_ selector: AudioObjectPropertySelector,
                           scope: AudioObjectPropertyScope = kAudioObjectPropertyScopeGlobal) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress(mSelector: selector, mScope: scope, mElement: kAudioObjectPropertyElementMain)
}

private func deviceString(_ device: AudioDeviceID, _ selector: AudioObjectPropertySelector) -> String? {
    var address = deviceAddress(selector)
    var value: Unmanaged<CFString>?
    var size = UInt32(MemoryLayout<Unmanaged<CFString>?>.size)
    guard AudioObjectGetPropertyData(device, &address, 0, nil, &size, &value) == noErr, let value else { return nil }
    return value.takeRetainedValue() as String
}

private func inputChannels(_ device: AudioDeviceID) -> Int {
    var address = deviceAddress(kAudioDevicePropertyStreamConfiguration, scope: kAudioDevicePropertyScopeInput)
    var size: UInt32 = 0
    guard AudioObjectGetPropertyDataSize(device, &address, 0, nil, &size) == noErr, size > 0 else { return 0 }
    let raw = UnsafeMutableRawPointer.allocate(byteCount: Int(size), alignment: MemoryLayout<AudioBufferList>.alignment)
    defer { raw.deallocate() }
    guard AudioObjectGetPropertyData(device, &address, 0, nil, &size, raw) == noErr else { return 0 }
    let buffers = UnsafeMutableAudioBufferListPointer(raw.assumingMemoryBound(to: AudioBufferList.self))
    return buffers.reduce(0) { $0 + Int($1.mNumberChannels) }
}

private func defaultInputDevice() -> AudioDeviceID? {
    var address = deviceAddress(kAudioHardwarePropertyDefaultInputDevice)
    var device = AudioDeviceID(kAudioObjectUnknown)
    var size = UInt32(MemoryLayout<AudioDeviceID>.size)
    guard AudioObjectGetPropertyData(AudioObjectID(kAudioObjectSystemObject), &address, 0, nil, &size, &device) == noErr,
          device != kAudioObjectUnknown else { return nil }
    return device
}

/// Every device with input channels that a person could mean by "microphone":
/// not this app's own private system-audio aggregate, and not the aggregates
/// macOS makes for itself.
private func inputDevices() -> [(id: AudioDeviceID, uid: String, name: String)] {
    var address = deviceAddress(kAudioHardwarePropertyDevices)
    var size: UInt32 = 0
    let system = AudioObjectID(kAudioObjectSystemObject)
    guard AudioObjectGetPropertyDataSize(system, &address, 0, nil, &size) == noErr, size > 0 else { return [] }
    var ids = [AudioDeviceID](repeating: AudioDeviceID(kAudioObjectUnknown), count: Int(size) / MemoryLayout<AudioDeviceID>.size)
    guard AudioObjectGetPropertyData(system, &address, 0, nil, &size, &ids) == noErr else { return [] }
    return ids.compactMap { id in
        guard inputChannels(id) > 0,
              let uid = deviceString(id, kAudioDevicePropertyDeviceUID),
              let name = deviceString(id, kAudioObjectPropertyName),
              !uid.hasPrefix("CADefaultDeviceAggregate"), name != "Cereal System Audio" else { return nil }
        return (id, uid, name)
    }
}

/// The chosen microphone, if it is plugged in right now.
func preferredInputDeviceID() -> AudioDeviceID? {
    guard let uid = preferredInput.get() else { return nil }
    return inputDevices().first { $0.uid == uid }?.id
}

private func copyDeviceString(_ string: String, to buffer: UnsafeMutablePointer<CChar>?, capacity: Int) -> Int {
    let bytes = Array(string.utf8CString)
    if let buffer, capacity > 0 {
        if capacity >= bytes.count { _ = bytes.withUnsafeBufferPointer { memcpy(buffer, $0.baseAddress!, bytes.count) } }
        else { buffer[0] = 0 }
    }
    return bytes.count
}

/// `{"default": "<uid>" | null, "devices": [{"uid": …, "name": …}]}`. Returns
/// the bytes required including the NUL; a buffer that is too small is left
/// empty. Opens no device and asks macOS for no permission.
@_cdecl("mn_audio_input_devices")
public func mn_audio_input_devices(_ output: UnsafeMutablePointer<CChar>?, _ capacity: Int) -> Int {
    let devices = inputDevices()
    let standard = defaultInputDevice()
    let json: [String: Any] = [
        "default": devices.first { $0.id == standard }?.uid as Any? ?? NSNull(),
        "devices": devices.map { ["uid": $0.uid, "name": $0.name] },
    ]
    guard let data = try? JSONSerialization.data(withJSONObject: json, options: [.sortedKeys]),
          let string = String(data: data, encoding: .utf8) else { return 0 }
    return copyDeviceString(string, to: output, capacity: capacity)
}

/// Choose the microphone of the captures that start from now on, by device
/// UID. NULL or an empty string follows the Mac's default input.
@_cdecl("mn_audio_set_input_device")
public func mn_audio_set_input_device(_ uid: UnsafePointer<CChar>?) {
    let value = uid.flatMap { String(validatingCString: $0) }.flatMap { $0.isEmpty ? nil : $0 }
    preferredInput.set(value)
}

/// The first of the person's preferred languages, as macOS spells it
/// ("en-US"). Same buffer contract as the other string calls.
@_cdecl("mn_preferred_language")
public func mn_preferred_language(_ output: UnsafeMutablePointer<CChar>?, _ capacity: Int) -> Int {
    copyDeviceString(Locale.preferredLanguages.first ?? "", to: output, capacity: capacity)
}
