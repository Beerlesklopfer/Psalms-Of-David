package com.the_abraham_box.psalm

import android.Manifest
import android.content.pm.PackageManager
import android.os.Build
import android.provider.Settings
import androidx.core.app.ActivityCompat
import androidx.core.content.ContextCompat
import com.ryanheise.audioservice.AudioServiceActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.MethodChannel

/// The activity of the audio service (it keeps the engine for playback with the screen locked). It also asks for the permission to
/// show the notification with the playback controls (Android 13 and later); playback works without it.
class MainActivity : AudioServiceActivity() {
    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, "com.the-abraham-box.psalm/notifications").setMethodCallHandler { call, result ->
            when (call.method) {
                "request" -> {
                    if (Build.VERSION.SDK_INT >= 33 &&
                        ContextCompat.checkSelfPermission(this, Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
                    ) {
                        ActivityCompat.requestPermissions(this, arrayOf(Manifest.permission.POST_NOTIFICATIONS), 4711)
                    }
                    result.success(null)
                }
                // The name the person gave the device ("Jörgs Pixel"), as the system settings show it; none on a device that has not got one.
                "deviceName" -> {
                    val name = try {
                        Settings.Global.getString(contentResolver, "device_name")
                    } catch (e: Exception) {
                        null
                    } ?: try {
                        Settings.Secure.getString(contentResolver, "bluetooth_name")
                    } catch (e: Exception) {
                        null
                    }
                    result.success(name)
                }
                else -> result.notImplemented()
            }
        }
    }
}
