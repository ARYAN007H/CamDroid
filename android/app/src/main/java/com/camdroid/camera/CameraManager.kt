package com.camdroid.camera

import android.content.Context
import android.graphics.Rect
import android.graphics.SurfaceTexture
import android.hardware.camera2.*
import android.hardware.camera2.params.MeteringRectangle
import android.os.Build
import android.os.Handler
import android.os.HandlerThread
import android.util.Log
import android.util.Range
import android.util.Size
import android.view.Surface
import com.camdroid.encoder.ResolutionPreset
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlin.coroutines.resume
import kotlin.coroutines.resumeWithException

/**
 * Manages the Camera2 API lifecycle.
 *
 * Opens the camera and creates a CaptureSession with two output surfaces:
 * 1. Preview surface (for on-screen display via SurfaceTexture)
 * 2. Encoder surface (MediaCodec input for video encoding)
 *
 * Supports multi-lens optical zoom handoff (0.5x Ultra-Wide to Telephoto via CONTROL_ZOOM_RATIO),
 * tap-to-focus / tap-to-expose metering regions, manual ISO/shutter, and white balance.
 */
class CameraManager(private val context: Context) {
    companion object {
        private const val TAG = "CamManager"
    }

    private val systemCameraManager = context.getSystemService(Context.CAMERA_SERVICE) as android.hardware.camera2.CameraManager
    private var cameraDevice: CameraDevice? = null
    private var captureSession: CameraCaptureSession? = null
    private var captureRequestBuilder: CaptureRequest.Builder? = null
    private var cameraThread: HandlerThread? = null
    private var cameraHandler: Handler? = null

    private var currentCameraId: String? = null
    var isUsingFrontCamera = false
        private set

    // Camera characteristics
    private var characteristics: CameraCharacteristics? = null
    var minZoom: Float = 1f
        private set
    var maxZoom: Float = 1f
        private set
    var sensorOrientation: Int = 0
        private set

    // Current control state
    var currentZoom = 1f
        private set
    var flashEnabled = false
        private set
    var mirrorEnabled = false
        private set

    /** Initialize the camera handler thread. */
    fun initialize() {
        if (cameraThread == null) {
            cameraThread = HandlerThread("CameraThread").also { it.start() }
            cameraHandler = Handler(cameraThread!!.looper)
        }
    }

    /** Get the logical camera ID for the requested facing (front or back). */
    private fun getCameraId(useFront: Boolean): String? {
        val facing = if (useFront) CameraCharacteristics.LENS_FACING_FRONT
                     else CameraCharacteristics.LENS_FACING_BACK
        return systemCameraManager.cameraIdList.firstOrNull { id ->
            systemCameraManager.getCameraCharacteristics(id)
                .get(CameraCharacteristics.LENS_FACING) == facing
        }
    }

    /** Find the best matching output size for the requested resolution. */
    fun getBestOutputSize(cameraId: String, preset: ResolutionPreset): Size {
        val chars = systemCameraManager.getCameraCharacteristics(cameraId)
        val map = chars.get(CameraCharacteristics.SCALER_STREAM_CONFIGURATION_MAP) ?: return Size(preset.width, preset.height)
        val sizes = map.getOutputSizes(SurfaceTexture::class.java) ?: return Size(preset.width, preset.height)

        return sizes.firstOrNull { it.width == preset.width && it.height == preset.height }
            ?: sizes.filter { it.width >= preset.width && it.height >= preset.height }
                .minByOrNull { it.width.toLong() * it.height }
            ?: sizes.maxByOrNull { it.width.toLong() * it.height }
            ?: Size(preset.width, preset.height)
    }

