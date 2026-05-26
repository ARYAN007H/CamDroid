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

    // Stats
    private val frameCount = AtomicLong(0)
    private val isStreaming = AtomicBoolean(false)

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

            currentConfig = EncoderConfig(
                codec = codec,
                resolution = resolution,
                fps = fps,
                bitrate = EncoderConfig.getDefaultBitrate(codec, resolution)
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

        // Acquire wake lock
        val pm = getSystemService(POWER_SERVICE) as PowerManager
        wakeLock = pm.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "CamDroid::StreamWake").apply { acquire() }

        // Battery monitor
        batteryMonitor = BatteryMonitor(this).apply {
            onLowBattery = {
                Log.w(TAG, "Low battery — reducing quality")
                lifecycleScope.launch {
                    // Reduce bitrate by 50% on low battery
                    videoEncoder?.updateBitrate(currentConfig.bitrate / 2)
                }
            }
            startMonitoring()
        }

        // Start TCP server
        streamServer = StreamServer(
            port = serverPort,
            onControlCommand = { cmd -> handleControlCommand(cmd) },
            onClientConnected = {
                lifecycleScope.launch { onClientConnected() }
            },
            onClientDisconnected = {
                updateNotification("Waiting for client...")
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

    private suspend fun onClientConnected() {
        Log.i(TAG, "Client connected, starting encoder and camera...")

        // Send server capabilities
        val capabilities = JsonObject().apply {
            addProperty("version", "1.0")
            addProperty("device", NetworkUtils.getDeviceName())
            add("codecs", gson.toJsonTree(listOf("h264", "h265", "mjpeg")))
            add("resolutions", gson.toJsonTree(listOf("1080p", "1440p", "4k")))
            add("fps", gson.toJsonTree(listOf(30, 60)))
            addProperty("audio", true)
            addProperty("battery", batteryMonitor?.batteryLevel ?: 100)
        }
        streamServer?.sendMetadata(gson.toJson(capabilities))

        // Start video encoder
        videoEncoder = VideoEncoder(currentConfig) { data, isConfig, isKeyFrame, pts ->
            if (isConfig) {
                streamServer?.sendVideoConfig(data)
            } else {
                streamServer?.sendVideoFrame(data)
                frameCount.incrementAndGet()
            }
        }.also { it.start() }

        // Start audio encoder
        if (audioEnabled) {
            audioEncoder = AudioEncoder { data, isConfig, pts ->
                if (isConfig) {
                    streamServer?.sendAudioConfig(data)
                } else {
                    streamServer?.sendAudioFrame(data)
                }
            }.also { it.start() }
        }

        // Start camera
        val encoderSurface = videoEncoder?.inputSurface ?: return
        startCamera(encoderSurface)

        val ipAddress = NetworkUtils.getWifiIpAddress(this) ?: "?"
        updateNotification("Streaming • ${currentConfig.resolution.displayName}@${currentConfig.fps}fps • $ipAddress:$serverPort")
    }

    private suspend fun startCamera(encoderSurface: Surface) {
        // Create a dummy SurfaceTexture for the preview output.
        // In the full app, this would be connected to the Compose preview.
        val texId = 0 // Dummy texture ID
        previewSurfaceTexture = SurfaceTexture(texId).apply {
            setDefaultBufferSize(currentConfig.resolution.width, currentConfig.resolution.height)
        }
        previewSurface = Surface(previewSurfaceTexture)

        cameraManager = CameraManager(this).apply {
            initialize()
            openCamera(previewSurface!!, encoderSurface, useFrontCamera, currentConfig.fps)
        }
        Log.i(TAG, "Camera started")
    }

    private fun handleControlCommand(cmd: JsonObject) {
        val command = cmd.get("cmd")?.asString ?: return
        Log.d(TAG, "Handling control: $command")

        when (command) {
            "switch_camera" -> {
                lifecycleScope.launch {
                    useFrontCamera = !useFrontCamera
                    restartCamera()
                }
            }
            "set_zoom" -> {
                val value = cmd.get("value")?.asFloat ?: return
                cameraManager?.setZoom(value)
            }
            "set_focus" -> {
                val mode = cmd.get("mode")?.asString ?: return
                val distance = cmd.get("distance")?.asFloat
                cameraManager?.setFocus(mode, distance)
            }
            "set_exposure" -> {
                val comp = cmd.get("compensation")?.asInt ?: return
                cameraManager?.setExposureCompensation(comp)
            }
            "set_white_balance" -> {
                val mode = cmd.get("mode")?.asString ?: return
                cameraManager?.setWhiteBalance(mode)
            }
            "set_flash" -> {
                val enabled = cmd.get("enabled")?.asBoolean ?: return
                cameraManager?.setFlash(enabled)
            }
            "set_mirror" -> {
                val enabled = cmd.get("enabled")?.asBoolean ?: return
                cameraManager?.setMirror(enabled)
            }
            "set_resolution" -> {
                val value = cmd.get("value")?.asString ?: return
                val preset = ResolutionPreset.values().firstOrNull { it.displayName.equals(value, true) } ?: return
                currentConfig = currentConfig.copy(resolution = preset, bitrate = EncoderConfig.getDefaultBitrate(currentConfig.codec, preset))
                lifecycleScope.launch { restartEncoder() }
            }
            "set_fps" -> {
                val value = cmd.get("value")?.asInt ?: return
                currentConfig = currentConfig.copy(fps = value)
                lifecycleScope.launch { restartEncoder() }
            }
            "set_codec" -> {
                val value = cmd.get("value")?.asString ?: return
                val codec = VideoCodecType.values().firstOrNull {
                    it.displayName.equals(value, true) || it.name.equals(value, true)
                } ?: return
                currentConfig = currentConfig.copy(codec = codec, bitrate = EncoderConfig.getDefaultBitrate(codec, currentConfig.resolution))
                lifecycleScope.launch { restartEncoder() }
            }
            "set_bitrate" -> {
                val value = cmd.get("value")?.asLong ?: return
                videoEncoder?.updateBitrate(value.toInt())
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

    private suspend fun restartEncoder() {
        cameraManager?.close()
        videoEncoder?.stop()
        audioEncoder?.stop()

        videoEncoder = VideoEncoder(currentConfig) { data, isConfig, isKeyFrame, pts ->
            if (isConfig) streamServer?.sendVideoConfig(data) else streamServer?.sendVideoFrame(data)
        }.also { it.start() }

        if (audioEnabled) {
            audioEncoder = AudioEncoder { data, isConfig, pts ->
                if (isConfig) streamServer?.sendAudioConfig(data) else streamServer?.sendAudioFrame(data)
            }.also { it.start() }
        }

        val encoderSurface = videoEncoder?.inputSurface ?: return
        startCamera(encoderSurface)
    }

    private suspend fun restartCamera() {
        cameraManager?.close()
        val encoderSurface = videoEncoder?.inputSurface ?: return
        startCamera(encoderSurface)
    }

    private fun stopStreaming() {
        isStreaming.set(false)
        cameraManager?.close()
        videoEncoder?.stop()
        audioEncoder?.stop()
        streamServer?.stop()
        nsdHelper?.unregisterService()
        batteryMonitor?.stopMonitoring()
        previewSurface?.release()
        previewSurfaceTexture?.release()
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
