package com.camdroid.ui.navigation

import androidx.compose.animation.*
import androidx.compose.animation.core.tween
import androidx.compose.runtime.*
import androidx.compose.ui.platform.LocalContext
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.rememberNavController
import com.camdroid.data.SettingsRepository
import com.camdroid.ui.screens.*

/**
 * Navigation route constants.
 */
object Routes {
    const val ONBOARDING = "onboarding"
    const val HOME = "home"
    const val VIEWFINDER = "viewfinder"
    const val SETTINGS = "settings"
    const val ABOUT = "about"
}

/**
 * Root navigation graph for CamDroid.
 * Start destination is either onboarding (first launch) or home.
 */
@Composable
fun CamDroidNavGraph() {
    val context = LocalContext.current
    val settingsRepository = remember { SettingsRepository(context) }
    val isFirstLaunch by settingsRepository.isFirstLaunch.collectAsState(initial = null)

    // Don't render anything until we know if it's the first launch
    if (isFirstLaunch == null) return

    val navController = rememberNavController()
    val startDestination = if (isFirstLaunch == true) Routes.ONBOARDING else Routes.HOME

    NavHost(
        navController = navController,
        startDestination = startDestination,
        enterTransition = {
            fadeIn(animationSpec = tween(300)) +
                slideInHorizontally(animationSpec = tween(300)) { it / 4 }
        },
        exitTransition = {
            fadeOut(animationSpec = tween(200)) +
                slideOutHorizontally(animationSpec = tween(200)) { -it / 4 }
        },
        popEnterTransition = {
            fadeIn(animationSpec = tween(300)) +
                slideInHorizontally(animationSpec = tween(300)) { -it / 4 }
        },
        popExitTransition = {
            fadeOut(animationSpec = tween(200)) +
                slideOutHorizontally(animationSpec = tween(200)) { it / 4 }
        }
    ) {
        composable(Routes.ONBOARDING) {
            OnboardingScreen(
                onComplete = {
                    navController.navigate(Routes.HOME) {
                        popUpTo(Routes.ONBOARDING) { inclusive = true }
                    }
                },
                settingsRepository = settingsRepository
            )
        }

        composable(Routes.HOME) {
            HomeScreen(
                onStartStreaming = {
                    navController.navigate(Routes.VIEWFINDER)
                },
                onNavigateToSettings = {
                    navController.navigate(Routes.SETTINGS)
                },
                onNavigateToAbout = {
                    navController.navigate(Routes.ABOUT)
                },
                settingsRepository = settingsRepository
            )
        }

        composable(Routes.VIEWFINDER) {
            ViewfinderScreen(
                onBack = {
                    navController.popBackStack()
                },
                settingsRepository = settingsRepository
            )
        }

        composable(Routes.SETTINGS) {
            SettingsScreen(
                onBack = {
                    navController.popBackStack()
                },
                settingsRepository = settingsRepository
            )
        }

        composable(Routes.ABOUT) {
            AboutScreen(
                onBack = {
                    navController.popBackStack()
                }
            )
        }
    }
}
