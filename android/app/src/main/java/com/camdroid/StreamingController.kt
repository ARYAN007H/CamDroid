package com.camdroid

import android.view.Surface
import kotlinx.coroutines.flow.MutableStateFlow

/**
 * Shared state and events between the MainActivity UI and CameraStreamingService.
 */
object StreamingController {
    // Current streaming state
    val isStreaming = MutableStateFlow(false)
    val isConnected = MutableStateFlow(false)
    val batteryLevel = MutableStateFlow(100)
    val isCharging = MutableStateFlow(false)
    val currentFps = MutableStateFlow(0f)
    val currentBitrate = MutableStateFlow(0f)

    // Camera characteristics & controls
    val maxZoom = MutableStateFlow(1f)
    val currentZoom = MutableStateFlow(1f)
    val useFrontCamera = MutableStateFlow(false)
    val flashEnabled = MutableStateFlow(false)
    val mirrorEnabled = MutableStateFlow(false)

    // Focus mode: "auto" or "manual"
    val focusMode = MutableStateFlow("auto")
    val focusDistance = MutableStateFlow(0f) // 0.0 to 10.0 diopters

    // Exposure mode: "auto" or "manual"
    val exposureMode = MutableStateFlow("auto")
    val exposureCompensation = MutableStateFlow(0) // -4 to +4 EV
    val manualIso = MutableStateFlow(400) // 100 to 6400
    val manualShutterSpeedNs = MutableStateFlow(16666666L) // default 1/60s

    // White Balance mode: "auto", "daylight", "tungsten", "fluorescent", "cloudy"
    val whiteBalanceMode = MutableStateFlow("auto")

    // Dim Mode state (screen-saver mode for background usage)
    val isDimMode = MutableStateFlow(false)

    // Active preview surface provided by the Compose TextureView
    var activePreviewSurface: Surface? = null
        set(value) {
            field = value
            onPreviewSurfaceChanged?.invoke(value)
        }

    // Callbacks to be set by the service
    var onPreviewSurfaceChanged: ((Surface?) -> Unit)? = null
    var onZoomChanged: ((Float) -> Unit)? = null
    var onFocusChanged: ((String, Float) -> Unit)? = null
    var onExposureChanged: ((Int) -> Unit)? = null
    var onManualExposureChanged: ((Int?, Long?) -> Unit)? = null
    var onWhiteBalanceChanged: ((String) -> Unit)? = null
    var onFlashChanged: ((Boolean) -> Unit)? = null
    var onCameraToggle: ((Boolean) -> Unit)? = null
}
