package com.camdroid.camera

import android.content.Context
import android.graphics.SurfaceTexture
import android.hardware.camera2.*
import android.os.Handler
import android.os.HandlerThread
import android.util.Log
import android.util.Range
import android.util.Size
import android.view.Surface
import com.camdroid.encoder.ResolutionPreset
import kotlinx.coroutines.CompletableDeferred
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
 * Also provides camera controls: zoom, focus, exposure, white balance, flash, mirror.
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
    private var highSpeedRequested = false
    var maxZoom: Float = 1f
        private set
    var sensorOrientation: Int = 0
        private set

    // Current control state
    private var currentZoom = 1f
    private var flashEnabled = false
    private var mirrorEnabled = false

    /** Initialize the camera handler thread. */
    fun initialize() {
        cameraThread = HandlerThread("CameraThread").also { it.start() }
        cameraHandler = Handler(cameraThread!!.looper)
    }

    /** Get the camera ID for the requested facing (front or back). */
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

        // Find exact match or closest larger size
        return sizes.firstOrNull { it.width == preset.width && it.height == preset.height }
            ?: sizes.filter { it.width >= preset.width && it.height >= preset.height }
                .minByOrNull { it.width.toLong() * it.height }
            ?: sizes.maxByOrNull { it.width.toLong() * it.height }
            ?: Size(preset.width, preset.height)
    }

    /** Maximum frame rate the Camera2 HAL actually advertises for this facing. */
    fun queryMaxFps(useFront: Boolean, preset: ResolutionPreset? = null): Int {
        val aeMax = queryAeMaxFps(useFront)
        // 60+ fps is only honest when the constrained high-speed list contains
        // a FIXED range at the target: the framework rejects floating ranges
        // when a preview and encoder surface are both attached, and the vendor
        // pipelines that re-time the sensor are auth-gated to platform cameras
        // (this OPPO's "cameraUnit" answers "authentication fails" to
        // third-party apps — probed exhaustively Nov 6).
        val hsFixed60 = preset != null && hasFixedHighSpeedRange(useFront, preset, 60)
        val hsMax = if (preset != null) queryHighSpeedMaxFps(useFront, preset) else 0
        Log.i(
            TAG,
            "queryMaxFps(front=$useFront, preset=$preset): aeMax=$aeMax hsMax=$hsMax fixed60=$hsFixed60 " +
                "sizes=${highSpeedConfigs(useFront).joinToString { "${it.width}x${it.height}[${it.fpsMin}-${it.fpsMax}]" }}"
        )
        if (hsFixed60) {
            return 60
        }
        return aeMax
    }

    /** Whether the high-speed list for `preset` contains a fixed [fps, fps] range. */
    private fun hasFixedHighSpeedRange(useFront: Boolean, preset: ResolutionPreset, fps: Int): Boolean {
        val cameraId = getCameraId(useFront) ?: return false
        val chars = systemCameraManager.getCameraCharacteristics(cameraId)
        val map = chars.get(CameraCharacteristics.SCALER_STREAM_CONFIGURATION_MAP) ?: return false
        val size = map.highSpeedVideoSizes.firstOrNull {
            it.width == preset.width && it.height == preset.height
        } ?: return false
        return map.getHighSpeedVideoFpsRangesFor(size).any { it.lower == fps && it.upper == fps }
    }

    private fun queryAeMaxFps(useFront: Boolean): Int {
        val cameraId = getCameraId(useFront) ?: return 30
        val chars = systemCameraManager.getCameraCharacteristics(cameraId)
        val ranges = chars.get(CameraCharacteristics.CONTROL_AE_AVAILABLE_TARGET_FPS_RANGES)
            ?: return 30
        return ranges.maxOf { it.upper }
    }

    /** One parsed `[width, height, fps_min, fps_max]` high-speed tuple. */
    private data class HighSpeedConfig(val width: Int, val height: Int, val fpsMin: Int, val fpsMax: Int)

    private fun highSpeedConfigs(useFront: Boolean): List<HighSpeedConfig> {
        val cameraId = getCameraId(useFront) ?: return emptyList()
        val chars = systemCameraManager.getCameraCharacteristics(cameraId)
        val map = chars.get(CameraCharacteristics.SCALER_STREAM_CONFIGURATION_MAP)
            ?: return emptyList()
        // The standard Java surface for the high-speed metadata: per-size fps
        // ranges on the stream configuration map.
        return map.highSpeedVideoSizes.flatMap { size ->
            map.getHighSpeedVideoFpsRangesFor(size).map { range ->
                HighSpeedConfig(size.width, size.height, range.lower, range.upper)
            }
        }
    }

    private fun queryHighSpeedMaxFps(useFront: Boolean, preset: ResolutionPreset): Int =
        highSpeedConfigs(useFront)
            .filter { it.width == preset.width && it.height == preset.height }
            .maxOfOrNull { it.fpsMax } ?: 0

    /** Whether the HAL's output-size map contains an exact mode for the preset. */
    fun supportsResolution(useFront: Boolean, preset: ResolutionPreset): Boolean {
        val cameraId = getCameraId(useFront) ?: return true
        val chars = systemCameraManager.getCameraCharacteristics(cameraId)
        val map = chars.get(CameraCharacteristics.SCALER_STREAM_CONFIGURATION_MAP) ?: return true
        val sizes = map.getOutputSizes(SurfaceTexture::class.java) ?: return true
        return sizes.any { it.width == preset.width && it.height == preset.height }
    }

    /**
     * Open the camera and create a capture session with the given output surfaces.
     *
     * @param previewSurface Surface for on-screen preview
     * @param encoderSurface Surface from MediaCodec encoder for video encoding
     * @param useFront Whether to use the front camera
     * @param targetFps Target frame rate
     */


    @Suppress("MissingPermission")
    suspend fun openCamera(
        previewSurface: Surface?,
        encoderSurface: Surface,
        useFront: Boolean = false,
        targetFps: Int = 60,
        preset: ResolutionPreset? = null
    ) {
        val cameraId = getCameraId(useFront) ?: throw IllegalStateException("No ${if (useFront) "front" else "back"} camera found")
        currentCameraId = cameraId
        isUsingFrontCamera = useFront

        characteristics = systemCameraManager.getCameraCharacteristics(cameraId)
        val maxZoomRatio = characteristics?.get(CameraCharacteristics.SCALER_AVAILABLE_MAX_DIGITAL_ZOOM) ?: 1f
        maxZoom = maxZoomRatio
        sensorOrientation = characteristics?.get(CameraCharacteristics.SENSOR_ORIENTATION) ?: 0

        Log.i(TAG, "Opening camera $cameraId (${if (useFront) "front" else "back"}), maxZoom=$maxZoom, orientation=$sensorOrientation")

        // Open the camera device
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

        // Create capture request with both surfaces as targets
        val builder = device.createCaptureRequest(CameraDevice.TEMPLATE_RECORD).apply {
            previewSurface?.let { addTarget(it) }
            addTarget(encoderSurface)

            // Auto focus
            set(CaptureRequest.CONTROL_AF_MODE, CaptureRequest.CONTROL_AF_MODE_CONTINUOUS_VIDEO)
            // Auto exposure
            set(CaptureRequest.CONTROL_AE_MODE, CaptureRequest.CONTROL_AE_MODE_ON)
            // Auto white balance
            set(CaptureRequest.CONTROL_AWB_MODE, CaptureRequest.CONTROL_AWB_MODE_AUTO)
            // EIS stays off: this HAL throttles the sensor when it is enabled
            // (measured 20 fps at 1080p with it on), and a webcam wants a
            // steady frame rate over warp stabilisation.

            // Set target FPS range. Above 30 fps only the constrained
            // high-speed session can deliver, and only through a FIXED range:
            // the framework rejects floating ranges when both a preview and an
            // encoder surface are attached (createHighSpeedRequestList rule).
            // Devices whose high-speed list lacks a fixed [target,target]
            // (this OPPO: 1080p is [30,120]/[120,120]) stay on the AE list.
            val hsRange = if (targetFps >= 60 && preset != null) {
                findBestHighSpeedRange(useFront, preset, targetFps)
            } else {
                null
            }
            val useHighSpeed = hsRange != null && hsRange.lower == hsRange.upper
            val fpsRange = if (useHighSpeed && hsRange != null) hsRange else findBestFpsRange(targetFps)
            set(CaptureRequest.CONTROL_AE_TARGET_FPS_RANGE, fpsRange)
            Log.i(TAG, "FPS range: $fpsRange${if (useHighSpeed) " (high-speed session)" else ""}")
            highSpeedRequested = useHighSpeed
        }
        captureRequestBuilder = builder

        // Create capture session with output surfaces. A failed high-speed
        // configure falls back to a regular 30 fps session instead of crashing
        // the service (stricter HALs reject non-listed preview sizes).
        val targets = buildList {
            previewSurface?.let { add(it) }
            add(encoderSurface)
        }
        val session = try {
            createSession(device, targets, highSpeedRequested)
        } catch (e: Exception) {
            if (!highSpeedRequested) throw e
            Log.w(TAG, "High-speed session unavailable (${e.message}); falling back to 30 fps")
            highSpeedRequested = false
            builder.set(
                CaptureRequest.CONTROL_AE_TARGET_FPS_RANGE,
                findBestFpsRange(minOf(targetFps, 30))
            )
            createSession(device, targets, false)
        }
        captureSession = session

        // Start the repeating capture request. Constrained high-speed sessions
        // only batch frames when the repeating request is packaged with
        // createHighSpeedRequestList — a bare request repeats at normal speed
        // (measured 30 fps even with a 60 fps range).
        if (highSpeedRequested) {
            try {
                val highSpeedSession =
                    session as android.hardware.camera2.CameraConstrainedHighSpeedCaptureSession
                val requestList = highSpeedSession.createHighSpeedRequestList(builder.build())
                // Modern API surface: the batched list repeats via the base
                // class's burst submission (the List overload of
                // setRepeatingRequest no longer exists on the stub).
                session.setRepeatingBurst(requestList, null, cameraHandler)
            } catch (e: Exception) {
                Log.w(TAG, "High-speed request list rejected (${e.message}); falling back to 30 fps")
                highSpeedRequested = false
                builder.set(
                    CaptureRequest.CONTROL_AE_TARGET_FPS_RANGE,
                    findBestFpsRange(minOf(targetFps, 30))
                )
                try { session.close() } catch (_: Exception) {}
                val regular = createSession(device, targets, false)
                captureSession = regular
                regular.setRepeatingRequest(builder.build(), null, cameraHandler)
            }
        } else {
            session.setRepeatingRequest(builder.build(), null, cameraHandler)
        }
        Log.i(TAG, "Camera capture session started")
    }

    private suspend fun createSession(
        device: CameraDevice,
        targets: List<Surface>,
        highSpeed: Boolean
    ): CameraCaptureSession = suspendCancellableCoroutine { cont ->
        val callback = object : CameraCaptureSession.StateCallback() {
            override fun onConfigured(session: CameraCaptureSession) {
                cont.resume(session)
            }
            override fun onConfigureFailed(session: CameraCaptureSession) {
                if (cont.isActive) cont.resumeWithException(Exception("Session configure failed"))
            }
        }

        if (highSpeed && android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.P) {
            val outputs = targets.map { android.hardware.camera2.params.OutputConfiguration(it) }
            val sessionConfig = android.hardware.camera2.params.SessionConfiguration(
                android.hardware.camera2.params.SessionConfiguration.SESSION_HIGH_SPEED,
                outputs,
                { command -> cameraHandler?.post(command) ?: command.run() },
                callback
            )
            device.createCaptureSession(sessionConfig)
        } else if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.P) {
            device.createCaptureSession(
                android.hardware.camera2.params.SessionConfiguration(
                    android.hardware.camera2.params.SessionConfiguration.SESSION_REGULAR,
                    targets.map { android.hardware.camera2.params.OutputConfiguration(it) },
                    { command -> cameraHandler?.post(command) ?: command.run() },
                    callback
                )
            )
        } else {
            @Suppress("DEPRECATION")
            device.createCaptureSession(targets, callback, cameraHandler)
        }
    }

    /** Find the best FPS range that includes the target FPS. */
    private fun findBestFpsRange(targetFps: Int): Range<Int> {
        val ranges = characteristics?.get(CameraCharacteristics.CONTROL_AE_AVAILABLE_TARGET_FPS_RANGES) ?: return Range(30, 30)

        // Prefer a fixed [target, target] range: auto-exposure then cannot slow
        // the sensor down for longer exposures in dim light (reads as ~20 fps).
        ranges.firstOrNull { it.lower == targetFps && it.upper == targetFps }?.let { return it }

        // Otherwise take the range capped at the target with the highest lower
        // bound, so the sensor runs as close to the target as the hardware allows.
        ranges.filter { it.upper == targetFps }
            .maxByOrNull { it.lower }
            ?.let { return it }

        // Fall back to highest available range
        return ranges.maxByOrNull { it.upper } ?: Range(30, 30)
    }

    /**
     * Best fps range among the high-speed ranges the HAL exposes for `preset`.
     * Camera2 synthesises [min,min], [min,max] and [max,max] per high-speed
     * configuration — this HAL lists 1080p as [30,120]/[120,120], so a literal
     * [60,60] does not exist and must not be submitted. Prefer a fixed range
     * at the target, else the narrowest range containing it.
     */
    private fun findBestHighSpeedRange(
        useFront: Boolean,
        preset: ResolutionPreset,
        targetFps: Int,
    ): Range<Int>? {
        val cameraId = getCameraId(useFront) ?: return null
        val chars = systemCameraManager.getCameraCharacteristics(cameraId)
        val map = chars.get(CameraCharacteristics.SCALER_STREAM_CONFIGURATION_MAP) ?: return null
        val size = map.highSpeedVideoSizes.firstOrNull {
            it.width == preset.width && it.height == preset.height
        } ?: return null
        val ranges = map.getHighSpeedVideoFpsRangesFor(size)
        ranges.firstOrNull { it.lower == targetFps && it.upper == targetFps }?.let { return it }
        return ranges.filter { it.lower <= targetFps && targetFps <= it.upper }
            .minByOrNull { it.upper - it.lower }
    }

    /** Apply the current capture request settings. */
    private fun applySettings() {
        val builder = captureRequestBuilder ?: return
        val session = captureSession ?: return
        try {
            if (highSpeedRequested) {
                // The high-speed session must repeat a batched request list.
                val hs = session as android.hardware.camera2.CameraConstrainedHighSpeedCaptureSession
                session.setRepeatingBurst(hs.createHighSpeedRequestList(builder.build()), null, cameraHandler)
            } else {
                session.setRepeatingRequest(builder.build(), null, cameraHandler)
            }
        } catch (e: Exception) {
            Log.e(TAG, "Failed to apply settings", e)
        }
    }

    // ========== Camera Controls ==========

    /** Set the zoom ratio (1.0 = no zoom). */
    fun setZoom(ratio: Float) {
        val builder = captureRequestBuilder ?: return
        val chars = characteristics ?: return
        val clampedRatio = ratio.coerceIn(1f, maxZoom)
        currentZoom = clampedRatio

        val sensorRect = chars.get(CameraCharacteristics.SENSOR_INFO_ACTIVE_ARRAY_SIZE) ?: return
        val cropWidth = (sensorRect.width() / clampedRatio).toInt()
        val cropHeight = (sensorRect.height() / clampedRatio).toInt()
        val cropLeft = (sensorRect.width() - cropWidth) / 2
        val cropTop = (sensorRect.height() - cropHeight) / 2
        val cropRect = android.graphics.Rect(cropLeft, cropTop, cropLeft + cropWidth, cropTop + cropHeight)

        builder.set(CaptureRequest.SCALER_CROP_REGION, cropRect)
        applySettings()
        Log.d(TAG, "Zoom set to ${clampedRatio}x")
    }

    /** Set focus mode: "auto" or "manual" with distance in diopters. */
    fun setFocus(mode: String, distance: Float? = null) {
        val builder = captureRequestBuilder ?: return
        when (mode) {
            "auto" -> {
                builder.set(CaptureRequest.CONTROL_AF_MODE, CaptureRequest.CONTROL_AF_MODE_CONTINUOUS_VIDEO)
            }
            "manual" -> {
                builder.set(CaptureRequest.CONTROL_AF_MODE, CaptureRequest.CONTROL_AF_MODE_OFF)
                distance?.let { builder.set(CaptureRequest.LENS_FOCUS_DISTANCE, it) }
            }
        }
        applySettings()
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

    /** Set whether the video should be mirrored. */
    fun setMirror(enabled: Boolean) {
        mirrorEnabled = enabled
        // Note: mirroring is applied in the OpenGL rendering or by the receiver
    }

    /** Close the camera and release all resources. */
    fun close() {
        captureSession?.close()
        captureSession = null
        cameraDevice?.close()
        cameraDevice = null
        captureRequestBuilder = null
        cameraThread?.quitSafely()
        cameraThread = null
        cameraHandler = null
        Log.i(TAG, "Camera closed")
    }
}
