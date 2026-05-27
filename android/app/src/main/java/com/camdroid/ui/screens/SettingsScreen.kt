package com.camdroid.ui.screens

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import com.camdroid.data.SettingsRepository
import com.camdroid.ui.components.SectionHeader
import kotlinx.coroutines.launch

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SettingsScreen(
    onBack: () -> Unit,
    settingsRepository: SettingsRepository
) {
    val scope = rememberCoroutineScope()

    // Collect current settings
    val codec by settingsRepository.codec.collectAsState(initial = "H.264")
    val resolution by settingsRepository.resolution.collectAsState(initial = "1080p")
    val fps by settingsRepository.fps.collectAsState(initial = 60)
    val port by settingsRepository.port.collectAsState(initial = 4747)
    val audioEnabled by settingsRepository.audioEnabled.collectAsState(initial = true)
    val keepScreenOn by settingsRepository.keepScreenOn.collectAsState(initial = true)
    val autoDiscovery by settingsRepository.autoDiscovery.collectAsState(initial = true)

    // Picker dialog states
    var showCodecPicker by remember { mutableStateOf(false) }
    var showResPicker by remember { mutableStateOf(false) }

    Scaffold(
        topBar = {
            TopAppBar(
                title = {
                    Text(
                        "Settings",
                        style = MaterialTheme.typography.titleLarge,
                        fontWeight = FontWeight.Bold
                    )
                },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(
                            Icons.AutoMirrored.Filled.ArrowBack,
                            contentDescription = "Back"
                        )
                    }
                },
                colors = TopAppBarDefaults.topAppBarColors(
                    containerColor = MaterialTheme.colorScheme.surface
                )
            )
        },
        containerColor = MaterialTheme.colorScheme.surface
    ) { padding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(padding)
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 20.dp),
            verticalArrangement = Arrangement.spacedBy(4.dp)
        ) {
            Spacer(Modifier.height(4.dp))

            // ── Video Settings ──
            SectionHeader(title = "VIDEO")

            // Codec
            SettingsRow(
                title = "Video Codec",
                subtitle = "Hardware-accelerated encoding format",
                value = codec,
                icon = Icons.Default.VideoSettings,
                onClick = { showCodecPicker = true }
            )

            // Resolution
            SettingsRow(
                title = "Resolution",
                subtitle = when (resolution) {
                    "1080p" -> "1920×1080 (Full HD)"
                    "1440p" -> "2560×1440 (QHD)"
                    "4k" -> "3840×2160 (Ultra HD)"
                    else -> resolution
                },
                value = resolution.uppercase(),
                icon = Icons.Default.AspectRatio,
                onClick = { showResPicker = true }
            )

            // FPS
            SettingsRow(
                title = "Frame Rate",
                subtitle = "Higher values are smoother but use more bandwidth",
                value = "$fps FPS",
                icon = Icons.Default.Speed,
                onClick = {
                    scope.launch {
                        settingsRepository.setFps(if (fps == 60) 30 else 60)
                    }
                }
            )

            Spacer(Modifier.height(8.dp))

            // ── Audio Settings ──
            SectionHeader(title = "AUDIO")

            SettingsToggleRow(
                title = "Audio Streaming",
                subtitle = "Stream microphone audio alongside video",
                icon = Icons.Default.Mic,
                checked = audioEnabled,
                onCheckedChange = { scope.launch { settingsRepository.setAudioEnabled(it) } }
            )

            Spacer(Modifier.height(8.dp))

            // ── Network Settings ──
            SectionHeader(title = "NETWORK")

            SettingsRow(
                title = "Server Port",
                subtitle = "TCP port the streaming server listens on",
                value = "$port",
                icon = Icons.Default.Router,
                onClick = { /* TODO: port picker dialog */ }
            )

            SettingsToggleRow(
                title = "Auto-Discovery (mDNS)",
                subtitle = "Allow desktop clients to find this device automatically",
                icon = Icons.Default.WifiFind,
                checked = autoDiscovery,
                onCheckedChange = { scope.launch { settingsRepository.setAutoDiscovery(it) } }
            )

            Spacer(Modifier.height(8.dp))

            // ── General ──
            SectionHeader(title = "GENERAL")

            SettingsToggleRow(
                title = "Keep Screen On",
                subtitle = "Prevent screen from sleeping while streaming",
                icon = Icons.Default.ScreenLockPortrait,
                checked = keepScreenOn,
                onCheckedChange = { scope.launch { settingsRepository.setKeepScreenOn(it) } }
            )

            Spacer(Modifier.height(32.dp))
        }
    }

    // ── Codec Picker Dialog ──
    if (showCodecPicker) {
        PickerDialog(
            title = "Select Codec",
            options = listOf(
                "H.264" to "Best compatibility — works everywhere",
                "H.265" to "Better quality at lower bitrate (HEVC)",
                "MJPEG" to "Lowest latency, highest bandwidth"
            ),
            selected = codec,
            onSelected = { selected ->
                scope.launch { settingsRepository.setCodec(selected) }
                showCodecPicker = false
            },
            onDismiss = { showCodecPicker = false }
        )
    }

    // ── Resolution Picker Dialog ──
    if (showResPicker) {
        PickerDialog(
            title = "Select Resolution",
            options = listOf(
                "1080p" to "1920×1080 — Full HD",
                "1440p" to "2560×1440 — Quad HD",
                "4k" to "3840×2160 — Ultra HD"
            ),
            selected = resolution,
            onSelected = { selected ->
                scope.launch { settingsRepository.setResolution(selected) }
                showResPicker = false
            },
            onDismiss = { showResPicker = false }
        )
    }
}

