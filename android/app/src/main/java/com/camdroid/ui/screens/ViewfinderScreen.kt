package com.camdroid.ui.screens

import android.content.Intent
import android.graphics.SurfaceTexture
import android.view.Surface
import android.view.TextureView
import androidx.compose.animation.*
import androidx.compose.animation.core.*
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.gestures.detectTransformGestures
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.core.content.ContextCompat
import com.camdroid.CameraStreamingService
import com.camdroid.StreamingController
import com.camdroid.data.SettingsRepository
import com.camdroid.ui.components.*
import com.camdroid.ui.theme.*

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ViewfinderScreen(
    onBack: () -> Unit,
    settingsRepository: SettingsRepository
) {
    val context = LocalContext.current

    // Persisted settings
    val savedCodec by settingsRepository.codec.collectAsState(initial = "H.264")
    val savedResolution by settingsRepository.resolution.collectAsState(initial = "1080p")
    val savedFps by settingsRepository.fps.collectAsState(initial = 60)
    val savedPort by settingsRepository.port.collectAsState(initial = 4747)

    // Live state from StreamingController
    val isStreaming by StreamingController.isStreaming.collectAsState()
    val isConnected by StreamingController.isConnected.collectAsState()
    val batteryLevel by StreamingController.batteryLevel.collectAsState()
    val isCharging by StreamingController.isCharging.collectAsState()
    val currentFps by StreamingController.currentFps.collectAsState()
    val currentBitrate by StreamingController.currentBitrate.collectAsState()
    val maxZoom by StreamingController.maxZoom.collectAsState()
    val currentZoom by StreamingController.currentZoom.collectAsState()
    val useFrontCamera by StreamingController.useFrontCamera.collectAsState()
    val flashEnabled by StreamingController.flashEnabled.collectAsState()
    val isDimMode by StreamingController.isDimMode.collectAsState()

    // Pro controls bottom sheet state
    var showProControls by remember { mutableStateOf(false) }

    // Focus ring animation state
    var focusTapPosition by remember { mutableStateOf<Offset?>(null) }
    var showFocusRing by remember { mutableStateOf(false) }
    val focusRingScale by animateFloatAsState(
        targetValue = if (showFocusRing) 1f else 1.5f,
        animationSpec = tween(200),
        label = "focusRing",
        finishedListener = {
            if (showFocusRing) {
                // Hide after a delay via LaunchedEffect below
            }
        }
    )
    val focusRingAlpha by animateFloatAsState(
        targetValue = if (showFocusRing) 1f else 0f,
        animationSpec = tween(300),
        label = "focusAlpha"
    )

    LaunchedEffect(showFocusRing) {
        if (showFocusRing) {
            kotlinx.coroutines.delay(1200)
            showFocusRing = false
        }
    }

    Box(
        modifier = Modifier
            .fillMaxSize()
            .background(Color.Black)
    ) {
        // ── Camera Preview ──
        if (isStreaming && !isDimMode) {
            AndroidView(
                factory = { ctx ->
                    TextureView(ctx).apply {
                        surfaceTextureListener = object : TextureView.SurfaceTextureListener {
                            override fun onSurfaceTextureAvailable(st: SurfaceTexture, w: Int, h: Int) {
                                StreamingController.activePreviewSurface = Surface(st)
                            }
                            override fun onSurfaceTextureSizeChanged(st: SurfaceTexture, w: Int, h: Int) {}
                            override fun onSurfaceTextureDestroyed(st: SurfaceTexture): Boolean {
                                StreamingController.activePreviewSurface = null
                                return true
                            }
                            override fun onSurfaceTextureUpdated(st: SurfaceTexture) {}
                        }
                    }
                },
                modifier = Modifier
                    .fillMaxSize()
                    .pointerInput(Unit) {
                        detectTransformGestures { _, _, zoom, _ ->
                            val newZoom = (currentZoom * zoom).coerceIn(1f, maxZoom)
                            StreamingController.onZoomChanged?.invoke(newZoom)
                        }
                    }
                    .pointerInput(Unit) {
                        detectTapGestures { offset ->
                            focusTapPosition = offset
                            showFocusRing = true
                            // Trigger tap-to-focus
                            val normalizedX = offset.x / size.width
                            val normalizedY = offset.y / size.height
                            StreamingController.onFocusChanged?.invoke("auto", 0f)
                        }
                    }
            )
        } else if (!isStreaming) {
            // Idle — show a prompt to start
            Box(
                modifier = Modifier
                    .fillMaxSize()
                    .background(MaterialTheme.colorScheme.surface),
                contentAlignment = Alignment.Center
            ) {
                Column(
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = Arrangement.spacedBy(12.dp)
                ) {
                    Icon(
                        Icons.Default.Videocam,
                        contentDescription = null,
                        tint = MaterialTheme.colorScheme.primary.copy(alpha = 0.5f),
                        modifier = Modifier.size(64.dp)
                    )
                    Text(
                        "Tap the shutter to start streaming",
                        style = MaterialTheme.typography.bodyLarge,
                        color = MaterialTheme.colorScheme.onSurfaceVariant
                    )
                }
            }
        }

        // ── Focus Ring Overlay ──
        if (focusTapPosition != null && focusRingAlpha > 0f) {
            val pos = focusTapPosition!!
            Box(
                modifier = Modifier
                    .fillMaxSize()
                    .drawBehind {
                        drawCircle(
                            color = Color.White.copy(alpha = focusRingAlpha * 0.8f),
                            radius = 40.dp.toPx() * focusRingScale,
                            center = pos,
                            style = Stroke(width = 2.dp.toPx())
                        )
                    }
            )
        }

        // ── Dim Mode Overlay ──
        if (isStreaming && isDimMode) {
            Box(
                modifier = Modifier
                    .fillMaxSize()
                    .background(Color.Black)
                    .clickable { StreamingController.isDimMode.value = false },
                contentAlignment = Alignment.Center
            ) {
                Column(
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = Arrangement.spacedBy(12.dp)
                ) {
                    Icon(
                        Icons.Default.Brightness2,
                        contentDescription = null,
                        tint = Color.DarkGray,
                        modifier = Modifier.size(48.dp)
                    )
                    Text(
                        "Dim Mode — Tap to wake",
                        style = MaterialTheme.typography.bodyMedium,
                        color = Color.DarkGray
                    )
                }
            }
        }

        // ── UI Overlay (visible when not dimmed) ──
        if (!isDimMode) {

            // Top HUD
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .statusBarsPadding()
                    .padding(horizontal = 12.dp, vertical = 8.dp)
                    .align(Alignment.TopStart),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically
            ) {
                // Back button
                IconButton(
                    onClick = onBack,
                    colors = IconButtonDefaults.iconButtonColors(
                        containerColor = MaterialTheme.colorScheme.surfaceContainerHigh.copy(alpha = 0.7f)
                    ),
                    modifier = Modifier.size(40.dp)
                ) {
                    Icon(
                        Icons.AutoMirrored.Filled.ArrowBack,
                        contentDescription = "Back",
                        tint = Color.White,
                        modifier = Modifier.size(20.dp)
                    )
                }

                // Stats HUD
                AnimatedVisibility(visible = isStreaming) {
                    Row(
                        horizontalArrangement = Arrangement.spacedBy(8.dp)
                    ) {
                        StatChip(
                            label = "FPS",
                            value = String.format("%.0f", currentFps)
                        )
                        StatChip(
                            label = "Mbps",
                            value = String.format("%.1f", currentBitrate)
                        )
                    }
                }

                Row(
                    horizontalArrangement = Arrangement.spacedBy(6.dp),
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    if (isStreaming) {
                        IconButton(
                            onClick = { StreamingController.isDimMode.value = true },
                            colors = IconButtonDefaults.iconButtonColors(
                                containerColor = MaterialTheme.colorScheme.surfaceContainerHigh.copy(alpha = 0.7f)
                            ),
                            modifier = Modifier.size(36.dp)
                        ) {
                            Icon(
                                Icons.Default.Brightness2,
                                contentDescription = "Dim Mode",
                                tint = Color.White,
                                modifier = Modifier.size(16.dp)
                            )
                        }
                    }
                    BatteryIndicator(level = batteryLevel, isCharging = isCharging)
                }
            }

            // ── Bottom Controls ──
            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .align(Alignment.BottomCenter)
                    .navigationBarsPadding()
                    .padding(bottom = 16.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.spacedBy(12.dp)
            ) {
                // Zoom Wheel (visible when streaming)
                AnimatedVisibility(visible = isStreaming) {
                    ZoomWheel(
                        currentZoom = currentZoom,
                        onZoomSelected = { zoom ->
                            StreamingController.onZoomChanged?.invoke(zoom.coerceIn(1f, maxZoom))
                        }
                    )
                }

                // Control bar: Flash / Pro / Shutter / Flip / Dim
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(horizontal = 24.dp),
                    horizontalArrangement = Arrangement.SpaceEvenly,
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    // Flash
                    AnimatedVisibility(visible = isStreaming) {
                        ControlButton(
                            icon = if (flashEnabled) Icons.Default.FlashOn else Icons.Default.FlashOff,
                            label = "Flash",
                            isActive = flashEnabled,
                            onClick = { StreamingController.onFlashChanged?.invoke(!flashEnabled) },
                            size = 44.dp
                        )
                    }

                    // Pro controls toggle
                    AnimatedVisibility(visible = isStreaming) {
                        ControlButton(
                            icon = Icons.Default.Tune,
                            label = "Pro",
                            isActive = showProControls,
                            onClick = { showProControls = !showProControls },
                            size = 44.dp
                        )
                    }

                    // Shutter button
                    ShutterButton(
                        isStreaming = isStreaming,
                        onClick = {
                            if (isStreaming) {
                                val stopIntent = Intent(context, CameraStreamingService::class.java).apply {
                                    action = CameraStreamingService.ACTION_STOP
                                }
                                context.startService(stopIntent)
                            } else {
                                val codecStr = when (savedCodec) {
                                    "H.264" -> "h264"
                                    "H.265" -> "h265"
                                    "MJPEG" -> "mjpeg"
                                    else -> "h264"
                                }
                                val intent = Intent(context, CameraStreamingService::class.java).apply {
                                    putExtra(CameraStreamingService.EXTRA_USE_FRONT, useFrontCamera)
                                    putExtra(CameraStreamingService.EXTRA_CODEC, codecStr)
                                    putExtra(CameraStreamingService.EXTRA_RESOLUTION, savedResolution)
                                    putExtra(CameraStreamingService.EXTRA_FPS, savedFps)
                                    putExtra(CameraStreamingService.EXTRA_PORT, savedPort)
                                }
                                ContextCompat.startForegroundService(context, intent)
                            }
                        }
                    )

                    // Camera flip
                    AnimatedVisibility(visible = isStreaming) {
                        ControlButton(
                            icon = Icons.Default.FlipCameraAndroid,
                            label = "Flip",
                            onClick = { StreamingController.onCameraToggle?.invoke(!useFrontCamera) },
                            size = 44.dp
                        )
                    }

                    // Mirror
                    AnimatedVisibility(visible = isStreaming) {
                        ControlButton(
                            icon = Icons.Default.Flip,
                            label = "Mirror",
                            onClick = {
                                val current = StreamingController.mirrorEnabled.value
                                StreamingController.onFlashChanged?.invoke(!current)
                            },
                            size = 44.dp
                        )
                    }
                }

                // Label
                Text(
                    text = when {
                        isStreaming && isConnected -> "● Streaming Active"
                        isStreaming -> "Waiting for connection..."
                        else -> "Ready"
                    },
                    style = MaterialTheme.typography.labelSmall,
                    color = when {
                        isStreaming && isConnected -> StatusGreen
                        isStreaming -> StatusYellow
                        else -> Color.White.copy(alpha = 0.5f)
                    },
                    letterSpacing = 1.sp,
                    fontWeight = FontWeight.Medium
                )
            }
        }

        // ── Pro Controls Bottom Sheet ──
        AnimatedVisibility(
            visible = showProControls && isStreaming && !isDimMode,
            enter = slideInVertically(initialOffsetY = { it }) + fadeIn(),
            exit = slideOutVertically(targetOffsetY = { it }) + fadeOut(),
            modifier = Modifier.align(Alignment.BottomCenter)
        ) {
            ProControlsSheet()
        }
    }
}

