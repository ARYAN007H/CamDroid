package com.camdroid.encoder

/**
 * Configuration presets for the video/audio encoders.
 */

enum class VideoCodecType(val mimeType: String, val displayName: String) {
    H264("video/avc", "H.264"),
    H265("video/hevc", "H.265"),
    MJPEG("video/mjpeg", "MJPEG")
}

enum class ResolutionPreset(val width: Int, val height: Int, val displayName: String) {
    FHD(1920, 1080, "1080p"),
    QHD(2560, 1440, "1440p"),
    UHD(3840, 2160, "4K")
}

data class EncoderConfig(
    val codec: VideoCodecType = VideoCodecType.H264,
    val resolution: ResolutionPreset = ResolutionPreset.FHD,
    val fps: Int = 60,
    val bitrate: Int = getDefaultBitrate(VideoCodecType.H264, ResolutionPreset.FHD),
    val iFrameInterval: Int = 1,
    val adaptiveBitrate: Boolean = true
) {
    companion object {
        /** Get the default bitrate for a codec+resolution combination. */
        fun getDefaultBitrate(codec: VideoCodecType, resolution: ResolutionPreset): Int {
            val baseBitrate = when (resolution) {
                ResolutionPreset.FHD -> 6_000_000
                ResolutionPreset.QHD -> 12_000_000
                ResolutionPreset.UHD -> 20_000_000
            }
            // H.265 achieves same quality at ~60% the bitrate of H.264
            return when (codec) {
                VideoCodecType.H264 -> baseBitrate
                VideoCodecType.H265 -> (baseBitrate * 0.6).toInt()
                VideoCodecType.MJPEG -> baseBitrate * 2 // MJPEG needs more bandwidth
            }
        }
    }
}

data class AudioConfig(
    val sampleRate: Int = 44100,
    val channelCount: Int = 1,
    val bitRate: Int = 128_000
)