// ============================================================
// Reusable Settings Row Components
// ============================================================

@Composable
private fun SettingsRow(
    title: String,
    subtitle: String,
    value: String,
    icon: androidx.compose.ui.graphics.vector.ImageVector,
    onClick: () -> Unit
) {
    Card(
        onClick = onClick,
        shape = RoundedCornerShape(16.dp),
        colors = CardDefaults.cardColors(
            containerColor = MaterialTheme.colorScheme.surfaceContainerHigh
        ),
        modifier = Modifier.fillMaxWidth()
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(16.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(14.dp)
        ) {
            Icon(
                imageVector = icon,
                contentDescription = null,
                tint = MaterialTheme.colorScheme.primary,
                modifier = Modifier.size(24.dp)
            )
            Column(modifier = Modifier.weight(1f)) {
                Text(
                    text = title,
                    style = MaterialTheme.typography.bodyLarge,
                    fontWeight = FontWeight.Medium,
                    color = MaterialTheme.colorScheme.onSurface
                )
                Text(
                    text = subtitle,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant
                )
            }
            Text(
                text = value,
                style = MaterialTheme.typography.labelLarge,
                fontWeight = FontWeight.Bold,
                color = MaterialTheme.colorScheme.primary
            )
        }
    }
}

@Composable
private fun SettingsToggleRow(
    title: String,
    subtitle: String,
    icon: androidx.compose.ui.graphics.vector.ImageVector,
    checked: Boolean,
    onCheckedChange: (Boolean) -> Unit
) {
    Card(
        shape = RoundedCornerShape(16.dp),
        colors = CardDefaults.cardColors(
            containerColor = MaterialTheme.colorScheme.surfaceContainerHigh
        ),
        modifier = Modifier.fillMaxWidth()
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 16.dp, vertical = 10.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(14.dp)
        ) {
            Icon(
                imageVector = icon,
                contentDescription = null,
                tint = MaterialTheme.colorScheme.primary,
                modifier = Modifier.size(24.dp)
            )
            Column(modifier = Modifier.weight(1f)) {
                Text(
                    text = title,
                    style = MaterialTheme.typography.bodyLarge,
                    fontWeight = FontWeight.Medium,
                    color = MaterialTheme.colorScheme.onSurface
                )
                Text(
                    text = subtitle,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant
                )
            }
            Switch(
                checked = checked,
                onCheckedChange = onCheckedChange
            )
        }
    }
}

@Composable
private fun PickerDialog(
    title: String,
    options: List<Pair<String, String>>,
    selected: String,
    onSelected: (String) -> Unit,
    onDismiss: () -> Unit
) {
    AlertDialog(
        onDismissRequest = onDismiss,
        title = {
            Text(
                title,
                style = MaterialTheme.typography.titleLarge,
                fontWeight = FontWeight.Bold
            )
        },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                options.forEach { (value, description) ->
                    Card(
                        onClick = { onSelected(value) },
                        shape = RoundedCornerShape(12.dp),
                        colors = CardDefaults.cardColors(
                            containerColor = if (selected == value)
                                MaterialTheme.colorScheme.primaryContainer.copy(alpha = 0.3f)
                            else
                                MaterialTheme.colorScheme.surfaceContainerHighest
                        ),
                        modifier = Modifier.fillMaxWidth()
                    ) {
                        Row(
                            modifier = Modifier
                                .fillMaxWidth()
                                .padding(14.dp),
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(12.dp)
                        ) {
                            RadioButton(
                                selected = selected == value,
                                onClick = { onSelected(value) }
                            )
                            Column {
                                Text(
                                    value,
                                    style = MaterialTheme.typography.bodyLarge,
                                    fontWeight = FontWeight.SemiBold,
                                    color = MaterialTheme.colorScheme.onSurface
                                )
                                Text(
                                    description,
                                    style = MaterialTheme.typography.bodySmall,
                                    color = MaterialTheme.colorScheme.onSurfaceVariant
                                )
                            }
                        }
                    }
                }
            }
        },
        confirmButton = {
            TextButton(onClick = onDismiss) {
                Text("Cancel")
            }
        },
        containerColor = MaterialTheme.colorScheme.surfaceContainerHigh,
        shape = RoundedCornerShape(24.dp)
    )
}
