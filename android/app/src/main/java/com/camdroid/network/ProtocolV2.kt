package com.camdroid.network

import java.io.InputStream
import java.io.OutputStream
import java.nio.ByteBuffer
import java.nio.ByteOrder

/**
 * Timestamped CamDroid protocol v2 envelope.
 *
 * It deliberately lives beside the legacy ProtocolHandler while both sides
 * migrate their session handshakes. Every v2 packet has a stable sender
 * session, sequence number, and monotonic capture time in nanoseconds.
 */
object ProtocolV2 {
    val MAGIC: ByteArray = byteArrayOf(0x43, 0x4d, 0x44, 0x32) // CMD2
    const val VERSION: Byte = 2
    const val HEADER_SIZE = 36
    const val MAX_PAYLOAD_SIZE = 16 * 1024 * 1024

    object Flags {
        const val NONE = 0
        const val KEY_FRAME = 1 shl 0
        const val CONFIGURATION = 1 shl 1
        const val DISCONTINUITY = 1 shl 2
        private const val KNOWN = KEY_FRAME or CONFIGURATION or DISCONTINUITY

        fun validate(value: Int) {
            require(value and KNOWN == value) { "unsupported media flags: 0x${value.toString(16)}" }
        }
    }

    enum class MediaKind(val wireValue: Byte) {
        VIDEO(0x01),
        AUDIO(0x02),
        CONTROL(0x03),
        METADATA(0x04),
        HEARTBEAT(0x05),
        VIDEO_CONFIG(0x06),
        AUDIO_CONFIG(0x07);

        companion object {
            fun fromWireValue(value: Byte): MediaKind =
                entries.firstOrNull { it.wireValue == value }
                    ?: throw IllegalArgumentException("unsupported media kind: 0x${value.toUByte().toString(16)}")
        }
    }

    data class Packet(
        val kind: MediaKind,
        val flags: Int,
        val sessionId: Long,
        val sequence: Long,
        val captureTimestampNs: Long,
        val payload: ByteArray,
    ) {
        init {
            Flags.validate(flags)
            require(payload.size <= MAX_PAYLOAD_SIZE) { "v2 payload too large: ${payload.size} bytes" }
        }

        override fun equals(other: Any?): Boolean = other is Packet &&
            kind == other.kind &&
            flags == other.flags &&
            sessionId == other.sessionId &&
            sequence == other.sequence &&
            captureTimestampNs == other.captureTimestampNs &&
            payload.contentEquals(other.payload)

        override fun hashCode(): Int {
            var result = kind.hashCode()
            result = 31 * result + flags
            result = 31 * result + sessionId.hashCode()
            result = 31 * result + sequence.hashCode()
            result = 31 * result + captureTimestampNs.hashCode()
            return 31 * result + payload.contentHashCode()
        }
    }

    fun encode(packet: Packet): ByteArray {
        val bytes = ByteBuffer.allocate(HEADER_SIZE + packet.payload.size).order(ByteOrder.LITTLE_ENDIAN)
        bytes.put(MAGIC)
        bytes.put(VERSION)
        bytes.put(packet.kind.wireValue)
        bytes.putShort(packet.flags.toShort())
        bytes.putLong(packet.sessionId)
        bytes.putLong(packet.sequence)
        bytes.putLong(packet.captureTimestampNs)
        bytes.putInt(packet.payload.size)
        bytes.put(packet.payload)
        return bytes.array()
    }

    fun decode(bytes: ByteArray): Packet {
        require(bytes.size >= HEADER_SIZE) { "truncated v2 header: expected $HEADER_SIZE bytes, received ${bytes.size}" }
        require(bytes.copyOfRange(0, MAGIC.size).contentEquals(MAGIC)) { "invalid v2 protocol magic" }

        val buffer = ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN)
        buffer.position(4)
        val version = buffer.get()
        require(version == VERSION) { "unsupported protocol version: ${version.toUByte()}" }
        val kind = MediaKind.fromWireValue(buffer.get())
        val flags = buffer.short.toInt() and 0xffff
        Flags.validate(flags)
        val sessionId = buffer.long
        val sequence = buffer.long
        val captureTimestampNs = buffer.long
        val payloadLength = buffer.int
        require(payloadLength >= 0 && payloadLength <= MAX_PAYLOAD_SIZE) {
            "v2 payload too large: $payloadLength bytes"
        }
        val expectedLength = HEADER_SIZE + payloadLength
        require(bytes.size >= expectedLength) {
            "truncated v2 payload: declared $payloadLength bytes, received ${bytes.size - HEADER_SIZE}"
        }
        require(bytes.size == expectedLength) { "v2 frame has ${bytes.size - expectedLength} trailing bytes" }
        val payload = ByteArray(payloadLength)
        buffer.get(payload)
        return Packet(kind, flags, sessionId, sequence, captureTimestampNs, payload)
    }

    fun readFrom(input: InputStream): Packet {
        val header = readExactly(input, HEADER_SIZE) ?: throw IllegalStateException("truncated v2 header")
        val payloadLength = ByteBuffer.wrap(header, 32, 4).order(ByteOrder.LITTLE_ENDIAN).int
        require(payloadLength >= 0 && payloadLength <= MAX_PAYLOAD_SIZE) {
            "v2 payload too large: $payloadLength bytes"
        }
        val payload = readExactly(input, payloadLength) ?: throw IllegalStateException("truncated v2 payload")
        return decode(header + payload)
    }

    fun writeTo(output: OutputStream, packet: Packet) {
        output.write(encode(packet))
    }

    private fun readExactly(input: InputStream, length: Int): ByteArray? {
        val bytes = ByteArray(length)
        var offset = 0
        while (offset < length) {
            val read = input.read(bytes, offset, length - offset)
            if (read < 0) return null
            offset += read
        }
        return bytes
    }
}
