import Flutter
import UIKit

@main
@objc class AppDelegate: FlutterAppDelegate, FlutterImplicitEngineDelegate {
  override func application(
    _ application: UIApplication,
    didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]?
  ) -> Bool {
    excludeAppFilesFromBackup()
    return super.application(application, didFinishLaunchingWithOptions: launchOptions)
  }

  /// The pairing (server address, certificate, token) and the downloaded parts live in
  /// `Library/Application Support`. They stay out of the iCloud backup: a token must not travel to another
  /// device, and the audio can be loaded again from the server.
  private func excludeAppFilesFromBackup() {
    let manager = FileManager.default
    guard var folder = manager.urls(for: .applicationSupportDirectory, in: .userDomainMask).first else { return }
    try? manager.createDirectory(at: folder, withIntermediateDirectories: true)
    var values = URLResourceValues()
    values.isExcludedFromBackup = true
    try? folder.setResourceValues(values)
  }

  func didInitializeImplicitFlutterEngine(_ engineBridge: FlutterImplicitEngineBridge) {
    GeneratedPluginRegistrant.register(with: engineBridge.pluginRegistry)
    registerVoiceChannel(engineBridge)
  }

  /// Siri / Shortcuts (PsalmsIntents.swift) leave a JSON command in UserDefaults; Flutter takes it once with `takePending`.
  private func registerVoiceChannel(_ engineBridge: FlutterImplicitEngineBridge) {
    guard let registrar = engineBridge.pluginRegistry.registrar(forPlugin: "PsalmsVoice") else { return }
    let channel = FlutterMethodChannel(
      name: "com.the-abraham-box.psalm/voice", binaryMessenger: registrar.messenger())
    channel.setMethodCallHandler { call, result in
      guard call.method == "takePending" else {
        result(FlutterMethodNotImplemented)
        return
      }
      let defaults = UserDefaults.standard
      let pending = defaults.string(forKey: PsalmsVoice.pendingKey)
      defaults.removeObject(forKey: PsalmsVoice.pendingKey)
      result(pending)
    }
  }
}
