package com.camdroid.ui.components

import androidx.compose.animation.*
import androidx.compose.animation.core.*
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.*
import androidx.compose.material.icons.outlined.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.scale
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.camdroid.ui.theme.*

// ============================================================
// Status Badge — connection state indicator
// ============================================================
@Composable
fun StatusBadge(
    isConnected: Boolean,
    ipAddress: String?,
    port: Int,
    modifier: Modifier = Modifier
) {
    val dotColor by animateColorAsState(
        if (isConnected) StatusGreen else StatusYellow,
        label = "statusColor"
    )

    Row(
        modifier = modifier
            .clip(RoundedCornerShape(20.dp))
            .background(MaterialTheme.colorScheme.surfaceContainerHigh.copy(alpha = 0.9f))
            .padding(horizontal = 14.dp, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(8.dp)
    ) {
        val infiniteTransition = rememberInfiniteTransition(label = "pulse")
        val pulseAlpha by infiniteTransition.animateFloat(
            initialValue = 0.5f,
            targetValue = 1f,
            animationSpec = infiniteRepeatable(
                animation = tween(1000),
                repeatMode = RepeatMode.Reverse
            ),
            label = "pulseAlpha"
        )

        Box(
            Modifier
                .size(10.dp)
                .clip(CircleShape)
                .background(dotColor.copy(alpha = if (isConnected) 1f else pulseAlpha))
        )

        Text(
            text = if (isConnected) "Connected" else "Waiting",
            style = MaterialTheme.typography.labelLarge,
            color = MaterialTheme.colorScheme.onSurface
        )

        ipAddress?.let {
            Text(
                text = "• $it:$port",
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.7f)
            )
        }
    }
}

