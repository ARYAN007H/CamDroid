package com.camdroid.data

import android.content.Context
import androidx.datastore.core.DataStore
import androidx.datastore.preferences.core.*
import androidx.datastore.preferences.preferencesDataStore
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.map

private val Context.dataStore: DataStore<Preferences> by preferencesDataStore(name = "camdroid_settings")

/**
 * Persisted user preferences backed by Jetpack DataStore.
 */
class SettingsRepository(private val context: Context) {

    // ── Keys ──
    private object Keys {
        val IS_FIRST_LAUNCH = booleanPreferencesKey("is_first_launch")
        val CODEC = stringPreferencesKey("codec")
        val RESOLUTION = stringPreferencesKey("resolution")
        val FPS = intPreferencesKey("fps")
        val PORT = intPreferencesKey("port")
        val AUDIO_ENABLED = booleanPreferencesKey("audio_enabled")
        val KEEP_SCREEN_ON = booleanPreferencesKey("keep_screen_on")
        val USE_FRONT_CAMERA = booleanPreferencesKey("use_front_camera")
        val AUTO_DISCOVERY = booleanPreferencesKey("auto_discovery")
    }

    // ── First Launch ──
    val isFirstLaunch: Flow<Boolean> = context.dataStore.data.map { prefs ->
        prefs[Keys.IS_FIRST_LAUNCH] ?: true
    }

    suspend fun setFirstLaunchComplete() {
        context.dataStore.edit { prefs ->
            prefs[Keys.IS_FIRST_LAUNCH] = false
        }
    }

    // ── Codec ──
    val codec: Flow<String> = context.dataStore.data.map { prefs ->
        prefs[Keys.CODEC] ?: "H.264"
    }

    suspend fun setCodec(value: String) {
        context.dataStore.edit { it[Keys.CODEC] = value }
    }

    // ── Resolution ──
    val resolution: Flow<String> = context.dataStore.data.map { prefs ->
        prefs[Keys.RESOLUTION] ?: "1080p"
    }

    suspend fun setResolution(value: String) {
        context.dataStore.edit { it[Keys.RESOLUTION] = value }
    }

    // ── FPS ──
    val fps: Flow<Int> = context.dataStore.data.map { prefs ->
        prefs[Keys.FPS] ?: 60
    }

    suspend fun setFps(value: Int) {
        context.dataStore.edit { it[Keys.FPS] = value }
    }

    // ── Port ──
    val port: Flow<Int> = context.dataStore.data.map { prefs ->
        prefs[Keys.PORT] ?: 4747
    }

    suspend fun setPort(value: Int) {
        context.dataStore.edit { it[Keys.PORT] = value }
    }

    // ── Audio Enabled ──
    val audioEnabled: Flow<Boolean> = context.dataStore.data.map { prefs ->
        prefs[Keys.AUDIO_ENABLED] ?: true
    }

    suspend fun setAudioEnabled(value: Boolean) {
        context.dataStore.edit { it[Keys.AUDIO_ENABLED] = value }
    }

    // ── Keep Screen On ──
    val keepScreenOn: Flow<Boolean> = context.dataStore.data.map { prefs ->
        prefs[Keys.KEEP_SCREEN_ON] ?: true
    }

    suspend fun setKeepScreenOn(value: Boolean) {
        context.dataStore.edit { it[Keys.KEEP_SCREEN_ON] = value }
    }

    // ── Use Front Camera ──
    val useFrontCamera: Flow<Boolean> = context.dataStore.data.map { prefs ->
        prefs[Keys.USE_FRONT_CAMERA] ?: false
    }

    suspend fun setUseFrontCamera(value: Boolean) {
        context.dataStore.edit { it[Keys.USE_FRONT_CAMERA] = value }
    }

    // ── Auto Discovery ──
    val autoDiscovery: Flow<Boolean> = context.dataStore.data.map { prefs ->
        prefs[Keys.AUTO_DISCOVERY] ?: true
    }

    suspend fun setAutoDiscovery(value: Boolean) {
        context.dataStore.edit { it[Keys.AUTO_DISCOVERY] = value }
    }
}
