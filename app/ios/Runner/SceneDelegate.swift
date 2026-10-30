import Flutter
import UIKit

class SceneDelegate: FlutterSceneDelegate {
  // A Siri media intent that Siri handed over with .handleInApp arrives as an NSUserActivity: at the start of the scene
  // (the app was closed) or while the scene exists. The super calls keep Flutter's own handling.
  override func scene(
    _ scene: UIScene, willConnectTo session: UISceneSession, options connectionOptions: UIScene.ConnectionOptions
  ) {
    super.scene(scene, willConnectTo: session, options: connectionOptions)
    for activity in connectionOptions.userActivities {
      SiriMediaHandoff.handle(activity)
    }
  }

  override func scene(_ scene: UIScene, continue userActivity: NSUserActivity) {
    super.scene(scene, continue: userActivity)
    SiriMediaHandoff.handle(userActivity)
  }
}
