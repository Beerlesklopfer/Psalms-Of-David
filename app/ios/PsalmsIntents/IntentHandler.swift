// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import Intents

/// Entry point of the Intents extension: Siri asks which object handles an intent. Only INPlayMediaIntent ("Play <title>" without the name of
/// the app) is supported; the library is in the app, so the extension only passes the spoken text on (see PlayMediaIntentHandler.swift).
class IntentHandler: INExtension {
  override func handler(for intent: INIntent) -> Any {
    if intent is INPlayMediaIntent {
      return PlayMediaIntentHandler()
    }
    return self
  }
}
