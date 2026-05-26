package com.camdroid.encoder

import android.media.MediaCodec
import android.media.MediaCodecInfo
import android.media.MediaFormat
import android.os.Bundle
import android.os.Handler
import android.os.HandlerThread
import android.util.Log
import android.view.Surface

/**
 * Hardware video encoder using MediaCodec.
 *
 * Supports H.264, H.265/HEVC, and MJPEG codecs with Surface input.
 * Provides an input Surface that can be connected to Camera2's CaptureSession.
 */
class VideoEncoder(
    private val config: EncoderConfig,
    private val onEncodedData: (data: ByteArray, isConfig: Boolean, isKeyFrame: Boolean, pts: Long) -> Unit
) {
    companion object {
        private const val TAG = "VideoEncoder"
    }

    private var encoder: MediaCodec? = null
    private var handlerThread: HandlerThread? = null
    private var handler: Handler? = null

    /** The input Surface — connect this to Camera2 as a capture output target. */
    var inputSurface: Surface? = null
        private set

    /** Cached codec configuration data (SPS/PPS for H.264, VPS/SPS/PPS for H.265). */
    var codecConfigData: ByteArray? = null
        private set

    private var isRunning = false

    /**
     * Configure and start the encoder.
     * Must be called before accessing [inputSurface].
     */
    fun start() {
        Log.i(TAG, "Starting encoder: ${config.codec.displayName} ${config.resolution.displayName} @ ${config.fps}fps, ${config.bitrate / 1_000_000}Mbps")

        handlerThread = HandlerThread("VideoEncoder").also { it.start() }
        handler = Handler(handlerThread!!.looper)

        val format = MediaFormat.createVideoFormat(
            config.codec.mimeType,
            config.resolution.width,
            config.resolution.height
        ).apply {
            setInteger(MediaFormat.KEY_COLOR_FORMAT, MediaCodecInfo.CodecCapabilities.COLOR_FormatSurface)
            setInteger(MediaFormat.KEY_BIT_RATE, config.bitrate)
            setInteger(MediaFormat.KEY_FRAME_RATE, config.fps)
            setInteger(MediaFormat.KEY_I_FRAME_INTERVAL, config.iFrameInterval)
            setInteger(MediaFormat.KEY_BITRATE_MODE, MediaCodecInfo.EncoderCapabilities.BITRATE_MODE_CBR)
        }

        val codec = MediaCodec.createEncoderByType(config.codec.mimeType)

        // Set async callback BEFORE configure for async mode
        codec.setCallback(encoderCallback, handler)
        codec.configure(format, null, null, MediaCodec.CONFIGURE_FLAG_ENCODE)
        inputSurface = codec.createInputSurface()
        codec.start()

        encoder = codec
        isRunning = true
        Log.i(TAG, "Encoder started, input surface ready")
    }

    /** Dynamically update the encoder bitrate (for adaptive bitrate). */
    fun updateBitrate(newBitrateBps: Int) {
        encoder?.let { enc ->
            val params = Bundle().apply {
                putInt(MediaCodec.PARAMETER_KEY_VIDEO_BITRATE, newBitrateBps)
            }
            enc.setParameters(params)
            Log.d(TAG, "Bitrate updated to ${newBitrateBps / 1_000_000f}Mbps")
        }
    }

    /** Request an immediate keyframe from the encoder. */
    fun requestKeyFrame() {
        encoder?.let { enc ->
            val params = Bundle().apply {
                putInt(MediaCodec.PARAMETER_KEY_REQUEST_SYNC_FRAME, 0)
            }
            enc.setParameters(params)
            Log.d(TAG, "Keyframe requested")
        }
    }

    /** Stop and release the encoder. */
    fun stop() {
        isRunning = false
        try {
            encoder?.signalEndOfInputStream()
            encoder?.stop()
            encoder?.release()
        } catch (e: Exception) {
            Log.w(TAG, "Error stopping encoder", e)
        }
        encoder = null
        inputSurface?.release()
        inputSurface = null
        handlerThread?.quitSafely()
        handlerThread = null
        handler = null
        Log.i(TAG, "Encoder stopped")
    }

    private val encoderCallback = object : MediaCodec.Callback() {
        override fun onInputBufferAvailable(codec: MediaCodec, index: Int) {
            // Not used with Surface input — frames come via the Surface
        }

        override fun onOutputBufferAvailable(codec: MediaCodec, index: Int, info: MediaCodec.BufferInfo) {
            if (!isRunning) return

            val outputBuffer = codec.getOutputBuffer(index) ?: return

            try {
                if (info.flags and MediaCodec.BUFFER_FLAG_CODEC_CONFIG != 0) {
                    // Codec configuration data (SPS/PPS for H.264, VPS/SPS/PPS for H.265)
                    val configData = ByteArray(info.size)
                    outputBuffer.get(configData)
                    codecConfigData = configData
                    onEncodedData(configData, true, false, info.presentationTimeUs)
                } else if (info.size > 0) {
                    val isKeyFrame = info.flags and MediaCodec.BUFFER_FLAG_KEY_FRAME != 0
                    val frameData = ByteArray(info.size)
                    outputBuffer.get(frameData)
                    onEncodedData(frameData, false, isKeyFrame, info.presentationTimeUs)
                }
            } finally {
                codec.releaseOutputBuffer(index, false)
            }
        }

        override fun onOutputFormatChanged(codec: MediaCodec, format: MediaFormat) {
            Log.i(TAG, "Encoder output format changed: $format")
        }

        override fun onError(codec: MediaCodec, e: MediaCodec.CodecException) {
            Log.e(TAG, "Encoder error", e)
        }
    }
}
