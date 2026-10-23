package com.camdroid.network

import java.util.ArrayDeque
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.concurrent.thread

/**
 * Owns bounded encoded-media queues and the only thread allowed to write
 * media packets. Encoder callbacks only copy and enqueue data here.
 */
class MediaSender(
    private val sessionId: Long,
    private val writeMedia: (ProtocolV2.Packet) -> Unit,
    private val videoCapacity: Int = 3,
    private val audioCapacity: Int = 16,
) {
    data class Stats(
        val queuedVideoFrames: Int,
        val queuedAudioFrames: Int,
        val droppedVideoFrames: Long,
        val droppedAudioFrames: Long,
        val sentVideoFrames: Long,
        val sentAudioFrames: Long,
        val sentBytes: Long,
    )

    private data class Frame(
        val kind: ProtocolV2.MediaKind,
        val flags: Int,
        val sequence: Long,
        val captureTimestampNs: Long,
        val payload: ByteArray,
    ) {
        val priority: Boolean get() = flags and (ProtocolV2.Flags.CONFIGURATION or ProtocolV2.Flags.KEY_FRAME) != 0
    }

    private val lock = Object()
    private val videoQueue = ArrayDeque<Frame>()
    private val audioQueue = ArrayDeque<Frame>()
    private val running = AtomicBoolean(false)
    private var worker: Thread? = null
    private var nextSequence = 0L
    private var droppedVideo = 0L
    private var droppedAudio = 0L
    private var sentVideo = 0L
    private var sentAudio = 0L
    private var sentBytes = 0L

    init {
        require(videoCapacity > 0)
        require(audioCapacity > 0)
    }

    fun start() {
        if (!running.compareAndSet(false, true)) return
        worker = thread(name = "CamDroidMediaSender", isDaemon = true) { sendLoop() }
    }

    fun stop() {
        if (!running.compareAndSet(true, false)) return
        synchronized(lock) { lock.notifyAll() }
        worker?.join(1_000)
        worker = null
    }

    fun offerVideo(
        payload: ByteArray,
        captureTimestampNs: Long,
        isConfig: Boolean = false,
        isKeyFrame: Boolean = false,
        discontinuity: Boolean = false,
    ) = offer(
        videoQueue,
        videoCapacity,
        Frame(
            kind = if (isConfig) ProtocolV2.MediaKind.VIDEO_CONFIG else ProtocolV2.MediaKind.VIDEO,
            flags = (if (isConfig) ProtocolV2.Flags.CONFIGURATION else 0) or
                (if (isKeyFrame) ProtocolV2.Flags.KEY_FRAME else 0) or
                (if (discontinuity) ProtocolV2.Flags.DISCONTINUITY else 0),
            sequence = nextSequence(),
            captureTimestampNs = captureTimestampNs,
            payload = payload.copyOf(),
        ),
        video = true,
    )

    fun offerAudio(payload: ByteArray, captureTimestampNs: Long, isConfig: Boolean = false) = offer(
        audioQueue,
        audioCapacity,
        Frame(
            kind = if (isConfig) ProtocolV2.MediaKind.AUDIO_CONFIG else ProtocolV2.MediaKind.AUDIO,
            flags = if (isConfig) ProtocolV2.Flags.CONFIGURATION else 0,
            sequence = nextSequence(),
            captureTimestampNs = captureTimestampNs,
            payload = payload.copyOf(),
        ),
        video = false,
    )

    fun stats(): Stats = synchronized(lock) {
        Stats(videoQueue.size, audioQueue.size, droppedVideo, droppedAudio, sentVideo, sentAudio, sentBytes)
    }

    private fun nextSequence(): Long = synchronized(lock) { nextSequence++ }

    private fun offer(queue: ArrayDeque<Frame>, capacity: Int, frame: Frame, video: Boolean) {
        synchronized(lock) {
            if (queue.size == capacity) {
                val stale = queue.firstOrNull { !it.priority }
                if (stale != null) {
                    queue.remove(stale)
                    if (video) droppedVideo++ else droppedAudio++
                } else if (!frame.priority) {
                    if (video) droppedVideo++ else droppedAudio++
                    return
                } else {
                    if (video) droppedVideo++ else droppedAudio++
                    return
                }
            }
            queue.addLast(frame)
            lock.notifyAll()
        }
    }

    private fun sendLoop() {
        while (running.get()) {
            val frame = synchronized(lock) {
                while (running.get() && videoQueue.isEmpty() && audioQueue.isEmpty()) lock.wait()
                if (!running.get()) return
                if (videoQueue.isNotEmpty()) videoQueue.removeFirst() else audioQueue.removeFirst()
            }
            try {
                writeMedia(ProtocolV2.Packet(frame.kind, frame.flags, sessionId, frame.sequence, frame.captureTimestampNs, frame.payload))
                synchronized(lock) {
                    if (frame.kind == ProtocolV2.MediaKind.VIDEO || frame.kind == ProtocolV2.MediaKind.VIDEO_CONFIG) sentVideo++ else sentAudio++
                    sentBytes += frame.payload.size
                }
            } catch (_: Exception) {
                // The transport session owns reconnect/error handling; this worker
                // must never propagate a socket failure into an encoder callback.
            }
        }
    }
}
