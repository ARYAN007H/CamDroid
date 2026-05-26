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
        targetFps: Int = 60
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
            // Video stabilization
            set(CaptureRequest.CONTROL_VIDEO_STABILIZATION_MODE, CaptureRequest.CONTROL_VIDEO_STABILIZATION_MODE_ON)

            // Set target FPS range
            val fpsRange = findBestFpsRange(targetFps)
            set(CaptureRequest.CONTROL_AE_TARGET_FPS_RANGE, fpsRange)
            Log.i(TAG, "FPS range: $fpsRange")
        }
        captureRequestBuilder = builder

        // Create capture session with output surfaces
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

            if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.P) {
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

        // Start the repeating capture request
        session.setRepeatingRequest(builder.build(), null, cameraHandler)
        Log.i(TAG, "Camera capture session started")
    }

    /** Find the best FPS range that includes the target FPS. */
    private fun findBestFpsRange(targetFps: Int): Range<Int> {
        val ranges = characteristics?.get(CameraCharacteristics.CONTROL_AE_AVAILABLE_TARGET_FPS_RANGES) ?: return Range(30, 30)

        // Prefer exact match where upper == targetFps
        val exact = ranges.firstOrNull { it.upper == targetFps }
        if (exact != null) return exact

        // Prefer range that includes targetFps
        val containing = ranges.filter { it.lower <= targetFps && it.upper >= targetFps }
            .maxByOrNull { it.upper }
        if (containing != null) return containing

        // Fall back to highest available range
        return ranges.maxByOrNull { it.upper } ?: Range(30, 30)
    }

    /** Apply the current capture request settings. */
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
