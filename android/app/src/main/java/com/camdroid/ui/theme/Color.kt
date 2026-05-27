package com.camdroid.ui.theme

import androidx.compose.ui.graphics.Color

// ============================================================
// CamDroid Fallback Palette (pre-Android 12 / Material 3 static)
// Used when dynamic colors are not available.
// ============================================================

// Primary — electric cyan (signature brand color)
val md_theme_dark_primary = Color(0xFF00E5FF)
val md_theme_dark_onPrimary = Color(0xFF003640)
val md_theme_dark_primaryContainer = Color(0xFF004D5B)
val md_theme_dark_onPrimaryContainer = Color(0xFF6FF7FF)

// Secondary — muted teal
val md_theme_dark_secondary = Color(0xFF80CBC4)
val md_theme_dark_onSecondary = Color(0xFF003731)
val md_theme_dark_secondaryContainer = Color(0xFF1A4E48)
val md_theme_dark_onSecondaryContainer = Color(0xFFA7F2EA)

// Tertiary — soft lavender
val md_theme_dark_tertiary = Color(0xFFBBB2EA)
val md_theme_dark_onTertiary = Color(0xFF2C2258)
val md_theme_dark_tertiaryContainer = Color(0xFF423970)
val md_theme_dark_onTertiaryContainer = Color(0xFFDDD4FF)

// Error
val md_theme_dark_error = Color(0xFFFFB4AB)
val md_theme_dark_onError = Color(0xFF690005)
val md_theme_dark_errorContainer = Color(0xFF93000A)
val md_theme_dark_onErrorContainer = Color(0xFFFFDAD6)

// Background & Surface
val md_theme_dark_background = Color(0xFF0E1113)
val md_theme_dark_onBackground = Color(0xFFE1E3E4)
val md_theme_dark_surface = Color(0xFF0E1113)
val md_theme_dark_onSurface = Color(0xFFE1E3E4)
val md_theme_dark_surfaceVariant = Color(0xFF1E2528)
val md_theme_dark_onSurfaceVariant = Color(0xFFBFC8CC)

// Outline
val md_theme_dark_outline = Color(0xFF899296)
val md_theme_dark_outlineVariant = Color(0xFF3F484C)

// Inverse
val md_theme_dark_inverseSurface = Color(0xFFE1E3E4)
val md_theme_dark_inverseOnSurface = Color(0xFF2E3132)
val md_theme_dark_inversePrimary = Color(0xFF006878)

// Surface containers (M3 tonal elevation)
val md_theme_dark_surfaceContainerLowest = Color(0xFF080B0D)
val md_theme_dark_surfaceContainerLow = Color(0xFF121719)
val md_theme_dark_surfaceContainer = Color(0xFF171C1F)
val md_theme_dark_surfaceContainerHigh = Color(0xFF1E2528)
val md_theme_dark_surfaceContainerHighest = Color(0xFF262D30)

// ============================================================
// Semantic / Status Colors (used across all themes)
// ============================================================
val StatusGreen = Color(0xFF00E676)
val StatusYellow = Color(0xFFFFD600)
val StatusRed = Color(0xFFFF1744)
val BatteryWarning = Color(0xFFFF6D00)
val StreamingRed = Color(0xFFEF5350)

// ============================================================
// Legacy aliases — for gradual migration in components
// ============================================================
val CyanPrimary = md_theme_dark_primary
val CyanDark = md_theme_dark_primaryContainer
val CyanLight = Color(0xFF18FFFF)
val SurfaceDark = md_theme_dark_background
val SurfaceContainer = md_theme_dark_surfaceContainer
val SurfaceOverlay = Color(0x99000000)
val TextPrimary = md_theme_dark_onBackground
val TextSecondary = md_theme_dark_onSurfaceVariant
val TextDim = Color(0x80FFFFFF)
