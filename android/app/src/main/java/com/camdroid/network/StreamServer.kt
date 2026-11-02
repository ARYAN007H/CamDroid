package com.camdroid.network

import android.util.Log
import com.google.gson.Gson
import com.google.gson.JsonObject
import kotlinx.coroutines.*
import kotlinx.coroutines.channels.Channel
import java.io.BufferedOutputStream
import java.io.InputStream
import java.io.OutputStream
import java.net.InetSocketAddress
import java.net.ServerSocket
import java.net.Socket
import java.util.concurrent.atomic.AtomicLong

/**
 * TCP stream server that accepts a client connection and streams
 * video/audio/telemetry data using the CamDroid v2.0 binary protocol.
 *
 * Uses a dedicated async sender queue so MediaCodec hardware threads never block
 * on TCP socket I/O, and performs keyframe-aware P-frame shedding under Wi-Fi congestion.
 */
class StreamServer(
    private val port: Int = 4747,
    private val onControlCommand: (JsonObject) -> Unit,
    private val onClientConnected: () -> Unit,
    private val onClientDisconnected: () -> Unit,
    private val onNetworkCongestion: (droppedFrames: Long, sendLatencyMs: Long) -> Unit = { _, _ -> }
) {
    companion object {
        private const val TAG = "StreamServer"
        private const val HEARTBEAT_INTERVAL_MS = 2000L
        private const val SOCKET_SEND_BUFFER_BYTES = 512 * 1024
        private const val MAX_QUEUED_VIDEO_FRAMES = 3
        private const val CONGESTION_SEND_MS_THRESHOLD = 28L
    }

    private data class OutboundPacket(
        val type: Byte,
        val flags: Byte,
        val ptsUs: Long,
        val payload: ByteArray,
        val isDroppableVideoFrame: Boolean = false
    )

    private var serverSocket: ServerSocket? = null
    private var clientSocket: Socket? = null
    private var outputStream: OutputStream? = null
    private var isRunning = false
    private var serverJob: Job? = null
    private var senderJob: Job? = null
    private var heartbeatJob: Job? = null
    private var readerJob: Job? = null
    private val gson = Gson()

    private val queueLock = Any()
    private val outboundQueue = ArrayDeque<OutboundPacket>(16)
    private var wakeSenderSignal = Channel<Unit>(Channel.CONFLATED)

    val droppedFrames = AtomicLong(0L)
    val isClientConnected: Boolean
        get() = clientSocket?.isConnected == true && clientSocket?.isClosed == false

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

    private fun handleClient(socket: Socket, scope: CoroutineScope) {
        closeClient()

        clientSocket = socket
        socket.tcpNoDelay = true
        socket.sendBufferSize = SOCKET_SEND_BUFFER_BYTES
        socket.keepAlive = true
        outputStream = BufferedOutputStream(socket.getOutputStream(), 64 * 1024)
        wakeSenderSignal = Channel(Channel.CONFLATED)
        droppedFrames.set(0L)

        Log.i(TAG, "Client connected: ${socket.inetAddress.hostAddress}")
        onClientConnected()

        // Dedicated non-blocking sender loop (drains outboundQueue on IO thread)
        senderJob = scope.launch(Dispatchers.IO) {
            senderLoop()
        }

        // Start reading control commands from the client
        readerJob = scope.launch(Dispatchers.IO) {
            readControlLoop(socket.getInputStream())
        }

        // Start heartbeat
        heartbeatJob = scope.launch(Dispatchers.IO) {
            while (isActive && isClientConnected) {
                sendHeartbeat()
                delay(HEARTBEAT_INTERVAL_MS)
            }
        }
    }

    private suspend fun senderLoop() {
        while (currentCoroutineContext().isActive && isRunning && isClientConnected) {
            val packet: OutboundPacket? = synchronized(queueLock) {
                outboundQueue.removeFirstOrNull()
            }

            if (packet == null) {
                wakeSenderSignal.receiveCatching()
                continue
            }

            val out = outputStream ?: break
            val startNs = System.nanoTime()
            try {
                ProtocolHandler.writePacket(
                    output = out,
                    type = packet.type,
                    payload = packet.payload,
                    flags = packet.flags,
                    ptsUs = packet.ptsUs
                )
                val elapsedMs = (System.nanoTime() - startNs) / 1_000_000L
                if (packet.type == ProtocolHandler.VIDEO_FRAME && elapsedMs > CONGESTION_SEND_MS_THRESHOLD) {
                    onNetworkCongestion(droppedFrames.get(), elapsedMs)
                }
            } catch (e: Exception) {
                Log.d(TAG, "Send failed (client disconnected): ${e.message}")
                break
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
                        Log.d(TAG, "Heartbeat from client (pts=${packet.ptsUs})")
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

    /** Queue a video frame non-blockingly; shed stale P-frames if network is congested. */
    fun sendVideoFrame(
        data: ByteArray,
        isKeyFrame: Boolean = false,
        isMirrored: Boolean = false,
        ptsUs: Long = System.nanoTime() / 1000L
    ) {
        var flags = 0
        if (isKeyFrame) flags = flags or ProtocolHandler.FLAG_KEYFRAME.toInt()
        if (isMirrored) flags = flags or ProtocolHandler.FLAG_MIRRORED.toInt()

        var shedCount = 0L
        synchronized(queueLock) {
            val queuedVideoCount = outboundQueue.count { it.type == ProtocolHandler.VIDEO_FRAME }
            if (queuedVideoCount >= MAX_QUEUED_VIDEO_FRAMES) {
                // Shed queued droppable P-frames to keep glass-to-glass latency minimal
                val iterator = outboundQueue.iterator()
                while (iterator.hasNext()) {
                    val item = iterator.next()
                    if (item.isDroppableVideoFrame) {
                        iterator.remove()
                        shedCount++
                    }
                }
                // If still full and current frame is a P-frame, drop it and request an IDR frame
                if (!isKeyFrame && outboundQueue.count { it.type == ProtocolHandler.VIDEO_FRAME } >= MAX_QUEUED_VIDEO_FRAMES) {
                    shedCount++
                    val totalDropped = droppedFrames.addAndGet(shedCount)
                    onNetworkCongestion(totalDropped, CONGESTION_SEND_MS_THRESHOLD)
                    return
                }
            }

            outboundQueue.addLast(
                OutboundPacket(
                    type = ProtocolHandler.VIDEO_FRAME,
                    flags = flags.toByte(),
                    ptsUs = ptsUs,
                    payload = data,
                    isDroppableVideoFrame = !isKeyFrame
                )
            )
        }

        if (shedCount > 0) {
            val totalDropped = droppedFrames.addAndGet(shedCount)
            onNetworkCongestion(totalDropped, CONGESTION_SEND_MS_THRESHOLD)
        }
        wakeSenderSignal.trySend(Unit)
    }

    /** Send an audio frame to the connected client with hardware PTS. */
    fun sendAudioFrame(data: ByteArray, ptsUs: Long = System.nanoTime() / 1000L) {
        enqueueControlOrConfig(ProtocolHandler.AUDIO_FRAME, ProtocolHandler.FLAG_NONE, ptsUs, data)
    }

    /** Send video codec configuration (SPS/PPS/VPS) with priority. */
    fun sendVideoConfig(data: ByteArray, ptsUs: Long = System.nanoTime() / 1000L) {
        enqueueControlOrConfig(ProtocolHandler.VIDEO_CONFIG, ProtocolHandler.FLAG_CONFIG, ptsUs, data, highPriority = true)
    }

    /** Send audio codec configuration with priority. */
    fun sendAudioConfig(data: ByteArray, ptsUs: Long = System.nanoTime() / 1000L) {
        enqueueControlOrConfig(ProtocolHandler.AUDIO_CONFIG, ProtocolHandler.FLAG_CONFIG, ptsUs, data, highPriority = true)
    }

    /** Send metadata as JSON. */
    fun sendMetadata(json: String) {
        enqueueControlOrConfig(
            ProtocolHandler.METADATA,
            ProtocolHandler.FLAG_NONE,
            System.nanoTime() / 1000L,
            json.toByteArray(Charsets.UTF_8),
            highPriority = true
        )
    }

    /** Send a heartbeat. */
    fun sendHeartbeat() {
        enqueueControlOrConfig(
            ProtocolHandler.HEARTBEAT,
            ProtocolHandler.FLAG_NONE,
            System.nanoTime() / 1000L,
            ByteArray(0)
        )
    }

    private fun enqueueControlOrConfig(
        type: Byte,
        flags: Byte,
        ptsUs: Long,
        payload: ByteArray,
        highPriority: Boolean = false
    ) {
        if (!isClientConnected) return
        val pkt = OutboundPacket(type, flags, ptsUs, payload, isDroppableVideoFrame = false)
        synchronized(queueLock) {
            if (highPriority) {
                outboundQueue.addFirst(pkt)
            } else {
                if (outboundQueue.size < 64) {
                    outboundQueue.addLast(pkt)
                }
            }
        }
        wakeSenderSignal.trySend(Unit)
    }

    private fun closeClient() {
        readerJob?.cancel()
        heartbeatJob?.cancel()
        senderJob?.cancel()
        synchronized(queueLock) {
            outboundQueue.clear()
        }
        wakeSenderSignal.close()
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