/**
 * Pro camera controls in a bottom sheet.
 */
@Composable
private fun ProControlsSheet() {
    val focusMode by StreamingController.focusMode.collectAsState()
    val focusDistance by StreamingController.focusDistance.collectAsState()
    val exposureMode by StreamingController.exposureMode.collectAsState()
    val exposureCompensation by StreamingController.exposureCompensation.collectAsState()
    val manualIso by StreamingController.manualIso.collectAsState()
    val manualShutterSpeedNs by StreamingController.manualShutterSpeedNs.collectAsState()
    val whiteBalanceMode by StreamingController.whiteBalanceMode.collectAsState()

    Card(
        modifier = Modifier
            .fillMaxWidth()
            .navigationBarsPadding()
            .padding(horizontal = 12.dp, vertical = 130.dp),
        shape = RoundedCornerShape(topStart = 24.dp, topEnd = 24.dp, bottomStart = 16.dp, bottomEnd = 16.dp),
        colors = CardDefaults.cardColors(
            containerColor = MaterialTheme.colorScheme.surfaceContainerHigh.copy(alpha = 0.92f)
        )
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .padding(20.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp)
        ) {
            // Handle bar
            Box(
                modifier = Modifier
                    .width(40.dp)
                    .height(4.dp)
                    .clip(RoundedCornerShape(2.dp))
                    .background(MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.3f))
                    .align(Alignment.CenterHorizontally)
            )

            Text(
                "Pro Controls",
                style = MaterialTheme.typography.titleSmall,
                fontWeight = FontWeight.Bold,
                color = MaterialTheme.colorScheme.primary
            )

            // ── Focus ──
            Column {
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    Text("Focus", style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                        FilterChip(
                            selected = focusMode == "auto",
                            onClick = { StreamingController.onFocusChanged?.invoke("auto", 0f) },
                            label = { Text("Auto", style = MaterialTheme.typography.labelSmall) },
                            modifier = Modifier.height(28.dp)
                        )
                        FilterChip(
                            selected = focusMode == "manual",
                            onClick = { StreamingController.onFocusChanged?.invoke("manual", focusDistance) },
                            label = { Text("Manual", style = MaterialTheme.typography.labelSmall) },
                            modifier = Modifier.height(28.dp)
                        )
                    }
                }
                if (focusMode == "manual") {
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.SpaceBetween
                    ) {
                        Text("Distance", style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                        Text("${String.format("%.1f", focusDistance)}d", style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.primary, fontWeight = FontWeight.Bold)
                    }
                    Slider(
                        value = focusDistance,
                        onValueChange = { StreamingController.onFocusChanged?.invoke("manual", it) },
                        valueRange = 0f..10f,
                        modifier = Modifier.height(32.dp),
                        colors = SliderDefaults.colors(
                            thumbColor = MaterialTheme.colorScheme.primary,
                            activeTrackColor = MaterialTheme.colorScheme.primary
                        )
                    )
                }
            }

            HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.3f))

            // ── Exposure ──
            Column {
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    Text("Exposure", style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                        FilterChip(
                            selected = exposureMode == "auto",
                            onClick = { StreamingController.onManualExposureChanged?.invoke(null, null) },
                            label = { Text("Auto", style = MaterialTheme.typography.labelSmall) },
                            modifier = Modifier.height(28.dp)
                        )
                        FilterChip(
                            selected = exposureMode == "manual",
                            onClick = { StreamingController.onManualExposureChanged?.invoke(manualIso, manualShutterSpeedNs) },
                            label = { Text("Manual", style = MaterialTheme.typography.labelSmall) },
                            modifier = Modifier.height(28.dp)
                        )
                    }
                }

                if (exposureMode == "auto") {
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.SpaceBetween
                    ) {
                        Text("EV", style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                        Text("${if (exposureCompensation > 0) "+" else ""}$exposureCompensation", style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.primary, fontWeight = FontWeight.Bold)
                    }
                    Slider(
                        value = exposureCompensation.toFloat(),
                        onValueChange = { StreamingController.onExposureChanged?.invoke(it.toInt()) },
                        valueRange = -4f..4f,
                        steps = 7,
                        modifier = Modifier.height(32.dp),
                        colors = SliderDefaults.colors(
                            thumbColor = MaterialTheme.colorScheme.primary,
                            activeTrackColor = MaterialTheme.colorScheme.primary
                        )
                    )
                } else {
                    // ISO
                    Row(modifier = Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
                        Text("ISO", style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                        Text("$manualIso", style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.primary, fontWeight = FontWeight.Bold)
                    }
                    Slider(
                        value = manualIso.toFloat(),
                        onValueChange = { StreamingController.onManualExposureChanged?.invoke(it.toInt(), manualShutterSpeedNs) },
                        valueRange = 100f..3200f,
                        modifier = Modifier.height(32.dp),
                        colors = SliderDefaults.colors(thumbColor = MaterialTheme.colorScheme.primary, activeTrackColor = MaterialTheme.colorScheme.primary)
                    )
                    // Shutter speed chips
                    Text("Shutter", style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                        listOf(16666666L to "1/60", 8000000L to "1/125", 4000000L to "1/250", 2000000L to "1/500").forEach { (speed, label) ->
                            FilterChip(
                                selected = manualShutterSpeedNs == speed,
                                onClick = { StreamingController.onManualExposureChanged?.invoke(manualIso, speed) },
                                label = { Text(label, style = MaterialTheme.typography.labelSmall) },
                                modifier = Modifier.height(28.dp)
                            )
                        }
                    }
                }
            }

            HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.3f))

            // ── White Balance ──
            Column {
                Text("White Balance", style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                Spacer(Modifier.height(4.dp))
                Row(
                    horizontalArrangement = Arrangement.spacedBy(4.dp)
                ) {
                    listOf("auto" to "Auto", "daylight" to "☀️", "tungsten" to "💡", "fluorescent" to "🔬", "cloudy" to "☁️").forEach { (mode, icon) ->
                        FilterChip(
                            selected = whiteBalanceMode == mode,
                            onClick = { StreamingController.onWhiteBalanceChanged?.invoke(mode) },
                            label = { Text(icon, style = MaterialTheme.typography.labelSmall) },
                            modifier = Modifier.height(28.dp)
                        )
                    }
                }
            }
        }
    }
}
