package com.camdroid.network

data class VideoMode(val width: Int, val height: Int, val fps: Int, val minBitrateBps: Long, val maxBitrateBps: Long)
data class CapabilitySet(val protocolVersions: Set<Int>, val codecs: Set<String>, val videoModes: Set<VideoMode>, val audioCodecs: Set<String>) {
    fun supports(request: StreamRequest): SessionError? = when {
        2 !in protocolVersions -> SessionError(SessionErrorCode.UNSUPPORTED_VERSION, "Protocol version 2 is not supported")
        codecs.none { it.equals(request.codec, true) } -> SessionError(SessionErrorCode.UNSUPPORTED_MODE, "Codec ${request.codec} is not supported")
        videoModes.none { it.width == request.width && it.height == request.height && it.fps == request.fps } -> SessionError(SessionErrorCode.UNSUPPORTED_MODE, "${request.width}x${request.height}@${request.fps} is not supported")
        videoModes.first { it.width == request.width && it.height == request.height && it.fps == request.fps }.let { request.bitrateBps !in it.minBitrateBps..it.maxBitrateBps } -> SessionError(SessionErrorCode.UNSUPPORTED_MODE, "Bitrate is unsupported")
        request.audioEnabled && audioCodecs.isEmpty() -> SessionError(SessionErrorCode.UNSUPPORTED_MODE, "Audio is unavailable")
        else -> null
    }
}
data class StreamRequest(val codec: String, val width: Int, val height: Int, val fps: Int, val bitrateBps: Long, val audioEnabled: Boolean)
data class NegotiatedStream(val sessionId: Long, val request: StreamRequest)
enum class SessionErrorCode { INVALID_TRANSITION, UNSUPPORTED_VERSION, UNSUPPORTED_MODE, REJECTED, PROTOCOL_ERROR }
data class SessionError(val code: SessionErrorCode, val message: String)
sealed interface SessionPhase { data object Idle : SessionPhase; data object Connecting : SessionPhase; data object Negotiating : SessionPhase; data object AwaitingStartAck : SessionPhase; data class Streaming(val stream: NegotiatedStream) : SessionPhase; data object AwaitingReconfigureAck : SessionPhase; data object Reconnecting : SessionPhase; data object Stopped : SessionPhase; data class Failed(val error: SessionError) : SessionPhase }
sealed interface SessionEvent { data object ConnectRequested : SessionEvent; data object TransportConnected : SessionEvent; data class CapabilitiesReceived(val capabilities: CapabilitySet) : SessionEvent; data class StartRequested(val request: StreamRequest) : SessionEvent; data class StartAcknowledged(val stream: NegotiatedStream) : SessionEvent; data class ReconfigureRequested(val request: StreamRequest) : SessionEvent; data class ReconfigureAcknowledged(val stream: NegotiatedStream) : SessionEvent; data object ConnectionLost : SessionEvent; data object StopRequested : SessionEvent }
sealed interface SessionEffect { data class SendStart(val request: StreamRequest) : SessionEffect; data class SendReconfigure(val request: StreamRequest) : SessionEffect; data class StartMedia(val stream: NegotiatedStream) : SessionEffect; data class ReconfigureMedia(val stream: NegotiatedStream) : SessionEffect; data object ClearMediaQueues : SessionEffect }

class StreamSessionState {
    var phase: SessionPhase = SessionPhase.Idle; private set
    var capabilities: CapabilitySet? = null; private set
    var pending: StreamRequest? = null; private set
    var queueGeneration: Long = 0; private set

    fun apply(event: SessionEvent): Result<List<SessionEffect>> = when (event) {
        SessionEvent.ConnectRequested -> transition(setOf(SessionPhase.Idle::class, SessionPhase.Stopped::class, SessionPhase.Reconnecting::class), SessionPhase.Connecting)
        SessionEvent.TransportConnected -> transition(setOf(SessionPhase.Connecting::class), SessionPhase.Negotiating)
        is SessionEvent.CapabilitiesReceived -> if (phase is SessionPhase.Negotiating && 2 in event.capabilities.protocolVersions) { capabilities = event.capabilities; Result.success(emptyList()) } else failure(if (2 !in event.capabilities.protocolVersions) SessionError(SessionErrorCode.UNSUPPORTED_VERSION, "Protocol version 2 is not supported") else invalid(event))
        is SessionEvent.StartRequested -> request(event.request, false)
        is SessionEvent.ReconfigureRequested -> request(event.request, true)
        is SessionEvent.StartAcknowledged -> acknowledge(event.stream, false)
        is SessionEvent.ReconfigureAcknowledged -> acknowledge(event.stream, true)
        SessionEvent.ConnectionLost -> { capabilities = null; pending = null; queueGeneration++; phase = SessionPhase.Reconnecting; Result.success(listOf(SessionEffect.ClearMediaQueues)) }
        SessionEvent.StopRequested -> { pending = null; phase = SessionPhase.Stopped; Result.success(listOf(SessionEffect.ClearMediaQueues)) }
    }

    private fun request(request: StreamRequest, reconfigure: Boolean): Result<List<SessionEffect>> {
        val validPhase = if (reconfigure) phase is SessionPhase.Streaming else phase is SessionPhase.Negotiating
        val error = capabilities?.let { it.supports(request) }
            ?: if (capabilities == null) SessionError(SessionErrorCode.INVALID_TRANSITION, "Capabilities have not arrived") else null
        if (!validPhase) return failure(invalid(request))
        if (error != null) return failure(error)
        pending = request; phase = if (reconfigure) SessionPhase.AwaitingReconfigureAck else SessionPhase.AwaitingStartAck
        return Result.success(listOf(if (reconfigure) SessionEffect.SendReconfigure(request) else SessionEffect.SendStart(request)))
    }
    private fun acknowledge(stream: NegotiatedStream, reconfigure: Boolean): Result<List<SessionEffect>> {
        val validPhase = if (reconfigure) phase is SessionPhase.AwaitingReconfigureAck else phase is SessionPhase.AwaitingStartAck
        if (!validPhase || pending != stream.request) return failure(invalid(stream))
        pending = null; phase = SessionPhase.Streaming(stream)
        return if (reconfigure) { queueGeneration++; Result.success(listOf(SessionEffect.ClearMediaQueues, SessionEffect.ReconfigureMedia(stream))) } else Result.success(listOf(SessionEffect.StartMedia(stream)))
    }
    private fun transition(allowed: Set<kotlin.reflect.KClass<out SessionPhase>>, next: SessionPhase): Result<List<SessionEffect>> = if (allowed.any { it.isInstance(phase) }) { phase = next; Result.success(emptyList()) } else failure(invalid(next))
    private fun invalid(event: Any) = SessionError(SessionErrorCode.INVALID_TRANSITION, "Cannot apply $event while $phase")
    private fun failure(error: SessionError): Result<List<SessionEffect>> { phase = SessionPhase.Failed(error); return Result.failure(IllegalStateException(error.message)) }
}
