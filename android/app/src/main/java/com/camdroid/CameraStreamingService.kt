package com.camdroid

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Intent
import android.content.pm.ServiceInfo
import android.graphics.SurfaceTexture
import android.os.Build
import android.os.PowerManager
import android.util.Log
import android.util.Size
import android.view.Surface
import androidx.core.app.NotificationCompat
import androidx.core.app.ServiceCompat
import androidx.lifecycle.LifecycleService
import androidx.lifecycle.lifecycleScope
import com.camdroid.camera.CameraManager
import com.camdroid.encoder.*
import com.camdroid.network.NsdHelper
import com.camdroid.network.StreamServer
import com.camdroid.util.BatteryMonitor
import com.camdroid.util.NetworkUtils
import com.google.gson.Gson
import com.google.gson.JsonObject
import kotlinx.coroutines.*
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicLong

/**
 * Foreground service that orchestrates camera streaming.
 *
 * Ties together: Camera2 → MediaCodec Encoder → TCP Server → Desktop Client
 * Also manages: mDNS, audio, battery monitoring, and remote control commands.
 */
class CameraStreamingService : LifecycleService() {
    companion object {
        private const val TAG = "StreamingService"
        private const val NOTIFICATION_CHANNEL_ID = "camdroid_streaming"
        private const val NOTIFICATION_ID = 1001
        const val ACTION_STOP = "ACTION_STOP"
        const val EXTRA_USE_FRONT = "use_front"
        const val EXTRA_CODEC = "codec"
        const val EXTRA_RESOLUTION = "resolution"
        const val EXTRA_FPS = "fps"
        const val EXTRA_PORT = "port"
    }

    private var cameraManager: CameraManager? = null
    private var videoEncoder: VideoEncoder? = null
    private var audioEncoder: AudioEncoder? = null
    private var streamServer: StreamServer? = null
    private var nsdHelper: NsdHelper? = null
    private var batteryMonitor: BatteryMonitor? = null
    private var wakeLock: PowerManager.WakeLock? = null

    private var previewSurfaceTexture: SurfaceTexture? = null
    private var previewSurface: Surface? = null

    // Current configuration
    private var currentConfig = EncoderConfig()
    private var useFrontCamera = false
    private var serverPort = 4747
    private var audioEnabled = true

    // Stats & Adaptive Bitrate (ABR) state
    private val frameCount = AtomicLong(0)
    private val encodedBytesCount = AtomicLong(0)
    private val isStreaming = AtomicBoolean(false)
    private val isMediaPipelineActive = AtomicBoolean(false)
    private var fallbackStartJob: Job? = null

    @Volatile
    private var activeBitrateBps: Int = EncoderConfig.getDefaultBitrate(VideoCodecType.H264, ResolutionPreset.FHD)
    @Volatile
    private var lastAbrAdjustmentMs: Long = 0L
    @Volatile
    private var lastKeyframeRequestMs: Long = 0L

    private val gson = Gson()

    override fun onCreate() {
        super.onCreate()
        createNotificationChannel()
        startForegroundWithType()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        super.onStartCommand(intent, flags, startId)

        when (intent?.action) {
            ACTION_STOP -> {
                stopStreaming()
                stopSelf()
                return START_NOT_STICKY
            }
        }

        // Parse configuration from intent
        intent?.let {
            useFrontCamera = it.getBooleanExtra(EXTRA_USE_FRONT, false)
            serverPort = it.getIntExtra(EXTRA_PORT, 4747)

            val codecName = it.getStringExtra(EXTRA_CODEC) ?: "h264"
            val resName = it.getStringExtra(EXTRA_RESOLUTION) ?: "1080p"
            val fps = it.getIntExtra(EXTRA_FPS, 60)

            val codec = VideoCodecType.values().firstOrNull { c ->
                c.displayName.equals(codecName, ignoreCase = true) || c.mimeType.contains(codecName, ignoreCase = true)
            } ?: VideoCodecType.H264

            val resolution = ResolutionPreset.values().firstOrNull { r ->
                r.displayName.equals(resName, ignoreCase = true)
            } ?: ResolutionPreset.FHD

            val defaultBitrate = EncoderConfig.getDefaultBitrate(codec, resolution)
            activeBitrateBps = defaultBitrate
            currentConfig = EncoderConfig(
                codec = codec,
                resolution = resolution,
                fps = fps,
                bitrate = defaultBitrate
            )
        }

        startStreaming()
        return START_STICKY
    }

