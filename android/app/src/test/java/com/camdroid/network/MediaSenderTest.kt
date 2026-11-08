package com.camdroid.network

import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class MediaSenderTest {
    @Test
    fun offerVideoDoesNotWaitForABlockedSocketWriter() {
        val writerEntered = CountDownLatch(1)
        val releaseWriter = CountDownLatch(1)
        val sender = MediaSender(sessionId = 1, writeMedia = {
            writerEntered.countDown()
            releaseWriter.await(1, TimeUnit.SECONDS)
        })
        sender.start()
        sender.offerVideo(byteArrayOf(1), captureTimestampNs = 1)
        assertTrue(writerEntered.await(1, TimeUnit.SECONDS))

        val startedAt = System.nanoTime()
        sender.offerVideo(byteArrayOf(2), captureTimestampNs = 2)
        val elapsedMs = (System.nanoTime() - startedAt) / 1_000_000

        assertTrue("offerVideo blocked for ${elapsedMs}ms", elapsedMs < 50)
        releaseWriter.countDown()
        sender.stop()
    }

    @Test
    fun fullVideoQueueDropsTheOldestNonPriorityFrameAndKeepsTheNewest() {
        val writerEntered = CountDownLatch(1)
        val releaseWriter = CountDownLatch(1)
        val delivered = mutableListOf<Long>()
        val sender = MediaSender(sessionId = 1, videoCapacity = 3, writeMedia = { packet ->
            synchronized(delivered) { delivered += packet.captureTimestampNs }
            if (packet.captureTimestampNs == 1L) {
                writerEntered.countDown()
                releaseWriter.await(1, TimeUnit.SECONDS)
            }
        })
        sender.start()

        sender.offerVideo(byteArrayOf(1), captureTimestampNs = 1)
        assertTrue(writerEntered.await(1, TimeUnit.SECONDS))
        sender.offerVideo(byteArrayOf(2), captureTimestampNs = 2)
        sender.offerVideo(byteArrayOf(3), captureTimestampNs = 3)
        sender.offerVideo(byteArrayOf(4), captureTimestampNs = 4)
        sender.offerVideo(byteArrayOf(5), captureTimestampNs = 5)
        releaseWriter.countDown()
        Thread.sleep(25)

        assertEquals(listOf(1L, 3L, 4L, 5L), delivered)
        assertEquals(1, sender.stats().droppedVideoFrames)
        sender.stop()
    }

    @Test
    fun configurationAndKeyframesAreRetainedWhenVideoQueueIsFull() {
        val writerEntered = CountDownLatch(1)
        val releaseWriter = CountDownLatch(1)
        val delivered = mutableListOf<Long>()
        val sender = MediaSender(sessionId = 1, videoCapacity = 3, writeMedia = { packet ->
            synchronized(delivered) { delivered += packet.captureTimestampNs }
            if (packet.captureTimestampNs == 1L) {
                writerEntered.countDown()
                releaseWriter.await(1, TimeUnit.SECONDS)
            }
        })
        sender.start()

        sender.offerVideo(byteArrayOf(1), captureTimestampNs = 1)
        assertTrue(writerEntered.await(1, TimeUnit.SECONDS))
        sender.offerVideo(byteArrayOf(2), captureTimestampNs = 2, isConfig = true)
        sender.offerVideo(byteArrayOf(3), captureTimestampNs = 3, isKeyFrame = true)
        sender.offerVideo(byteArrayOf(4), captureTimestampNs = 4)
        sender.offerVideo(byteArrayOf(5), captureTimestampNs = 5)
        releaseWriter.countDown()
        Thread.sleep(25)

        assertEquals(listOf(1L, 2L, 3L, 5L), delivered)
        sender.stop()
    }

    @Test
    fun audioQueueIsBounded() {
        val writerEntered = CountDownLatch(1)
        val releaseWriter = CountDownLatch(1)
        val sender = MediaSender(sessionId = 1, audioCapacity = 2, writeMedia = { packet ->
            if (packet.captureTimestampNs == 1L) {
                writerEntered.countDown()
                releaseWriter.await(1, TimeUnit.SECONDS)
            }
        })
        sender.start()

        sender.offerAudio(byteArrayOf(1), captureTimestampNs = 1)
        assertTrue(writerEntered.await(1, TimeUnit.SECONDS))
        sender.offerAudio(byteArrayOf(2), captureTimestampNs = 2)
        sender.offerAudio(byteArrayOf(3), captureTimestampNs = 3)
        sender.offerAudio(byteArrayOf(4), captureTimestampNs = 4)
        releaseWriter.countDown()

        Thread.sleep(25)
        assertEquals(1, sender.stats().droppedAudioFrames)
        sender.stop()
    }
}
