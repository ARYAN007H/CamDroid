package com.camdroid.util

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.os.BatteryManager
import android.util.Log

/**
 * Monitors battery level and triggers callbacks for battery saver mode.
 */
class BatteryMonitor(private val context: Context) {
    companion object {
        private const val TAG = "BatteryMonitor"
        const val LOW_BATTERY_THRESHOLD = 20
    }

    var batteryLevel: Int = 100
        private set
    var isCharging: Boolean = false
        private set
    var onLowBattery: (() -> Unit)? = null

    private var receiver: BroadcastReceiver? = null
    private var lowBatteryTriggered = false

    fun startMonitoring() {
        receiver = object : BroadcastReceiver() {
            override fun onReceive(context: Context, intent: Intent) {
                val level = intent.getIntExtra(BatteryManager.EXTRA_LEVEL, -1)
                val scale = intent.getIntExtra(BatteryManager.EXTRA_SCALE, -1)
                val status = intent.getIntExtra(BatteryManager.EXTRA_STATUS, -1)

                batteryLevel = if (level >= 0 && scale > 0) (level * 100 / scale) else 100
                isCharging = status == BatteryManager.BATTERY_STATUS_CHARGING ||
                        status == BatteryManager.BATTERY_STATUS_FULL

                if (batteryLevel <= LOW_BATTERY_THRESHOLD && !isCharging && !lowBatteryTriggered) {
                    lowBatteryTriggered = true
                    Log.w(TAG, "Low battery: $batteryLevel% — triggering power saver")
                    onLowBattery?.invoke()
                }

                if (batteryLevel > LOW_BATTERY_THRESHOLD) {
                    lowBatteryTriggered = false
                }
            }
        }
        context.registerReceiver(receiver, IntentFilter(Intent.ACTION_BATTERY_CHANGED))
    }

    fun stopMonitoring() {
        receiver?.let {
            try { context.unregisterReceiver(it) } catch (e: Exception) { }
        }
        receiver = null
    }
}
