package com.camdroid.network

import android.content.Context
import android.net.nsd.NsdManager
import android.net.nsd.NsdServiceInfo
import android.util.Log

/**
 * mDNS/NSD helper for registering the CamDroid streaming service.
 *
 * Registers a `_camdroid._tcp.` service so the desktop client can
 * auto-discover the phone on the local network.
 */
class NsdHelper(private val context: Context) {
    companion object {
        private const val TAG = "NsdHelper"
        const val SERVICE_TYPE = "_camdroid._tcp."
        const val SERVICE_NAME = "CamDroid"
    }

    private val nsdManager = context.getSystemService(Context.NSD_SERVICE) as NsdManager
    private var registrationListener: NsdManager.RegistrationListener? = null
    var registeredServiceName: String? = null
        private set

    /**
     * Register the CamDroid streaming service with mDNS.
     *
     * @param port The TCP port the stream server is listening on
     * @param deviceName Human-readable device name
     * @param codec Current codec name
     * @param resolution Current resolution
     */
    fun registerService(port: Int, deviceName: String, codec: String, resolution: String) {
        val serviceInfo = NsdServiceInfo().apply {
            serviceName = "$SERVICE_NAME-$deviceName"
            serviceType = SERVICE_TYPE
            setPort(port)
            setAttribute("version", "1.0")
            setAttribute("codecs", "h264,h265,mjpeg")
            setAttribute("resolution", resolution)
            setAttribute("device", deviceName)
        }

        registrationListener = object : NsdManager.RegistrationListener {
            override fun onServiceRegistered(info: NsdServiceInfo) {
                registeredServiceName = info.serviceName
                Log.i(TAG, "Service registered: ${info.serviceName} on port $port")
            }

            override fun onRegistrationFailed(info: NsdServiceInfo, errorCode: Int) {
                Log.e(TAG, "Service registration failed: error $errorCode")
            }

            override fun onServiceUnregistered(info: NsdServiceInfo) {
                Log.i(TAG, "Service unregistered: ${info.serviceName}")
            }

            override fun onUnregistrationFailed(info: NsdServiceInfo, errorCode: Int) {
                Log.e(TAG, "Service unregistration failed: error $errorCode")
            }
        }

        nsdManager.registerService(serviceInfo, NsdManager.PROTOCOL_DNS_SD, registrationListener)
    }

    /** Unregister the service and clean up. */
    fun unregisterService() {
        registrationListener?.let {
            try {
                nsdManager.unregisterService(it)
            } catch (e: Exception) {
                Log.w(TAG, "Error unregistering service", e)
            }
        }
        registrationListener = null
        registeredServiceName = null
    }
}
