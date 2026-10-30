// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import AppIntents
import Foundation

/// The hand-over from Siri / Shortcuts to the Flutter side: an intent writes one small JSON command into
/// UserDefaults, the app opens, and Flutter takes the command through the channel "com.the-abraham-box.psalm/voice"
/// (see AppDelegate.swift). Only playing and looking: no intent changes the library.
enum PsalmsVoice {
  /// The key of the command that waits for the app.
  static let pendingKey = "psalms.voice.pending"

  /// Writes `{"action": ..., "query": ...}`; `action` is one of continue, play, next, previous, new.
  static func post(action: String, query: String? = nil) {
    var command: [String: String] = ["action": action]
    if let query = query {
      command["query"] = query
    }
    guard let data = try? JSONSerialization.data(withJSONObject: command),
      let text = String(data: data, encoding: .utf8)
    else { return }
    UserDefaults.standard.set(text, forKey: pendingKey)
  }
}

@available(iOS 16.0, *)
struct ContinueListeningIntent: AppIntent {
  static let title: LocalizedStringResource = "Continue listening"
  static let description = IntentDescription("Continues where you stopped.")
  static let openAppWhenRun: Bool = true

  func perform() async throws -> some IntentResult & ProvidesDialog {
    PsalmsVoice.post(action: "continue")
    return .result(dialog: IntentDialog("Continuing."))
  }
}

@available(iOS 16.0, *)
struct PlayIntent: AppIntent {
  static let title: LocalizedStringResource = "Play"
  static let description = IntentDescription("Plays an audiobook, music or a playlist by its title, author or label.")
  static let openAppWhenRun: Bool = true

  @Parameter(title: "Title", requestValueDialog: IntentDialog("What would you like to hear?"))
  var query: String

  func perform() async throws -> some IntentResult & ProvidesDialog {
    PsalmsVoice.post(action: "play", query: query)
    return .result(dialog: IntentDialog("Looking for it."))
  }
}

@available(iOS 16.0, *)
struct NextTrackIntent: AppIntent {
  static let title: LocalizedStringResource = "Next title"
  static let description = IntentDescription("Goes to the next title.")
  static let openAppWhenRun: Bool = true

  func perform() async throws -> some IntentResult & ProvidesDialog {
    PsalmsVoice.post(action: "next")
    return .result(dialog: IntentDialog("Next title."))
  }
}

@available(iOS 16.0, *)
struct PreviousTrackIntent: AppIntent {
  static let title: LocalizedStringResource = "Previous title"
  static let description = IntentDescription("Goes back to the previous title.")
  static let openAppWhenRun: Bool = true

  func perform() async throws -> some IntentResult & ProvidesDialog {
    PsalmsVoice.post(action: "previous")
    return .result(dialog: IntentDialog("Previous title."))
  }
}

@available(iOS 16.0, *)
struct WhatIsNewIntent: AppIntent {
  static let title: LocalizedStringResource = "What is new"
  static let description = IntentDescription("Opens the start page with what is new.")
  static let openAppWhenRun: Bool = true

  func perform() async throws -> some IntentResult & ProvidesDialog {
    PsalmsVoice.post(action: "new")
    return .result(dialog: IntentDialog("Here is what is new."))
  }
}

/// The spoken phrases. Every phrase has to contain the app name; the German ones are in AppShortcuts.strings,
/// keyed by the English phrase.
@available(iOS 16.0, *)
struct PsalmsShortcuts: AppShortcutsProvider {
  static var appShortcuts: [AppShortcut] {
    AppShortcut(
      intent: ContinueListeningIntent(),
      phrases: [
        "Continue listening in \(.applicationName)",
        "Resume \(.applicationName)",
      ],
      shortTitle: "Continue listening",
      systemImageName: "play.circle"
    )
    AppShortcut(
      intent: PlayIntent(),
      phrases: [
        "Play in \(.applicationName)",
        "Play something in \(.applicationName)",
      ],
      shortTitle: "Play",
      systemImageName: "music.note"
    )
    AppShortcut(
      intent: NextTrackIntent(),
      phrases: [
        "Next title in \(.applicationName)"
      ],
      shortTitle: "Next title",
      systemImageName: "forward.end"
    )
    AppShortcut(
      intent: PreviousTrackIntent(),
      phrases: [
        "Previous title in \(.applicationName)"
      ],
      shortTitle: "Previous title",
      systemImageName: "backward.end"
    )
    AppShortcut(
      intent: WhatIsNewIntent(),
      phrases: [
        "What is new in \(.applicationName)"
      ],
      shortTitle: "What is new",
      systemImageName: "sparkles"
    )
  }
}
