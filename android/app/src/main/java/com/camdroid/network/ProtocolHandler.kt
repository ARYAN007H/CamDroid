package com.camdroid.network

import android.util.Log
import java.io.InputStream
import java.io.OutputStream
import java.nio.ByteBuffer
import java.nio.ByteOrder

/**
 * CamDroid binary wire protocol handler.
 *
 * Packet format: [Type: 1 byte] [Length: 4 bytes LE] [Payload: N bytes]
 */
object ProtocolHandler {
    private const val TAG = "Protocol"
    private const val MAX_PAYLOAD_SIZE = 16 * 1024 * 1024 // 16 MB

    // Packet type constants
    const val VIDEO_FRAME: Byte = 0x01
    const val AUDIO_FRAME: Byte = 0x02
    const val CONTROL_CMD: Byte = 0x03
    const val METADATA: Byte = 0x04
    const val HEARTBEAT: Byte = 0x05
    const val VIDEO_CONFIG: Byte = 0x06
    const val AUDIO_CONFIG: Byte = 0x07

    data class Packet(
        val type: Byte,
        val payload: ByteArray
    ) {
        override fun equals(other: Any?): Boolean {
            if (this === other) return true
            if (other !is Packet) return false
            return type == other.type && payload.contentEquals(other.payload)
        }
        override fun hashCode(): Int = 31 * type.hashCode() + payload.contentHashCode()
    }

    /**
     * Write a complete packet to the output stream.
     * Thread-safe when called with synchronized access to the stream.
     */
    fun writePacket(output: OutputStream, type: Byte, payload: ByteArray) {
        val header = ByteBuffer.allocate(5).order(ByteOrder.LITTLE_ENDIAN)
        header.put(type)
        header.putInt(payload.size)
        output.write(header.array())
        if (payload.isNotEmpty()) {
            output.write(payload)
        }
        output.flush()
    }

    /** Write a heartbeat packet (empty payload). */
    fun writeHeartbeat(output: OutputStream) {
        writePacket(output, HEARTBEAT, ByteArray(0))
    }

    /** Write a metadata packet with JSON payload. */
    fun writeMetadata(output: OutputStream, json: String) {
        writePacket(output, METADATA, json.toByteArray(Charsets.UTF_8))
    }

    /** Write a video configuration packet (SPS/PPS/VPS). */
    fun writeVideoConfig(output: OutputStream, configData: ByteArray) {
        writePacket(output, VIDEO_CONFIG, configData)
    }

    /** Write an audio configuration packet (AudioSpecificConfig). */
    fun writeAudioConfig(output: OutputStream, configData: ByteArray) {
        writePacket(output, AUDIO_CONFIG, configData)
    }

    /** Write a video frame packet. */
    fun writeVideoFrame(output: OutputStream, frameData: ByteArray) {
        writePacket(output, VIDEO_FRAME, frameData)
    }

    /** Write an audio frame packet (AAC with ADTS header). */
    fun writeAudioFrame(output: OutputStream, frameData: ByteArray) {
        writePacket(output, AUDIO_FRAME, frameData)
    }

    /**
     * Read a complete packet from the input stream.
     * Blocks until a full packet is received.
     * Returns null on stream end.
     */
    fun readPacket(input: InputStream): Packet? {
        // Read 1-byte type
        val typeByte = input.read()
        if (typeByte == -1) return null

        // Read 4-byte length (little-endian)
        val lengthBytes = ByteArray(4)
        var read = 0
        while (read < 4) {
            val n = input.read(lengthBytes, read, 4 - read)
            if (n == -1) return null
            read += n
        }
        val length = ByteBuffer.wrap(lengthBytes).order(ByteOrder.LITTLE_ENDIAN).int

        if (length < 0 || length > MAX_PAYLOAD_SIZE) {
            Log.e(TAG, "Invalid payload length: $length")
            return null
        }

        // Read payload
        val payload = ByteArray(length)
        read = 0
        while (read < length) {
            val n = input.read(payload, read, length - read)
            if (n == -1) return null
            read += n
        }

        return Packet(typeByte.toByte(), payload)
    }
}
