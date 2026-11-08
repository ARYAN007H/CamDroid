package com.camdroid.network

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class ProtocolV2Test {
    private fun timestampedVideoPacket() = ProtocolV2.Packet(
        kind = ProtocolV2.MediaKind.VIDEO,
        flags = ProtocolV2.Flags.KEY_FRAME or ProtocolV2.Flags.CONFIGURATION,
        sessionId = 0x0807_0605_0403_0201L,
        sequence = 0x100f_0e0d_0c0b_0a09L,
        captureTimestampNs = 0x1817_1615_1413_1211L,
        payload = byteArrayOf(0x00, 0x00, 0x00, 0x01, 0x65),
    )

    @Test
    fun timestampedVideoPacketRoundTripsWithItsIdentityFields() {
        // Changing field order, width, or byte order corrupts a frame identity.
        val packet = timestampedVideoPacket()

        assertEquals(packet, ProtocolV2.decode(ProtocolV2.encode(packet)))
    }

    @Test
    fun encodingMatchesTheSharedLittleEndianTestVector() {
        // This vector is deliberately literal and mirrored in desktop Rust.
        val encoded = ProtocolV2.encode(timestampedVideoPacket())
        val expected = byteArrayOf(
            0x43, 0x4d, 0x44, 0x32,
            0x02, 0x01, 0x03, 0x00,
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08,
            0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x10,
            0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18,
            0x05, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x01, 0x65,
        )

        assertEquals(36, ProtocolV2.HEADER_SIZE)
        assertArrayEquals(expected, encoded)
    }

    @Test
    fun decodeRejectsADeclaredPayloadLargerThanTheSafetyLimit() {
        // A peer-controlled length must not cause a large allocation.
        val bytes = ByteArray(ProtocolV2.HEADER_SIZE)
        ProtocolV2.MAGIC.copyInto(bytes)
        bytes[4] = ProtocolV2.VERSION
        bytes[5] = ProtocolV2.MediaKind.VIDEO.wireValue
        java.nio.ByteBuffer.wrap(bytes, 32, 4)
            .order(java.nio.ByteOrder.LITTLE_ENDIAN)
            .putInt(ProtocolV2.MAX_PAYLOAD_SIZE + 1)

        val error = runCatching { ProtocolV2.decode(bytes) }.exceptionOrNull()

        assertTrue(error?.message?.contains("too large") == true)
    }

    @Test
    fun decodeRejectsTruncatedPayloads() {
        // A complete header does not make a partial TCP payload valid.
        val fullPacket = ProtocolV2.encode(timestampedVideoPacket())
        val truncated = fullPacket.copyOf(fullPacket.size - 1)

        val error = runCatching { ProtocolV2.decode(truncated) }.exceptionOrNull()

        assertTrue(error?.message?.contains("truncated") == true)
    }

    @Test
    fun decodeRejectsUnsupportedVersions() {
        // A receiver must fail closed before dispatching incompatible media.
        val bytes = ProtocolV2.encode(timestampedVideoPacket())
        bytes[4] = 3

        val error = runCatching { ProtocolV2.decode(bytes) }.exceptionOrNull()

        assertTrue(error?.message?.contains("unsupported protocol version") == true)
    }
}
