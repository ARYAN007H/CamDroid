# CamDroid ProGuard Rules
-keepattributes Signature
-keepattributes *Annotation*

# Keep Gson serialization classes
-keep class com.camdroid.network.** { *; }

# Keep Camera2 callbacks
-keep class * implements android.hardware.camera2.CameraCaptureSession$StateCallback { *; }
-keep class * implements android.hardware.camera2.CameraDevice$StateCallback { *; }
