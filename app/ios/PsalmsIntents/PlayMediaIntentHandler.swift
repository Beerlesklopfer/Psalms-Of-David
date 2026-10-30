// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import Intents

/// Handles "Play <something>" for audio. The extension cannot know the library (it lives on the server of the family and the pairing is in the
/// app), so it accepts what was spoken as the one media item (identifier = title = the spoken text) and answers .handleInApp: the app
/// then starts the app and searches (Runner/SiriMediaHandoff.swift). No app group is needed, the intent itself carries the text.
class PlayMediaIntentHandler: NSObject, INPlayMediaIntentHandling {

  /// The text that was spoken: title, album and artist words together, without duplicates. Empty if nothing was spoken.
  static func spokenText(_ search: INMediaSearch?) -> String {
    guard let search = search else { return "" }
    var parts: [String] = []
    for part in [search.mediaName, search.albumName, search.artistName] {
      let text = (part ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
      if !text.isEmpty && !parts.contains(where: { $0.caseInsensitiveCompare(text) == .orderedSame }) {
        parts.append(text)
      }
    }
    return parts.joined(separator: " ")
  }

  func resolveMediaItems(
    for intent: INPlayMediaIntent,
    with completion: @escaping ([INPlayMediaMediaItemResolutionResult]) -> Void
  ) {
    let text = PlayMediaIntentHandler.spokenText(intent.mediaSearch)
    guard !text.isEmpty else {
      completion([INPlayMediaMediaItemResolutionResult.needsValue()])
      return
    }
    let item = INMediaItem(
      identifier: text, title: text, type: intent.mediaSearch?.mediaType ?? .unknown, artwork: nil)
    completion(INPlayMediaMediaItemResolutionResult.successes(with: [item]))
  }

  func confirm(intent: INPlayMediaIntent, completion: @escaping (INPlayMediaIntentResponse) -> Void) {
    completion(INPlayMediaIntentResponse(code: .ready, userActivity: nil))
  }

  func handle(intent: INPlayMediaIntent, completion: @escaping (INPlayMediaIntentResponse) -> Void) {
    // Audio is played by the app itself: Siri starts the app and hands the intent over.
    completion(INPlayMediaIntentResponse(code: .handleInApp, userActivity: nil))
  }
}