// ============================================================
// Battery Indicator
// ============================================================
@Composable
fun BatteryIndicator(
    level: Int,
    isCharging: Boolean,
    modifier: Modifier = Modifier
) {
    val color = when {
        level <= 20 -> StatusRed
        level <= 50 -> BatteryWarning
        else -> StatusGreen
    }

    Row(
        modifier = modifier
            .clip(RoundedCornerShape(8.dp))
            .background(MaterialTheme.colorScheme.surfaceContainerHigh.copy(alpha = 0.9f))
            .padding(horizontal = 10.dp, vertical = 6.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(4.dp)
    ) {
        Icon(
            imageVector = if (isCharging) Icons.Default.BatteryChargingFull else Icons.Default.Battery5Bar,
            contentDescription = "Battery",
            tint = color,
            modifier = Modifier.size(16.dp)
        )
        Text(
            text = "$level%",
            style = MaterialTheme.typography.labelSmall,
            fontWeight = FontWeight.Bold,
            color = color
        )
    }
}

// ============================================================
// Settings Chip — compact label + value chip
// ============================================================
@Composable
fun SettingsChip(
    label: String,
    value: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier
) {
    Row(
        modifier = modifier
            .clip(RoundedCornerShape(12.dp))
            .background(MaterialTheme.colorScheme.surfaceContainerHigh)
            .clickable(onClick = onClick)
            .padding(horizontal = 14.dp, vertical = 10.dp),
        horizontalArrangement = Arrangement.spacedBy(6.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        Text(
            text = label,
            style = MaterialTheme.typography.labelSmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant
        )
        Text(
            text = value,
            style = MaterialTheme.typography.labelLarge,
            fontWeight = FontWeight.Bold,
            color = MaterialTheme.colorScheme.primary
        )
    }
}

// ============================================================
// Control Button — circular icon action
// ============================================================
@Composable
fun ControlButton(
    icon: ImageVector,
    label: String,
    isActive: Boolean = false,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    size: Dp = 48.dp
) {
    Column(
        modifier = modifier
            .clip(RoundedCornerShape(12.dp))
            .clickable(onClick = onClick)
            .padding(8.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(4.dp)
    ) {
        Box(
            modifier = Modifier
                .size(size)
                .clip(CircleShape)
                .background(
                    if (isActive)
                        MaterialTheme.colorScheme.primary.copy(alpha = 0.15f)
                    else
                        MaterialTheme.colorScheme.surfaceContainerHigh
                )
                .border(
                    width = 1.5.dp,
                    color = if (isActive) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.outlineVariant,
                    shape = CircleShape
                ),
            contentAlignment = Alignment.Center
        ) {
            Icon(
                imageVector = icon,
                contentDescription = label,
                tint = if (isActive) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.size(22.dp)
            )
        }
        Text(
            text = label,
            style = MaterialTheme.typography.labelSmall,
            color = if (isActive) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.7f)
        )
    }
}

// ============================================================
// Shutter Button — camera-style start/stop button
// ============================================================
@Composable
fun ShutterButton(
    isStreaming: Boolean,
    onClick: () -> Unit,
    modifier: Modifier = Modifier
) {
    val ringScale by animateFloatAsState(
        targetValue = if (isStreaming) 1f else 1.06f,
        animationSpec = spring(dampingRatio = Spring.DampingRatioMediumBouncy),
        label = "ringScale"
    )

    val innerScale by animateFloatAsState(
        targetValue = if (isStreaming) 0.45f else 1f,
        animationSpec = spring(dampingRatio = Spring.DampingRatioMediumBouncy),
        label = "innerScale"
    )

    val innerShape by animateFloatAsState(
        targetValue = if (isStreaming) 6f else 28f,
        animationSpec = tween(250),
        label = "innerCorner"
    )

    // Pulsing ring when streaming
    val infiniteTransition = rememberInfiniteTransition(label = "recording")
    val pulseScale by infiniteTransition.animateFloat(
        initialValue = 1f,
        targetValue = 1.08f,
        animationSpec = infiniteRepeatable(
            animation = tween(800, easing = EaseInOutCubic),
            repeatMode = RepeatMode.Reverse
        ),
        label = "pulseScale"
    )

    Box(
        modifier = modifier
            .size(76.dp)
            .scale(if (isStreaming) pulseScale else ringScale)
            .clip(CircleShape)
            .border(
                width = 4.dp,
                color = if (isStreaming) StreamingRed else MaterialTheme.colorScheme.onSurface,
                shape = CircleShape
            )
            .clickable(
                interactionSource = remember { MutableInteractionSource() },
                indication = null,
                onClick = onClick
            ),
        contentAlignment = Alignment.Center
    ) {
        Box(
            modifier = Modifier
                .fillMaxSize(innerScale)
                .clip(RoundedCornerShape(innerShape.dp))
                .background(
                    if (isStreaming) StreamingRed
                    else MaterialTheme.colorScheme.onSurface
                )
        )
    }
}

// ============================================================
// Zoom Wheel — horizontal scrollable zoom level selector
// ============================================================
@Composable
fun ZoomWheel(
    currentZoom: Float,
    onZoomSelected: (Float) -> Unit,
    modifier: Modifier = Modifier
) {
    val levels = listOf(0.5f, 1f, 2f, 5f, 10f)

    Row(
        modifier = modifier
            .clip(RoundedCornerShape(20.dp))
            .background(MaterialTheme.colorScheme.surfaceContainerHighest.copy(alpha = 0.85f))
            .padding(horizontal = 6.dp, vertical = 4.dp),
        horizontalArrangement = Arrangement.spacedBy(2.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        levels.forEach { level ->
            val isSelected = kotlin.math.abs(currentZoom - level) < 0.1f ||
                (level == levels.last() && currentZoom >= level) ||
                (level != levels.last() && levels.indexOf(level) < levels.lastIndex &&
                    currentZoom >= level && currentZoom < levels[levels.indexOf(level) + 1])

            val isExact = kotlin.math.abs(currentZoom - level) < 0.1f

            Box(
                modifier = Modifier
                    .size(if (isExact) 38.dp else 32.dp)
                    .clip(CircleShape)
                    .background(
                        if (isExact)
                            MaterialTheme.colorScheme.primary.copy(alpha = 0.2f)
                        else
                            Color.Transparent
                    )
                    .clickable { onZoomSelected(level) },
                contentAlignment = Alignment.Center
            ) {
                Text(
                    text = if (level < 1f) ".5" else "${level.toInt()}x",
                    style = MaterialTheme.typography.labelSmall,
                    fontSize = if (isExact) 12.sp else 10.sp,
                    fontWeight = if (isExact) FontWeight.Bold else FontWeight.Normal,
                    color = if (isExact) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurface.copy(alpha = 0.7f)
                )
            }
        }
    }
}

// ============================================================
// Connection Card — for the home screen
// ============================================================
@Composable
fun ConnectionCard(
    title: String,
    subtitle: String,
    icon: ImageVector,
    isActive: Boolean = false,
    onClick: () -> Unit,
    modifier: Modifier = Modifier
) {
    Card(
        onClick = onClick,
        modifier = modifier.fillMaxWidth(),
        shape = RoundedCornerShape(20.dp),
        colors = CardDefaults.cardColors(
            containerColor = if (isActive)
                MaterialTheme.colorScheme.primaryContainer.copy(alpha = 0.3f)
            else
                MaterialTheme.colorScheme.surfaceContainerHigh
        ),
        border = if (isActive) CardDefaults.outlinedCardBorder() else null
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(20.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(16.dp)
        ) {
            Box(
                modifier = Modifier
                    .size(48.dp)
                    .clip(CircleShape)
                    .background(MaterialTheme.colorScheme.primary.copy(alpha = 0.1f)),
                contentAlignment = Alignment.Center
            ) {
                Icon(
                    imageVector = icon,
                    contentDescription = null,
                    tint = MaterialTheme.colorScheme.primary,
                    modifier = Modifier.size(24.dp)
                )
            }
            Column(modifier = Modifier.weight(1f)) {
                Text(
                    text = title,
                    style = MaterialTheme.typography.titleMedium,
                    fontWeight = FontWeight.SemiBold,
                    color = MaterialTheme.colorScheme.onSurface
                )
                Text(
                    text = subtitle,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant
                )
            }
            Icon(
                imageVector = Icons.Default.ChevronRight,
                contentDescription = null,
                tint = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.5f)
            )
        }
    }
}

// ============================================================
// Stat Chip — compact real-time stat display
// ============================================================
@Composable
fun StatChip(
    label: String,
    value: String,
    modifier: Modifier = Modifier
) {
    Column(
        modifier = modifier
            .clip(RoundedCornerShape(12.dp))
            .background(MaterialTheme.colorScheme.surfaceContainerHigh.copy(alpha = 0.85f))
            .padding(horizontal = 12.dp, vertical = 8.dp),
        horizontalAlignment = Alignment.CenterHorizontally
    ) {
        Text(
            text = value,
            style = MaterialTheme.typography.labelLarge,
            fontWeight = FontWeight.Bold,
            color = MaterialTheme.colorScheme.primary
        )
        Text(
            text = label,
            style = MaterialTheme.typography.labelSmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.6f)
        )
    }
}

// ============================================================
// Section Header — used in settings / home
// ============================================================
@Composable
fun SectionHeader(
    title: String,
    modifier: Modifier = Modifier
) {
    Text(
        text = title,
        style = MaterialTheme.typography.titleSmall,
        fontWeight = FontWeight.SemiBold,
        color = MaterialTheme.colorScheme.primary,
        letterSpacing = 1.sp,
        modifier = modifier.padding(horizontal = 4.dp, vertical = 8.dp)
    )
}
