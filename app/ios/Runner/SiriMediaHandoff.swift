// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import Flutter
import Intents
import UIKit

/// Receives an INPlayMediaIntent that the Intents extension (app/ios/PsalmsIntents) answered with .handleInApp, and turns it into the same
/// JSON command as the App Intents (PsalmsVoice.post): {"action":"play","query":"<spoken text>"}, or {"action":"continue"} when nothing was spoken.
/// The Flutter side takes it through the channel (AppDelegate.swift).
enum SiriMediaHandoff {
  private static var lastText: String?
  private static var lastTime = Date.distantPast

  /// The spoken text: the identifier of the item that the extension resolved (it is the spoken text), else the words of the media search.
  static func spokenText(_ intent: INPlayMediaIntent) -> String {
    if let item = intent.mediaItems?.first {
      let text = (item.identifier ?? item.title ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
      if !text.isEmpty { return text }
    }
    var parts: [String] = []
    for part in [intent.mediaSearch?.mediaName, intent.mediaSearch?.albumName, intent.mediaSearch?.artistName] {
      let text = (part ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
      if !text.isEmpty { parts.append(text) }
    }
    return parts.joined(separator: " ")
  }

  /// Writes the command and wakes the Flutter side. Returns false if the intent is not a play-media intent.
  /// The same intent can arrive through the app delegate and the scene delegate: the same text within 5 seconds is taken once.
  @discardableResult
  static func handle(_ intent: INIntent?) -> Bool {
    guard let play = intent as? INPlayMediaIntent else { return false }
    let text = spokenText(play)
    if text == lastText && Date().timeIntervalSince(lastTime) < 5 { return true }
    lastText = text
    lastTime = Date()
    if text.isEmpty {
      PsalmsVoice.post(action: "continue")
    } else {
      PsalmsVoice.post(action: "play", query: text)
    }
    DispatchQueue.main.async {
      (UIApplication.shared.delegate as? AppDelegate)?.voiceChannel?.invokeMethod("pendingChanged", arguments: nil)
    }
    return true
  }

  /// The intent of an NSUserActivity that Siri handed over (activity.interaction exists for such activities).
  @discardableResult
  static func handle(_ activity: NSUserActivity) -> Bool {
    return handle(activity.interaction?.intent)
  }
}
