package com.camdroid.encoder

import android.annotation.SuppressLint
import android.media.AudioFormat
import android.media.AudioRecord
import android.media.MediaCodec
import android.media.MediaCodecInfo
import android.media.MediaFormat
import android.media.MediaRecorder
import android.util.Log
import kotlin.concurrent.thread

/**
 * AAC audio encoder using AudioRecord + MediaCodec.
 *
 * Captures audio from the device microphone, encodes it as AAC-LC,
 * and wraps each frame with an ADTS header for raw streaming.
 */
class AudioEncoder(
    private val config: AudioConfig = AudioConfig(),
    private val onEncodedAudio: (data: ByteArray, isConfig: Boolean, pts: Long) -> Unit
) {
    companion object {
        private const val TAG = "AudioEncoder"
    }

    private var audioRecord: AudioRecord? = null
    private var codec: MediaCodec? = null
    private var isRecording = false
    private var recordingThread: Thread? = null

    private val channelConfig = if (config.channelCount == 1)
        AudioFormat.CHANNEL_IN_MONO else AudioFormat.CHANNEL_IN_STEREO

    private val bufferSize = AudioRecord.getMinBufferSize(
        config.sampleRate, channelConfig, AudioFormat.ENCODING_PCM_16BIT
    ).coerceAtLeast(4096)

    @SuppressLint("MissingPermission")
    fun start() {
        Log.i(TAG, "Starting audio encoder: ${config.sampleRate}Hz, ${config.channelCount}ch, ${config.bitRate / 1000}kbps")

        audioRecord = AudioRecord(
            MediaRecorder.AudioSource.MIC,
            config.sampleRate,
            channelConfig,
            AudioFormat.ENCODING_PCM_16BIT,
            bufferSize * 2
        )

        val format = MediaFormat.createAudioFormat(
            MediaFormat.MIMETYPE_AUDIO_AAC,
            config.sampleRate,
            config.channelCount
        ).apply {
            setInteger(MediaFormat.KEY_BIT_RATE, config.bitRate)
            setInteger(MediaFormat.KEY_AAC_PROFILE, MediaCodecInfo.CodecProfileLevel.AACObjectLC)
            setInteger(MediaFormat.KEY_MAX_INPUT_SIZE, bufferSize * 2)
        }

        codec = MediaCodec.createEncoderByType(MediaFormat.MIMETYPE_AUDIO_AAC).also {
            it.configure(format, null, null, MediaCodec.CONFIGURE_FLAG_ENCODE)
            it.start()
        }

        isRecording = true
        audioRecord?.startRecording()

        recordingThread = thread(name = "AudioEncoder") {
            encodeLoop()
        }

        Log.i(TAG, "Audio encoder started")
    }

    fun stop() {
        isRecording = false
        recordingThread?.join(2000)
        recordingThread = null

        try {
            audioRecord?.stop()
            audioRecord?.release()
        } catch (e: Exception) {
            Log.w(TAG, "Error stopping AudioRecord", e)
        }
        audioRecord = null

        try {
            codec?.stop()
            codec?.release()
        } catch (e: Exception) {
            Log.w(TAG, "Error stopping codec", e)
        }
        codec = null

        Log.i(TAG, "Audio encoder stopped")
    }

    private fun encodeLoop() {
        val pcmBuffer = ByteArray(bufferSize)
        val bufferInfo = MediaCodec.BufferInfo()
        val encoder = codec ?: return
        val recorder = audioRecord ?: return

        while (isRecording) {
            // Read PCM from mic
            val bytesRead = recorder.read(pcmBuffer, 0, pcmBuffer.size)
            if (bytesRead <= 0) continue

            // Feed to encoder
            val inputIndex = encoder.dequeueInputBuffer(10_000)
            if (inputIndex >= 0) {
                val inputBuffer = encoder.getInputBuffer(inputIndex) ?: continue
                inputBuffer.clear()
                inputBuffer.put(pcmBuffer, 0, bytesRead)
                val pts = System.nanoTime() / 1000
                encoder.queueInputBuffer(inputIndex, 0, bytesRead, pts, 0)
            }

            // Drain encoder outputs
            drainEncoder(encoder, bufferInfo)
        }

        // Signal EOS
        val inputIndex = encoder.dequeueInputBuffer(10_000)
        if (inputIndex >= 0) {
            encoder.queueInputBuffer(inputIndex, 0, 0, 0, MediaCodec.BUFFER_FLAG_END_OF_STREAM)
        }
        drainEncoder(encoder, bufferInfo)
    }

    private fun drainEncoder(encoder: MediaCodec, bufferInfo: MediaCodec.BufferInfo) {
        while (true) {
            val outputIndex = encoder.dequeueOutputBuffer(bufferInfo, 0)
            if (outputIndex < 0) break

            val outputBuffer = encoder.getOutputBuffer(outputIndex) ?: continue

            try {
                if (bufferInfo.flags and MediaCodec.BUFFER_FLAG_CODEC_CONFIG != 0) {
                    val configData = ByteArray(bufferInfo.size)
                    outputBuffer.get(configData)
                    onEncodedAudio(configData, true, bufferInfo.presentationTimeUs)
                } else if (bufferInfo.size > 0) {
                    val aacData = ByteArray(bufferInfo.size)
                    outputBuffer.get(aacData)
                    // Wrap with ADTS header for raw streaming
                    val adtsFrame = addAdtsHeader(aacData)
                    onEncodedAudio(adtsFrame, false, bufferInfo.presentationTimeUs)
                }
            } finally {
                encoder.releaseOutputBuffer(outputIndex, false)
            }

            if (bufferInfo.flags and MediaCodec.BUFFER_FLAG_END_OF_STREAM != 0) break
        }
    }

    /**
     * Add a 7-byte ADTS header to a raw AAC frame for elementary stream transport.
     */
    private fun addAdtsHeader(aacData: ByteArray): ByteArray {
        val profile = 2 // AAC-LC
        val freqIdx = when (config.sampleRate) {
            96000 -> 0; 88200 -> 1; 64000 -> 2; 48000 -> 3
            44100 -> 4; 32000 -> 5; 24000 -> 6; 22050 -> 7
            16000 -> 8; 12000 -> 9; 11025 -> 10; 8000 -> 11
            else -> 4
        }
        val chanCfg = config.channelCount
        val packetLen = aacData.size + 7

        val adts = ByteArray(7)
        adts[0] = 0xFF.toByte()
        adts[1] = 0xF1.toByte()
        adts[2] = (((profile - 1) shl 6) + (freqIdx shl 2) + (chanCfg shr 2)).toByte()
        adts[3] = (((chanCfg and 3) shl 6) + (packetLen shr 11)).toByte()
        adts[4] = ((packetLen and 0x7FF) shr 3).toByte()
        adts[5] = (((packetLen and 7) shl 5) + 0x1F).toByte()
        adts[6] = 0xFC.toByte()

        return adts + aacData
    }
}
