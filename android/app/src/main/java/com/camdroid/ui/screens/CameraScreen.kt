package com.camdroid.ui.screens

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.animation.*
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.content.ContextCompat
import com.camdroid.CameraStreamingService
import com.camdroid.ui.components.*
import com.camdroid.ui.theme.*
import com.camdroid.util.NetworkUtils

/**
 * Main camera streaming screen.
 */
@Composable
fun CameraScreen() {
    val context = LocalContext.current

    // State
    var isStreaming by remember { mutableStateOf(false) }
    var isConnected by remember { mutableStateOf(false) }
    var useFrontCamera by remember { mutableStateOf(false) }
    var flashEnabled by remember { mutableStateOf(false) }
    var selectedCodec by remember { mutableStateOf("H.264") }
    var selectedResolution by remember { mutableStateOf("1080p") }
    var selectedFps by remember { mutableIntStateOf(60) }
    var batteryLevel by remember { mutableIntStateOf(100) }
    var isCharging by remember { mutableStateOf(false) }
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

    // Settings dialog state
    var showCodecPicker by remember { mutableStateOf(false) }
    var showResPicker by remember { mutableStateOf(false) }

    Box(
        modifier = Modifier
            .fillMaxSize()
            .background(SurfaceDark)
    ) {
        // Camera preview area (placeholder — in production would use TextureView/GLSurfaceView)
        Box(
            modifier = Modifier
                .fillMaxSize()
                .background(
                    Brush.verticalGradient(
                        colors = listOf(Color(0xFF0D1117), Color(0xFF161B22), Color(0xFF0D1117))
                    )
                ),
            contentAlignment = Alignment.Center
        ) {
            if (!isStreaming) {
                Column(
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = Arrangement.spacedBy(8.dp)
                ) {
                    Icon(
                        Icons.Default.Videocam,
                        contentDescription = null,
                        tint = TextDim,
                        modifier = Modifier.size(64.dp)
                    )
                    Text(
                        "Tap the button below to start streaming",
                        style = MaterialTheme.typography.bodyMedium,
                        color = TextDim,
                        textAlign = TextAlign.Center
                    )
                }
            } else {
                // Live indicator
                Column(
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = Arrangement.spacedBy(4.dp)
                ) {
                    Text(
                        "Camera Preview Active",
                        style = MaterialTheme.typography.bodySmall,
                        color = TextDim
                    )
                    Text(
                        "(Preview will show here when connected via TextureView)",
                        style = MaterialTheme.typography.labelSmall,
                        color = TextDim.copy(alpha = 0.5f)
                    )
                }
            }
        }

        // Top bar overlay
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

            BatteryIndicator(
                level = batteryLevel,
                isCharging = isCharging
            )
        }

        // Settings chips
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
                    value = "${selectedFps}",
                    onClick = { selectedFps = if (selectedFps == 60) 30 else 60 }
                )
            }
        }

        // Bottom controls
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .align(Alignment.BottomCenter)
                .navigationBarsPadding()
                .padding(bottom = 24.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(16.dp)
        ) {
            // Camera controls row
            AnimatedVisibility(visible = isStreaming) {
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(horizontal = 24.dp),
                    horizontalArrangement = Arrangement.SpaceEvenly
                ) {
                    ControlButton(
                        icon = Icons.Default.FlipCameraAndroid,
                        label = "Flip",
                        onClick = { useFrontCamera = !useFrontCamera }
                    )
                    ControlButton(
                        icon = Icons.Default.FlashOn,
                        label = "Flash",
                        isActive = flashEnabled,
                        onClick = { flashEnabled = !flashEnabled }
                    )
                    ControlButton(
                        icon = Icons.Default.ZoomIn,
                        label = "Zoom",
                        onClick = { /* TODO: zoom slider */ }
                    )
                    ControlButton(
                        icon = Icons.Default.Tune,
                        label = "Settings",
                        onClick = { /* TODO: settings panel */ }
                    )
                }
            }

            // Start/stop button
            StreamButton(
                isStreaming = isStreaming,
                onClick = {
                    if (!allPermissionsGranted) {
                        permissionLauncher.launch(requiredPermissions.toTypedArray())
                        return@StreamButton
                    }

                    if (isStreaming) {
                        // Stop streaming
                        val stopIntent = Intent(context, CameraStreamingService::class.java).apply {
                            action = CameraStreamingService.ACTION_STOP
                        }
                        context.startService(stopIntent)
                        isStreaming = false
                    } else {
                        // Start streaming
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
                        isStreaming = true
                    }
                }
            )

            // Bottom label
            Text(
                text = if (isStreaming) "Streaming to OBS" else "CamDroid",
                style = MaterialTheme.typography.labelSmall,
                color = TextDim,
                letterSpacing = 2.sp,
                fontWeight = FontWeight.Medium
            )
        }
    }

    // Codec picker dialog
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
                                .padding(12.dp)
                                .then(
                                    Modifier.let {
                                        if (true) it else it // clickable added below
                                    }
                                ),
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
                TextButton(onClick = { showCodecPicker = false }) {
                    Text("Cancel")
                }
            },
            containerColor = SurfaceContainer
        )
    }

    // Resolution picker dialog
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
                TextButton(onClick = { showResPicker = false }) {
                    Text("Cancel")
                }
            },
            containerColor = SurfaceContainer
        )
    }
}