    private fun startForegroundWithType() {
        val notification = buildNotification("Initializing...")
        ServiceCompat.startForeground(
            this,
            NOTIFICATION_ID,
            notification,
            ServiceInfo.FOREGROUND_SERVICE_TYPE_CAMERA or ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE
        )
    }

    private fun startStreaming() {
        if (isStreaming.getAndSet(true)) return
        Log.i(TAG, "Starting streaming pipeline...")

        StreamingController.isStreaming.value = true
        StreamingController.useFrontCamera.value = useFrontCamera

        // Setup event listeners for bidirectional controls sync
        StreamingController.onZoomChanged = { ratio ->
            cameraManager?.setZoom(ratio)
            StreamingController.currentZoom.value = ratio
        }
        StreamingController.onFocusChanged = { mode, dist ->
            cameraManager?.setFocus(mode, dist)
            StreamingController.focusMode.value = mode
            StreamingController.focusDistance.value = dist
        }
        StreamingController.onExposureChanged = { ev ->
            cameraManager?.setExposureCompensation(ev)
            StreamingController.exposureCompensation.value = ev
        }
        StreamingController.onManualExposureChanged = { iso, shutter ->
            cameraManager?.setManualExposure(iso, shutter)
            if (iso == null && shutter == null) {
                StreamingController.exposureMode.value = "auto"
            } else {
                StreamingController.exposureMode.value = "manual"
                iso?.let { StreamingController.manualIso.value = it }
                shutter?.let { StreamingController.manualShutterSpeedNs.value = it }
            }
        }
        StreamingController.onWhiteBalanceChanged = { mode ->
            cameraManager?.setWhiteBalance(mode)
            StreamingController.whiteBalanceMode.value = mode
        }
        StreamingController.onFlashChanged = { enabled ->
            cameraManager?.setFlash(enabled)
            StreamingController.flashEnabled.value = enabled
        }
        StreamingController.onCameraToggle = { useFront ->
            lifecycleScope.launch {
                useFrontCamera = useFront
                StreamingController.useFrontCamera.value = useFront
                if (isMediaPipelineActive.get()) {
                    restartCamera()
                }
            }
        }
        StreamingController.onPreviewSurfaceChanged = {
            if (isStreaming.get() && isMediaPipelineActive.get()) {
                lifecycleScope.launch {
                    restartCamera()
                }
            }
        }

        // Real-time telemetry & Adaptive Bitrate (ABR) recovery coroutine (1Hz)
        lifecycleScope.launch {
            var lastCount = 0L
            var lastBytes = 0L
            var lastTime = System.currentTimeMillis()
            var cleanSeconds = 0

            while (isStreaming.get()) {
                delay(1000)
                val currentCount = frameCount.get()
                val currentBytes = encodedBytesCount.get()
                val currentTime = System.currentTimeMillis()
                val deltaFrames = currentCount - lastCount
                val deltaBytes = currentBytes - lastBytes
                val deltaTimeMs = currentTime - lastTime

                if (deltaTimeMs > 0) {
                    val fps = (deltaFrames * 1000f) / deltaTimeMs
                    val bps = (deltaBytes * 8L * 1000L) / deltaTimeMs
                    StreamingController.currentFps.value = fps
                    StreamingController.currentBitrate.value = bps / 1_000_000f

                    // Broadcast real-time status metadata packet to desktop client
                    if (streamServer?.isClientConnected == true && isMediaPipelineActive.get()) {
                        val statusJson = JsonObject().apply {
                            addProperty("type", "status")
                            addProperty("fps", fps)
                            addProperty("bitrate", bps)
                            addProperty("resolution", "${currentConfig.resolution.width}x${currentConfig.resolution.height}")
                            addProperty("codec", currentConfig.codec.displayName.lowercase())
                            addProperty("battery", batteryMonitor?.batteryLevel ?: StreamingController.batteryLevel.value)
                            addProperty("charging", StreamingController.isCharging.value)
                            addProperty("temperature", 36.5f)
                            addProperty("dropped_frames", streamServer?.droppedFrames?.get() ?: 0L)
                            addProperty("zoom", StreamingController.currentZoom.value)
                            addProperty("min_zoom", StreamingController.minZoom.value)
                            addProperty("max_zoom", StreamingController.maxZoom.value)
                            addProperty("use_front_camera", useFrontCamera)
                            addProperty("flash", StreamingController.flashEnabled.value)
                            addProperty("mirror", StreamingController.mirrorEnabled.value)
                            addProperty("focus_mode", StreamingController.focusMode.value)
                            addProperty("focus_distance", StreamingController.focusDistance.value)
                            addProperty("exposure_compensation", StreamingController.exposureCompensation.value)
                            addProperty("white_balance", StreamingController.whiteBalanceMode.value)
                        }
                        streamServer?.sendMetadata(gson.toJson(statusJson))
                    }

                    // Adaptive Bitrate (ABR) gentle ramp-up after 3 clean seconds without congestion
                    if (currentConfig.adaptiveBitrate && isMediaPipelineActive.get()) {
                        val timeSinceCongestion = currentTime - lastAbrAdjustmentMs
                        if (timeSinceCongestion > 3000L && activeBitrateBps < currentConfig.bitrate) {
                            cleanSeconds++
                            if (cleanSeconds >= 3) {
                                cleanSeconds = 0
                                val steppedUp = (activeBitrateBps * 1.12).toInt().coerceAtMost(currentConfig.bitrate)
                                activeBitrateBps = steppedUp
                                videoEncoder?.updateBitrate(steppedUp)
                            }
                        } else {
                            cleanSeconds = 0
                        }
                    }
                }
                lastCount = currentCount
                lastBytes = currentBytes
                lastTime = currentTime
            }
        }

        // Acquire wake lock
        val pm = getSystemService(POWER_SERVICE) as PowerManager
        wakeLock = pm.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "CamDroid::StreamWake").apply { acquire() }

