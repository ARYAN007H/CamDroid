package com.camdroid.network

import android.util.Log
import java.io.InputStream
import java.io.OutputStream
import java.nio.ByteBuffer
import java.nio.ByteOrder

/**
 * CamDroid binary wire protocol handler v2.0.
 *
 * Packet format (14-byte header):
 * [Type: 1 byte] [Flags: 1 byte] [PTS_us: 8 bytes LE] [Length: 4 bytes LE] [Payload: N bytes]
 */
object ProtocolHandler {
    private const val TAG = "Protocol"
    private const val HEADER_SIZE = 14
    private const val MAX_PAYLOAD_SIZE = 16 * 1024 * 1024 // 16 MB

    // Packet type constants
    const val VIDEO_FRAME: Byte = 0x01
    const val AUDIO_FRAME: Byte = 0x02
    const val CONTROL_CMD: Byte = 0x03
    const val METADATA: Byte = 0x04
    const val HEARTBEAT: Byte = 0x05
    const val VIDEO_CONFIG: Byte = 0x06
    const val AUDIO_CONFIG: Byte = 0x07

    // Packet flag bitmasks
    const val FLAG_NONE: Byte = 0x00
    const val FLAG_KEYFRAME: Byte = 0x01
    const val FLAG_CONFIG: Byte = 0x02
    const val FLAG_MIRRORED: Byte = 0x04

    data class Packet(
        val type: Byte,
        val flags: Byte = FLAG_NONE,
        val ptsUs: Long = 0L,
        val payload: ByteArray
    ) {
        val isKeyFrame: Boolean get() = (flags.toInt() and FLAG_KEYFRAME.toInt()) != 0
        val isConfig: Boolean get() = (flags.toInt() and FLAG_CONFIG.toInt()) != 0

        override fun equals(other: Any?): Boolean {
            if (this === other) return true
            if (other !is Packet) return false
            return type == other.type &&
                flags == other.flags &&
                ptsUs == other.ptsUs &&
                payload.contentEquals(other.payload)
        }

        override fun hashCode(): Int {
            var result = type.hashCode()
            result = 31 * result + flags.hashCode()
            result = 31 * result + ptsUs.hashCode()
            result = 31 * result + payload.contentHashCode()
            return result
        }
    }

    /**
     * Write a complete v2.0 packet to the output stream.
     * Thread-safe when called from the single dedicated network writer coroutine.
     */
    fun writePacket(
        output: OutputStream,
        type: Byte,
        payload: ByteArray,
        flags: Byte = FLAG_NONE,
        ptsUs: Long = System.nanoTime() / 1000L
    ) {
        val header = ByteBuffer.allocate(HEADER_SIZE).order(ByteOrder.LITTLE_ENDIAN)
        header.put(type)
        header.put(flags)
        header.putLong(ptsUs)
        header.putInt(payload.size)
        output.write(header.array())
        if (payload.isNotEmpty()) {
            output.write(payload)
        }
        output.flush()
    }

    /** Write a heartbeat packet (empty payload, carries current monotonic timestamp). */
    fun writeHeartbeat(output: OutputStream) {
        writePacket(output, HEARTBEAT, ByteArray(0), FLAG_NONE, System.nanoTime() / 1000L)
    }

    /** Write a metadata packet with JSON payload. */
    fun writeMetadata(output: OutputStream, json: String) {
        writePacket(output, METADATA, json.toByteArray(Charsets.UTF_8))
    }

    /** Write a video configuration packet (SPS/PPS/VPS). */
    fun writeVideoConfig(output: OutputStream, configData: ByteArray, ptsUs: Long = 0L) {
        writePacket(output, VIDEO_CONFIG, configData, FLAG_CONFIG, ptsUs)
    }

    /** Write an audio configuration packet (AudioSpecificConfig). */
    fun writeAudioConfig(output: OutputStream, configData: ByteArray, ptsUs: Long = 0L) {
        writePacket(output, AUDIO_CONFIG, configData, FLAG_CONFIG, ptsUs)
    }

    /** Write a video frame packet with keyframe and mirror flags plus hardware PTS. */
    fun writeVideoFrame(
        output: OutputStream,
        frameData: ByteArray,
        isKeyFrame: Boolean = false,
        isMirrored: Boolean = false,
        ptsUs: Long = 0L
    ) {
        var flags = 0
        if (isKeyFrame) flags = flags or FLAG_KEYFRAME.toInt()
        if (isMirrored) flags = flags or FLAG_MIRRORED.toInt()
        writePacket(output, VIDEO_FRAME, frameData, flags.toByte(), ptsUs)
    }

    /** Write an audio frame packet (AAC with ADTS header) with hardware PTS. */
    fun writeAudioFrame(output: OutputStream, frameData: ByteArray, ptsUs: Long = 0L) {
        writePacket(output, AUDIO_FRAME, frameData, FLAG_NONE, ptsUs)
    }

    /**
     * Read a complete v2.0 packet from the input stream.
     * Blocks until a full 14-byte header + payload is received.
     * Returns null on clean stream end.
     */
    fun readPacket(input: InputStream): Packet? {
        val headerBytes = ByteArray(HEADER_SIZE)
        var read = 0
        while (read < HEADER_SIZE) {
            val n = input.read(headerBytes, read, HEADER_SIZE - read)
            if (n == -1) return null
            read += n
        }

        val headerBuf = ByteBuffer.wrap(headerBytes).order(ByteOrder.LITTLE_ENDIAN)
        val typeByte = headerBuf.get()
        val flagsByte = headerBuf.get()
        val ptsUs = headerBuf.long
        val length = headerBuf.int

        if (length < 0 || length > MAX_PAYLOAD_SIZE) {
            Log.e(TAG, "Invalid payload length: $length")
            return null
        }

        val payload = ByteArray(length)
        read = 0
        while (read < length) {
            val n = input.read(payload, read, length - read)
            if (n == -1) return null
            read += n
        }

        return Packet(typeByte, flagsByte, ptsUs, payload)
    }
}

