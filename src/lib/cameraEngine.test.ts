import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import {
  kelvinToRgb,
  computeRingLightStyle,
  buildCanvasFilterString,
  normalizeTapCoordinates,
  computeViewportTransform,
  encodeV2PacketHeader,
  decodeV2PacketHeader,
  computeAbrTargetBitrate,
  FLAG_KEYFRAME,
  FLAG_MIRRORED,
  PACKET_HEADER_SIZE_V2,
} from './cameraEngine.ts';

describe('CamDroid Studio — Pure Camera Engine & Wire Protocol v2.0', () => {
  it('converts Kelvin color temperatures accurately for Virtual Ring Light', () => {
    const warm = kelvinToRgb(2700);
    const neutral = kelvinToRgb(4500);
    const daylight = kelvinToRgb(6500);

    assert.equal(warm.r, 255);
    assert.ok(warm.b < neutral.b, '2700K should have less blue than 4500K');
    assert.ok(neutral.b < daylight.b, '4500K should have less blue than 6500K');
    assert.equal(daylight.r, 255);
    assert.ok(daylight.b >= 250);
  });

  it('increases scene brightness multiplier, display backlight %, and sensor EV gain whenever Ring Light is ON', () => {
    const offState = computeRingLightStyle({
      enabled: false,
      kelvin: 4500,
      intensity: 0.85,
      widthPx: 36,
    });
    assert.equal(offState.brightnessMultiplier, 1.0);
    assert.equal(offState.displayBrightnessPercent, 65);
    assert.equal(offState.sensorEvGain, 0.0);
    assert.equal(offState.subjectFillBoost, 0);

    const onLow = computeRingLightStyle({
      enabled: true,
      kelvin: 3000,
      intensity: 0.3,
      widthPx: 36,
    });
    const onHigh = computeRingLightStyle({
      enabled: true,
      kelvin: 4500,
      intensity: 1.0,
      widthPx: 36,
    });

    assert.ok(
      onLow.brightnessMultiplier > offState.brightnessMultiplier,
      'Ring light ON at 30% must increase scene brightness over OFF'
    );
    assert.ok(
      onHigh.brightnessMultiplier > onLow.brightnessMultiplier,
      'Ring light ON at 100% must increase brightness more than 30%'
    );
    assert.equal(onHigh.displayBrightnessPercent, 100);
    assert.ok(onHigh.sensorEvGain >= 1.5);
  });

  it('builds composite CSS filter strings that increase brightness when Ring Light is enabled', () => {
    const filterOff = buildCanvasFilterString({
      filterId: 'original',
      exposureEv: 0,
      studioLight: false,
      studioLightIntensity: 0.5,
      ringLight: { enabled: false, kelvin: 4500, intensity: 0.9, widthPx: 36 },
    });
    const filterOn = buildCanvasFilterString({
      filterId: 'original',
      exposureEv: 0,
      studioLight: false,
      studioLightIntensity: 0.5,
      ringLight: { enabled: true, kelvin: 4500, intensity: 0.9, widthPx: 36 },
    });

    assert.ok(filterOff.includes('brightness(1.000)'));
    assert.ok(!filterOn.includes('brightness(1.000)'), 'Ring light ON must boost filter brightness');
  });

  it('normalizes Tap-to-Focus coordinates and accounts for horizontal mirroring', () => {
    const rect = { left: 100, top: 50, width: 800, height: 450 };
    const unmirrored = normalizeTapCoordinates(300, 275, rect, false);
    assert.equal(unmirrored.x, 0.25);
    assert.equal(unmirrored.y, 0.5);

    const mirrored = normalizeTapCoordinates(300, 275, rect, true);
    assert.equal(mirrored.x, 0.75);
    assert.equal(mirrored.displayX, 0.25);
  });

  it('computes optical zoom and Center Stage auto-framing transforms', () => {
    const transform = computeViewportTransform({
      zoom: 2.0,
      mirrored: true,
      centerStage: true,
      subjectOffsetX: 0.2,
      subjectOffsetY: -0.1,
    });
    assert.ok(transform.scale > 2.0);
    assert.equal(transform.mirrorScaleX, -1);
  });

  it('encodes and decodes 14-byte v2.0 wire protocol headers with microsecond PTS and flags', () => {
    const original = {
      packetType: 0x01,
      flags: FLAG_KEYFRAME | FLAG_MIRRORED,
      ptsUs: 987654321012n,
      payloadLength: 65536,
    };
    const encoded = encodeV2PacketHeader(original);
    assert.equal(encoded.byteLength, PACKET_HEADER_SIZE_V2);

    const decoded = decodeV2PacketHeader(encoded);
    assert.equal(decoded.packetType, original.packetType);
    assert.equal(decoded.flags, FLAG_KEYFRAME | FLAG_MIRRORED);
    assert.equal(decoded.ptsUs, original.ptsUs);
    assert.equal(decoded.payloadLength, original.payloadLength);
  });

  it('adjusts Adaptive Bitrate (ABR) on congestion and clean recovery intervals', () => {
    const steppedDown = computeAbrTargetBitrate({
      currentBitrateBps: 10_000_000,
      minBitrateBps: 2_500_000,
      maxBitrateBps: 15_000_000,
      congested: true,
      cleanSeconds: 0,
    });
    assert.equal(steppedDown, 8_000_000);

    const rampedUp = computeAbrTargetBitrate({
      currentBitrateBps: 8_000_000,
      minBitrateBps: 2_500_000,
      maxBitrateBps: 15_000_000,
      congested: false,
      cleanSeconds: 4,
    });
    assert.equal(rampedUp, 8_960_000);
  });
});