        // Battery monitor
        batteryMonitor = BatteryMonitor(this).apply {
            onLowBattery = {
                Log.w(TAG, "Low battery — reducing quality")
                lifecycleScope.launch {
                    activeBitrateBps = currentConfig.bitrate / 2
                    videoEncoder?.updateBitrate(activeBitrateBps)
                }
            }
            startMonitoring()
        }

        // Start TCP server with congestion callback wired to keyframe recovery + ABR step-down
        streamServer = StreamServer(
            port = serverPort,
            onControlCommand = { cmd -> handleControlCommand(cmd) },
            onClientConnected = {
                lifecycleScope.launch { onClientConnected() }
            },
            onClientDisconnected = {
                fallbackStartJob?.cancel()
                isMediaPipelineActive.set(false)
                StreamingController.isConnected.value = false
                lifecycleScope.launch {
                    cameraManager?.close()
                    videoEncoder?.stop()
                    audioEncoder?.stop()
                }
                updateNotification("Waiting for client...")
            },
            onNetworkCongestion = { _, _ ->
                val now = System.currentTimeMillis()
                // Request immediate IDR keyframe (debounced to 250ms) so dropped P-frames never corrupt video
                if (now - lastKeyframeRequestMs > 250L) {
                    lastKeyframeRequestMs = now
                    videoEncoder?.requestKeyFrame()
                }
                // Step down bitrate by 20% down to a floor of 35% of nominal bitrate
                if (currentConfig.adaptiveBitrate && now - lastAbrAdjustmentMs > 800L) {
                    lastAbrAdjustmentMs = now
                    val minFloor = (currentConfig.bitrate * 0.35).toInt().coerceAtLeast(1_500_000)
                    val reduced = (activeBitrateBps * 0.80).toInt().coerceAtLeast(minFloor)
                    if (reduced < activeBitrateBps) {
                        activeBitrateBps = reduced
                        videoEncoder?.updateBitrate(reduced)
                    }
                }
            }
        ).also { it.start(lifecycleScope) }