    @Suppress("MissingPermission")
    suspend fun openCamera(
        previewSurface: Surface?,
        encoderSurface: Surface,
        useFront: Boolean = false,
        targetFps: Int = 60
    ) {
        val cameraId = getCameraId(useFront) ?: throw IllegalStateException("No ${if (useFront) "front" else "back"} camera found")
        currentCameraId = cameraId
        isUsingFrontCamera = useFront

        val chars = systemCameraManager.getCameraCharacteristics(cameraId)
        characteristics = chars

        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            val zoomRange = chars.get(CameraCharacteristics.CONTROL_ZOOM_RATIO_RANGE)
            if (zoomRange != null) {
                minZoom = zoomRange.lower
                maxZoom = zoomRange.upper
            } else {
                minZoom = 1f
                maxZoom = chars.get(CameraCharacteristics.SCALER_AVAILABLE_MAX_DIGITAL_ZOOM) ?: 1f
            }
        } else {
            minZoom = 1f
            maxZoom = chars.get(CameraCharacteristics.SCALER_AVAILABLE_MAX_DIGITAL_ZOOM) ?: 1f
        }
        sensorOrientation = chars.get(CameraCharacteristics.SENSOR_ORIENTATION) ?: 0

        Log.i(TAG, "Opening camera $cameraId (${if (useFront) "front" else "back"}), zoomRange=[$minZoom..$maxZoom], orientation=$sensorOrientation")

        val device = suspendCancellableCoroutine<CameraDevice> { cont ->
            systemCameraManager.openCamera(cameraId, object : CameraDevice.StateCallback() {
                override fun onOpened(camera: CameraDevice) {
                    cont.resume(camera)
                }
                override fun onDisconnected(camera: CameraDevice) {
                    camera.close()
                    if (cont.isActive) cont.resumeWithException(Exception("Camera disconnected"))
                }
                override fun onError(camera: CameraDevice, error: Int) {
                    camera.close()
                    if (cont.isActive) cont.resumeWithException(Exception("Camera error: $error"))
                }
            }, cameraHandler)
        }
        cameraDevice = device

        val builder = device.createCaptureRequest(CameraDevice.TEMPLATE_RECORD).apply {
            previewSurface?.let { addTarget(it) }
            addTarget(encoderSurface)

            set(CaptureRequest.CONTROL_AF_MODE, CaptureRequest.CONTROL_AF_MODE_CONTINUOUS_VIDEO)
            set(CaptureRequest.CONTROL_AE_MODE, CaptureRequest.CONTROL_AE_MODE_ON)
            set(CaptureRequest.CONTROL_AWB_MODE, CaptureRequest.CONTROL_AWB_MODE_AUTO)
            set(CaptureRequest.CONTROL_VIDEO_STABILIZATION_MODE, CaptureRequest.CONTROL_VIDEO_STABILIZATION_MODE_ON)

            val fpsRange = findBestFpsRange(targetFps)
            set(CaptureRequest.CONTROL_AE_TARGET_FPS_RANGE, fpsRange)
            Log.i(TAG, "FPS range: $fpsRange")
        }
        captureRequestBuilder = builder

