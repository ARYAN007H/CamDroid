package com.camdroid.ui.components

import androidx.compose.animation.*
import androidx.compose.animation.core.*
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.CircleShape
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
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.camdroid.ui.theme.*

/**
 * Status indicator pill showing connection state.
 */
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
            .background(SurfaceContainer.copy(alpha = 0.9f))
            .padding(horizontal = 14.dp, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(8.dp)
    ) {
        // Pulsing dot
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
            color = TextPrimary
        )

        ipAddress?.let {
            Text(
                text = "• $it:$port",
                style = MaterialTheme.typography.labelSmall,
                color = TextDim
            )
        }
    }
}

/**
 * Control button used in the camera controls bar.
 */
@Composable
fun ControlButton(
    icon: ImageVector,
    label: String,
    isActive: Boolean = false,
    onClick: () -> Unit,
    modifier: Modifier = Modifier
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
                .size(48.dp)
                .clip(CircleShape)
                .background(
                    if (isActive) CyanPrimary.copy(alpha = 0.2f) else SurfaceContainer
                )
                .border(
                    width = 1.5.dp,
                    color = if (isActive) CyanPrimary else Color.White.copy(alpha = 0.1f),
                    shape = CircleShape
                ),
            contentAlignment = Alignment.Center
        ) {
            Icon(
                imageVector = icon,
                contentDescription = label,
                tint = if (isActive) CyanPrimary else TextSecondary,
                modifier = Modifier.size(22.dp)
            )
        }
        Text(
            text = label,
            style = MaterialTheme.typography.labelSmall,
            color = if (isActive) CyanPrimary else TextDim
        )
    }
}

/**
 * Large start/stop streaming button.
 */
@Composable
fun StreamButton(
    isStreaming: Boolean,
    onClick: () -> Unit,
    modifier: Modifier = Modifier
) {
    val scale by animateFloatAsState(
        targetValue = if (isStreaming) 1f else 1.05f,
        animationSpec = spring(dampingRatio = Spring.DampingRatioMediumBouncy),
        label = "buttonScale"
    )

    Box(
        modifier = modifier
            .size(80.dp)
            .clip(CircleShape)
            .background(
                Brush.radialGradient(
                    colors = if (isStreaming) listOf(StatusRed, StatusRed.copy(alpha = 0.7f))
                    else listOf(CyanPrimary, CyanDark)
                )
            )
            .clickable(onClick = onClick),
        contentAlignment = Alignment.Center
    ) {
        Icon(
            imageVector = if (isStreaming) Icons.Default.Stop else Icons.Default.Videocam,
            contentDescription = if (isStreaming) "Stop" else "Start",
            tint = if (isStreaming) Color.White else Color.Black,
            modifier = Modifier.size(36.dp)
        )
    }
}

/**
 * Battery indicator with color-coded status.
 */
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
            .background(SurfaceContainer.copy(alpha = 0.9f))
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

/**
 * Settings chip for displaying and selecting encoder settings.
 */
@Composable
fun SettingsChip(
    label: String,
    value: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier
) {
    Row(
        modifier = modifier
            .clip(RoundedCornerShape(8.dp))
            .background(SurfaceContainer)
            .clickable(onClick = onClick)
            .padding(horizontal = 12.dp, vertical = 8.dp),
        horizontalArrangement = Arrangement.spacedBy(6.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        Text(
            text = label,
            style = MaterialTheme.typography.labelSmall,
            color = TextDim
        )
        Text(
            text = value,
            style = MaterialTheme.typography.labelLarge,
            fontWeight = FontWeight.Bold,
            color = CyanPrimary
        )
    }
}