        // Register mDNS service
        val deviceName = NetworkUtils.getDeviceName()
        nsdHelper = NsdHelper(this).apply {
            registerService(serverPort, deviceName, currentConfig.codec.displayName, currentConfig.resolution.displayName)
        }

        val ipAddress = NetworkUtils.getWifiIpAddress(this) ?: "unknown"
        updateNotification("Waiting • $ipAddress:$serverPort")
        Log.i(TAG, "Server started on $ipAddress:$serverPort")
    }

    /**
     * Phase 1 of Two-Phase Handshake:
     * Send server capabilities immediately on connect and wait for the client's `start` command
     * (with a 1500ms fallback auto-start for legacy clients).
     */
    private suspend fun onClientConnected() {
        Log.i(TAG, "Client connected — sending capabilities and awaiting 'start' command...")
        StreamingController.isConnected.value = true

        val capabilities = JsonObject().apply {
            addProperty("type", "capabilities")
            addProperty("version", "2.0")
            addProperty("device", NetworkUtils.getDeviceName())
            add("codecs", gson.toJsonTree(listOf("h264", "h265", "mjpeg")))
            add("resolutions", gson.toJsonTree(listOf("1080p", "1440p", "4k")))
            add("fps", gson.toJsonTree(listOf(30, 60)))
            addProperty("audio", true)
            addProperty("battery", batteryMonitor?.batteryLevel ?: 100)
            addProperty("min_zoom", StreamingController.minZoom.value)
            addProperty("max_zoom", StreamingController.maxZoom.value)
        }
        streamServer?.sendMetadata(gson.toJson(capabilities))

        // Fallback auto-start if a client does not send a 'start' command within 1.5s
        fallbackStartJob?.cancel()
        fallbackStartJob = lifecycleScope.launch {
            delay(1500)
            if (streamServer?.isClientConnected == true && !isMediaPipelineActive.get()) {
                Log.i(TAG, "No 'start' command received within 1.5s — auto-starting media pipeline with current config")
                startMediaPipeline()
            }
        }
    }

    /**
     * Phase 2 of Two-Phase Handshake:
     * Starts (or restarts) the hardware encoder + Camera2 pipeline using `currentConfig`.
     */
    private suspend fun startMediaPipeline() {
        fallbackStartJob?.cancel()
        cameraManager?.close()
        videoEncoder?.stop()
        audioEncoder?.stop()

        activeBitrateBps = currentConfig.bitrate
        isMediaPipelineActive.set(true)

        // Start hardware video encoder
        videoEncoder = VideoEncoder(currentConfig) { data, isConfig, isKeyFrame, pts ->
            encodedBytesCount.addAndGet(data.size.toLong())
            if (isConfig) {
                streamServer?.sendVideoConfig(data, pts)
            } else {
                streamServer?.sendVideoFrame(
                    data = data,
                    isKeyFrame = isKeyFrame,
                    isMirrored = StreamingController.mirrorEnabled.value,
                    ptsUs = pts
                )
                frameCount.incrementAndGet()
            }
        }.also { it.start() }

        // Start hardware audio encoder
        if (audioEnabled) {
            audioEncoder = AudioEncoder { data, isConfig, pts ->
                encodedBytesCount.addAndGet(data.size.toLong())
                if (isConfig) {
                    streamServer?.sendAudioConfig(data, pts)
                } else {
                    streamServer?.sendAudioFrame(data, pts)
                }
            }.also { it.start() }
        }

        // Start Camera2 capture session connected to the encoder Surface
        val encoderSurface = videoEncoder?.inputSurface ?: return
        startCamera(encoderSurface)

        // Request an immediate IDR keyframe so the desktop decoder locks on within 1 frame
        videoEncoder?.requestKeyFrame()

        val ipAddress = NetworkUtils.getWifiIpAddress(this) ?: "?"
        updateNotification("Streaming • ${currentConfig.resolution.displayName}@${currentConfig.fps}fps • $ipAddress:$serverPort")
    }

    private suspend fun startCamera(encoderSurface: Surface) {
        val previewSurf = StreamingController.activePreviewSurface
        cameraManager = CameraManager(this).apply {
            initialize()
            openCamera(previewSurf, encoderSurface, useFrontCamera, currentConfig.fps)
            StreamingController.minZoom.value = minZoom
            StreamingController.maxZoom.value = maxZoom
        }
        Log.i(TAG, "Camera started (zoom range ${StreamingController.minZoom.value}x..${StreamingController.maxZoom.value}x)")
    }

    private fun handleControlCommand(cmd: JsonObject) {
        val command = cmd.get("cmd")?.asString ?: return
        Log.d(TAG, "Handling control: $command")

        when (command) {
            "start" -> {
                val codecStr = cmd.get("codec")?.asString ?: currentConfig.codec.displayName
                val resStr = cmd.get("resolution")?.asString ?: currentConfig.resolution.displayName
                val fpsVal = cmd.get("fps")?.asInt ?: currentConfig.fps
                val audioVal = if (cmd.has("audio")) cmd.get("audio").asBoolean else audioEnabled

                val codec = VideoCodecType.values().firstOrNull {
                    it.displayName.equals(codecStr, true) || it.name.equals(codecStr, true)
                } ?: VideoCodecType.H264

                val preset = ResolutionPreset.values().firstOrNull {
                    it.displayName.equals(resStr, true)
                } ?: ResolutionPreset.FHD

                audioEnabled = audioVal
                currentConfig = currentConfig.copy(
                    codec = codec,
                    resolution = preset,
                    fps = fpsVal,
                    bitrate = EncoderConfig.getDefaultBitrate(codec, preset)
                )
                lifecycleScope.launch { startMediaPipeline() }
            }
            "switch_camera" -> {
                lifecycleScope.launch {
                    useFrontCamera = !useFrontCamera
                    StreamingController.useFrontCamera.value = useFrontCamera
                    restartCamera()
                }
            }
            "set_zoom" -> {
                val value = cmd.get("value")?.asFloat ?: return
                cameraManager?.setZoom(value)
                StreamingController.currentZoom.value = value
            }
            "set_focus" -> {
                val mode = cmd.get("mode")?.asString ?: return
                val distance = if (cmd.has("distance") && !cmd.get("distance").isJsonNull) cmd.get("distance").asFloat else 0f
                val tapX = if (cmd.has("x") && !cmd.get("x").isJsonNull) cmd.get("x").asFloat else null
                val tapY = if (cmd.has("y") && !cmd.get("y").isJsonNull) cmd.get("y").asFloat else null
                cameraManager?.setFocus(mode, distance, tapX, tapY)
                StreamingController.focusMode.value = mode
                StreamingController.focusDistance.value = distance
            }
            "set_exposure" -> {
                val comp = cmd.get("compensation")?.asInt ?: return
                cameraManager?.setExposureCompensation(comp)
                StreamingController.exposureCompensation.value = comp
            }
            "set_manual_exposure" -> {
                val iso = if (cmd.has("iso") && !cmd.get("iso").isJsonNull) cmd.get("iso").asInt else null
                val shutter = if (cmd.has("shutter_ns") && !cmd.get("shutter_ns").isJsonNull) cmd.get("shutter_ns").asLong else null
                cameraManager?.setManualExposure(iso, shutter)
            }
            "set_white_balance" -> {
                val mode = cmd.get("mode")?.asString ?: return
                cameraManager?.setWhiteBalance(mode)
                StreamingController.whiteBalanceMode.value = mode
            }
            "set_flash" -> {
                val enabled = cmd.get("enabled")?.asBoolean ?: return
                cameraManager?.setFlash(enabled)
                StreamingController.flashEnabled.value = enabled
            }
            "set_mirror" -> {
                val enabled = cmd.get("enabled")?.asBoolean ?: return
                cameraManager?.setMirror(enabled)
                StreamingController.mirrorEnabled.value = enabled
            }
            "set_resolution" -> {
                val value = cmd.get("value")?.asString ?: return
                val preset = ResolutionPreset.values().firstOrNull { it.displayName.equals(value, true) } ?: return
                currentConfig = currentConfig.copy(
                    resolution = preset,
                    bitrate = EncoderConfig.getDefaultBitrate(currentConfig.codec, preset)
                )
                lifecycleScope.launch { startMediaPipeline() }
            }
            "set_fps" -> {
                val value = cmd.get("value")?.asInt ?: return
                currentConfig = currentConfig.copy(fps = value)
                lifecycleScope.launch { startMediaPipeline() }
            }
            "set_codec" -> {
                val value = cmd.get("value")?.asString ?: return
                val codec = VideoCodecType.values().firstOrNull {
                    it.displayName.equals(value, true) || it.name.equals(value, true)
                } ?: return
                currentConfig = currentConfig.copy(
                    codec = codec,
                    bitrate = EncoderConfig.getDefaultBitrate(codec, currentConfig.resolution)
                )
                lifecycleScope.launch { startMediaPipeline() }
            }
            "set_bitrate" -> {
                val value = cmd.get("value")?.asLong ?: return
                if (value <= 0L) {
                    currentConfig = currentConfig.copy(adaptiveBitrate = true)
                    activeBitrateBps = currentConfig.bitrate
                    videoEncoder?.updateBitrate(activeBitrateBps)
                } else {
                    currentConfig = currentConfig.copy(adaptiveBitrate = false)
                    activeBitrateBps = value.toInt()
                    videoEncoder?.updateBitrate(activeBitrateBps)
                }
            }
            "request_keyframe" -> {
                videoEncoder?.requestKeyFrame()
            }
            "stop" -> {
                stopStreaming()
                stopSelf()
            }
        }
    }

    private suspend fun restartCamera() {
        cameraManager?.close()
        val encoderSurface = videoEncoder?.inputSurface ?: return
        startCamera(encoderSurface)
        videoEncoder?.requestKeyFrame()
    }

    private fun stopStreaming() {
        fallbackStartJob?.cancel()
        isStreaming.set(false)
        isMediaPipelineActive.set(false)
        StreamingController.isStreaming.value = false
        StreamingController.isConnected.value = false

        StreamingController.onZoomChanged = null
        StreamingController.onFocusChanged = null
        StreamingController.onExposureChanged = null
        StreamingController.onManualExposureChanged = null
        StreamingController.onWhiteBalanceChanged = null
        StreamingController.onFlashChanged = null
        StreamingController.onCameraToggle = null
        StreamingController.onPreviewSurfaceChanged = null

        cameraManager?.close()
        videoEncoder?.stop()
        audioEncoder?.stop()
        streamServer?.stop()
        nsdHelper?.unregisterService()
        batteryMonitor?.stopMonitoring()
        wakeLock?.let { if (it.isHeld) it.release() }
        Log.i(TAG, "Streaming stopped")
    }

    override fun onDestroy() {
        stopStreaming()
        super.onDestroy()
    }

    // ========== Notification ==========

    private fun createNotificationChannel() {
        val channel = NotificationChannel(
            NOTIFICATION_CHANNEL_ID,
            "Camera Streaming",
            NotificationManager.IMPORTANCE_LOW
        ).apply {
            description = "Active camera streaming notification"
        }
        getSystemService(NotificationManager::class.java).createNotificationChannel(channel)
    }

    private fun buildNotification(text: String): Notification {
        val stopIntent = Intent(this, CameraStreamingService::class.java).apply {
            action = ACTION_STOP
        }
        val stopPendingIntent = PendingIntent.getService(
            this, 0, stopIntent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        )

        val openIntent = packageManager.getLaunchIntentForPackage(packageName)
        val openPendingIntent = openIntent?.let {
            PendingIntent.getActivity(this, 0, it, PendingIntent.FLAG_IMMUTABLE)
        }

        return NotificationCompat.Builder(this, NOTIFICATION_CHANNEL_ID)
            .setContentTitle("CamDroid")
            .setContentText(text)
            .setSmallIcon(android.R.drawable.ic_menu_camera)
            .setOngoing(true)
            .setContentIntent(openPendingIntent)
            .addAction(android.R.drawable.ic_media_pause, "Stop", stopPendingIntent)
            .setCategory(NotificationCompat.CATEGORY_SERVICE)
            .setPriority(NotificationCompat.PRIORITY_LOW)
            .build()
    }

    private fun updateNotification(text: String) {
        val notification = buildNotification(text)
        getSystemService(NotificationManager::class.java).notify(NOTIFICATION_ID, notification)
    }
}