        val session = suspendCancellableCoroutine<CameraCaptureSession> { cont ->
            val callback = object : CameraCaptureSession.StateCallback() {
                override fun onConfigured(session: CameraCaptureSession) {
                    cont.resume(session)
                }
                override fun onConfigureFailed(session: CameraCaptureSession) {
                    if (cont.isActive) cont.resumeWithException(Exception("Session configure failed"))
                }
            }

            val targets = buildList {
                previewSurface?.let { add(it) }
                add(encoderSurface)
            }

            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
                val outputs = targets.map { android.hardware.camera2.params.OutputConfiguration(it) }
                val sessionConfig = android.hardware.camera2.params.SessionConfiguration(
                    android.hardware.camera2.params.SessionConfiguration.SESSION_REGULAR,
                    outputs,
                    { command -> cameraHandler?.post(command) ?: command.run() },
                    callback
                )
                device.createCaptureSession(sessionConfig)
            } else {
                @Suppress("DEPRECATION")
                device.createCaptureSession(targets, callback, cameraHandler)
            }
        }
        captureSession = session

        session.setRepeatingRequest(builder.build(), null, cameraHandler)
        Log.i(TAG, "Camera capture session started")
    }

    private fun findBestFpsRange(targetFps: Int): Range<Int> {
        val ranges = characteristics?.get(CameraCharacteristics.CONTROL_AE_AVAILABLE_TARGET_FPS_RANGES) ?: return Range(30, 30)

        // Prefer fixed framerate range [targetFps, targetFps] for consistent video pacing
        val fixed = ranges.firstOrNull { it.lower == targetFps && it.upper == targetFps }
        if (fixed != null) return fixed

        val exact = ranges.firstOrNull { it.upper == targetFps }
        if (exact != null) return exact

        val containing = ranges.filter { it.lower <= targetFps && it.upper >= targetFps }
            .maxByOrNull { it.upper }
        if (containing != null) return containing

        return ranges.maxByOrNull { it.upper } ?: Range(30, 30)
    }

    private fun applySettings() {
        val builder = captureRequestBuilder ?: return
        val session = captureSession ?: return
        try {
            session.setRepeatingRequest(builder.build(), null, cameraHandler)
        } catch (e: Exception) {
            Log.e(TAG, "Failed to apply settings", e)
        }
    }

    // ========== Camera Controls ==========

    /**
     * Set the zoom ratio (e.g., 0.5 = ultra-wide, 1.0 = main wide, 2.0/5.0 = telephoto).
     * Uses CONTROL_ZOOM_RATIO on Android 11+ for seamless multi-lens optical handoff.
     */
    fun setZoom(ratio: Float) {
        val builder = captureRequestBuilder ?: return
        val chars = characteristics ?: return
        val clampedRatio = ratio.coerceIn(minZoom, maxZoom)
        currentZoom = clampedRatio

        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R && chars.get(CameraCharacteristics.CONTROL_ZOOM_RATIO_RANGE) != null) {
            builder.set(CaptureRequest.CONTROL_ZOOM_RATIO, clampedRatio)
        } else {
            val sensorRect = chars.get(CameraCharacteristics.SENSOR_INFO_ACTIVE_ARRAY_SIZE) ?: return
            val effectiveRatio = clampedRatio.coerceAtLeast(1f)
            val cropWidth = (sensorRect.width() / effectiveRatio).toInt()
            val cropHeight = (sensorRect.height() / effectiveRatio).toInt()
            val cropLeft = (sensorRect.width() - cropWidth) / 2
            val cropTop = (sensorRect.height() - cropHeight) / 2
            val cropRect = Rect(cropLeft, cropTop, cropLeft + cropWidth, cropTop + cropHeight)
            builder.set(CaptureRequest.SCALER_CROP_REGION, cropRect)
        }
        applySettings()
        Log.d(TAG, "Zoom set to ${clampedRatio}x")
    }

    /**
     * Set focus mode: "auto", "manual" (with distance in diopters), or "tap" (normalized 0..1 x, y).
     */
    fun setFocus(mode: String, distance: Float? = null, tapX: Float? = null, tapY: Float? = null) {
        val builder = captureRequestBuilder ?: return
        val chars = characteristics ?: return

        when (mode) {
            "auto" -> {
                builder.set(CaptureRequest.CONTROL_AF_MODE, CaptureRequest.CONTROL_AF_MODE_CONTINUOUS_VIDEO)
                builder.set(CaptureRequest.CONTROL_AF_REGIONS, null)
                applySettings()
            }
            "manual" -> {
                builder.set(CaptureRequest.CONTROL_AF_MODE, CaptureRequest.CONTROL_AF_MODE_OFF)
                distance?.let { builder.set(CaptureRequest.LENS_FOCUS_DISTANCE, it) }
                applySettings()
            }
            "tap" -> {
                val sensorRect = chars.get(CameraCharacteristics.SENSOR_INFO_ACTIVE_ARRAY_SIZE) ?: return
                val nx = (tapX ?: 0.5f).coerceIn(0.05f, 0.95f)
                val ny = (tapY ?: 0.5f).coerceIn(0.05f, 0.95f)
                val halfBox = (sensorRect.width() * 0.075f).toInt().coerceAtLeast(64)
                val cx = (nx * sensorRect.width()).toInt()
                val cy = (ny * sensorRect.height()).toInt()
                val focusRect = Rect(
                    (cx - halfBox).coerceAtLeast(0),
                    (cy - halfBox).coerceAtLeast(0),
                    (cx + halfBox).coerceAtMost(sensorRect.width()),
                    (cy + halfBox).coerceAtMost(sensorRect.height())
                )
                val region = arrayOf(MeteringRectangle(focusRect, MeteringRectangle.METERING_WEIGHT_MAX - 1))

                builder.set(CaptureRequest.CONTROL_AF_MODE, CaptureRequest.CONTROL_AF_MODE_AUTO)
                builder.set(CaptureRequest.CONTROL_AF_REGIONS, region)
                builder.set(CaptureRequest.CONTROL_AE_REGIONS, region)
                applySettings()

                // Trigger AF lock on the tapped region
                try {
                    builder.set(CaptureRequest.CONTROL_AF_TRIGGER, CameraMetadata.CONTROL_AF_TRIGGER_START)
                    captureSession?.capture(builder.build(), null, cameraHandler)
                    builder.set(CaptureRequest.CONTROL_AF_TRIGGER, CameraMetadata.CONTROL_AF_TRIGGER_IDLE)
                } catch (e: Exception) {
                    Log.w(TAG, "Failed to trigger tap AF", e)
                }
            }
        }
    }

    /** Set exposure compensation in EV steps. */
    fun setExposureCompensation(ev: Int) {
        val builder = captureRequestBuilder ?: return
        builder.set(CaptureRequest.CONTROL_AE_EXPOSURE_COMPENSATION, ev)
        applySettings()
    }

    /** Set manual ISO and Shutter Speed. Pass null to return to auto exposure. */
    fun setManualExposure(iso: Int?, shutterSpeedNs: Long?) {
        val builder = captureRequestBuilder ?: return
        if (iso == null && shutterSpeedNs == null) {
            builder.set(CaptureRequest.CONTROL_AE_MODE, CaptureRequest.CONTROL_AE_MODE_ON)
        } else {
            builder.set(CaptureRequest.CONTROL_AE_MODE, CaptureRequest.CONTROL_AE_MODE_OFF)
            iso?.let { builder.set(CaptureRequest.SENSOR_SENSITIVITY, it) }
            shutterSpeedNs?.let { builder.set(CaptureRequest.SENSOR_EXPOSURE_TIME, it) }
        }
        applySettings()
    }

    /** Set white balance mode. */
    fun setWhiteBalance(mode: String) {
        val builder = captureRequestBuilder ?: return
        val wbMode = when (mode) {
            "auto" -> CaptureRequest.CONTROL_AWB_MODE_AUTO
            "daylight" -> CaptureRequest.CONTROL_AWB_MODE_DAYLIGHT
            "tungsten" -> CaptureRequest.CONTROL_AWB_MODE_INCANDESCENT
            "fluorescent" -> CaptureRequest.CONTROL_AWB_MODE_FLUORESCENT
            "cloudy" -> CaptureRequest.CONTROL_AWB_MODE_CLOUDY_DAYLIGHT
            else -> CaptureRequest.CONTROL_AWB_MODE_AUTO
        }
        builder.set(CaptureRequest.CONTROL_AWB_MODE, wbMode)
        applySettings()
    }

    /** Enable or disable the camera torch/flash. */
    fun setFlash(enabled: Boolean) {
        val builder = captureRequestBuilder ?: return
        flashEnabled = enabled
        builder.set(CaptureRequest.FLASH_MODE,
            if (enabled) CaptureRequest.FLASH_MODE_TORCH else CaptureRequest.FLASH_MODE_OFF)
        applySettings()
    }

    /** Set whether the video should be mirrored (propagated in v2.0 frame flags). */
    fun setMirror(enabled: Boolean) {
        mirrorEnabled = enabled
    }

    /** Close the camera and release all resources. */
    fun close() {
        try { captureSession?.close() } catch (_: Exception) {}
        captureSession = null
        try { cameraDevice?.close() } catch (_: Exception) {}
        cameraDevice = null
        captureRequestBuilder = null
        cameraThread?.quitSafely()
        cameraThread = null
        cameraHandler = null
        Log.i(TAG, "Camera closed")
    }
}

