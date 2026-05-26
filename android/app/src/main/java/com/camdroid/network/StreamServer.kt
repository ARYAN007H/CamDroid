package com.camdroid.network

import android.util.Log
import com.google.gson.Gson
import com.google.gson.JsonObject
import kotlinx.coroutines.*
import java.io.BufferedOutputStream
import java.io.InputStream
import java.io.OutputStream
import java.net.InetSocketAddress
import java.net.ServerSocket
import java.net.Socket

/**
 * TCP stream server that accepts a client connection and streams
 * video/audio data using the CamDroid binary protocol.
 */
class StreamServer(
    private val port: Int = 4747,
    private val onControlCommand: (JsonObject) -> Unit,
    private val onClientConnected: () -> Unit,
    private val onClientDisconnected: () -> Unit
) {
    companion object {
        private const val TAG = "StreamServer"
        private const val HEARTBEAT_INTERVAL_MS = 2000L
    }

    private var serverSocket: ServerSocket? = null
    private var clientSocket: Socket? = null
    private var outputStream: OutputStream? = null
    private var isRunning = false
    private var serverJob: Job? = null
    private var heartbeatJob: Job? = null
    private var readerJob: Job? = null
    private val gson = Gson()
    private val writeLock = Any()

    val isClientConnected: Boolean get() = clientSocket?.isConnected == true && clientSocket?.isClosed == false

    /**
     * Start the TCP server and wait for a client connection.
     */
    fun start(scope: CoroutineScope) {
        isRunning = true
        serverJob = scope.launch(Dispatchers.IO) {
            try {
                serverSocket = ServerSocket().apply {
                    reuseAddress = true
                    bind(InetSocketAddress(port))
                }
                Log.i(TAG, "Server listening on port $port")

                while (isRunning) {
                    try {
                        val socket = serverSocket?.accept() ?: break
                        handleClient(socket, scope)
                    } catch (e: Exception) {
                        if (isRunning) {
                            Log.e(TAG, "Accept error", e)
                            delay(1000)
                        }
                    }
                }
            } catch (e: Exception) {
                Log.e(TAG, "Server error", e)
            }
        }
    }

    private suspend fun handleClient(socket: Socket, scope: CoroutineScope) {
        // Close any existing client
        closeClient()

        clientSocket = socket
        socket.tcpNoDelay = true
        outputStream = BufferedOutputStream(socket.getOutputStream())

        Log.i(TAG, "Client connected: ${socket.inetAddress.hostAddress}")
        onClientConnected()

        // Start reading control commands from the client
        readerJob = scope.launch(Dispatchers.IO) {
            readControlLoop(socket.getInputStream())
        }

        // Start heartbeat
        heartbeatJob = scope.launch(Dispatchers.IO) {
            while (isActive && isClientConnected) {
                try {
                    sendHeartbeat()
                } catch (e: Exception) {
                    Log.d(TAG, "Heartbeat failed, client may have disconnected")
                    break
                }
                delay(HEARTBEAT_INTERVAL_MS)
            }
        }
    }

    private fun readControlLoop(input: InputStream) {
        try {
            while (isRunning && isClientConnected) {
                val packet = ProtocolHandler.readPacket(input) ?: break

                when (packet.type) {
                    ProtocolHandler.CONTROL_CMD -> {
                        val json = String(packet.payload, Charsets.UTF_8)
                        try {
                            val cmd = gson.fromJson(json, JsonObject::class.java)
                            Log.d(TAG, "Control command: $json")
                            onControlCommand(cmd)
                        } catch (e: Exception) {
                            Log.e(TAG, "Failed to parse control command: $json", e)
                        }
                    }
                    ProtocolHandler.METADATA -> {
                        val json = String(packet.payload, Charsets.UTF_8)
                        Log.i(TAG, "Client metadata: $json")
                    }
                    ProtocolHandler.HEARTBEAT -> {
                        Log.d(TAG, "Heartbeat from client")
                    }
                    else -> {
                        Log.w(TAG, "Unexpected packet type from client: ${packet.type}")
                    }
                }
            }
        } catch (e: Exception) {
            if (isRunning) {
                Log.d(TAG, "Client read loop ended: ${e.message}")
            }
        }

        Log.i(TAG, "Client disconnected")
        onClientDisconnected()
    }

    /** Send a video frame to the connected client. */
    fun sendVideoFrame(data: ByteArray) {
        sendPacket(ProtocolHandler.VIDEO_FRAME, data)
    }

    /** Send an audio frame to the connected client. */
    fun sendAudioFrame(data: ByteArray) {
        sendPacket(ProtocolHandler.AUDIO_FRAME, data)
    }

    /** Send video codec configuration (SPS/PPS). */
    fun sendVideoConfig(data: ByteArray) {
        sendPacket(ProtocolHandler.VIDEO_CONFIG, data)
    }

    /** Send audio codec configuration. */
    fun sendAudioConfig(data: ByteArray) {
        sendPacket(ProtocolHandler.AUDIO_CONFIG, data)
    }

    /** Send metadata as JSON. */
    fun sendMetadata(json: String) {
        sendPacket(ProtocolHandler.METADATA, json.toByteArray(Charsets.UTF_8))
    }

    /** Send a heartbeat. */
    fun sendHeartbeat() {
        sendPacket(ProtocolHandler.HEARTBEAT, ByteArray(0))
    }

    private fun sendPacket(type: Byte, payload: ByteArray) {
        val out = outputStream ?: return
        synchronized(writeLock) {
            try {
                ProtocolHandler.writePacket(out, type, payload)
            } catch (e: Exception) {
                Log.d(TAG, "Send failed (client may have disconnected): ${e.message}")
            }
        }
    }

    private fun closeClient() {
        readerJob?.cancel()
        heartbeatJob?.cancel()
        try { clientSocket?.close() } catch (e: Exception) { }
        clientSocket = null
        outputStream = null
    }

    /** Stop the server and close all connections. */
    fun stop() {
        isRunning = false
        closeClient()
        serverJob?.cancel()
        try { serverSocket?.close() } catch (e: Exception) { }
        serverSocket = null
        Log.i(TAG, "Server stopped")
    }
}
