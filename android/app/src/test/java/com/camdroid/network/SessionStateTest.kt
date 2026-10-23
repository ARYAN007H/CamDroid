package com.camdroid.network

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class SessionStateTest {
    private val capabilities = CapabilitySet(setOf(2), setOf("h264"), setOf(VideoMode(1920, 1080, 30, 1_000_000, 12_000_000)), setOf("aac"))
    private val request = StreamRequest("h264", 1920, 1080, 30, 8_000_000, true)
    private fun negotiating() = StreamSessionState().also { it.apply(SessionEvent.ConnectRequested); it.apply(SessionEvent.TransportConnected); it.apply(SessionEvent.CapabilitiesReceived(capabilities)) }

    @Test fun startWaitsForAcknowledgement() {
        val state = negotiating()
        assertTrue(state.apply(SessionEvent.StartRequested(request)).isSuccess)
        assertEquals(SessionPhase.AwaitingStartAck, state.phase)
        val stream = NegotiatedStream(1, request)
        assertEquals(listOf(SessionEffect.StartMedia(stream)), state.apply(SessionEvent.StartAcknowledged(stream)).getOrThrow())
        assertEquals(SessionPhase.Streaming(stream), state.phase)
    }

    @Test fun unsupportedCodecDoesNotBecomeActive() {
        val state = negotiating()
        assertTrue(state.apply(SessionEvent.StartRequested(request.copy(codec = "hevc"))).isFailure)
        assertTrue(state.phase is SessionPhase.Failed)
    }

    @Test fun reconnectClearsOldQueues() {
        val state = negotiating()
        state.apply(SessionEvent.StartRequested(request)); state.apply(SessionEvent.StartAcknowledged(NegotiatedStream(1, request)))
        assertEquals(listOf(SessionEffect.ClearMediaQueues), state.apply(SessionEvent.ConnectionLost).getOrThrow())
        assertEquals(1, state.queueGeneration)
        assertEquals(SessionPhase.Reconnecting, state.phase)
    }
}
