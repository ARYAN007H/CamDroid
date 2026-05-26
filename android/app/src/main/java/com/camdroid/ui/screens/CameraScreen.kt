package com.camdroid.ui.screens

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.SurfaceTexture
import android.os.Build
import android.view.Surface
import android.view.TextureView
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.animation.*
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.blur
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.core.content.ContextCompat
import com.camdroid.CameraStreamingService
import com.camdroid.StreamingController
import com.camdroid.ui.components.*
import com.camdroid.ui.theme.*
import com.camdroid.util.NetworkUtils

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun CameraScreen() {
    val context = LocalContext.current

    // Collect flows from StreamingController
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

    val focusMode by StreamingController.focusMode.collectAsState()
    val focusDistance by StreamingController.focusDistance.collectAsState()

    val exposureMode by StreamingController.exposureMode.collectAsState()
    val exposureCompensation by StreamingController.exposureCompensation.collectAsState()
    val manualIso by StreamingController.manualIso.collectAsState()
    val manualShutterSpeedNs by StreamingController.manualShutterSpeedNs.collectAsState()

    val whiteBalanceMode by StreamingController.whiteBalanceMode.collectAsState()
    val isDimMode by StreamingController.isDimMode.collectAsState()

    val ipAddress = remember { NetworkUtils.getWifiIpAddress(context) }
    val port = 4747

    // Permission handling
    val requiredPermissions = buildList {
        add(Manifest.permission.CAMERA)
        add(Manifest.permission.RECORD_AUDIO)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            add(Manifest.permission.POST_NOTIFICATIONS)
        }
    }

    var allPermissionsGranted by remember {
        mutableStateOf(
            requiredPermissions.all {
                ContextCompat.checkSelfPermission(context, it) == PackageManager.PERMISSION_GRANTED
            }
        )
    }

    val permissionLauncher = rememberLauncherForActivityResult(
        ActivityResultContracts.RequestMultiplePermissions()
    ) { results ->
        allPermissionsGranted = results.values.all { it }
    }

    LaunchedEffect(Unit) {
        if (!allPermissionsGranted) {
            permissionLauncher.launch(requiredPermissions.toTypedArray())
        }
    }

    // Picker Dialog states
    var showCodecPicker by remember { mutableStateOf(false) }
    var showResPicker by remember { mutableStateOf(false) }
    var selectedCodec by remember { mutableStateOf("H.264") }
    var selectedResolution by remember { mutableStateOf("1080p") }
    var selectedFps by remember { mutableIntStateOf(60) }

    Box(
        modifier = Modifier
            .fillMaxSize()
            .background(Color.Black)
    ) {
        // 1. Live Camera Preview (Only shown when streaming is active and dim mode is OFF)
        if (isStreaming && !isDimMode) {
            AndroidView(
                factory = { ctx ->
                    TextureView(ctx).apply {
                        surfaceTextureListener = object : TextureView.SurfaceTextureListener {
                            override fun onSurfaceTextureAvailable(st: SurfaceTexture, w: Int, h: Int) {
                                val surface = Surface(st)
                                StreamingController.activePreviewSurface = surface
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
                modifier = Modifier.fillMaxSize()
            )
        } else if (!isStreaming) {
            // Idle placeholder
            Box(
                modifier = Modifier
                    .fillMaxSize()
                    .background(
                        Brush.verticalGradient(
                            colors = listOf(Color(0xFF0F172A), Color(0xFF020617))
                        )
                    ),
                contentAlignment = Alignment.Center
            ) {
                Column(
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = Arrangement.spacedBy(12.dp)
                ) {
                    Icon(
                        Icons.Default.Videocam,
                        contentDescription = null,
                        tint = CyanPrimary.copy(alpha = 0.8f),
                        modifier = Modifier.size(72.dp)
                    )
                    Text(
                        "CamDroid Streaming Server",
                        style = MaterialTheme.typography.titleLarge,
                        fontWeight = FontWeight.Bold,
                        color = Color.White
                    )
                    Text(
                        "Tap below to start streaming",
                        style = MaterialTheme.typography.bodyMedium,
                        color = TextDim
                    )
                }
            }
        }

        // 2. Dim Mode Screen Overlay (Pure Black Cover)
        if (isStreaming && isDimMode) {
            Box(
                modifier = Modifier
                    .fillMaxSize()
                    .background(Color.Black)
                    .pointerInput(Unit) {
                        detectTapGestures(
                            onDoubleTap = {
                                StreamingController.isDimMode.value = false
                            },
                            onTap = {
                                StreamingController.isDimMode.value = false
                            }
                        )
                    },
                contentAlignment = Alignment.Center
            ) {
                Column(
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = Arrangement.spacedBy(16.dp)
                ) {
                    Icon(
                        Icons.Default.PowerSettingsNew,
                        contentDescription = null,
                        tint = Color.DarkGray,
                        modifier = Modifier.size(64.dp)
                    )
                    Text(
                        "Dim Mode Active",
                        style = MaterialTheme.typography.titleMedium,
                        color = Color.DarkGray,
                        fontWeight = FontWeight.Bold
                    )
                    Text(
                        "Tap anywhere to wake up screen",
                        style = MaterialTheme.typography.bodySmall,
                        color = Color.DarkGray.copy(alpha = 0.7f)
                    )
                }
            }
        }

        // 3. UI Controls Overlay (Visible only when not in Dim Mode)
        if (!isDimMode) {
            // HUD Top overlay
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(16.dp)
                    .statusBarsPadding()
                    .align(Alignment.TopStart),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically
            ) {
                StatusBadge(
                    isConnected = isConnected,
                    ipAddress = ipAddress,
                    port = port
                )

                Row(
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    if (isStreaming) {
                        // Dim Mode toggle button
                        IconButton(
                            onClick = { StreamingController.isDimMode.value = true },
                            colors = IconButtonDefaults.iconButtonColors(
                                containerColor = SurfaceContainer.copy(alpha = 0.8f)
                            ),
                            modifier = Modifier.size(38.dp)
                        ) {
                            Icon(
                                Icons.Default.SettingsBrightness,
                                contentDescription = "Dim Mode",
                                tint = Color.White,
                                modifier = Modifier.size(18.dp)
                            )
                        }
                    }

                    BatteryIndicator(
                        level = batteryLevel,
                        isCharging = isCharging
                    )
                }
            }

            // Codec, resolution, fps chips (Visible only when idle)
            AnimatedVisibility(
                visible = !isStreaming,
                modifier = Modifier
                    .align(Alignment.TopCenter)
                    .padding(top = 80.dp)
                    .statusBarsPadding()
            ) {
                Row(
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                    modifier = Modifier.padding(horizontal = 16.dp)
                ) {
                    SettingsChip(
                        label = "Codec",
                        value = selectedCodec,
                        onClick = { showCodecPicker = true }
                    )
                    SettingsChip(
                        label = "Res",
                        value = selectedResolution,
                        onClick = { showResPicker = true }
                    )
                    SettingsChip(
                        label = "FPS",
                        value = "$selectedFps",
                        onClick = { selectedFps = if (selectedFps == 60) 30 else 60 }
                    )
                }
            }

            // Real-time FPS/Bitrate HUD overlay (Visible when streaming)
            AnimatedVisibility(
                visible = isStreaming,
                modifier = Modifier
                    .align(Alignment.TopCenter)
                    .padding(top = 80.dp)
                    .statusBarsPadding()
            ) {
                Box(
                    modifier = Modifier
                        .clip(RoundedCornerShape(16.dp))
                        .background(SurfaceContainer.copy(alpha = 0.7f))
                        .padding(horizontal = 16.dp, vertical = 6.dp)
                ) {
                    Row(
                        horizontalArrangement = Arrangement.spacedBy(16.dp),
                        verticalAlignment = Alignment.CenterVertically
                    ) {
                        Text(
                            text = "FPS: ${String.format("%.1f", currentFps)}",
                            color = CyanPrimary,
                            fontSize = 12.sp,
                            fontWeight = FontWeight.Bold
                        )
                        Text(
                            text = "Bitrate: ${String.format("%.1f", currentBitrate)} Mbps",
                            color = CyanPrimary,
                            fontSize = 12.sp,
                            fontWeight = FontWeight.Bold
                        )
                    }
                }
            }

            // Camera Controls Drawers/Sliders (Visible when streaming)
            AnimatedVisibility(
                visible = isStreaming,
                modifier = Modifier
                    .align(Alignment.CenterEnd)
                    .padding(end = 16.dp)
                    .fillMaxHeight(0.6f)
                    .width(280.dp)
            ) {
                Box(
                    modifier = Modifier
                        .clip(RoundedCornerShape(24.dp))
                        .background(SurfaceContainer.copy(alpha = 0.85f))
                        .padding(16.dp)
                ) {
                    Column(
                        modifier = Modifier
                            .fillMaxSize()
                            .verticalScroll(rememberScrollState()),
                        verticalArrangement = Arrangement.spacedBy(16.dp)
                    ) {
                        Text(
                            "Pro Camera Controls",
                            color = Color.White,
                            fontSize = 14.sp,
                            fontWeight = FontWeight.Bold,
                            textAlign = TextAlign.Center,
                            modifier = Modifier.fillMaxWidth()
                        )

                        HorizontalDivider(color = Color.White.copy(alpha = 0.1f))

                        // Zoom Slider
                        Column {
                            Row(
                                modifier = Modifier.fillMaxWidth(),
                                horizontalArrangement = Arrangement.SpaceBetween
                            ) {
                                Text("Zoom", color = TextSecondary, fontSize = 11.sp)
                                Text("${String.format("%.1f", currentZoom)}x", color = CyanPrimary, fontSize = 11.sp, fontWeight = FontWeight.Bold)
                            }
                            Slider(
                                value = currentZoom,
                                onValueChange = { StreamingController.onZoomChanged?.invoke(it) },
                                valueRange = 1f..maxZoom.coerceAtLeast(1.1f),
                                colors = SliderDefaults.colors(
                                    thumbColor = CyanPrimary,
                                    activeTrackColor = CyanPrimary,
                                    inactiveTrackColor = Color.DarkGray
                                )
                            )
                        }

                        // Focus Controls
                        Column {
                            Text("Focus Mode", color = TextSecondary, fontSize = 11.sp)
                            Spacer(Modifier.height(4.dp))
                            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                                Button(
                                    onClick = { StreamingController.onFocusChanged?.invoke("auto", 0f) },
                                    colors = ButtonDefaults.buttonColors(
                                        containerColor = if (focusMode == "auto") CyanPrimary else Color.DarkGray
                                    ),
                                    modifier = Modifier.weight(1f),
                                    contentPadding = PaddingValues(0.dp)
                                ) {
                                    Text("Auto", fontSize = 10.sp, color = if (focusMode == "auto") Color.Black else Color.White)
                                }
                                Button(
                                    onClick = { StreamingController.onFocusChanged?.invoke("manual", focusDistance) },
                                    colors = ButtonDefaults.buttonColors(
                                        containerColor = if (focusMode == "manual") CyanPrimary else Color.DarkGray
                                    ),
                                    modifier = Modifier.weight(1f),
                                    contentPadding = PaddingValues(0.dp)
                                ) {
                                    Text("Manual", fontSize = 10.sp, color = if (focusMode == "manual") Color.Black else Color.White)
                                }
                            }
                            if (focusMode == "manual") {
                                Spacer(Modifier.height(4.dp))
                                Row(
                                    modifier = Modifier.fillMaxWidth(),
                                    horizontalArrangement = Arrangement.SpaceBetween
                                ) {
                                    Text("Focus Distance", color = TextSecondary, fontSize = 10.sp)
                                    Text("${String.format("%.1f", focusDistance)} diopters", color = CyanPrimary, fontSize = 10.sp)
                                }
                                Slider(
                                    value = focusDistance,
                                    onValueChange = { StreamingController.onFocusChanged?.invoke("manual", it) },
                                    valueRange = 0f..10f,
                                    colors = SliderDefaults.colors(
                                        thumbColor = CyanPrimary,
                                        activeTrackColor = CyanPrimary
                                    )
                                )
                            }
                        }

                        // Exposure Mode & settings
                        Column {
                            Text("Exposure Mode", color = TextSecondary, fontSize = 11.sp)
                            Spacer(Modifier.height(4.dp))
                            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                                Button(
                                    onClick = { StreamingController.onManualExposureChanged?.invoke(null, null) },
                                    colors = ButtonDefaults.buttonColors(
                                        containerColor = if (exposureMode == "auto") CyanPrimary else Color.DarkGray
                                    ),
                                    modifier = Modifier.weight(1f),
                                    contentPadding = PaddingValues(0.dp)
                                ) {
                                    Text("Auto", fontSize = 10.sp, color = if (exposureMode == "auto") Color.Black else Color.White)
                                }
                                Button(
                                    onClick = { StreamingController.onManualExposureChanged?.invoke(manualIso, manualShutterSpeedNs) },
                                    colors = ButtonDefaults.buttonColors(
                                        containerColor = if (exposureMode == "manual") CyanPrimary else Color.DarkGray
                                    ),
                                    modifier = Modifier.weight(1f),
                                    contentPadding = PaddingValues(0.dp)
                                ) {
                                    Text("Manual", fontSize = 10.sp, color = if (exposureMode == "manual") Color.Black else Color.White)
                                }
                            }

                            if (exposureMode == "auto") {
                                Spacer(Modifier.height(8.dp))
                                Row(
                                    modifier = Modifier.fillMaxWidth(),
                                    horizontalArrangement = Arrangement.SpaceBetween
                                ) {
                                    Text("Exposure Comp", color = TextSecondary, fontSize = 10.sp)
                                    Text("${if (exposureCompensation > 0) "+" else ""}$exposureCompensation EV", color = CyanPrimary, fontSize = 10.sp)
                                }
                                Slider(
                                    value = exposureCompensation.toFloat(),
                                    onValueChange = { StreamingController.onExposureChanged?.invoke(it.toInt()) },
                                    valueRange = -4f..4f,
                                    steps = 7,
                                    colors = SliderDefaults.colors(
                                        thumbColor = CyanPrimary,
                                        activeTrackColor = CyanPrimary
                                    )
                                )
                            } else {
                                Spacer(Modifier.height(8.dp))
                                Row(
                                    modifier = Modifier.fillMaxWidth(),
                                    horizontalArrangement = Arrangement.SpaceBetween
                                ) {
                                    Text("ISO (Sensitivity)", color = TextSecondary, fontSize = 10.sp)
                                    Text("$manualIso", color = CyanPrimary, fontSize = 10.sp)
                                }
                                Slider(
                                    value = manualIso.toFloat(),
                                    onValueChange = { StreamingController.onManualExposureChanged?.invoke(it.toInt(), manualShutterSpeedNs) },
                                    valueRange = 100f..3200f,
                                    colors = SliderDefaults.colors(
                                        thumbColor = CyanPrimary,
                                        activeTrackColor = CyanPrimary
                                    )
                                )

                                Spacer(Modifier.height(4.dp))
                                Text("Shutter Speed", color = TextSecondary, fontSize = 10.sp)
                                Row(
                                    modifier = Modifier.fillMaxWidth(),
                                    horizontalArrangement = Arrangement.spacedBy(4.dp)
                                ) {
                                    listOf(
                                        16666666L to "1/60",
                                        8000000L to "1/125",
                                        4000000L to "1/250",
                                        2000000L to "1/500"
                                    ).forEach { (speed, label) ->
                                        Button(
                                            onClick = { StreamingController.onManualExposureChanged?.invoke(manualIso, speed) },
                                            colors = ButtonDefaults.buttonColors(
                                                containerColor = if (manualShutterSpeedNs == speed) CyanPrimary else Color.DarkGray
                                            ),
                                            modifier = Modifier.weight(1f),
                                            contentPadding = PaddingValues(0.dp)
                                        ) {
                                            Text(label, fontSize = 8.sp, color = if (manualShutterSpeedNs == speed) Color.Black else Color.White)
                                        }
                                    }
                                }
                            }
                        }

                        // White Balance Mode
                        Column {
                            Text("White Balance Preset", color = TextSecondary, fontSize = 11.sp)
                            Spacer(Modifier.height(4.dp))
                            Row(
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .horizontalScroll(rememberScrollState())
                                    .height(60.dp),
                                horizontalArrangement = Arrangement.spacedBy(4.dp)
                            ) {
                                listOf("auto", "daylight", "tungsten", "fluorescent", "cloudy").forEach { wb ->
                                    Box(
                                        modifier = Modifier
                                            .clip(RoundedCornerShape(8.dp))
                                            .background(if (whiteBalanceMode == wb) CyanPrimary else Color.DarkGray)
                                            .clickable { StreamingController.onWhiteBalanceChanged?.invoke(wb) }
                                            .padding(horizontal = 8.dp, vertical = 6.dp)
                                    ) {
                                        Text(
                                            wb.uppercase(),
                                            fontSize = 8.sp,
                                            color = if (whiteBalanceMode == wb) Color.Black else Color.White,
                                            fontWeight = FontWeight.Bold
                                        )
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Bottom controls (Start/Stop, camera swap, flash)
            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .align(Alignment.BottomCenter)
                    .navigationBarsPadding()
                    .padding(bottom = 24.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.spacedBy(16.dp)
            ) {
                // Flash and swap camera
                AnimatedVisibility(visible = isStreaming) {
                    Row(
                        modifier = Modifier
                            .fillMaxWidth()
                            .padding(horizontal = 24.dp),
                        horizontalArrangement = Arrangement.SpaceEvenly
                    ) {
                        ControlButton(
                            icon = Icons.Default.FlipCameraAndroid,
                            label = "Flip Camera",
                            onClick = { StreamingController.onCameraToggle?.invoke(!useFrontCamera) }
                        )
                        ControlButton(
                            icon = Icons.Default.FlashOn,
                            label = "Flash",
                            isActive = flashEnabled,
                            onClick = { StreamingController.onFlashChanged?.invoke(!flashEnabled) }
                        )
                    }
                }

                // Main Stream Action Button
                StreamButton(
                    isStreaming = isStreaming,
                    onClick = {
                        if (!allPermissionsGranted) {
                            permissionLauncher.launch(requiredPermissions.toTypedArray())
                            return@StreamButton
                        }

                        if (isStreaming) {
                            val stopIntent = Intent(context, CameraStreamingService::class.java).apply {
                                action = CameraStreamingService.ACTION_STOP
                            }
                            context.startService(stopIntent)
                        } else {
                            val codecStr = when (selectedCodec) {
                                "H.264" -> "h264"
                                "H.265" -> "h265"
                                "MJPEG" -> "mjpeg"
                                else -> "h264"
                            }
                            val intent = Intent(context, CameraStreamingService::class.java).apply {
                                putExtra(CameraStreamingService.EXTRA_USE_FRONT, useFrontCamera)
                                putExtra(CameraStreamingService.EXTRA_CODEC, codecStr)
                                putExtra(CameraStreamingService.EXTRA_RESOLUTION, selectedResolution)
                                putExtra(CameraStreamingService.EXTRA_FPS, selectedFps)
                                putExtra(CameraStreamingService.EXTRA_PORT, port)
                            }
                            ContextCompat.startForegroundService(context, intent)
                        }
                    }
                )

                Text(
                    text = if (isStreaming) "Streaming Active" else "CamDroid",
                    style = MaterialTheme.typography.labelSmall,
                    color = TextDim,
                    letterSpacing = 2.sp,
                    fontWeight = FontWeight.Medium
                )
            }
        }
    }

    // Codec Picker
    if (showCodecPicker) {
        AlertDialog(
            onDismissRequest = { showCodecPicker = false },
            title = { Text("Select Codec") },
            text = {
                Column {
                    listOf("H.264", "H.265", "MJPEG").forEach { codec ->
                        Row(
                            modifier = Modifier
                                .fillMaxWidth()
                                .clip(RoundedCornerShape(8.dp))
                                .background(if (selectedCodec == codec) CyanPrimary.copy(alpha = 0.15f) else Color.Transparent)
                                .clickable {
                                    selectedCodec = codec
                                    showCodecPicker = false
                                }
                                .padding(12.dp),
                            verticalAlignment = Alignment.CenterVertically
                        ) {
                            RadioButton(
                                selected = selectedCodec == codec,
                                onClick = {
                                    selectedCodec = codec
                                    showCodecPicker = false
                                },
                                colors = RadioButtonDefaults.colors(selectedColor = CyanPrimary)
                            )
                            Spacer(Modifier.width(8.dp))
                            Text(codec, style = MaterialTheme.typography.bodyLarge)
                        }
                    }
                }
            },
            confirmButton = {
                TextButton(onClick = { showCodecPicker = false }) { Text("Cancel") }
            },
            containerColor = SurfaceContainer
        )
    }

    // Resolution Picker
    if (showResPicker) {
        AlertDialog(
            onDismissRequest = { showResPicker = false },
            title = { Text("Select Resolution") },
            text = {
                Column {
                    listOf("1080p" to "1920×1080", "1440p" to "2560×1440", "4k" to "3840×2160").forEach { (res, desc) ->
                        Row(
                            modifier = Modifier
                                .fillMaxWidth()
                                .clip(RoundedCornerShape(8.dp))
                                .background(if (selectedResolution == res) CyanPrimary.copy(alpha = 0.15f) else Color.Transparent)
                                .clickable {
                                    selectedResolution = res
                                    showResPicker = false
                                }
                                .padding(12.dp),
                            verticalAlignment = Alignment.CenterVertically
                        ) {
                            RadioButton(
                                selected = selectedResolution == res,
                                onClick = {
                                    selectedResolution = res
                                    showResPicker = false
                                },
                                colors = RadioButtonDefaults.colors(selectedColor = CyanPrimary)
                            )
                            Spacer(Modifier.width(8.dp))
                            Column {
                                Text(res.uppercase(), style = MaterialTheme.typography.bodyLarge, fontWeight = FontWeight.Bold)
                                Text(desc, style = MaterialTheme.typography.labelSmall, color = TextDim)
                            }
                        }
                    }
                }
            },
            confirmButton = {
                TextButton(onClick = { showResPicker = false }) { Text("Cancel") }
            },
            containerColor = SurfaceContainer
        )
    }
}
